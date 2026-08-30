//! Slate OS Pac-Man -- classic maze chase arcade game.
//!
//! Features a 28x31 grid-based maze with the classic Pac-Man layout,
//! arrow-key movement, 4 ghosts with chase/scatter AI, power pellets
//! that make ghosts vulnerable, wrap-around tunnel, 3 lives, score
//! tracking, level progression, and menu/pause/game-over states.
//! Randomness comes from the shared `randrange` crate, seeded from the
//! system so that two players do not get the same game.
//!
//! Controls: arrow keys to move, P or Esc to pause, N for a new game.
//! While the game is on the menu, paused or over, a click on one of the sheet's
//! lines does what that line says the keyboard does -- the pause sheet offers
//! two of them, so a click has to name which. A click elsewhere on the sheet
//! does nothing rather than falling through to the board behind it.
//!
//! ## What this program was
//!
//! `main` built a `PacmanApp` and dropped it. There was no window: the
//! drawing pass returned a `Vec<RenderCommand>` placed against a fixed
//! `CELL_SIZE` of 18 pixels, so the picture was the same 528x702 whatever
//! window it went into, and `handle_event` -- the only way in for a
//! keystroke -- had no caller and no return value to tell a key it acted on
//! from one it ignored. Twelve blanket `#![allow(...)]` at the top of the
//! file, `dead_code` and `unused_imports` among them, are what kept a
//! compiler from saying so.
//!
//! It now opens a real window, fits the maze to the size that window reports
//! each frame, records a hit box for everything it draws, and answers keys
//! and clicks through one body that the tests drive too.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// -- Catppuccin Mocha palette ------------------------------------------------
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// -- The maze's size, which is a rule of the game --------------------------
//
// 28 by 31 is the classic Pac-Man maze and is not a drawing size: it is how
// far the tunnel runs, how many dots a level holds, and how much room there
// is to turn a corner ahead of a ghost. So the grid keeps its size in cells
// and the window decides only how large a cell is drawn.
const MAZE_COLS: usize = 28;
const MAZE_ROWS: usize = 31;

/// The same two numbers as the signed ones a position is measured in.
///
/// A position is an `i32` because a step can take it off the board before
/// anything checks -- that is how the tunnel and the bounds test work -- so
/// every comparison against the maze's size needs the signed spelling. Named
/// once here rather than cast at each of the dozen comparisons.
const COLS_I32: i32 = MAZE_COLS as i32;
const ROWS_I32: i32 = MAZE_ROWS as i32;

/// The size the window asks for when it opens.
///
/// A cell of about 18 pixels, which is what the program used to draw at
/// unconditionally, plus room for the header and the footer.
const WINDOW_WIDTH: f32 = 528.0;
const WINDOW_HEIGHT: f32 = 738.0;

/// How often the window wakes the game up.
///
/// The game itself moves on its own clock -- a player step every 140 ms, a
/// ghost step every 160 -- so this only has to be fine enough that those
/// thresholds are not crossed late. Sixty a second is one screen refresh.
const TICK: Duration = Duration::from_millis(16);

/// Dot radius, in cells. A dot is a ninth of a cell across.
const DOT_RADIUS_CELLS: f32 = 2.0 / 18.0;
/// Power pellet radius, in cells.
const POWER_PELLET_RADIUS_CELLS: f32 = 5.0 / 18.0;

/// Points for eating a normal dot.
const DOT_POINTS: u32 = 10;
/// Points for eating a power pellet.
const POWER_PELLET_POINTS: u32 = 50;
/// Base points for eating the first ghost during a power pellet.
const GHOST_BASE_POINTS: u32 = 200;

/// Duration of power pellet effect in milliseconds.
const POWER_DURATION_MS: u64 = 8000;
/// Duration of ghost frightened flash near end in milliseconds.
const POWER_FLASH_MS: u64 = 2000;

/// Player movement interval in milliseconds.
const PLAYER_MOVE_MS: u64 = 140;
/// Ghost movement interval in milliseconds.
const GHOST_MOVE_MS: u64 = 160;
/// Frightened ghost movement interval in milliseconds.
const GHOST_FRIGHTENED_MOVE_MS: u64 = 220;

/// How long scatter mode lasts (ms).
const SCATTER_DURATION_MS: u64 = 7000;
/// How long chase mode lasts (ms).
const CHASE_DURATION_MS: u64 = 20000;

/// Initial number of lives.
const INITIAL_LIVES: u32 = 3;

/// How many life tokens the footer draws before it stops counting.
///
/// A player who has earned more lives than this still has them; the footer
/// simply stops widening, because the row shares the strip with the dot count
/// and would otherwise walk into it.
const MAX_LIVES_SHOWN: u32 = 5;

/// Tunnel row (0-indexed).
const TUNNEL_ROW: usize = 14;
/// The same row, signed.
const TUNNEL_ROW_I32: i32 = TUNNEL_ROW as i32;

/// Where each ghost heads while it is scattering: one corner apiece.
///
/// Written out rather than derived from [`MAZE_COLS`] and [`MAZE_ROWS`]: the
/// maze is a fixed 28-by-31 picture spelled out in the layout below, so its
/// corners are fixed too, and `MAZE_COLS as i32 - 3` is an arithmetic
/// expression the lint against silent overflow cannot see through.
const SCATTER_BLINKY: Pos = Pos::new(0, 25);
const SCATTER_PINKY: Pos = Pos::new(0, 2);
const SCATTER_INKY: Pos = Pos::new(30, 27);
const SCATTER_CLYDE: Pos = Pos::new(30, 0);

/// The cell just outside the ghost house door.
///
/// An eaten ghost aims here, and turns back into a live one on arrival.
const GHOST_HOUSE_DOOR: Pos = Pos::new(13, 14);

/// The cell a ghost is placed in when it leaves the house.
const GHOST_HOUSE_EXIT: Pos = Pos::new(11, 14);

// -- What the window can be asked about --------------------------------------

/// Everything the drawing pass records a hit box for.
///
/// The game is played with the keyboard, so most of these exist so that a test
/// can ask *where* a thing was drawn rather than so a player can click it. The
/// sheet targets are the exception: a click on a sheet does what the sheet says
/// the keyboard does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The strip along the top holding the three readings.
    Header,
    Score,
    HighScore,
    Level,
    /// The fitted maze area, recorded before anything drawn inside it.
    Board,
    /// A dot, by grid position.
    Dot(u8, u8),
    /// A power pellet, by grid position.
    Pellet(u8, u8),
    Player,
    /// A ghost, by its index in `ghosts`.
    Ghost(u8),
    /// The strip along the bottom holding the lives and the dot count.
    Footer,
    Lives,
    /// One of the pac-man tokens standing for a remaining life.
    Life(u8),
    Dots,
    /// The dimmed sheet over the board while on the menu, paused or over.
    Overlay,
    OverlayTitle,
    /// The line that says a new game can be started.
    NewGame,
    /// The line that says the game can be resumed.
    Resume,
    /// A line of the menu sheet that only explains a control.
    Controls(u8),
    /// A line of the game-over sheet reporting a final number.
    FinalStat(u8),
}

/// The bands a window is divided into, solved from the size it reports.
///
/// Every number here is a share of the live window rather than a constant.
/// The program used to place the header at a fixed 48 pixels and the maze at a
/// fixed 18 pixels a cell, so the picture was the same 528x702 whatever window
/// it went into -- a larger one left a band of nothing down two sides and a
/// smaller one cut the maze off.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// The whole window.
    window: Rect,
    /// The readings along the top.
    header: Rect,
    /// What is left for the maze.
    body: Rect,
    /// The lives and the dot count along the bottom.
    footer: Rect,
    /// Font size for a header reading.
    head: f32,
    /// Font size for a sheet's body line.
    font: f32,
    /// Font size for a sheet's title.
    title: f32,
    /// Font size for a footer reading.
    small: f32,
    /// The gap between bands, and the margin at the window's edge.
    pad: f32,
}

impl Layout {
    /// Solve the bands for a window of this size.
    #[must_use]
    pub fn new(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Held to half the shorter side as well as to the clamp: a floor of
        // two pixels put the bands' left edge at x = 2 in a window one pixel
        // wide, which is a band that starts outside the window it is in.
        let pad = (w.min(h) * 0.014).clamp(2.0, 12.0).min(w.min(h) / 2.0);
        let head = (h / 42.0).clamp(9.0, 20.0);
        let font = (h / 44.0).clamp(9.0, 18.0);
        let title = (h / 24.0).clamp(14.0, 34.0);
        let small = (h / 58.0).clamp(7.0, 14.0);

        // Shares of `h`, each held to what there is: a band taller than the
        // window would leave the next one a negative height, and a rectangle
        // of negative height draws inside out.
        let header_h = (h * 0.07).clamp(18.0, 52.0).min(h);
        let footer_h = (h * 0.05).clamp(14.0, 40.0).min(h);
        let header = Rect::new(
            pad,
            pad,
            (w - pad * 2.0).max(0.0),
            (header_h - pad).max(0.0),
        );
        let body_y = (header.bottom() + pad).min(h);
        let footer_y = (h - footer_h).max(body_y);
        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            body: Rect::new(
                pad,
                body_y,
                (w - pad * 2.0).max(0.0),
                (footer_y - body_y - pad).max(0.0),
            ),
            footer: Rect::new(
                pad,
                footer_y,
                (w - pad * 2.0).max(0.0),
                (h - footer_y - pad).max(0.0),
            ),
            head,
            font,
            title,
            small,
            pad,
        }
    }
}

/// Where the maze goes, and how a cell reaches the screen.
///
/// The maze is 28 by 31 *cells*, and those two numbers are rules of the game
/// rather than a drawing size, so the window decides only how large a cell is
/// drawn: the largest square cell that fits the space in both directions, with
/// the whole grid centred and the leftover left as margin. Square, because a
/// stretched cell would make a corner look reachable from further away in one
/// direction than the other.
///
/// One number is solved -- `cell` -- and every position, radius and line width
/// follows from it, so the drawing pass and the hit test cannot disagree about
/// where anything is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Board {
    /// The grid's own rectangle, centred in the area it was given.
    rect: Rect,
    /// One cell's side, in pixels.
    cell: f32,
}

impl Board {
    /// Fit the grid into `area`.
    #[must_use]
    pub fn new(area: Rect) -> Self {
        // A backwards area is not reachable through `Layout`, which clamps its
        // bands, but `Board::new` is callable on its own and documents no
        // precondition, so a caller that hands it one gets an empty grid
        // rather than one drawn inside out.
        let aw = area.w.max(0.0);
        let ah = area.h.max(0.0);
        let cell = (aw / MAZE_COLS as f32).min(ah / MAZE_ROWS as f32).max(0.0);
        let w = cell * MAZE_COLS as f32;
        let h = cell * MAZE_ROWS as f32;
        Self {
            rect: Rect::new(area.x + (aw - w) / 2.0, area.y + (ah - h) / 2.0, w, h),
            cell,
        }
    }

    /// The rectangle a cell occupies.
    #[must_use]
    pub fn cell_rect(&self, row: i32, col: i32) -> Rect {
        Rect::new(
            self.rect.x + col as f32 * self.cell,
            self.rect.y + row as f32 * self.cell,
            self.cell,
            self.cell,
        )
    }

    /// The centre of a cell, in window coordinates.
    #[must_use]
    pub fn centre_of(&self, row: i32, col: i32) -> (f32, f32) {
        let r = self.cell_rect(row, col);
        (r.x + self.cell / 2.0, r.y + self.cell / 2.0)
    }

    /// A length given in cells, in pixels.
    ///
    /// Never thinner than a pixel where it is meant to be visible: a dot that
    /// rounds to nothing in a small window is a dot the player cannot see but
    /// still has to eat.
    #[must_use]
    pub fn scaled(&self, cells: f32) -> f32 {
        (cells * self.cell).max(1.0)
    }
}

/// One line of a sheet, measured before any of them is placed.
struct SheetLine {
    text: String,
    /// The name a test finds the line by, where the line is worth naming.
    target: Option<Target>,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// A grid index as the signed number the board works in.
///
/// The maze is 28 by 31, so every index in it fits an `i32` many times over.
/// The conversion is checked anyway: an index that did not fit is answered
/// with one so far off the board that whatever is drawn at it lands outside
/// the window, which is visible, rather than wrapping to a negative row, which
/// would silently draw in the wrong place.
fn grid(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(i32::MAX)
}

/// A grid index as the small number a `Target` carries.
///
/// Same reasoning as [`grid`]: 30 is the largest index the maze has, so the
/// conversion cannot fail, and the saturating answer is a target no cell owns
/// rather than a target belonging to a different cell.
fn byte(i: usize) -> u8 {
    u8::try_from(i).unwrap_or(u8::MAX)
}

/// The same for a count that is already a `u32`.
fn byte_u32(i: u32) -> u8 {
    u8::try_from(i).unwrap_or(u8::MAX)
}

/// A small count as a length.
///
/// Written out rather than suppressing the precision lint across the file:
/// the values here are a life index and a pulse counter, both far inside the
/// range an `f32` holds exactly.
#[expect(
    clippy::cast_precision_loss,
    reason = "a life index and a pulse counter are small"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

/// The radius Pac-Man and a ghost are drawn at.
///
/// A hair inside the cell so two tokens in neighbouring cells do not touch,
/// and never thinner than a pixel: a token that rounds away is a token the
/// player cannot see but can still be caught by.
fn token_radius(b: &Board) -> f32 {
    (b.cell / 2.0 - b.scaled(1.0 / 18.0)).max(1.0)
}

/// The square a disc of this radius occupies.
fn square_at(cx: f32, cy: f32, r: f32) -> Rect {
    Rect::new(cx - r, cy - r, r * 2.0, r * 2.0)
}

/// Fill a rectangle, if there is one to fill.
///
/// A rectangle of zero or negative size is not a small picture but a backwards
/// one, and the renderer would draw it inside out. Windows this program can be
/// given -- one pixel tall, or nothing at all while a resize is in flight --
/// produce them, so the guard is on the one place that emits fills rather than
/// repeated at each of the twenty call sites.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.w <= 0.0 || r.h <= 0.0 {
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

/// Fill a circle of `r` centred on `(cx, cy)`.
fn disc(f: &mut Frame<Target>, cx: f32, cy: f32, r: f32, color: Color) {
    fill(f, square_at(cx, cy, r), color, CornerRadii::all(r));
}

/// Draw a line of text with its left edge at `x`, and answer the box it fills.
///
/// The box is measured, not guessed, so a hit box taken from it is the width
/// of the words that were actually drawn.
fn label(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) -> Rect {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    Rect::new(
        x,
        y,
        text::measure(s, size, weight),
        text::line_height(size, weight),
    )
}

/// The same, centred on `cx` by measuring the string.
fn centred(
    f: &mut Frame<Target>,
    cx: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) -> Rect {
    let w = text::measure(s, size, weight);
    label(f, cx - w / 2.0, y, s, color, size, weight)
}

/// The walls and the ghost door.
///
/// A free function rather than a method: the maze is the only state it reads,
/// and passing it in is what lets a test draw a maze the game is not playing.
fn draw_maze(f: &mut Frame<Target>, maze: &[[Cell; MAZE_COLS]; MAZE_ROWS], b: &Board) {
    for (row, cells) in maze.iter().enumerate() {
        for (col, cell) in cells.iter().enumerate() {
            let r = b.cell_rect(grid(row), grid(col));
            match cell {
                Cell::Wall => fill(f, r, BLUE, CornerRadii::all(b.scaled(2.0 / 18.0))),
                // The door is a lintel across the top of its cell, not a full
                // block: the ghosts pass through it and the player does not,
                // so it has to read as a gap rather than as wall.
                Cell::GhostDoor => fill(
                    f,
                    Rect::new(r.x, r.y, r.w, r.h / 3.0),
                    LAVENDER,
                    CornerRadii::ZERO,
                ),
                _ => {}
            }
        }
    }
}

// -- Random numbers -----------------------------------------------------------
// This crate used to carry its own LCG, whose `next_u64() % bound` handed back
// the low bits of a power-of-two-modulus generator.  The game's one draw site
// is a frightened ghost's flee target, two draws back to back with bounds 31
// and 28.  31 is odd and behaved; 28 = 4 x 7 is not, and cost the ghosts most
// of the maze -- see `random_maze_cell` below.
use randrange::{RandomSource, SeededRng, seed_from_system};

/// A uniformly random maze cell, used as a frightened ghost's flee target.
///
/// A free function rather than an `SeededRng` method because a maze cell is this
/// game's unit, not the generator's.
///
/// Under the old reduction the column was `state % 28`.  An even bound
/// preserves the state's parity, so a ghost's column parity was fixed by
/// where in the draw counter its turn fell, and the factor of four in 28
/// fixed `col % 4` as well.  Each of the four frightened ghosts could
/// therefore flee to just **7 of the 28 columns** -- one residue class
/// mod 4 -- and so to 217 of the maze's 868 cells.  All four together
/// reached 14, and the seed chose only *which* 14, so the frightened
/// scatter was a pair of fixed vertical combs.
fn random_maze_cell(rng: &mut SeededRng) -> Pos {
    Pos::new(
        i32::try_from(rng.below(MAZE_ROWS)).unwrap_or(0),
        i32::try_from(rng.below(MAZE_COLS)).unwrap_or(0),
    )
}

// -- Direction ---------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn is_opposite(self, other: Direction) -> bool {
        matches!(
            (self, other),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        )
    }

    fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
            Direction::Left => (0, -1),
            Direction::Right => (0, 1),
        }
    }
}

// -- Grid position -----------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: i32,
    col: i32,
}

impl Pos {
    const fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    /// One step in `dir`.
    ///
    /// Saturating: a step is one cell, so the only way to reach the end of an
    /// `i32` here is a position that was already impossible, and stopping at
    /// the end is a position [`in_bounds`](Self::in_bounds) rejects rather
    /// than one that has wrapped round to the far edge of the maze.
    fn moved(self, dir: Direction) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row.saturating_add(dr),
            col: self.col.saturating_add(dc),
        }
    }

    /// `steps` steps in `dir`, saturating for the reason [`moved`](Self::moved)
    /// gives.
    ///
    /// The target a ghost aims at is allowed to be off the board -- that is
    /// how Pinky ends up aiming four cells past a player who is standing at
    /// the edge -- so this deliberately does not clamp to the maze.
    fn ahead(self, dir: Direction, steps: i32) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row.saturating_add(dr.saturating_mul(steps)),
            col: self.col.saturating_add(dc.saturating_mul(steps)),
        }
    }

    /// The point as far beyond `self` as `from` is behind it.
    ///
    /// This is Inky's aim: the vector from another ghost to a point ahead of
    /// the player, doubled. Like [`ahead`](Self::ahead) it may land off the
    /// board, and is only ever used as something to measure distance to.
    fn reflected_from(self, from: Pos) -> Self {
        Self {
            row: self.row.saturating_add(self.row.saturating_sub(from.row)),
            col: self.col.saturating_add(self.col.saturating_sub(from.col)),
        }
    }

    fn in_bounds(self) -> bool {
        self.row >= 0 && self.row < ROWS_I32 && self.col >= 0 && self.col < COLS_I32
    }

    /// Whether this position is in the tunnel's mouth -- on the tunnel row and
    /// past either end of it.
    fn in_tunnel_mouth(self) -> bool {
        self.row == TUNNEL_ROW_I32 && (self.col < 0 || self.col >= COLS_I32)
    }

    /// This position as a pair of grid indices, or `None` if it is off the
    /// board.
    ///
    /// The one place a position becomes an index. It used to be written
    /// `self.maze[pos.row as usize][pos.col as usize]` at six call sites, each
    /// preceded by its own bounds test -- and `as usize` on a negative `i32`
    /// is a very large index, so any site that ever lost its test would not
    /// read the wrong cell but panic.
    fn index(self) -> Option<(usize, usize)> {
        if !self.in_bounds() {
            return None;
        }
        let row = usize::try_from(self.row).ok()?;
        let col = usize::try_from(self.col).ok()?;
        Some((row, col))
    }

    /// Wrap position for the tunnel (horizontal wrap-around at tunnel row).
    fn tunnel_wrap(self) -> Self {
        if self.row == TUNNEL_ROW_I32 {
            // `rem_euclid` is the wrap this wants: it answers a column in
            // `0..COLS` for a negative one too, which the plain `%` does not,
            // and it says so in one operation rather than in the three the
            // hand-rolled `((c % n) + n) % n` needed.
            Self {
                row: self.row,
                col: self.col.rem_euclid(COLS_I32),
            }
        } else {
            self
        }
    }

    /// Manhattan distance to another position.
    ///
    /// Saturating throughout: this is compared against other distances to pick
    /// a ghost's next step, so an impossible position must give a large answer
    /// -- one no candidate step beats -- rather than a wrapped negative one,
    /// which would look like the closest step of all.
    fn manhattan_distance(self, other: Pos) -> i32 {
        self.row
            .saturating_sub(other.row)
            .saturating_abs()
            .saturating_add(self.col.saturating_sub(other.col).saturating_abs())
    }
}

// -- Cell types in the maze --------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Wall,
    Empty,
    Dot,
    PowerPellet,
    GhostHouse,
    GhostDoor,
}

impl Cell {
    fn is_walkable(self) -> bool {
        matches!(self, Cell::Empty | Cell::Dot | Cell::PowerPellet)
    }

    fn is_ghost_walkable(self) -> bool {
        matches!(
            self,
            Cell::Empty | Cell::Dot | Cell::PowerPellet | Cell::GhostHouse | Cell::GhostDoor
        )
    }
}

// -- Ghost identity ----------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhostId {
    Blinky, // Red - chases player directly
    Pinky,  // Pink - targets 4 cells ahead of player
    Inky,   // Cyan/Teal - uses Blinky's position for targeting
    Clyde,  // Orange/Peach - chases when far, scatters when close
}

impl GhostId {
    const ALL: [GhostId; 4] = [
        GhostId::Blinky,
        GhostId::Pinky,
        GhostId::Inky,
        GhostId::Clyde,
    ];

    fn color(self) -> Color {
        match self {
            GhostId::Blinky => RED,
            GhostId::Pinky => LAVENDER,
            GhostId::Inky => TEAL,
            GhostId::Clyde => PEACH,
        }
    }

    /// Scatter target corner for each ghost.
    fn scatter_target(self) -> Pos {
        match self {
            GhostId::Blinky => SCATTER_BLINKY,
            GhostId::Pinky => SCATTER_PINKY,
            GhostId::Inky => SCATTER_INKY,
            GhostId::Clyde => SCATTER_CLYDE,
        }
    }

    /// The cell inside the ghost house this ghost waits in before release.
    ///
    /// Blinky's is the door itself: he is released at once, so he never sits
    /// there, and giving him a cell inside the house would put a live ghost
    /// somewhere a live ghost may not walk.
    fn house_cell(self) -> Pos {
        match self {
            GhostId::Blinky => GHOST_HOUSE_DOOR,
            GhostId::Pinky => Pos::new(14, 13),
            GhostId::Inky => Pos::new(14, 14),
            GhostId::Clyde => Pos::new(14, 15),
        }
    }

    /// How long after the level starts this ghost leaves the house.
    fn release_delay_ms(self) -> u64 {
        match self {
            GhostId::Blinky => 0,
            GhostId::Pinky => 1000,
            GhostId::Inky => 3000,
            GhostId::Clyde => 5000,
        }
    }
}

// -- Ghost mode --------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhostMode {
    Chase,
    Scatter,
    Frightened,
    Eaten,
}

// -- Ghost state -------------------------------------------------------------
#[derive(Clone, Debug)]
struct Ghost {
    id: GhostId,
    pos: Pos,
    direction: Direction,
    mode: GhostMode,
    /// Whether this ghost has been released from the ghost house.
    released: bool,
    /// Timer for release delay (ms).
    release_timer_ms: u64,
    /// Release delay threshold (ms).
    release_delay_ms: u64,
}

impl Ghost {
    /// A ghost waiting in `house_cell` until `release_delay_ms` have passed.
    ///
    /// There used to be a `home: Pos` field here holding `house_cell`. Nothing
    /// ever read it -- an eaten ghost aims at the house *door*, not at its own
    /// cell, and a released one is placed at the exit -- so it was a claim
    /// about the ghost that no behaviour backed. The blanket
    /// `#![allow(dead_code)]` at the top of the file is what let it sit there.
    fn new(id: GhostId, house_cell: Pos, release_delay_ms: u64) -> Self {
        // Blinky is out of the house from the first frame; the other three
        // wait in it.
        let released = id == GhostId::Blinky;
        Self {
            id,
            pos: if released {
                GHOST_HOUSE_EXIT
            } else {
                house_cell
            },
            direction: Direction::Left,
            mode: GhostMode::Scatter,
            released,
            release_timer_ms: 0,
            release_delay_ms,
        }
    }
}

// -- Game state enum ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    Menu,
    Playing,
    Paused,
    GameOver,
}

// -- Global ghost behavior mode (chase/scatter cycle) ------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobalGhostMode {
    Chase,
    Scatter,
}

// -- Classic Pac-Man maze layout ---------------------------------------------
// W = Wall, . = Dot, o = Power Pellet, _ = Empty, G = Ghost House, D = Ghost Door
// 28 columns x 31 rows
const MAZE_TEMPLATE: [&str; MAZE_ROWS] = [
    "WWWWWWWWWWWWWWWWWWWWWWWWWWWW", // row 0
    "W............WW............W", // row 1
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 2
    "WoWWWW.WWWWW.WW.WWWWW.WWWWoW", // row 3
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 4
    "W..........................W", // row 5
    "W.WWWW.WW.WWWWWWWW.WW.WWWW.W", // row 6
    "W.WWWW.WW.WWWWWWWW.WW.WWWW.W", // row 7
    "W......WW....WW....WW......W", // row 8
    "WWWWWW.WWWWW_WW_WWWWW.WWWWWW", // row 9
    "WWWWWW.WWWWW_WW_WWWWW.WWWWWW", // row 10
    "WWWWWW.WW__________WW.WWWWWW", // row 11
    "WWWWWW.WW_WWW__WWW_WW.WWWWWW", // row 12
    "WWWWWW.WW_WGGDDGGW_WW.WWWWWW", // row 13
    "______._____GGGG_____.______", // row 14  (tunnel row)
    "WWWWWW.WW_WGGGGGGW_WW.WWWWWW", // row 15
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 16
    "WWWWWW.WW__________WW.WWWWWW", // row 17
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 18
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 19
    "W............WW............W", // row 20
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 21
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 22
    "Wo..WW................WW..oW", // row 23
    "WWW.WW.WW.WWWWWWWW.WW.WW.WWW", // row 24
    "WWW.WW.WW.WWWWWWWW.WW.WW.WWW", // row 25
    "W......WW....WW....WW......W", // row 26
    "W.WWWWWWWWWW.WW.WWWWWWWWWW.W", // row 27
    "W.WWWWWWWWWW.WW.WWWWWWWWWW.W", // row 28
    "W..........................W", // row 29
    "WWWWWWWWWWWWWWWWWWWWWWWWWWWW", // row 30
];

/// Parse the maze template into a grid of cells.
///
/// A row of the template that is short, or missing altogether, leaves the rest
/// of that row wall -- which is what the grid starts as. That is the safe
/// failure: a wall is somewhere the player cannot go, so a mistyped template
/// gives an unreachable pocket rather than a hole out of the maze.
fn parse_maze() -> [[Cell; MAZE_COLS]; MAZE_ROWS] {
    let mut grid = [[Cell::Wall; MAZE_COLS]; MAZE_ROWS];
    for (row, cells) in grid.iter_mut().enumerate() {
        let Some(line) = MAZE_TEMPLATE.get(row).map(|l| l.as_bytes()) else {
            continue;
        };
        for (col, cell) in cells.iter_mut().enumerate() {
            let Some(&glyph) = line.get(col) else {
                continue;
            };
            *cell = match glyph {
                b'.' => Cell::Dot,
                b'o' => Cell::PowerPellet,
                b'_' => Cell::Empty,
                b'G' => Cell::GhostHouse,
                b'D' => Cell::GhostDoor,
                // 'W' and anything unrecognised alike: wall.
                _ => Cell::Wall,
            };
        }
    }
    grid
}

/// Count total dots (including power pellets) in the maze.
fn count_dots(grid: &[[Cell; MAZE_COLS]; MAZE_ROWS]) -> u32 {
    count_cells(grid, Cell::Dot).saturating_add(count_cells(grid, Cell::PowerPellet))
}

/// Count the cells of one kind on a board.
fn count_cells(grid: &[[Cell; MAZE_COLS]; MAZE_ROWS], want: Cell) -> u32 {
    let mut count = 0u32;
    for row in grid {
        for cell in row {
            if *cell == want {
                count = count.saturating_add(1);
            }
        }
    }
    count
}

// -- Main app struct ---------------------------------------------------------
pub struct PacmanApp {
    /// The maze grid.
    maze: [[Cell; MAZE_COLS]; MAZE_ROWS],
    /// Player position.
    player_pos: Pos,
    /// Player movement direction.
    player_dir: Direction,
    /// Queued direction (buffered input).
    queued_dir: Option<Direction>,
    /// The four ghosts.
    ghosts: Vec<Ghost>,
    /// Current game state.
    state: GameState,
    /// Current score.
    score: u32,
    /// High score.
    high_score: u32,
    /// Number of lives remaining.
    lives: u32,
    /// Current level (1-based).
    level: u32,
    /// Total dots remaining.
    dots_remaining: u32,
    /// Total dots at level start.
    total_dots: u32,
    /// Power pellet timer (remaining ms).
    power_timer_ms: u64,
    /// Number of ghosts eaten during current power pellet.
    ghosts_eaten_this_power: u32,
    /// Global ghost behavior mode.
    global_ghost_mode: GlobalGhostMode,
    /// Timer for the current global ghost mode phase (ms elapsed).
    ghost_mode_timer_ms: u64,
    /// Accumulated time for player movement.
    player_move_accum_ms: u64,
    /// Accumulated time for ghost movement.
    ghost_move_accum_ms: u64,
    /// Animation pulse counter.
    pulse_counter: u32,
    /// Mouth animation angle (for pac-man rendering).
    mouth_open: bool,
    /// RNG.
    rng: SeededRng,
    /// Total elapsed game time in ms.
    elapsed_total_ms: u64,
    /// The size the last frame was drawn at, and so the size the next click
    /// is read against.
    size: (f32, f32),
}

impl PacmanApp {
    /// A game seeded from the system, so two players do not get the same one.
    ///
    /// The module doc has claimed this since the window was wired; the code
    /// said `with_seed(42)`, so every launch on every machine ran the same
    /// ghost draws for ever. `seed_from_system` falls back to a fixed seed only
    /// when the kernel's randomness is out of reach, which is a fixed game
    /// rather than no game.
    fn new() -> Self {
        Self::with_seed(seed_from_system(0x5041_434D_414E))
    }

    fn with_seed(seed: u64) -> Self {
        let maze = parse_maze();
        let total = count_dots(&maze);
        let mut app = Self {
            maze,
            player_pos: Pos::new(23, 14),
            player_dir: Direction::Left,
            queued_dir: None,
            ghosts: Vec::new(),
            state: GameState::Menu,
            score: 0,
            high_score: 0,
            lives: INITIAL_LIVES,
            level: 1,
            dots_remaining: total,
            total_dots: total,
            power_timer_ms: 0,
            ghosts_eaten_this_power: 0,
            global_ghost_mode: GlobalGhostMode::Scatter,
            ghost_mode_timer_ms: 0,
            player_move_accum_ms: 0,
            ghost_move_accum_ms: 0,
            pulse_counter: 0,
            mouth_open: true,
            rng: SeededRng::new(seed),
            elapsed_total_ms: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.init_ghosts();
        app
    }

    /// The size the next frame will be drawn at, and the next click read
    /// against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// The size the last frame was drawn at.
    #[must_use]
    pub const fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Initialize the four ghosts in their starting positions.
    ///
    /// Driven off `GhostId::ALL` rather than four hand-written pushes, so a
    /// fifth ghost added to the enum arrives on the board instead of being
    /// silently left off it.
    fn init_ghosts(&mut self) {
        self.ghosts.clear();
        for id in GhostId::ALL {
            self.ghosts
                .push(Ghost::new(id, id.house_cell(), id.release_delay_ms()));
        }
    }

    /// Start a new game.
    ///
    /// Built by replacing the whole app, so anything that is *not* a fact
    /// about the game has to be carried across by hand: the high score, which
    /// survives a game by definition, and the window size, which is not the
    /// game's state at all. Forgetting the size here would send the next frame
    /// back to the size the window opened at, so a player who resized the
    /// window and then pressed N would watch the maze jump.
    fn start_new_game(&mut self) {
        let high = self.high_score;
        let size = self.size;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = high;
        self.size = size;
        self.state = GameState::Playing;
    }

    /// Reset positions after losing a life (keep maze state).
    fn reset_positions(&mut self) {
        self.player_pos = Pos::new(23, 14);
        self.player_dir = Direction::Left;
        self.queued_dir = None;
        self.init_ghosts();
        self.power_timer_ms = 0;
        self.ghosts_eaten_this_power = 0;
        self.global_ghost_mode = GlobalGhostMode::Scatter;
        self.ghost_mode_timer_ms = 0;
        self.player_move_accum_ms = 0;
        self.ghost_move_accum_ms = 0;
    }

    /// Advance to the next level.
    fn next_level(&mut self) {
        self.level = self.level.saturating_add(1);
        self.maze = parse_maze();
        self.total_dots = count_dots(&self.maze);
        self.dots_remaining = self.total_dots;
        self.reset_positions();
    }

    /// The cell at a position, or `None` if the position is off the board.
    fn cell_at(&self, pos: Pos) -> Option<Cell> {
        let (row, col) = pos.index()?;
        self.maze.get(row).and_then(|r| r.get(col)).copied()
    }

    /// Replace the cell at a position. Off the board, nothing happens.
    fn set_cell(&mut self, pos: Pos, cell: Cell) {
        if let Some((row, col)) = pos.index()
            && let Some(slot) = self.maze.get_mut(row).and_then(|r| r.get_mut(col))
        {
            *slot = cell;
        }
    }

    /// Check if a position is walkable for the player.
    fn is_walkable(&self, pos: Pos) -> bool {
        // The tunnel's mouth is off the board and still walkable: that is what
        // makes the wrap-around work.
        if pos.in_tunnel_mouth() {
            return true;
        }
        self.cell_at(pos).is_some_and(Cell::is_walkable)
    }

    /// Check if a position is walkable for a ghost.
    ///
    /// The mode is what makes the two answers differ: only a pair of eyes on
    /// its way home may cross the door and stand in the house. A live ghost --
    /// chasing, scattering or frightened -- is held to the same cells the
    /// player walks, so it cannot sit out a power pellet inside the house.
    ///
    /// This used to be three predicates that were all the same set. `Cell` has
    /// six variants, and `is_walkable() || cell == GhostDoor || cell ==
    /// GhostHouse` covers every one of them except `Wall` -- which is exactly
    /// what the `Eaten` arm covers, and exactly what the deleted
    /// `is_ghost_passable` ("not a wall") answered. So the mode argument
    /// decided nothing and the house door stood open in both directions.
    fn is_ghost_walkable(&self, pos: Pos, ghost_mode: GhostMode) -> bool {
        if pos.in_tunnel_mouth() {
            return true;
        }
        self.cell_at(pos).is_some_and(|cell| match ghost_mode {
            GhostMode::Eaten => cell.is_ghost_walkable(),
            _ => cell.is_walkable(),
        })
    }

    /// Get the chase target for a ghost based on its AI personality.
    fn ghost_chase_target(&self, ghost_id: GhostId) -> Pos {
        match ghost_id {
            GhostId::Blinky => {
                // Directly targets the player.
                self.player_pos
            }
            GhostId::Pinky => {
                // Targets 4 cells ahead of the player in their direction.
                self.player_pos.ahead(self.player_dir, 4)
            }
            GhostId::Inky => {
                // Uses Blinky's position: target is 2 cells ahead of player,
                // then doubled from Blinky's position.
                let ahead = self.player_pos.ahead(self.player_dir, 2);
                let blinky_pos = self
                    .ghosts
                    .iter()
                    .find(|g| g.id == GhostId::Blinky)
                    .map_or(self.player_pos, |g| g.pos);
                ahead.reflected_from(blinky_pos)
            }
            GhostId::Clyde => {
                // Chases player when far, scatters to corner when within 8 cells.
                let dist = self.player_pos.manhattan_distance(
                    self.ghosts
                        .iter()
                        .find(|g| g.id == GhostId::Clyde)
                        .map_or(self.player_pos, |g| g.pos),
                );
                if dist > 8 {
                    self.player_pos
                } else {
                    GhostId::Clyde.scatter_target()
                }
            }
        }
    }

    /// Choose the best direction for a ghost to move toward a target.
    fn ghost_choose_direction(
        &self,
        ghost_pos: Pos,
        current_dir: Direction,
        target: Pos,
        ghost_mode: GhostMode,
    ) -> Direction {
        let mut best_dir = current_dir;
        let mut best_dist = i32::MAX;

        // Ghosts prefer directions in this order: Up, Left, Down, Right
        let preferred_order = [
            Direction::Up,
            Direction::Left,
            Direction::Down,
            Direction::Right,
        ];

        for &dir in &preferred_order {
            // Ghosts cannot reverse direction (except when mode changes).
            if dir.is_opposite(current_dir) {
                continue;
            }
            let next = ghost_pos.moved(dir).tunnel_wrap();
            if self.is_ghost_walkable(next, ghost_mode) {
                let dist = next.manhattan_distance(target);
                if dist < best_dist {
                    best_dist = dist;
                    best_dir = dir;
                }
            }
        }
        best_dir
    }

    /// Move the player one step in the current direction.
    fn move_player(&mut self) {
        // Try the queued direction first.
        if let Some(qd) = self.queued_dir {
            let next = self.player_pos.moved(qd).tunnel_wrap();
            if self.is_walkable(next) {
                self.player_dir = qd;
                self.queued_dir = None;
            }
        }

        let next = self.player_pos.moved(self.player_dir).tunnel_wrap();
        if self.is_walkable(next) {
            self.player_pos = next;
            self.mouth_open = !self.mouth_open;

            // Check what is at the new position. Off the board -- in the
            // tunnel's mouth -- there is nothing to eat, which `cell_at`
            // answers with `None`.
            match self.cell_at(next) {
                Some(Cell::Dot) => {
                    self.set_cell(next, Cell::Empty);
                    self.score = self.score.saturating_add(DOT_POINTS);
                    self.dots_remaining = self.dots_remaining.saturating_sub(1);
                }
                Some(Cell::PowerPellet) => {
                    self.set_cell(next, Cell::Empty);
                    self.score = self.score.saturating_add(POWER_PELLET_POINTS);
                    self.dots_remaining = self.dots_remaining.saturating_sub(1);
                    self.activate_power_pellet();
                }
                _ => {}
            }

            // Update high score.
            if self.score > self.high_score {
                self.high_score = self.score;
            }
        }
    }

    /// Activate power pellet mode.
    fn activate_power_pellet(&mut self) {
        self.power_timer_ms = POWER_DURATION_MS;
        self.ghosts_eaten_this_power = 0;
        for ghost in &mut self.ghosts {
            if ghost.mode != GhostMode::Eaten {
                ghost.mode = GhostMode::Frightened;
                // Reverse direction when becoming frightened.
                ghost.direction = match ghost.direction {
                    Direction::Up => Direction::Down,
                    Direction::Down => Direction::Up,
                    Direction::Left => Direction::Right,
                    Direction::Right => Direction::Left,
                };
            }
        }
    }

    /// Move all ghosts.
    fn move_ghosts(&mut self) {
        let global_mode = self.global_ghost_mode;

        for i in 0..self.ghosts.len() {
            // Everything this step needs is read out before anything moves:
            // choosing a direction reads the whole board, so the ghost cannot
            // be held mutably across it.
            let Some((ghost_mode, ghost_pos, current_dir, ghost_id)) = self
                .ghosts
                .get(i)
                .filter(|g| g.released)
                .map(|g| (g.mode, g.pos, g.direction, g.id))
            else {
                continue;
            };

            let target = match ghost_mode {
                GhostMode::Chase => self.ghost_chase_target(ghost_id),
                GhostMode::Scatter => ghost_id.scatter_target(),
                GhostMode::Frightened => random_maze_cell(&mut self.rng),
                GhostMode::Eaten => GHOST_HOUSE_DOOR,
            };

            let new_dir = self.ghost_choose_direction(ghost_pos, current_dir, target, ghost_mode);
            let new_pos = ghost_pos.moved(new_dir).tunnel_wrap();

            // Verify the new position is walkable for a ghost in this mode.
            // The direction was chosen under the same predicate, so this is a
            // second opinion rather than a new rule -- it matters only when
            // every direction was blocked and `best_dir` fell back to the one
            // the ghost was already going.
            if self.is_ghost_walkable(new_pos, ghost_mode) {
                if let Some(ghost) = self.ghosts.get_mut(i) {
                    ghost.pos = new_pos;
                    ghost.direction = new_dir;
                }
            }

            // Check if eaten ghost reached home. Reviving puts it back at the
            // exit facing out, the same place and pose a ghost released from
            // the house starts in. Leaving it standing on the door would strand
            // it: the door's other three neighbours are house and wall, a live
            // ghost may walk neither, and a ghost may not reverse -- so the one
            // way off the door is the way it came.
            if ghost_mode == GhostMode::Eaten
                && let Some(ghost) = self.ghosts.get_mut(i)
                && ghost.pos == GHOST_HOUSE_DOOR
            {
                ghost.mode = match global_mode {
                    GlobalGhostMode::Chase => GhostMode::Chase,
                    GlobalGhostMode::Scatter => GhostMode::Scatter,
                };
                ghost.pos = GHOST_HOUSE_EXIT;
                ghost.direction = Direction::Up;
            }
        }
    }

    /// Check for collisions between player and ghosts.
    fn check_ghost_collisions(&mut self) {
        for i in 0..self.ghosts.len() {
            let Some(mode) = self
                .ghosts
                .get(i)
                .filter(|g| g.pos == self.player_pos)
                .map(|g| g.mode)
            else {
                continue;
            };
            match mode {
                GhostMode::Frightened => {
                    // Eat the ghost. Each one in the same power pellet is
                    // worth double the last, so the fourth is 1600 -- capped
                    // by the shift so a long chain cannot roll the multiplier
                    // over to zero and start paying nothing.
                    if let Some(ghost) = self.ghosts.get_mut(i) {
                        ghost.mode = GhostMode::Eaten;
                    }
                    let multiplier = 1u32
                        .checked_shl(self.ghosts_eaten_this_power)
                        .unwrap_or(u32::MAX);
                    self.score = self
                        .score
                        .saturating_add(GHOST_BASE_POINTS.saturating_mul(multiplier));
                    self.ghosts_eaten_this_power = self.ghosts_eaten_this_power.saturating_add(1);
                    if self.score > self.high_score {
                        self.high_score = self.score;
                    }
                }
                GhostMode::Eaten => {
                    // Eaten ghosts don't hurt the player.
                }
                _ => {
                    // Player dies.
                    self.lives = self.lives.saturating_sub(1);
                    if self.lives == 0 {
                        self.state = GameState::GameOver;
                    } else {
                        self.reset_positions();
                    }
                    return;
                }
            }
        }
    }

    /// Update ghost release timers.
    fn update_ghost_releases(&mut self, elapsed_ms: u64) {
        for ghost in &mut self.ghosts {
            if !ghost.released {
                ghost.release_timer_ms = ghost.release_timer_ms.saturating_add(elapsed_ms);
                if ghost.release_timer_ms >= ghost.release_delay_ms {
                    ghost.released = true;
                    ghost.pos = GHOST_HOUSE_EXIT;
                }
            }
        }
    }

    /// Update the global ghost mode (chase/scatter cycling).
    fn update_ghost_mode_cycle(&mut self, elapsed_ms: u64) {
        if self.is_power_active() {
            return; // Don't cycle during power pellet.
        }
        self.ghost_mode_timer_ms = self.ghost_mode_timer_ms.saturating_add(elapsed_ms);
        let threshold = match self.global_ghost_mode {
            GlobalGhostMode::Scatter => SCATTER_DURATION_MS,
            GlobalGhostMode::Chase => CHASE_DURATION_MS,
        };
        if self.ghost_mode_timer_ms >= threshold {
            self.ghost_mode_timer_ms = 0;
            self.global_ghost_mode = match self.global_ghost_mode {
                GlobalGhostMode::Scatter => GlobalGhostMode::Chase,
                GlobalGhostMode::Chase => GlobalGhostMode::Scatter,
            };
            // Update ghost modes (except frightened/eaten).
            let new_mode = match self.global_ghost_mode {
                GlobalGhostMode::Chase => GhostMode::Chase,
                GlobalGhostMode::Scatter => GhostMode::Scatter,
            };
            for ghost in &mut self.ghosts {
                if ghost.mode != GhostMode::Frightened && ghost.mode != GhostMode::Eaten {
                    ghost.mode = new_mode;
                    // Reverse direction on mode change.
                    ghost.direction = match ghost.direction {
                        Direction::Up => Direction::Down,
                        Direction::Down => Direction::Up,
                        Direction::Left => Direction::Right,
                        Direction::Right => Direction::Left,
                    };
                }
            }
        }
    }

    /// Update power pellet timer.
    fn update_power_timer(&mut self, elapsed_ms: u64) {
        if self.is_power_active() {
            self.power_timer_ms = self.power_timer_ms.saturating_sub(elapsed_ms);
            if self.power_timer_ms == 0 {
                // Power pellet ended: restore ghosts to normal mode.
                let normal_mode = match self.global_ghost_mode {
                    GlobalGhostMode::Chase => GhostMode::Chase,
                    GlobalGhostMode::Scatter => GhostMode::Scatter,
                };
                for ghost in &mut self.ghosts {
                    if ghost.mode == GhostMode::Frightened {
                        ghost.mode = normal_mode;
                    }
                }
            }
        }
    }

    /// Handle a game tick.
    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Playing {
            // Still update pulse for menu animation.
            self.pulse_counter = self.pulse_counter.wrapping_add(1);
            return;
        }

        self.elapsed_total_ms = self.elapsed_total_ms.saturating_add(elapsed_ms);
        self.pulse_counter = self.pulse_counter.wrapping_add(1);

        // Update ghost releases.
        self.update_ghost_releases(elapsed_ms);

        // Update global ghost mode cycle.
        self.update_ghost_mode_cycle(elapsed_ms);

        // Update power timer.
        self.update_power_timer(elapsed_ms);

        // Player movement.
        self.player_move_accum_ms = self.player_move_accum_ms.saturating_add(elapsed_ms);
        if self.player_move_accum_ms >= PLAYER_MOVE_MS {
            self.player_move_accum_ms = 0;
            self.move_player();
        }

        // Ghost movement.
        self.ghost_move_accum_ms = self.ghost_move_accum_ms.saturating_add(elapsed_ms);
        let ghost_interval = if self.is_power_active() {
            GHOST_FRIGHTENED_MOVE_MS
        } else {
            GHOST_MOVE_MS
        };
        if self.ghost_move_accum_ms >= ghost_interval {
            self.ghost_move_accum_ms = 0;
            self.move_ghosts();
        }

        // Check collisions.
        self.check_ghost_collisions();

        // Check level completion.
        if self.dots_remaining == 0 {
            self.next_level();
        }
    }

    /// Handle key input.
    ///
    /// Every arm says whether it acted. A key the game does not use has to
    /// come back `Ignored` so the window can pass it on rather than redrawing
    /// for nothing -- and so a test can tell "the game did nothing" from "the
    /// game did the wrong thing", which a `()` return cannot express.
    fn handle_key(&mut self, key: Key, pressed: bool) -> EventResult {
        // A release is the tail of a press this game has already acted on.
        if !pressed {
            return EventResult::Ignored;
        }

        match self.state {
            GameState::Menu => {
                if key == Key::N {
                    self.start_new_game();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            GameState::Playing => match key {
                Key::Up => {
                    self.queued_dir = Some(Direction::Up);
                    EventResult::Consumed
                }
                Key::Down => {
                    self.queued_dir = Some(Direction::Down);
                    EventResult::Consumed
                }
                Key::Left => {
                    self.queued_dir = Some(Direction::Left);
                    EventResult::Consumed
                }
                Key::Right => {
                    self.queued_dir = Some(Direction::Right);
                    EventResult::Consumed
                }
                Key::P | Key::Escape => {
                    self.state = GameState::Paused;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            GameState::Paused => match key {
                Key::P | Key::Escape => {
                    self.state = GameState::Playing;
                    EventResult::Consumed
                }
                Key::N => {
                    self.start_new_game();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            GameState::GameOver => {
                if key == Key::N {
                    self.start_new_game();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Handle a mouse press.
    ///
    /// The game is played with the keyboard, so a click does something only
    /// where a sheet is offering a choice in words: while the game is on the
    /// menu, paused or over, a click on the sheet does what the sheet says.
    /// Reading the target from the frame rather than from the coordinates is
    /// what keeps that promise true after the layout moves the sheet.
    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if ev.kind != MouseEventKind::Press(MouseButton::Left) {
            return EventResult::Ignored;
        }
        let target = self.frame(self.size.0, self.size.1).hit_test(ev.x, ev.y);
        // Every target the sheet draws is listed, not just the two lines that
        // name a key: a sheet covers the board, so a click on the dimmed area
        // is a click on the sheet and must not fall through to the game
        // underneath. `NewGame` comes first because the pause sheet offers
        // both, and starting a game is the choice the words name second.
        match target {
            Some(Target::NewGame) => {
                self.start_new_game();
                EventResult::Consumed
            }
            Some(Target::Resume) if self.state == GameState::Paused => {
                self.state = GameState::Playing;
                EventResult::Consumed
            }
            Some(
                Target::Overlay
                | Target::OverlayTitle
                | Target::Resume
                | Target::Controls(_)
                | Target::FinalStat(_),
            ) => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }

    // -- Rendering -----------------------------------------------------------

    /// Draw one frame at the size the window reports, recording where each
    /// thing landed.
    ///
    /// Everything is solved from `w` and `h` on the way through, so nothing
    /// here remembers where it put something last frame and there is no
    /// second copy of the layout for a hit test to disagree with.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::new(w, h);
        let board = Board::new(l.body);
        let mut f = Frame::new(w, h);

        // Everything is drawn inside the window. Without this a reading too
        // wide for a narrow window spilled past the edge, and a zero-sized
        // window still recorded a clickable `Press N to start` at its centre --
        // a control in a window with no pixels to show it.
        f.clip(l.window);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);

        // The board's own box goes down before anything inside it, so a dot,
        // a pellet or a ghost drawn on top of it answers a hit test first.
        f.hit(Target::Board, board.rect);
        draw_maze(&mut f, &self.maze, &board);
        self.draw_dots(&mut f, &board);
        self.draw_player(&mut f, &board);
        self.draw_ghosts(&mut f, &board);

        self.draw_footer(&mut f, &l);
        self.draw_sheet(&mut f, &l);
        f.unclip();
        f
    }

    /// The three readings along the top, each placed by measuring it.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::all(l.pad * 0.5));
        f.hit(Target::Header, l.header);
        if l.header.is_empty() {
            return;
        }
        // A reading wider than the band is cut off at the band rather than
        // written across the maze below it.
        f.clip(l.header);
        let inner = l.pad.max(2.0);
        let bold = FontWeightHint::Bold;
        let y = l.header.y + (l.header.h - text::line_height(l.head, bold)) / 2.0;

        let score = format!("SCORE: {}", self.score);
        let r = label(f, l.header.x + inner, y, &score, TEXT_COLOR, l.head, bold);
        f.hit(Target::Score, r);

        let hi = format!("HI: {}", self.high_score);
        let r = centred(
            f,
            l.header.centre().0,
            y,
            &hi,
            SUBTEXT0,
            l.head,
            FontWeightHint::Regular,
        );
        f.hit(Target::HighScore, r);

        // Right-aligned by measuring the string. It used to be placed 120
        // pixels in from the right edge, which is where "LVL 1" ends only for
        // as long as the level stays one digit and the font stays 14 points.
        let lvl = format!("LVL {}", self.level);
        let width = text::measure(&lvl, l.head, bold);
        let r = label(
            f,
            l.header.right() - inner - width,
            y,
            &lvl,
            LAVENDER,
            l.head,
            bold,
        );
        f.hit(Target::Level, r);
        f.unclip();
    }

    /// Dots and power pellets, each with the box a test can find it by.
    fn draw_dots(&self, f: &mut Frame<Target>, b: &Board) {
        let pulsing = (self.pulse_counter % 30) > 15;
        for (row, cells) in self.maze.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate() {
                let (cx, cy) = b.centre_of(grid(row), grid(col));
                let (radius, target) = match cell {
                    Cell::Dot => (
                        b.scaled(DOT_RADIUS_CELLS),
                        Target::Dot(byte(row), byte(col)),
                    ),
                    Cell::PowerPellet => {
                        // The pulse is a share of the pellet, not a pixel: the
                        // old fixed `+1.0` was invisible in a large window and
                        // a fifth of the pellet again in a small one.
                        let base = b.scaled(POWER_PELLET_RADIUS_CELLS);
                        let r = if pulsing { base * 1.2 } else { base };
                        (r, Target::Pellet(byte(row), byte(col)))
                    }
                    _ => continue,
                };
                disc(f, cx, cy, radius, YELLOW);
                f.hit(target, square_at(cx, cy, radius));
            }
        }
    }

    /// Pac-Man himself: a disc with a bite taken out of it in the direction
    /// he is facing.
    fn draw_player(&self, f: &mut Frame<Target>, b: &Board) {
        if self.state == GameState::Menu {
            return;
        }
        let (cx, cy) = b.centre_of(self.player_pos.row, self.player_pos.col);
        let radius = token_radius(b);

        disc(f, cx, cy, radius, YELLOW);

        if self.mouth_open {
            let reach = radius * 0.5;
            let (mx, my) = match self.player_dir {
                Direction::Right => (cx + reach, cy),
                Direction::Left => (cx - reach, cy),
                Direction::Up => (cx, cy - reach),
                Direction::Down => (cx, cy + reach),
            };
            let ms = radius * 0.45;
            fill(
                f,
                Rect::new(mx - ms / 2.0, my - ms / 2.0, ms, ms),
                BASE,
                CornerRadii::ZERO,
            );
        }

        // The eye sits at a share of the radius. It used to be nudged by a
        // flat `+1.0` or `-3.0` pixels, which put it clean off the head once a
        // cell was drawn smaller than the 18 pixels those numbers were
        // eyeballed against.
        let (ex, ey) = match self.player_dir {
            Direction::Right | Direction::Down => (cx + radius * 0.15, cy - radius * 0.35),
            Direction::Left => (cx - radius * 0.45, cy - radius * 0.35),
            Direction::Up => (cx + radius * 0.15, cy - radius * 0.5),
        };
        disc(f, ex, ey, (radius * 0.25).max(1.0), BASE);

        f.hit(Target::Player, square_at(cx, cy, radius));
    }

    /// The four ghosts, drawn after the player so one standing on him is the
    /// one a hit test finds.
    fn draw_ghosts(&self, f: &mut Frame<Target>, b: &Board) {
        if self.state == GameState::Menu {
            return;
        }
        let flashing = self.is_power_flashing() && (self.pulse_counter % 10) > 5;

        for (i, ghost) in self.ghosts.iter().enumerate() {
            let (cx, cy) = b.centre_of(ghost.pos.row, ghost.pos.col);
            let radius = token_radius(b);

            let body = match ghost.mode {
                GhostMode::Frightened => {
                    if flashing {
                        TEXT_COLOR
                    } else {
                        BLUE
                    }
                }
                GhostMode::Eaten => OVERLAY0,
                _ => ghost.id.color(),
            };

            // A rounded top and a square skirt, which is what makes the shape
            // read as a ghost rather than as a pill.
            fill(
                f,
                square_at(cx, cy, radius),
                body,
                CornerRadii::all(radius * 0.5),
            );
            fill(
                f,
                Rect::new(cx - radius, cy, radius * 2.0, radius),
                body,
                CornerRadii::ZERO,
            );

            let eye_r = (radius * 0.3).max(1.0);
            let eye_y = cy - radius * 0.2;
            if ghost.mode == GhostMode::Eaten {
                // An eaten ghost is a pair of eyes on its way home, so the
                // body above is drawn in the dimmed colour and the eyes are
                // the only part that reads.
                let r = (radius * 0.35).max(1.0);
                disc(f, cx - radius * 0.35, cy, r, TEXT_COLOR);
                disc(f, cx + radius * 0.35, cy, r, TEXT_COLOR);
            } else {
                disc(f, cx - radius * 0.4, eye_y, eye_r, TEXT_COLOR);
                disc(f, cx + radius * 0.4, eye_y, eye_r, TEXT_COLOR);

                // The pupils lean the way the ghost is going, by a share of
                // the head rather than by one pixel.
                let lean = radius * 0.15;
                let (px, py) = match ghost.direction {
                    Direction::Right => (lean, 0.0),
                    Direction::Left => (-lean, 0.0),
                    Direction::Up => (0.0, -lean),
                    Direction::Down => (0.0, lean),
                };
                let pupil = (radius * 0.18).max(1.0);
                disc(f, cx - radius * 0.4 + px, eye_y + py, pupil, BLUE);
                disc(f, cx + radius * 0.4 + px, eye_y + py, pupil, BLUE);
            }

            // The skirt reaches a full radius below the centre, so the box is
            // the body and the skirt together and not just the head.
            f.hit(
                Target::Ghost(byte(i)),
                Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0),
            );
        }
    }

    /// The lives and the dot count along the bottom.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, CornerRadii::all(l.pad * 0.5));
        f.hit(Target::Footer, l.footer);
        if l.footer.is_empty() {
            return;
        }
        f.clip(l.footer);
        let inner = l.pad.max(2.0);
        let plain = FontWeightHint::Regular;
        let line = text::line_height(l.small, plain);
        let y = l.footer.y + (l.footer.h - line) / 2.0;

        let word = label(f, l.footer.x + inner, y, "LIVES:", SUBTEXT0, l.small, plain);
        f.hit(Target::Lives, word);

        // The tokens start where the word ends, measured. They used to start a
        // flat 50 pixels in from the margin and step 22 apart, which lands
        // through the middle of the word in a small window and a gap short of
        // it in a large one.
        let token = (l.footer.h * 0.28).max(1.0);
        let step = token * 2.0 + inner;
        for i in 0..self.lives.min(MAX_LIVES_SHOWN) {
            let cx = word.right() + inner + token + f32_from_u32(i) * step;
            let cy = y + line / 2.0;
            disc(f, cx, cy, token, YELLOW);
            f.hit(Target::Life(byte_u32(i)), square_at(cx, cy, token));
        }

        let dots = format!("DOTS: {}", self.dots_remaining);
        let width = text::measure(&dots, l.small, plain);
        let r = label(
            f,
            l.footer.right() - inner - width,
            y,
            &dots,
            SUBTEXT0,
            l.small,
            plain,
        );
        f.hit(Target::Dots, r);
        f.unclip();
    }

    /// The sheet that covers the board while the game is on the menu, paused
    /// or over.
    ///
    /// Every line is stacked and centred from its own measured width. The old
    /// code placed each one at `centre - 80`, `- 90`, `- 100`, `- 70` or
    /// `- 50` -- a hand-tuned half-width per string, right only at the one
    /// font size those numbers were eyeballed at.
    fn draw_sheet(&self, f: &mut Frame<Target>, l: &Layout) {
        let (title, title_color, dim) = match self.state {
            GameState::Playing => return,
            GameState::Menu => ("PAC-MAN", YELLOW, 220),
            GameState::Paused => ("PAUSED", YELLOW, 180),
            GameState::GameOver => ("GAME OVER", RED, 200),
        };

        fill(f, l.window, Color::rgba(30, 30, 46, dim), CornerRadii::ZERO);
        f.hit(Target::Overlay, l.window);

        let plain = FontWeightHint::Regular;
        let mut lines = vec![SheetLine {
            text: title.to_string(),
            target: Some(Target::OverlayTitle),
            size: l.title,
            weight: FontWeightHint::Bold,
            color: title_color,
        }];
        match self.state {
            GameState::Menu => {
                lines.push(SheetLine {
                    text: "Press N to start".to_string(),
                    target: Some(Target::NewGame),
                    size: l.font,
                    weight: plain,
                    color: TEXT_COLOR,
                });
                lines.push(SheetLine {
                    text: "Arrow keys to move".to_string(),
                    target: Some(Target::Controls(0)),
                    size: l.small,
                    weight: plain,
                    color: SUBTEXT0,
                });
                lines.push(SheetLine {
                    text: "P to pause".to_string(),
                    target: Some(Target::Controls(1)),
                    size: l.small,
                    weight: plain,
                    color: SUBTEXT0,
                });
            }
            GameState::Paused => {
                lines.push(SheetLine {
                    text: "Press P or Esc to resume".to_string(),
                    target: Some(Target::Resume),
                    size: l.font,
                    weight: plain,
                    color: TEXT_COLOR,
                });
                lines.push(SheetLine {
                    text: "Press N for new game".to_string(),
                    target: Some(Target::NewGame),
                    size: l.font,
                    weight: plain,
                    color: SUBTEXT0,
                });
            }
            GameState::GameOver => {
                lines.push(SheetLine {
                    text: format!("Score: {}", self.score),
                    target: Some(Target::FinalStat(0)),
                    size: l.font,
                    weight: plain,
                    color: TEXT_COLOR,
                });
                lines.push(SheetLine {
                    text: format!("Level: {}", self.level),
                    target: Some(Target::FinalStat(1)),
                    size: l.font,
                    weight: plain,
                    color: SUBTEXT0,
                });
                lines.push(SheetLine {
                    text: "Press N for new game".to_string(),
                    target: Some(Target::NewGame),
                    size: l.font,
                    weight: plain,
                    color: TEXT_COLOR,
                });
            }
            GameState::Playing => {}
        }

        // Measure the whole stack before placing any of it, so the block is
        // centred on what it occupies rather than on its first line.
        let gap = l.font * 0.5;
        let mut total = 0.0;
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                total += gap;
            }
            total += text::line_height(line.size, line.weight);
        }

        let (cx, cy) = l.window.centre();
        let mut y = cy - total / 2.0;
        for line in &lines {
            let r = centred(f, cx, y, &line.text, line.color, line.size, line.weight);
            if let Some(t) = line.target {
                f.hit(t, r);
            }
            y += text::line_height(line.size, line.weight) + gap;
        }

        if self.state == GameState::Menu {
            // A pac-man walking across under the words. His travel is a share
            // of the window: the old version added the raw pulse counter as
            // pixels, so he walked 120 pixels whatever the window was wide.
            let r = (l.title * 0.35).max(1.0);
            let span = (l.window.w - r * 2.0).max(0.0);
            let along = f32_from_u32(self.pulse_counter % 120) / 120.0;
            disc(f, r + span * along, y + r, r, YELLOW);
        }
    }

    /// Check if power pellet mode is active.
    fn is_power_active(&self) -> bool {
        self.power_timer_ms > 0
    }

    /// Check if power pellet mode is flashing (near end).
    fn is_power_flashing(&self) -> bool {
        self.is_power_active() && self.power_timer_ms < POWER_FLASH_MS
    }
}

// -- Window plumbing ---------------------------------------------------------

/// The one body both the window and the test probe drive, so what a key or a
/// click does in a test is what it does on a screen.
///
/// The old `handle_event` returned nothing and had no caller at all: `main`
/// built the game and dropped it, so no event ever reached this match.
pub fn handle_event(app: &mut PacmanApp, event: &Event) -> EventResult {
    match event {
        Event::Key(ke) => app.handle_key(ke.key, ke.pressed),
        Event::Mouse(me) => app.handle_mouse(me),
        Event::Tick { elapsed_ms } => {
            app.handle_tick(*elapsed_ms);
            // The pulse counter advances on every tick in every state, so
            // there is always something new to draw -- the pellets breathe and
            // the menu's pac-man walks even with the game standing still.
            EventResult::Consumed
        }
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for PacmanApp {
    fn title(&self) -> String {
        "Pac-Man".to_string()
    }

    fn app_id(&self) -> String {
        "pacman".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the natural size is two small positive whole numbers"
    )]
    fn initial_size(&self) -> (u32, u32) {
        // Converted from the float pair rather than written out again: the two
        // spellings sat next to each other with a comment saying they could not
        // drift, which is a promise a comment cannot keep.
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

impl Probe for PacmanApp {
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
    let mut game = PacmanApp::new();
    app::launch("pacman", &mut game)
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss
    )]

    use super::*;
    use guitk::probe;
    use std::collections::BTreeSet;

    // -- Helpers --------------------------------------------------------------

    fn test_app() -> PacmanApp {
        PacmanApp::with_seed(12345)
    }

    fn playing_app() -> PacmanApp {
        let mut app = test_app();
        app.state = GameState::Playing;
        app
    }

    fn make_key_event(key: Key) -> KeyEvent {
        probe::press(key)
    }

    fn press_key(app: &mut PacmanApp, key: Key) -> EventResult {
        handle_event(app, &Event::Key(make_key_event(key)))
    }

    fn force_player_tick(app: &mut PacmanApp) {
        app.handle_tick(PLAYER_MOVE_MS + 1);
    }

    // -- Frightened-target tests ----------------------------------------------
    //
    // The generator's own behaviour -- determinism, bound respect, uniformity
    // -- is tested in `randrange`; what belongs here is the maze cell this
    // game builds on top of it.

    #[test]
    fn test_random_maze_cell_in_bounds() {
        let mut rng = SeededRng::new(99);
        for _ in 0..2000 {
            let cell = random_maze_cell(&mut rng);
            assert!(cell.in_bounds(), "flee target outside the maze: {cell:?}");
        }
    }

    /// Each frightened ghost must be able to flee anywhere, and it is the
    /// *per-ghost* reach that matters: the ghosts draw round-robin, one cell
    /// each per tick, so each ghost sees every fourth pair of draws.
    ///
    /// Under the old `state % MAZE_COLS` that stride was the whole defect.
    /// 28 is even, so the column's parity was pinned by where a ghost's turn
    /// fell in the draw counter, and 28 = 4 x 7 pinned `col % 4` too: each
    /// ghost reached 7 of the 28 columns and 217 of the 868 cells, all four
    /// together reached 14 columns, and the seed chose only which 14.
    ///
    /// Counted per seed rather than pooled across seeds, because the defect
    /// was per game: four ghosts over a handful of seeds cover every column
    /// between them while each one is still confined to a comb.
    #[test]
    fn every_frightened_ghost_can_flee_to_every_column() {
        const TICKS: usize = 2000;
        let ghosts = PacmanApp::new().ghosts.len();

        for seed in [42_u64, 7, 12_345, 999] {
            let mut rng = SeededRng::new(seed);
            let mut cells: Vec<BTreeSet<(i32, i32)>> = vec![BTreeSet::new(); ghosts];
            for _ in 0..TICKS {
                for seen in &mut cells {
                    let cell = random_maze_cell(&mut rng);
                    seen.insert((cell.row, cell.col));
                }
            }

            for (ghost, seen) in cells.iter().enumerate() {
                let cols: BTreeSet<i32> = seen.iter().map(|&(_, col)| col).collect();
                assert_eq!(
                    cols.len(),
                    MAZE_COLS,
                    "seed {seed}: ghost {ghost} fled to only {} of the {MAZE_COLS} columns \
                     ({:?} mod 4)",
                    cols.len(),
                    cols.iter().map(|c| c % 4).collect::<BTreeSet<_>>()
                );

                // 2000 draws over 868 cells leave about 87% of them hit; the
                // broken column stride capped this at 7 x 31 = 217.
                assert!(
                    seen.len() > 700,
                    "seed {seed}: ghost {ghost} reached only {} of the {} maze cells",
                    seen.len(),
                    MAZE_ROWS * MAZE_COLS
                );
            }
        }
    }

    // -- Direction tests ------------------------------------------------------

    #[test]
    fn test_direction_opposite_up_down() {
        assert!(Direction::Up.is_opposite(Direction::Down));
        assert!(Direction::Down.is_opposite(Direction::Up));
    }

    #[test]
    fn test_direction_opposite_left_right() {
        assert!(Direction::Left.is_opposite(Direction::Right));
        assert!(Direction::Right.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_same() {
        assert!(!Direction::Up.is_opposite(Direction::Up));
        assert!(!Direction::Left.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_perpendicular() {
        assert!(!Direction::Up.is_opposite(Direction::Left));
        assert!(!Direction::Down.is_opposite(Direction::Right));
    }

    #[test]
    fn test_direction_delta_up() {
        assert_eq!(Direction::Up.delta(), (-1, 0));
    }

    #[test]
    fn test_direction_delta_down() {
        assert_eq!(Direction::Down.delta(), (1, 0));
    }

    #[test]
    fn test_direction_delta_left() {
        assert_eq!(Direction::Left.delta(), (0, -1));
    }

    #[test]
    fn test_direction_delta_right() {
        assert_eq!(Direction::Right.delta(), (0, 1));
    }

    // -- Pos tests ------------------------------------------------------------

    #[test]
    fn test_pos_in_bounds_valid() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(MAZE_ROWS as i32 - 1, MAZE_COLS as i32 - 1).in_bounds());
        assert!(Pos::new(15, 14).in_bounds());
    }

    #[test]
    fn test_pos_in_bounds_invalid() {
        assert!(!Pos::new(-1, 0).in_bounds());
        assert!(!Pos::new(0, -1).in_bounds());
        assert!(!Pos::new(MAZE_ROWS as i32, 0).in_bounds());
        assert!(!Pos::new(0, MAZE_COLS as i32).in_bounds());
    }

    #[test]
    fn test_pos_moved() {
        let p = Pos::new(5, 5);
        assert_eq!(p.moved(Direction::Up), Pos::new(4, 5));
        assert_eq!(p.moved(Direction::Down), Pos::new(6, 5));
        assert_eq!(p.moved(Direction::Left), Pos::new(5, 4));
        assert_eq!(p.moved(Direction::Right), Pos::new(5, 6));
    }

    #[test]
    fn test_pos_tunnel_wrap_left() {
        let p = Pos::new(TUNNEL_ROW as i32, -1);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, MAZE_COLS as i32 - 1);
        assert_eq!(w.row, TUNNEL_ROW as i32);
    }

    #[test]
    fn test_pos_tunnel_wrap_right() {
        let p = Pos::new(TUNNEL_ROW as i32, MAZE_COLS as i32);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, 0);
    }

    #[test]
    fn test_pos_tunnel_no_wrap_other_row() {
        let p = Pos::new(5, -1);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, -1); // No wrap on non-tunnel rows.
    }

    #[test]
    fn test_pos_manhattan_distance() {
        assert_eq!(Pos::new(0, 0).manhattan_distance(Pos::new(3, 4)), 7);
        assert_eq!(Pos::new(5, 5).manhattan_distance(Pos::new(5, 5)), 0);
    }

    // -- Maze parsing tests ---------------------------------------------------

    #[test]
    fn test_maze_dimensions() {
        let maze = parse_maze();
        assert_eq!(maze.len(), MAZE_ROWS);
        assert_eq!(maze[0].len(), MAZE_COLS);
    }

    #[test]
    fn test_maze_corners_are_walls() {
        let maze = parse_maze();
        assert_eq!(maze[0][0], Cell::Wall);
        assert_eq!(maze[0][MAZE_COLS - 1], Cell::Wall);
        assert_eq!(maze[MAZE_ROWS - 1][0], Cell::Wall);
        assert_eq!(maze[MAZE_ROWS - 1][MAZE_COLS - 1], Cell::Wall);
    }

    #[test]
    fn test_maze_has_dots() {
        let maze = parse_maze();
        let dots = count_dots(&maze);
        assert!(dots > 0, "Maze should contain dots");
    }

    #[test]
    fn test_maze_has_power_pellets() {
        let maze = parse_maze();
        let power_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::PowerPellet)
            .count();
        assert_eq!(power_count, 4, "Should have 4 power pellets");
    }

    #[test]
    fn test_maze_has_ghost_house() {
        let maze = parse_maze();
        let gh_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::GhostHouse)
            .count();
        assert!(gh_count > 0, "Should have ghost house cells");
    }

    #[test]
    fn test_maze_has_ghost_door() {
        let maze = parse_maze();
        let door_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::GhostDoor)
            .count();
        assert!(door_count > 0, "Should have ghost door cells");
    }

    // -- Cell walkability tests -----------------------------------------------

    #[test]
    fn test_cell_walkability() {
        assert!(!Cell::Wall.is_walkable());
        assert!(Cell::Empty.is_walkable());
        assert!(Cell::Dot.is_walkable());
        assert!(Cell::PowerPellet.is_walkable());
        assert!(!Cell::GhostHouse.is_walkable());
        assert!(!Cell::GhostDoor.is_walkable());
    }

    #[test]
    fn test_cell_ghost_walkability() {
        assert!(!Cell::Wall.is_ghost_walkable());
        assert!(Cell::Empty.is_ghost_walkable());
        assert!(Cell::Dot.is_ghost_walkable());
        assert!(Cell::PowerPellet.is_ghost_walkable());
        assert!(Cell::GhostHouse.is_ghost_walkable());
        assert!(Cell::GhostDoor.is_ghost_walkable());
    }

    // -- GhostId tests --------------------------------------------------------

    #[test]
    fn test_ghost_colors_distinct() {
        let colors: Vec<Color> = GhostId::ALL.iter().map(|g| g.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_ghost_scatter_targets_distinct() {
        let targets: Vec<Pos> = GhostId::ALL.iter().map(|g| g.scatter_target()).collect();
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                assert_ne!(targets[i], targets[j]);
            }
        }
    }

    #[test]
    fn test_ghost_roster_is_the_four_ghosts() {
        let names: Vec<String> = GhostId::ALL.iter().map(probe::variant_name).collect();
        assert_eq!(names, ["Blinky", "Pinky", "Inky", "Clyde"]);
    }

    // -- Ghost initialization tests -------------------------------------------

    #[test]
    fn test_initial_ghost_count() {
        let app = test_app();
        assert_eq!(app.ghosts.len(), 4);
    }

    #[test]
    fn test_blinky_starts_released() {
        let app = test_app();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert!(blinky.released);
    }

    #[test]
    fn test_other_ghosts_start_unreleased() {
        let app = test_app();
        for ghost in &app.ghosts {
            if ghost.id != GhostId::Blinky {
                assert!(!ghost.released, "{:?} should start unreleased", ghost.id);
            }
        }
    }

    #[test]
    fn test_blinky_starts_outside_house() {
        let app = test_app();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert_eq!(blinky.pos, Pos::new(11, 14));
    }

    // -- Game state tests -----------------------------------------------------

    #[test]
    fn test_initial_state_is_menu() {
        let app = test_app();
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_initial_score_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_lives() {
        let app = test_app();
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn test_initial_level() {
        let app = test_app();
        assert_eq!(app.level, 1);
    }

    #[test]
    fn test_dots_remaining_equals_total() {
        let app = test_app();
        assert_eq!(app.dots_remaining, app.total_dots);
    }

    #[test]
    fn test_initial_no_power() {
        let app = test_app();
        assert!(!app.is_power_active());
    }

    #[test]
    fn test_initial_player_position() {
        let app = test_app();
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    #[test]
    fn test_initial_player_direction() {
        let app = test_app();
        assert_eq!(app.player_dir, Direction::Left);
    }

    // -- Key handling tests ---------------------------------------------------

    #[test]
    fn test_n_starts_game_from_menu() {
        let mut app = test_app();
        assert_eq!(app.state, GameState::Menu);
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_p_pauses_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_p_resumes_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_escape_pauses_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::Escape);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_escape_resumes_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::Escape);
        press_key(&mut app, Key::Escape);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_n_from_paused_starts_new() {
        let mut app = playing_app();
        app.score = 500;
        press_key(&mut app, Key::P);
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_n_from_game_over_starts_new() {
        let mut app = playing_app();
        app.state = GameState::GameOver;
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_arrow_keys_queue_direction() {
        let mut app = playing_app();
        press_key(&mut app, Key::Up);
        assert_eq!(app.queued_dir, Some(Direction::Up));
        press_key(&mut app, Key::Down);
        assert_eq!(app.queued_dir, Some(Direction::Down));
        press_key(&mut app, Key::Left);
        assert_eq!(app.queued_dir, Some(Direction::Left));
        press_key(&mut app, Key::Right);
        assert_eq!(app.queued_dir, Some(Direction::Right));
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = playing_app();
        let event = Event::Key(probe::release(Key::Up));
        assert_eq!(handle_event(&mut app, &event), EventResult::Ignored);
        assert_eq!(app.queued_dir, None);
    }

    // -- Movement tests -------------------------------------------------------

    #[test]
    fn test_player_moves_on_tick() {
        let mut app = playing_app();
        // Place player in an open area and set direction.
        app.player_pos = Pos::new(5, 14);
        app.player_dir = Direction::Left;
        app.queued_dir = None;
        let old_pos = app.player_pos;
        // Clear the cell to the left to ensure it is walkable.
        app.maze[5][13] = Cell::Empty;
        force_player_tick(&mut app);
        assert_ne!(app.player_pos, old_pos, "Player should have moved");
    }

    #[test]
    fn test_player_cannot_walk_into_wall() {
        let mut app = playing_app();
        // Place player next to a wall.
        app.player_pos = Pos::new(1, 1);
        app.player_dir = Direction::Up; // Row 0 is all walls.
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(
            app.player_pos,
            Pos::new(1, 1),
            "Player should not move into wall"
        );
    }

    #[test]
    fn test_tunnel_wrap_player_left() {
        let mut app = playing_app();
        app.player_pos = Pos::new(TUNNEL_ROW as i32, 0);
        app.player_dir = Direction::Left;
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(
            app.player_pos.col,
            MAZE_COLS as i32 - 1,
            "Player should wrap around tunnel"
        );
    }

    #[test]
    fn test_tunnel_wrap_player_right() {
        let mut app = playing_app();
        app.player_pos = Pos::new(TUNNEL_ROW as i32, MAZE_COLS as i32 - 1);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(app.player_pos.col, 0, "Player should wrap around tunnel");
    }

    // -- Dot eating tests -----------------------------------------------------

    #[test]
    fn test_eating_dot_scores_points() {
        let mut app = playing_app();
        // Place a dot and move player to it.
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        let old_score = app.score;
        let old_dots = app.dots_remaining;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.score, old_score + DOT_POINTS);
            assert_eq!(app.dots_remaining, old_dots - 1);
        }
    }

    #[test]
    fn test_eating_power_pellet_scores_points() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::PowerPellet;
        let old_score = app.score;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.score, old_score + POWER_PELLET_POINTS);
        }
    }

    #[test]
    fn test_power_pellet_activates_power() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::PowerPellet;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert!(app.is_power_active());
            assert_eq!(app.power_timer_ms, POWER_DURATION_MS);
        }
    }

    #[test]
    fn test_dot_consumed_after_eating() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.maze[5][14], Cell::Empty);
        }
    }

    // -- Power pellet behavior tests ------------------------------------------

    #[test]
    fn test_power_pellet_frightens_ghosts() {
        let mut app = playing_app();
        app.activate_power_pellet();
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Frightened);
        }
    }

    #[test]
    fn test_power_timer_decreases() {
        let mut app = playing_app();
        app.power_timer_ms = 5000;
        app.update_power_timer(1000);
        assert_eq!(app.power_timer_ms, 4000);
    }

    #[test]
    fn test_power_timer_expires() {
        let mut app = playing_app();
        app.activate_power_pellet();
        app.update_power_timer(POWER_DURATION_MS);
        assert_eq!(app.power_timer_ms, 0);
        assert!(!app.is_power_active());
    }

    #[test]
    fn test_power_flashing_near_end() {
        let mut app = playing_app();
        app.power_timer_ms = POWER_FLASH_MS - 100;
        assert!(app.is_power_flashing());
    }

    #[test]
    fn test_power_not_flashing_when_lots_remaining() {
        let mut app = playing_app();
        app.power_timer_ms = POWER_DURATION_MS;
        assert!(!app.is_power_flashing());
    }

    #[test]
    fn test_ghosts_return_to_normal_after_power() {
        let mut app = playing_app();
        app.global_ghost_mode = GlobalGhostMode::Chase;
        app.activate_power_pellet();
        // All frightened.
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Frightened);
        }
        // Expire power.
        app.update_power_timer(POWER_DURATION_MS);
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Chase);
        }
    }

    // -- Ghost collision tests ------------------------------------------------

    #[test]
    fn test_ghost_collision_kills_player() {
        let mut app = playing_app();
        let initial_lives = app.lives;
        app.ghosts[0].mode = GhostMode::Chase;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, initial_lives - 1);
    }

    #[test]
    fn test_ghost_collision_game_over_at_zero_lives() {
        let mut app = playing_app();
        app.lives = 1;
        app.ghosts[0].mode = GhostMode::Chase;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, 0);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_eating_frightened_ghost() {
        let mut app = playing_app();
        app.activate_power_pellet();
        app.ghosts[0].pos = app.player_pos;
        let old_score = app.score;
        app.check_ghost_collisions();
        assert_eq!(app.ghosts[0].mode, GhostMode::Eaten);
        assert!(app.score > old_score);
    }

    #[test]
    fn test_ghost_eating_score_doubles() {
        let mut app = playing_app();
        app.activate_power_pellet();
        // Eat first ghost: 200.
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.score, GHOST_BASE_POINTS); // 200

        // Eat second ghost: 400.
        app.ghosts[1].mode = GhostMode::Frightened;
        app.ghosts[1].pos = app.player_pos;
        let score_before = app.score;
        app.check_ghost_collisions();
        assert_eq!(app.score, score_before + GHOST_BASE_POINTS * 2); // +400
    }

    #[test]
    fn test_eaten_ghost_doesnt_hurt_player() {
        let mut app = playing_app();
        let initial_lives = app.lives;
        app.ghosts[0].mode = GhostMode::Eaten;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, initial_lives);
    }

    // -- Ghost release tests --------------------------------------------------

    #[test]
    fn test_ghost_release_after_delay() {
        let mut app = playing_app();
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(!pinky.released);
        // Pinky has 1000ms delay.
        app.update_ghost_releases(1000);
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(pinky.released);
    }

    #[test]
    fn test_ghost_not_released_before_delay() {
        let mut app = playing_app();
        app.update_ghost_releases(500);
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(!pinky.released, "Pinky should not release at 500ms");
    }

    #[test]
    fn test_released_ghost_count() {
        let app = playing_app();
        let released = app.ghosts.iter().filter(|g| g.released).count();
        assert_eq!(released, 1, "Only Blinky starts released");
    }

    // -- Ghost mode cycle tests -----------------------------------------------

    #[test]
    fn test_initial_ghost_mode_is_scatter() {
        let app = playing_app();
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Scatter);
    }

    #[test]
    fn test_ghost_mode_switches_to_chase() {
        let mut app = playing_app();
        app.update_ghost_mode_cycle(SCATTER_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Chase);
    }

    #[test]
    fn test_ghost_mode_switches_back_to_scatter() {
        let mut app = playing_app();
        app.update_ghost_mode_cycle(SCATTER_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Chase);
        app.update_ghost_mode_cycle(CHASE_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Scatter);
    }

    // -- Level tests ----------------------------------------------------------

    #[test]
    fn test_next_level_increments_level() {
        let mut app = playing_app();
        app.next_level();
        assert_eq!(app.level, 2);
    }

    #[test]
    fn test_next_level_resets_dots() {
        let mut app = playing_app();
        app.dots_remaining = 0;
        app.next_level();
        assert_eq!(app.dots_remaining, app.total_dots);
    }

    #[test]
    fn test_next_level_resets_player_pos() {
        let mut app = playing_app();
        app.player_pos = Pos::new(10, 10);
        app.next_level();
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    // -- Score / high score tests ---------------------------------------------

    #[test]
    fn test_high_score_preserved_on_new_game() {
        let mut app = playing_app();
        app.score = 1000;
        app.high_score = 1000;
        app.start_new_game();
        assert_eq!(app.high_score, 1000);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_high_score_updates_on_dot() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.high_score, DOT_POINTS);
        }
    }

    // -- Ghost AI target tests ------------------------------------------------

    #[test]
    fn test_blinky_targets_player() {
        let app = playing_app();
        let target = app.ghost_chase_target(GhostId::Blinky);
        assert_eq!(target, app.player_pos);
    }

    #[test]
    fn test_pinky_targets_ahead_of_player() {
        let mut app = playing_app();
        app.player_dir = Direction::Right;
        let target = app.ghost_chase_target(GhostId::Pinky);
        assert_eq!(target.row, app.player_pos.row);
        assert_eq!(target.col, app.player_pos.col + 4);
    }

    #[test]
    fn test_clyde_scatters_when_close() {
        let mut app = playing_app();
        // Place Clyde close to the player.
        app.ghosts[3].pos = Pos::new(app.player_pos.row + 2, app.player_pos.col);
        let target = app.ghost_chase_target(GhostId::Clyde);
        assert_eq!(target, GhostId::Clyde.scatter_target());
    }

    #[test]
    fn test_clyde_chases_when_far() {
        let mut app = playing_app();
        app.ghosts[3].pos = Pos::new(0, 0);
        let target = app.ghost_chase_target(GhostId::Clyde);
        assert_eq!(target, app.player_pos);
    }

    // -- Walkability tests ----------------------------------------------------

    #[test]
    fn test_is_walkable_empty() {
        let app = playing_app();
        // Find an empty cell in the maze.
        let mut found = false;
        for r in 0..MAZE_ROWS {
            for c in 0..MAZE_COLS {
                if app.maze[r][c] == Cell::Empty {
                    assert!(app.is_walkable(Pos::new(r as i32, c as i32)));
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
    }

    #[test]
    fn test_is_walkable_wall() {
        let app = playing_app();
        // Top-left corner is always a wall.
        assert!(!app.is_walkable(Pos::new(0, 0)));
    }

    #[test]
    fn test_is_walkable_tunnel() {
        let app = playing_app();
        assert!(
            app.is_walkable(Pos::new(TUNNEL_ROW as i32, -1)),
            "Tunnel wrap should be walkable"
        );
    }

    // -- Tick/event integration tests -----------------------------------------

    #[test]
    fn test_tick_in_menu_doesnt_move() {
        let mut app = test_app();
        let pos = app.player_pos;
        app.handle_tick(1000);
        assert_eq!(app.player_pos, pos, "No movement in menu state");
    }

    #[test]
    fn test_tick_in_paused_doesnt_move() {
        let mut app = playing_app();
        app.state = GameState::Paused;
        let pos = app.player_pos;
        app.handle_tick(1000);
        assert_eq!(app.player_pos, pos, "No movement in paused state");
    }

    #[test]
    fn test_count_cell_type_dots() {
        let app = test_app();
        let dot_count = count_cells(&app.maze, Cell::Dot);
        assert!(dot_count > 0);
    }

    #[test]
    fn test_count_cell_type_power_pellets() {
        let app = test_app();
        let pp_count = count_cells(&app.maze, Cell::PowerPellet);
        assert_eq!(pp_count, 4);
    }

    #[test]
    fn test_total_dots_is_dots_plus_power_pellets() {
        // `count_dots` is the sum of the two counts, not a third traversal
        // with its own idea of what is edible.
        let app = test_app();
        let dots = count_cells(&app.maze, Cell::Dot);
        let pellets = count_cells(&app.maze, Cell::PowerPellet);
        assert_eq!(app.total_dots, dots + pellets);
    }

    // -- Queued direction tests -----------------------------------------------

    #[test]
    fn test_queued_direction_applied() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 14);
        app.player_dir = Direction::Left;
        // Make sure the cell above is walkable.
        app.maze[4][14] = Cell::Empty;
        app.maze[5][13] = Cell::Empty;
        app.queued_dir = Some(Direction::Up);
        force_player_tick(&mut app);
        // Player should have moved up (queued direction was valid).
        assert_eq!(app.player_dir, Direction::Up);
    }

    // -- Reset tests ----------------------------------------------------------

    #[test]
    fn test_reset_positions_keeps_score() {
        let mut app = playing_app();
        app.score = 500;
        app.player_pos = Pos::new(10, 10);
        app.reset_positions();
        assert_eq!(app.score, 500);
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    #[test]
    fn test_reset_positions_resets_ghosts() {
        let mut app = playing_app();
        app.ghosts[0].pos = Pos::new(5, 5);
        app.reset_positions();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert_eq!(blinky.pos, Pos::new(11, 14));
    }

    // -- Ghost at position test -----------------------------------------------

    #[test]
    fn test_no_ghost_starts_inside_a_wall() {
        let app = playing_app();
        for ghost in &app.ghosts {
            assert_ne!(
                app.cell_at(ghost.pos),
                Some(Cell::Wall),
                "{:?} starts inside a wall at {:?}",
                ghost.id,
                ghost.pos
            );
        }
    }

    #[test]
    fn test_live_ghosts_never_stand_in_the_house() {
        // The house is a hiding place a live ghost must not have: while a power
        // pellet burns, a frightened ghost that could step inside would sit out
        // the pellet somewhere the player cannot follow.
        let mut app = playing_app();
        for tick in 0..2000 {
            app.handle_tick(16);
            for ghost in &app.ghosts {
                if !ghost.released || ghost.mode == GhostMode::Eaten {
                    continue;
                }
                let cell = app.cell_at(ghost.pos);
                assert!(
                    cell != Some(Cell::GhostHouse) && cell != Some(Cell::GhostDoor),
                    "tick {tick}: live {:?} stands on {cell:?} at {:?}",
                    ghost.id,
                    ghost.pos
                );
            }
        }
    }

    #[test]
    fn test_revived_ghost_leaves_the_door_and_keeps_moving() {
        // Reviving on the door would strand the ghost: its other three
        // neighbours are house and wall, and a ghost may not reverse, so the
        // only way off is the way it came.
        // Approach the door from directly above, heading down: that is the
        // arrival that strands, because the way back up is the reverse.
        let mut app = playing_app();
        {
            let ghost = &mut app.ghosts[0];
            ghost.mode = GhostMode::Eaten;
            ghost.pos = Pos::new(GHOST_HOUSE_DOOR.row - 1, GHOST_HOUSE_DOOR.col);
            ghost.direction = Direction::Down;
        }
        let mut revived_at = None;
        for _ in 0..200 {
            app.handle_tick(16);
            if app.ghosts[0].mode != GhostMode::Eaten {
                revived_at = Some(app.ghosts[0].pos);
                break;
            }
        }
        assert_eq!(
            revived_at,
            Some(GHOST_HOUSE_EXIT),
            "a ghost revives at the house exit, not on the door"
        );
        let mut seen = BTreeSet::new();
        for _ in 0..200 {
            app.handle_tick(16);
            let p = app.ghosts[0].pos;
            seen.insert((p.row, p.col));
        }
        assert!(
            seen.len() > 2,
            "revived ghost visited only {seen:?} -- it is stuck"
        );
    }

    #[test]
    fn test_released_ghosts_start_where_a_live_ghost_may_walk() {
        // A released ghost is a live ghost, and a live ghost may not walk the
        // door or the house -- so it must not be standing on one either.
        let app = playing_app();
        for ghost in app.ghosts.iter().filter(|g| g.released) {
            assert!(
                app.is_ghost_walkable(ghost.pos, ghost.mode),
                "{:?} is released but stands on {:?}, which it may not walk",
                ghost.id,
                app.cell_at(ghost.pos)
            );
        }
    }

    // -- Mouth animation test -------------------------------------------------

    #[test]
    fn test_mouth_toggles_on_move() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Empty;
        let initial_mouth = app.mouth_open;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_ne!(app.mouth_open, initial_mouth);
        }
    }

    // -- Window wiring: sizes to measure at ------------------------------------

    /// Sizes every geometry invariant is checked at.
    ///
    /// Not a round-numbers list: a layout that is right at 528x738 and wrong
    /// everywhere else is exactly the fault this app had, so the sizes are
    /// deliberately awkward -- very wide, very tall, smaller than the header
    /// and footer put together, and zero.
    const SIZES: [(f32, f32); 9] = [
        (528.0, 738.0),
        (320.0, 240.0),
        (1920.0, 1080.0),
        (2000.0, 300.0),
        (300.0, 2000.0),
        (140.0, 90.0),
        (37.0, 41.0),
        (1.0, 1.0),
        (0.0, 0.0),
    ];

    /// The three states the game can be drawn in.
    fn each_state() -> [(GameState, &'static str); 3] {
        [
            (GameState::Menu, "menu"),
            (GameState::Playing, "playing"),
            (GameState::GameOver, "game over"),
        ]
    }

    fn app_in(state: GameState) -> PacmanApp {
        let mut app = test_app();
        app.state = state;
        app
    }

    // -- Layout tests ----------------------------------------------------------

    #[test]
    fn layout_bands_never_have_a_negative_side() {
        // A rectangle of negative width draws inside out, and the old fixed
        // layout produced one for any window shorter than its header plus its
        // footer.
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("window", l.window),
                ("header", l.header),
                ("body", l.body),
                ("footer", l.footer),
            ] {
                assert!(
                    r.w >= 0.0 && r.h >= 0.0,
                    "{name} is {r:?} at {w}x{h}, which is inside out"
                );
            }
        }
    }

    #[test]
    fn layout_bands_stay_inside_the_window() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, r) in [("header", l.header), ("body", l.body), ("footer", l.footer)] {
                assert!(
                    r.x >= 0.0 && r.y >= 0.0 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{name} is {r:?}, outside a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn layout_bands_stack_in_order_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            assert!(
                l.header.bottom() <= l.body.y + 0.01,
                "header {:?} runs into body {:?} at {w}x{h}",
                l.header,
                l.body
            );
            assert!(
                l.body.bottom() <= l.footer.y + 0.01,
                "body {:?} runs into footer {:?} at {w}x{h}",
                l.body,
                l.footer
            );
        }
    }

    #[test]
    fn layout_font_sizes_are_positive_and_ordered() {
        // The title has to read as a title at every size, and a font of zero
        // or less is a line nobody can see.
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, size) in [
                ("head", l.head),
                ("font", l.font),
                ("title", l.title),
                ("small", l.small),
            ] {
                assert!(size > 0.0, "{name} font is {size} at {w}x{h}");
            }
            assert!(
                l.title >= l.font && l.font >= l.small,
                "fonts out of order at {w}x{h}: title {} font {} small {}",
                l.title,
                l.font,
                l.small
            );
        }
    }

    #[test]
    fn the_bands_are_shares_of_the_height_not_the_width() {
        // A header measured against the width grows when the window is widened,
        // which is the one direction that gives it no more room to fill. Where
        // the bands *end* must not move at all when only the width changes.
        //
        // The margin is the deliberate exception: `pad` is a share of the
        // shorter side, so it does change with the width, and every band's
        // near edge carries it. So the comparison is on the two pad-free
        // readings -- the header's bottom edge, which is the band height
        // itself, and the footer's top edge, which is measured up from the
        // window's bottom.
        let narrow = Layout::new(400.0, 738.0);
        let wide = Layout::new(1600.0, 738.0);
        assert!(
            (narrow.header.bottom() - wide.header.bottom()).abs() < 0.01,
            "the header ended at {} then at {} when only the width changed",
            narrow.header.bottom(),
            wide.header.bottom()
        );
        assert!(
            (narrow.footer.y - wide.footer.y).abs() < 0.01,
            "the footer began at {} then at {} when only the width changed",
            narrow.footer.y,
            wide.footer.y
        );
    }

    #[test]
    fn a_taller_window_gets_larger_type() {
        // The whole point of solving the layout from the window: at the sizes
        // between the clamps, growing the window grows the type.
        let small = Layout::new(528.0, 500.0);
        let large = Layout::new(528.0, 900.0);
        assert!(
            large.title > small.title && large.font > small.font,
            "type did not grow: {:?} then {:?}",
            (small.title, small.font),
            (large.title, large.font)
        );
    }

    // -- Board tests -----------------------------------------------------------

    #[test]
    fn board_cells_are_square_and_fill_the_grid() {
        for (w, h) in SIZES {
            let b = Board::new(Layout::new(w, h).body);
            assert!(b.cell >= 0.0, "cell side {} at {w}x{h}", b.cell);
            let expect_w = b.cell * MAZE_COLS as f32;
            let expect_h = b.cell * MAZE_ROWS as f32;
            assert!(
                (b.rect.w - expect_w).abs() < 0.01 && (b.rect.h - expect_h).abs() < 0.01,
                "grid {:?} is not {MAZE_COLS}x{MAZE_ROWS} cells of {} at {w}x{h}",
                b.rect,
                b.cell
            );
        }
    }

    #[test]
    fn board_is_centred_in_the_body() {
        for (w, h) in SIZES {
            let body = Layout::new(w, h).body;
            let b = Board::new(body);
            let left = b.rect.x - body.x;
            let right = body.right() - b.rect.right();
            let top = b.rect.y - body.y;
            let bottom = body.bottom() - b.rect.bottom();
            assert!(
                (left - right).abs() < 0.01 && (top - bottom).abs() < 0.01,
                "grid {:?} sits off-centre in body {:?} at {w}x{h}",
                b.rect,
                body
            );
        }
    }

    #[test]
    fn board_never_spills_out_of_the_body() {
        for (w, h) in SIZES {
            let body = Layout::new(w, h).body;
            let b = Board::new(body);
            assert!(
                b.rect.x >= body.x - 0.01
                    && b.rect.y >= body.y - 0.01
                    && b.rect.right() <= body.right() + 0.01
                    && b.rect.bottom() <= body.bottom() + 0.01,
                "grid {:?} spills out of body {:?} at {w}x{h}",
                b.rect,
                body
            );
        }
    }

    #[test]
    fn a_cell_moves_down_with_its_row_and_right_with_its_column() {
        // The grid is 28 wide and 31 tall, so a cell placed by its column in
        // both directions still lands inside the grid -- every box would be in
        // bounds and the maze would be a transposed smear.
        let b = Board::new(Layout::new(528.0, 738.0).body);
        let origin = b.cell_rect(0, 0);
        for step in 1..5 {
            let down = b.cell_rect(step, 0);
            let across = b.cell_rect(0, step);
            let f = step as f32 * b.cell;
            assert!(
                (down.y - (origin.y + f)).abs() < 0.01 && (down.x - origin.x).abs() < 0.01,
                "row {step} is at {down:?}, not {f} below {origin:?}"
            );
            assert!(
                (across.x - (origin.x + f)).abs() < 0.01 && (across.y - origin.y).abs() < 0.01,
                "column {step} is at {across:?}, not {f} right of {origin:?}"
            );
        }
    }

    #[test]
    fn two_different_cells_never_share_a_box() {
        let b = Board::new(Layout::new(528.0, 738.0).body);
        let mut seen = BTreeSet::new();
        for row in 0..ROWS_I32 {
            for col in 0..COLS_I32 {
                let (cx, cy) = b.centre_of(row, col);
                assert!(
                    seen.insert((cx.to_bits(), cy.to_bits())),
                    "cell ({row}, {col}) sits on a cell already placed"
                );
            }
        }
    }

    #[test]
    fn a_bigger_window_gets_a_bigger_cell() {
        let small = Board::new(Layout::new(528.0, 738.0).body);
        let large = Board::new(Layout::new(1056.0, 1476.0).body);
        assert!(
            large.cell > small.cell,
            "cell stayed at {} in a window twice the size",
            small.cell
        );
    }

    #[test]
    fn every_cell_of_the_grid_is_inside_the_grid() {
        let b = Board::new(Layout::new(528.0, 738.0).body);
        for row in 0..ROWS_I32 {
            for col in 0..COLS_I32 {
                let r = b.cell_rect(row, col);
                assert!(
                    r.x >= b.rect.x - 0.01
                        && r.y >= b.rect.y - 0.01
                        && r.right() <= b.rect.right() + 0.01
                        && r.bottom() <= b.rect.bottom() + 0.01,
                    "cell ({row}, {col}) is {r:?}, outside grid {:?}",
                    b.rect
                );
            }
        }
    }

    // -- Frame invariants ------------------------------------------------------

    #[test]
    fn the_frame_is_balanced_in_every_state_at_every_size() {
        // An unbalanced frame is a clip or a translation that was pushed and
        // not popped, which silently displaces everything drawn after it.
        for (state, label) in each_state() {
            for (w, h) in SIZES {
                let app = app_in(state);
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{label} at {w}x{h} left a clip or translation on the stack"
                );
            }
        }
    }

    #[test]
    fn every_hit_box_stays_inside_the_window() {
        for (state, label) in each_state() {
            for (w, h) in SIZES {
                let app = app_in(state);
                for (target, r) in app.frame(w, h).hits() {
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{label} at {w}x{h}: {target:?} is {r:?}, outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn no_hit_box_is_empty() {
        // An empty box can never be clicked, so recording one is a control the
        // user cannot reach and a test that would pass by finding it.
        for (state, label) in each_state() {
            for (w, h) in SIZES {
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let app = app_in(state);
                for (target, r) in app.frame(w, h).hits() {
                    assert!(
                        !r.is_empty(),
                        "{label} at {w}x{h}: {target:?} is empty at {r:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_zero_sized_window_draws_nothing_that_can_be_clicked() {
        let app = app_in(GameState::Menu);
        let f = app.frame(0.0, 0.0);
        assert!(f.is_balanced());
        assert_eq!(f.hit_test(0.0, 0.0), None, "a zero window has no controls");
    }

    #[test]
    fn the_board_hit_covers_the_whole_grid() {
        let app = app_in(GameState::Playing);
        let f = app.frame(528.0, 738.0);
        let board = Board::new(Layout::new(528.0, 738.0).body);
        let recorded = f
            .rect_of(|t| matches!(t, Target::Board))
            .expect("the board records a hit box");
        assert!(
            (recorded.x - board.rect.x).abs() < 0.01
                && (recorded.y - board.rect.y).abs() < 0.01
                && (recorded.w - board.rect.w).abs() < 0.01
                && (recorded.h - board.rect.h).abs() < 0.01,
            "board hit {recorded:?} is not the grid {:?}",
            board.rect
        );
    }

    // -- Sheet contents --------------------------------------------------------

    /// Every string the frame draws, in paint order.
    fn drawn_text(app: &PacmanApp, size: (f32, f32)) -> Vec<String> {
        app.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_sheet_offers_a_start_line_and_says_what_the_keys_do() {
        let app = app_in(GameState::Menu);
        for t in [
            Target::Overlay,
            Target::OverlayTitle,
            Target::NewGame,
            Target::Controls(0),
            Target::Controls(1),
        ] {
            assert!(probe::is_visible(&app, t), "{t:?} is missing from the menu");
        }
        let text = drawn_text(&app, PacmanApp::SIZE);
        assert!(text.contains(&"PAC-MAN".to_string()), "no title: {text:?}");
    }

    #[test]
    fn the_pause_sheet_offers_both_ways_out() {
        // Two lines, and a click has to say which -- that is the whole reason
        // `Resume` and `NewGame` are separate targets rather than one sheet.
        let app = app_in(GameState::Paused);
        assert!(probe::is_visible(&app, Target::Resume), "no resume line");
        assert!(probe::is_visible(&app, Target::NewGame), "no new-game line");
        assert_ne!(
            probe::rect_of(&app, Target::Resume),
            probe::rect_of(&app, Target::NewGame),
            "the two lines share a box, so a click cannot choose between them"
        );
    }

    #[test]
    fn the_game_over_sheet_reports_the_score_and_the_level_it_ended_on() {
        let mut app = app_in(GameState::GameOver);
        app.score = 4270;
        app.level = 6;
        let text = drawn_text(&app, PacmanApp::SIZE);
        assert!(
            text.contains(&"Score: 4270".to_string()),
            "the final score is not on the sheet: {text:?}"
        );
        assert!(
            text.contains(&"Level: 6".to_string()),
            "the final level is not on the sheet: {text:?}"
        );
    }

    #[test]
    fn nothing_covers_the_board_while_the_game_is_being_played() {
        let app = app_in(GameState::Playing);
        for t in [
            Target::Overlay,
            Target::OverlayTitle,
            Target::NewGame,
            Target::Resume,
            Target::Controls(0),
            Target::FinalStat(0),
        ] {
            assert!(
                !probe::is_visible(&app, t),
                "{t:?} is on screen during play"
            );
        }
    }

    #[test]
    fn sheet_lines_are_stacked_top_to_bottom_and_do_not_overlap() {
        for state in [GameState::Menu, GameState::Paused, GameState::GameOver] {
            let app = app_in(state);
            let f = app.frame(528.0, 738.0);
            let mut lines: Vec<(String, Rect)> = f
                .hits()
                .iter()
                .filter(|(t, _)| {
                    matches!(
                        t,
                        Target::OverlayTitle
                            | Target::NewGame
                            | Target::Resume
                            | Target::Controls(_)
                            | Target::FinalStat(_)
                    )
                })
                .map(|(t, r)| (format!("{t:?}"), *r))
                .collect();
            assert!(
                lines.len() >= 2,
                "{state:?} sheet has {} lines",
                lines.len()
            );
            // Paint order is top to bottom, so the recorded order is the order
            // on screen -- if it is not, the stack was placed out of sequence.
            for pair in lines.windows(2) {
                let (ref above, a) = pair[0];
                let (ref below, b) = pair[1];
                assert!(
                    a.bottom() <= b.y + 0.01,
                    "{state:?}: {above} at {a:?} overlaps {below} at {b:?}"
                );
            }
            lines.clear();
        }
    }

    #[test]
    fn the_sheet_is_centred_on_the_window_at_every_shape() {
        // The stack is measured before it is placed, so its middle is the
        // window's middle. It used to be centred on its first line, which put
        // a three-line sheet noticeably high.
        for (w, h) in [(528.0, 738.0), (900.0, 400.0), (300.0, 1200.0)] {
            let app = app_in(GameState::Menu);
            let f = app.frame(w, h);
            let lines: Vec<Rect> = f
                .hits()
                .iter()
                .filter(|(t, _)| {
                    matches!(
                        t,
                        Target::OverlayTitle | Target::NewGame | Target::Controls(_)
                    )
                })
                .map(|(_, r)| *r)
                .collect();
            let top = lines.iter().map(|r| r.y).fold(f32::MAX, f32::min);
            let bottom = lines.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
            let middle = top.midpoint(bottom);
            assert!(
                (middle - h / 2.0).abs() < h * 0.06,
                "sheet middle {middle} is not near {} in a {w}x{h} window",
                h / 2.0
            );
        }
    }

    #[test]
    fn the_sheet_lines_are_horizontally_centred() {
        let app = app_in(GameState::Paused);
        let f = app.frame(528.0, 738.0);
        for (t, r) in f.hits() {
            if !matches!(t, Target::OverlayTitle | Target::NewGame | Target::Resume) {
                continue;
            }
            let centre = r.x + r.w / 2.0;
            assert!(
                (centre - 264.0).abs() < 1.0,
                "{t:?} is centred at {centre}, not 264"
            );
        }
    }

    // -- Header and footer placement -------------------------------------------

    #[test]
    fn the_level_reading_is_right_aligned_at_every_width() {
        // It used to be drawn 120 pixels in from the right edge, which is
        // where "LVL 1" ends only while the level stays one digit and the font
        // stays 14 points.
        for (w, h) in [(528.0, 738.0), (900.0, 738.0), (1400.0, 900.0)] {
            let mut app = app_in(GameState::Playing);
            app.level = 12;
            let l = Layout::new(w, h);
            let r = probe::rect_of_sized(&app, Target::Level, (w, h)).expect("a level reading");
            let inset = l.header.right() - r.right();
            assert!(
                (inset - l.pad.max(2.0)).abs() < 1.0,
                "level reading sits {inset} from the right edge at {w}x{h}, not {}",
                l.pad.max(2.0)
            );
        }
    }

    #[test]
    fn a_wider_level_reading_starts_further_left() {
        // Right-aligned means the number growing pushes the text left, not
        // right off the edge.
        let mut one = app_in(GameState::Playing);
        one.level = 1;
        let mut many = app_in(GameState::Playing);
        many.level = 4321;
        let a = probe::rect_of(&one, Target::Level).expect("a level reading");
        let b = probe::rect_of(&many, Target::Level).expect("a level reading");
        assert!(
            b.x < a.x && (b.right() - a.right()).abs() < 1.0,
            "a longer level reading moved to {b:?} instead of growing leftwards from {a:?}"
        );
    }

    #[test]
    fn the_life_tokens_start_after_the_word_lives() {
        // They used to start a flat 50 pixels in from the margin, which lands
        // through the middle of the word in a small window.
        for (w, h) in [(528.0, 738.0), (1400.0, 900.0), (320.0, 500.0)] {
            let app = app_in(GameState::Playing);
            let word = probe::rect_of_sized(&app, Target::Lives, (w, h)).expect("the word LIVES");
            let first = probe::rect_of_sized(&app, Target::Life(0), (w, h)).expect("a life token");
            assert!(
                first.x >= word.right(),
                "at {w}x{h} the first token at {first:?} overlaps the word at {word:?}"
            );
        }
    }

    #[test]
    fn the_life_tokens_do_not_sit_on_top_of_each_other() {
        let mut app = app_in(GameState::Playing);
        app.lives = MAX_LIVES_SHOWN;
        let mut previous: Option<Rect> = None;
        for i in 0..MAX_LIVES_SHOWN {
            let r = probe::rect_of(&app, Target::Life(byte_u32(i))).expect("a life token");
            if let Some(p) = previous {
                assert!(
                    r.x >= p.right(),
                    "token {i} at {r:?} overlaps the one before it at {p:?}"
                );
            }
            previous = Some(r);
        }
    }

    #[test]
    fn the_footer_shows_no_more_tokens_than_it_has_room_for() {
        // A run of extra lives is a number in the footer, not an unbounded row
        // of tokens marching over the dot count.
        let mut app = app_in(GameState::Playing);
        app.lives = 40;
        let f = app.frame(528.0, 738.0);
        let tokens = f
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::Life(_)))
            .count();
        assert_eq!(tokens, MAX_LIVES_SHOWN as usize);
    }

    // -- Click routing ---------------------------------------------------------

    #[test]
    fn clicking_the_start_line_starts_the_game() {
        let mut app = app_in(GameState::Menu);
        assert_eq!(
            probe::click(&mut app, Target::NewGame),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn clicking_resume_resumes_the_paused_game() {
        let mut app = app_in(GameState::Paused);
        app.score = 120;
        assert_eq!(
            probe::click(&mut app, Target::Resume),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 120, "resuming is not restarting");
    }

    #[test]
    fn clicking_new_game_on_the_pause_sheet_restarts() {
        let mut app = app_in(GameState::Paused);
        app.score = 120;
        app.level = 4;
        assert_eq!(
            probe::click(&mut app, Target::NewGame),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0, "a new game starts at nothing");
        assert_eq!(app.level, 1);
    }

    #[test]
    fn starting_a_new_game_keeps_the_window_it_is_played_in() {
        // A new game is built by replacing the whole app, so anything that is
        // not a fact about the game has to be carried across by hand. The
        // window size is one of those: losing it would read the next click
        // against a window that is not on screen.
        let mut app = app_in(GameState::GameOver);
        let big = (1100.0, 850.0);
        app.resize(big.0, big.1);
        app.start_new_game();
        assert_eq!(app.size(), big);
    }

    #[test]
    fn clicking_new_game_after_a_loss_restarts_and_keeps_the_high_score() {
        let mut app = app_in(GameState::GameOver);
        app.score = 900;
        app.high_score = 900;
        assert_eq!(
            probe::click(&mut app, Target::NewGame),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
        assert_eq!(app.high_score, 900, "the high score outlives the game");
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn clicking_a_control_line_is_taken_and_changes_nothing() {
        // The menu's two control lines only say what the keys do. A click on
        // one is consumed rather than passed through to the board behind the
        // sheet, which would be a click on a maze the player cannot see.
        for line in [Target::Controls(0), Target::Controls(1)] {
            let mut app = app_in(GameState::Menu);
            assert_eq!(
                probe::click(&mut app, line),
                EventResult::Consumed,
                "{line:?} fell through the sheet"
            );
            assert_eq!(app.state, GameState::Menu, "{line:?} started something");
        }
    }

    #[test]
    fn clicking_the_dim_part_of_the_sheet_does_not_reach_the_board() {
        let mut app = app_in(GameState::Menu);
        // A corner: covered by the overlay, well away from every line.
        assert_eq!(
            app.click_at(2.0, 2.0, MouseButton::Left, PacmanApp::SIZE),
            EventResult::Consumed
        );
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn clicking_the_board_during_play_is_ignored() {
        let mut app = app_in(GameState::Playing);
        let before = app.player_pos;
        assert_eq!(
            probe::click(&mut app, Target::Board),
            EventResult::Ignored,
            "the board is not a control"
        );
        assert_eq!(app.player_pos, before);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn a_right_click_on_the_start_line_does_nothing() {
        let mut app = app_in(GameState::Menu);
        assert_eq!(
            probe::click_with(&mut app, Target::NewGame, MouseButton::Right),
            EventResult::Ignored
        );
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn a_release_is_not_a_click() {
        let mut app = app_in(GameState::Menu);
        let start = probe::rect_of(&app, Target::NewGame).expect("a start line");
        let (x, y) = start.centre();
        let out = handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            }),
        );
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(app.state, GameState::Menu);
    }

    // -- Resize plumbing -------------------------------------------------------

    #[test]
    fn a_resize_moves_the_controls_with_the_window() {
        // The point of the whole exercise: the boxes a click is read against
        // come from the size the window last reported, not from a constant.
        let small = (400.0, 600.0);
        let large = (1200.0, 900.0);
        let app = app_in(GameState::Menu);
        let at_small = probe::rect_of_sized(&app, Target::NewGame, small).expect("start line");
        let at_large = probe::rect_of_sized(&app, Target::NewGame, large).expect("start line");
        assert_ne!(
            at_small.centre(),
            at_large.centre(),
            "the start line sits at the same place in both windows"
        );
    }

    #[test]
    fn a_resize_event_is_what_moves_them() {
        let mut app = app_in(GameState::Menu);
        let large = (1200.0, 900.0);
        let target = probe::rect_of_sized(&app, Target::NewGame, large).expect("start line");
        let (x, y) = target.centre();

        // Click at the large window's start line while the app still believes
        // it is the default size. That point is off the right-hand edge of a
        // 528-wide window, so there is nothing there at all.
        assert!(
            x > PacmanApp::SIZE.0,
            "pick a point outside the small window"
        );
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                }),
            ),
            EventResult::Ignored,
            "a click past the window's edge hit something"
        );
        assert_eq!(app.state, GameState::Menu, "nothing started");

        // Tell it the window grew, then click the same point.
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Resize {
                    width: 1200,
                    height: 900
                },
            ),
            EventResult::Consumed
        );
        assert_eq!(app.size(), large);
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Mouse(MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Press(MouseButton::Left),
                }),
            ),
            EventResult::Consumed
        );
        assert_eq!(
            app.state,
            GameState::Playing,
            "the resize did not move the start line"
        );
    }

    #[test]
    fn rendering_adopts_the_size_it_was_given() {
        let mut app = app_in(GameState::Menu);
        let _ = app.render(640.0, 480.0);
        assert_eq!(app.size(), (640.0, 480.0));
    }

    #[test]
    fn a_negative_size_is_read_as_nothing_rather_than_inside_out() {
        let mut app = app_in(GameState::Menu);
        app.resize(-100.0, -50.0);
        assert_eq!(app.size(), (0.0, 0.0));
    }

    #[test]
    fn the_window_opens_at_the_size_its_frames_are_measured_for() {
        let app = app_in(GameState::Menu);
        let (w, h) = app.initial_size();
        assert_eq!((f32_from_u32(w), f32_from_u32(h)), PacmanApp::SIZE);
    }

    #[test]
    fn the_window_asks_to_be_woken_often_enough_to_animate() {
        // The game moves on its own clock; the tick only has to be fine enough
        // that a 140 ms player step is not visibly late.
        let app = app_in(GameState::Menu);
        let tick = app.tick_interval().expect("a game needs a clock");
        assert!(
            tick <= Duration::from_millis(PLAYER_MOVE_MS / 2),
            "a {tick:?} tick is too coarse for a {PLAYER_MOVE_MS} ms step"
        );
    }

    #[test]
    fn closing_the_window_exits() {
        let mut app = app_in(GameState::Playing);
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn a_consumed_event_asks_for_a_redraw_and_an_ignored_one_does_not() {
        let mut app = app_in(GameState::Menu);
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::N))),
            Response::Redraw
        ));
        let mut app = app_in(GameState::Playing);
        assert!(matches!(
            app.on_event(&Event::Key(probe::release(Key::Up))),
            Response::Idle
        ));
    }
}
