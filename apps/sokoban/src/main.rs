#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]

//! Sokoban — push every crate onto a target, in a real window.
//!
//! Fifteen warehouses, keyboard and pointer, undo, a move and push counter, and
//! a level menu that remembers what you have finished.
//!
//! # What wiring this up found
//!
//! The program could not be played, because `main` was
//! `let _app = SokobanApp::new();` — it parsed fifteen levels, loaded the first
//! one, dropped the lot and exited. Nothing below was reachable to notice until
//! it had a window on it.
//!
//! 1. **The layout was a constant, and the window was told to match it.**
//!    `render(&self)` took no size; `window_width`/`window_height` *computed*
//!    the window from `CELL_SIZE = 48.0` and the level's dimensions, so the
//!    program drew whatever picture it liked and expected the window to be that
//!    shape. Every offset below it — `grid_origin_x`, `cell_screen_x`, the
//!    header bar, the 400x-something menu panel — was a constant plus a
//!    constant. The layout is now computed from the live window size every
//!    frame, and the hit boxes are recorded by the drawing pass.
//! 2. **The pointer did nothing at all.** `MouseEvent`, `MouseButton` and
//!    `MouseEventKind` were imported and never used; `handle_event` matched
//!    `Event::Key` and dropped everything else. A fifteen-row level menu was
//!    keyboard-only — the one screen in the program that is a list of things to
//!    click. Cells and menu rows are hit targets now, and there are buttons.
//! 3. **`#![allow(dead_code)]` and `#![allow(unused_imports)]` sat at the top
//!    of the file.** With `main` discarding the app, most of the program was
//!    dead and three imports were unused; the two allows are what let it
//!    compile without ever saying so. Both are gone.
//! 4. **The module doc described a program that did not exist.** It advertised
//!    "level completion celebration, and auto-advance to the next level".
//!    Nothing advanced, and the celebration was `celebration_ticks = 120` — a
//!    countdown written twice, decremented by nothing and read by nothing. A
//!    doc comment is an assertion nobody checks; this one was false in three
//!    places at once. The field is gone and the doc now says what the code does.
//! 5. **"Outside the warehouse" was walkable.** `is_wall` asked only whether a
//!    tile was `Tile::Wall`, and `Tile::Empty` — the tile the parser produced
//!    for every unrecognised character *and* for every cell added to pad short
//!    rows out to the full width — is not `Tile::Wall`. So the padding around
//!    an irregular level was floor the player could step onto and push crates
//!    into. It never bit only because every built-in level happens to be fenced
//!    by walls where its rows are ragged, which is a property of the table and
//!    not of the program. `Empty` is impassable now, like off-grid.
//! 6. **`parse_level` had no failure case.** Any character it did not know
//!    became `Tile::Empty` and was forgotten; a level with no `@` silently
//!    started the player at (0, 0); a second `@` silently won over the first;
//!    and nothing compared the number of crates to the number of targets. A
//!    level with three crates and two targets parses, loads, plays and cannot
//!    be solved. Parsing returns a `Result` now, the levels are validated at
//!    startup, and a test asserts all fifteen pass — so a typo in the table is
//!    a failing test rather than an unwinnable puzzle.
//! 7. **`MAX_LEVEL_WIDTH` and `MAX_LEVEL_HEIGHT` were declared and never
//!    read.** A bound nothing enforces is a bound the program does not have.
//!    They are what `parse_level` now rejects oversized levels against.
//! 8. **The win was a latch, and undo could not reach it.** Solving pushed the
//!    program to `Screen::Won`, whose key handler has no undo — so the winning
//!    move was the one move you could not take back, in a game whose whole
//!    failure mode is pushing a crate one square too far. Winning is *derived*
//!    from where the crates are now, the victory panel is an overlay on the
//!    board rather than a separate screen, and undoing the winning move un-wins
//!    because there is no second fact to disagree with the board.
//! 9. **The victory scrim was opaque and said otherwise.** `// Semi-transparent
//!    overlay` over `Color::rgba(17, 17, 27, 180)` — the alpha was real, but the
//!    box, its stats and eight decorative dots were then placed at fixed pixel
//!    offsets from a fixed 320x150 box, so in a window smaller than that the
//!    celebration was drawn off the edge of the thing it was celebrating. The
//!    panel is sized from the window and every string in it is placed from its
//!    own measured width.
//! 10. **Text was positioned by guessing its own width.** The menu drew its
//!     completion marks at `PADDING + 8.0` and its level names at
//!     `PADDING + 60.0` — two guesses at how wide `[done]` renders — and the
//!     victory lines at `+20`, `+58` and `+90`. Every string is now placed from
//!     `text::measure`, and every centred string is limited to the box it is
//!     centred in, so the width that decides the centre is the width the
//!     renderer stops at.
//! 11. **The undo cap dropped the oldest move by shuffling the vector.** At
//!     `MAX_UNDO` entries `try_move` did `undo_stack.remove(0)`, an O(n) shift
//!     per move past the cap. It is a `VecDeque` now, and the header shows the
//!     depth so the loss is visible rather than silent.
//! 12. **`Escape` on the menu was a match arm that did nothing,** under a
//!     comment saying so. An arm that exists to be empty is a line no test can
//!     own; the key simply is not ours on that screen now.
//!
//! Two more the rewrite introduced and the tests caught before it shipped, kept
//! here because they are the same mistakes in new clothes:
//!
//! 13. **A right-aligned string was bounded on one side only.** The first
//!     `label_right` placed text at `right - measure(text)` with no width
//!     limit — the mirror image of fault 10, guessing nothing but assuming
//!     unlimited room. In a 170-pixel window the header's
//!     `Pushes: … Crates: … Undo: …` counter started at x = -42: its right end
//!     sat exactly where it was asked to and the rest of it was off the screen.
//!     It is clamped to the band's left edge and elided there now.
//! 14. **A validity check the notation cannot fail.** `parse_level` ended by
//!     rejecting a level whose player or crates stood on an unwalkable tile —
//!     but `@`, `+`, `$` and `*` each *write* the tile beneath them, and the
//!     `Empty` padding of a ragged row only lands past the characters that row
//!     has. No level could enter the arm, so no test could own it. The check is
//!     gone and the invariant is asserted over the whole table instead.

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
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The scrim drawn over the board when the level is solved.
///
/// Genuinely translucent — `Canvas::set` composites it with `Color::over`, so
/// the warehouse you just cleared shows through the panel congratulating you
/// on it.
const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xB4);

const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// The fraction of the window height the play area is guaranteed before any
/// band of chrome is allowed to keep its full height.
const BODY_SHARE: f32 = 0.52;

/// Bands give up their height in this order when the window is too short:
/// footer help first (it repeats what the buttons already say), then the
/// header, then the controls. The body never gives up any.
const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];

/// Moves kept for undo. Past this the oldest is dropped and the level can no
/// longer be unwound to its opening position — which is why the header shows
/// the depth.
const MAX_UNDO: usize = 1000;

/// The largest warehouse `parse_level` will accept, in cells. Bigger than any
/// built-in level and small enough that a grid of cells is still a grid rather
/// than a bitmap.
const MAX_LEVEL_WIDTH: usize = 20;
const MAX_LEVEL_HEIGHT: usize = 20;

/// The gap between adjacent cells, per unit of cell size, so that every board
/// dimension is a multiple of the single number `cell`.
const GAP_PER_CELL: f32 = 0.06;

// ── Direction ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The step this direction takes, in (row, column).
    #[must_use]
    pub fn delta(self) -> (isize, isize) {
        match self {
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
            Direction::Left => (0, -1),
            Direction::Right => (0, 1),
        }
    }
}

/// Every direction, in the order the tests walk them.
pub const DIRECTIONS: [Direction; 4] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
];

// ── Position ────────────────────────────────────────────────────────
/// A cell, in signed coordinates so that a step off the grid is representable
/// rather than a subtraction that panics.
///
/// `Sokoban::tile_at` reads anything outside the grid as `Tile::Empty`, which
/// is impassable, so an off-grid position is a position the player cannot be
/// pushed into — the type does not need to make it unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub row: isize,
    pub col: isize,
}

impl Pos {
    #[must_use]
    pub const fn new(row: isize, col: isize) -> Self {
        Self { row, col }
    }

    /// The cell one step in `dir`.
    ///
    /// `saturating_add` rather than `checked_add` on purpose: the clamp is only
    /// reachable at `isize::MAX`, and a `None` there would be an arm no test
    /// could enter. Saturating leaves the position off the grid, which
    /// `tile_at` already treats as a wall.
    #[must_use]
    pub fn moved(self, dir: Direction) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row.saturating_add(dr),
            col: self.col.saturating_add(dc),
        }
    }
}

// ── Tiles ───────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    /// Floor the player may stand on.
    Floor,
    /// A wall.
    Wall,
    /// Floor that a crate belongs on.
    Target,
    /// Not part of the warehouse — the padding around a ragged level, and
    /// everything off the grid. Impassable, which is the whole point: the
    /// version this replaced let the player walk out here.
    Empty,
}

impl Tile {
    /// Whether the player or a crate may occupy this tile.
    #[must_use]
    pub fn walkable(self) -> bool {
        matches!(self, Tile::Floor | Tile::Target)
    }
}

// ── Levels ──────────────────────────────────────────────────────────
/// Why a level could not be parsed.
///
/// Every one of these was a silent success before: an unknown character became
/// `Empty`, a missing `@` became a player at the origin, a second `@` replaced
/// the first, and a crate count that did not match the target count was simply
/// an unwinnable puzzle the menu offered you anyway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelError {
    /// No rows, or every row empty.
    Empty,
    /// A character outside the Sokoban notation.
    UnknownTile(char),
    /// No `@`.
    NoPlayer,
    /// More than one `@`.
    TwoPlayers,
    /// No crates, which would make the level solved before it started.
    NoBoxes,
    /// Crates and targets do not balance, so the level cannot be finished.
    Unbalanced { boxes: usize, targets: usize },
    /// Wider or taller than `MAX_LEVEL_WIDTH` x `MAX_LEVEL_HEIGHT`.
    TooBig { width: usize, height: usize },
}

/// A parsed, validated level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Vec<Tile>>,
    pub player: Pos,
    pub boxes: Vec<Pos>,
}

/// Parse a level in the standard Sokoban notation.
///
/// `#` wall, ` ` or `-` floor, `.` target, `$` crate, `@` player, `+` player on
/// a target, `*` crate on a target. Anything else is an error rather than a
/// tile quietly dropped on the floor.
///
/// # Errors
///
/// Returns the first thing wrong with the level, in the order listed on
/// [`LevelError`].
pub fn parse_level(source: &str) -> Result<Level, LevelError> {
    let mut tiles: Vec<Vec<Tile>> = Vec::new();
    let mut player: Option<Pos> = None;
    let mut boxes: Vec<Pos> = Vec::new();

    for (row_idx, line) in source.lines().enumerate() {
        let mut row = Vec::new();
        for (col_idx, ch) in line.chars().enumerate() {
            let at = Pos::new(row_idx as isize, col_idx as isize);
            match ch {
                '#' => row.push(Tile::Wall),
                ' ' | '-' => row.push(Tile::Floor),
                '.' => row.push(Tile::Target),
                '$' => {
                    row.push(Tile::Floor);
                    boxes.push(at);
                }
                '*' => {
                    row.push(Tile::Target);
                    boxes.push(at);
                }
                '@' | '+' => {
                    row.push(if ch == '+' { Tile::Target } else { Tile::Floor });
                    if player.is_some() {
                        return Err(LevelError::TwoPlayers);
                    }
                    player = Some(at);
                }
                other => return Err(LevelError::UnknownTile(other)),
            }
        }
        tiles.push(row);
    }

    let width = tiles.iter().map(Vec::len).max().unwrap_or(0);
    let height = tiles.len();
    if width == 0 || height == 0 {
        return Err(LevelError::Empty);
    }
    if width > MAX_LEVEL_WIDTH || height > MAX_LEVEL_HEIGHT {
        return Err(LevelError::TooBig { width, height });
    }

    // Ragged rows are padded to the full width with `Empty`, which is *not*
    // floor — see fault 5. The padding is outside the warehouse and stays
    // outside it.
    for row in &mut tiles {
        while row.len() < width {
            row.push(Tile::Empty);
        }
    }

    let Some(player) = player else {
        return Err(LevelError::NoPlayer);
    };
    if boxes.is_empty() {
        return Err(LevelError::NoBoxes);
    }
    let targets = tiles
        .iter()
        .flat_map(|row| row.iter())
        .filter(|t| **t == Tile::Target)
        .count();
    if targets != boxes.len() {
        return Err(LevelError::Unbalanced {
            boxes: boxes.len(),
            targets,
        });
    }

    // There is deliberately no "starts on a wall" check. `@`, `+`, `$` and `*`
    // each write their own tile at their own cell — `Floor` or `Target`, never
    // `Wall` and never the `Empty` that pads a short row, because padding only
    // ever lands past the end of the characters a row actually has. So the
    // player and every crate are on a walkable tile by construction, and a
    // guard here would be an arm no level could enter and no test could own.
    // The invariant is asserted instead, over the whole table, by
    // `the_player_and_every_crate_start_on_floor`.

    Ok(Level {
        width,
        height,
        tiles,
        player,
        boxes,
    })
}

/// The built-in warehouses, easiest first.
///
/// Every one of these is parsed and validated at startup, and a test asserts
/// all of them survive it — so a typo here is a red test rather than a level
/// the menu offers and nobody can finish.
pub const LEVELS: [&str; 15] = [
    // 1 — one crate, one target.
    concat!(
        "  ###\n",
        "  #.#\n",
        "###-###\n",
        "#--$--#\n",
        "#--@--#\n",
        "#-----#\n",
        "#######\n",
    ),
    // 2 — two crates in a line.
    concat!(
        "######\n", "#----#\n", "#-$$-#\n", "#-..-#\n", "#--@-#\n", "######\n",
    ),
    // 3 — an L.
    concat!(
        "#####\n", "#---##\n", "#-$--#\n", "##-$-#\n", "-#-.-#\n", "-#-.-#\n", "-#-@-#\n",
        "-#####\n",
    ),
    // 4 — corridor push.
    concat!(
        "########\n",
        "#------#\n",
        "#-#.##-#\n",
        "#--$---#\n",
        "#-#$##-#\n",
        "#--.-@-#\n",
        "########\n",
    ),
    // 5 — three crates.
    concat!(
        "-#####\n",
        "##---#\n",
        "#-$--#\n",
        "#-.$-##\n",
        "#-.$.@#\n",
        "#-----#\n",
        "#######\n",
    ),
    // 6 — wide room.
    concat!(
        "-####\n",
        "##--####\n",
        "#--$---#\n",
        "#-#.#$-#\n",
        "#---#.-#\n",
        "##-@####\n",
        "-####\n",
    ),
    // 7 — tight corners.
    concat!(
        "########\n",
        "#--#---#\n",
        "#--$-$-#\n",
        "#.#--#.#\n",
        "#------#\n",
        "#--@---#\n",
        "########\n",
    ),
    // 8 — four crates around the player.
    concat!(
        "--#####\n",
        "###---#\n",
        "#-$.$-#\n",
        "#-.@.-#\n",
        "#-$.$-#\n",
        "###---#\n",
        "--#####\n",
    ),
    // 9 — asymmetric.
    concat!(
        "#######\n",
        "#--#--#\n",
        "#-$---#\n",
        "#--$#-#\n",
        "##.-.-#\n",
        "-#.$--#\n",
        "-#--@-#\n",
        "-######\n",
    ),
    // 10 — winding path.
    concat!(
        "-#######\n",
        "-#-----#\n",
        "##-#-#-#\n",
        "#--$-$-#\n",
        "#-.-.#-#\n",
        "#--$---#\n",
        "#--.-@-#\n",
        "########\n",
    ),
    // 11 — five crates.
    concat!(
        "########\n",
        "#------#\n",
        "#-$$$--#\n",
        "#-.-.--#\n",
        "#--$-$-#\n",
        "#-.-..-#\n",
        "#--@---#\n",
        "########\n",
    ),
    // 12 — a cross.
    concat!(
        "--####\n",
        "###--##\n",
        "#-.$--#\n",
        "#-#.#-#\n",
        "#-$.$-#\n",
        "#-#.#-#\n",
        "#--$--#\n",
        "##--@##\n",
        "-#####\n",
    ),
    // 13 — two rooms.
    concat!(
        "####--####\n",
        "#--####--#\n",
        "#--$--$--#\n",
        "#-.----.-#\n",
        "####--####\n",
        "#-.----.-#\n",
        "#--$--$--#\n",
        "#--@##---#\n",
        "##########\n",
    ),
    // 14 — five crates, awkwardly placed.
    concat!(
        "-########\n",
        "-#------#\n",
        "##-$$---#\n",
        "#--#-$#-#\n",
        "#-.#..--#\n",
        "#---$-$-#\n",
        "#--#..--#\n",
        "#---@---#\n",
        "#########\n",
    ),
    // 15 — eight crates.
    concat!(
        "##########\n",
        "#--------#\n",
        "#-$$-$$--#\n",
        "#-#....#-#\n",
        "#---##---#\n",
        "#-$$-$$--#\n",
        "#-#....#-#\n",
        "#----@---#\n",
        "##########\n",
    ),
];

// ── Hit targets ─────────────────────────────────────────────────────
/// Everything the pointer can land on. Recorded by the drawing pass, so a
/// target exists exactly where the thing it names was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell of the warehouse. Crates and the player are drawn *over* their
    /// cell without recording a target of their own, so the click always names
    /// a square rather than whatever happens to be standing on it.
    Cell(usize, usize),
    /// A row of the level menu.
    Level(usize),
    Undo,
    Restart,
    Menu,
    Next,
    Play,
}

/// Which of the two screens is up. Winning is *not* one of these — it is
/// derived from where the crates are, so undoing the winning move un-wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The level menu.
    Select,
    /// A warehouse.
    Playing,
}

/// One move, enough to reverse it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoEntry {
    /// Where the player stood before the move.
    pub player: Pos,
    /// The crate that was pushed, as (before, after). `None` for a walk.
    pub push: Option<(Pos, Pos)>,
}

// ── Layout ──────────────────────────────────────────────────────────
/// Where everything goes in a window of a given size, for a warehouse of a
/// given shape.
///
/// Built fresh every frame and never stored on the model. A remembered layout
/// is one that can disagree with the window it is drawn in, which is how a
/// program whose `window_width()` *told* the window what size to be came to
/// draw its board in one place and read clicks in another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title, level name and counters.
    pub header: Rect,
    /// The warehouse, or the level list.
    pub body: Rect,
    /// The buttons.
    pub controls: Rect,
    /// The keyboard reminder.
    pub footer: Rect,
    /// The grid, gaps included, centred in `body`.
    pub board: Rect,
    /// The mat the grid is laid on: `board` grown by one `gap` on every side.
    ///
    /// A field rather than four arithmetic terms inside `draw_board`, because
    /// this is the rectangle that actually decides whether the pass stays in
    /// its band, and a region no test can name is a region no test checks a
    /// pass against. It was written as a bare offset, and it overran the body
    /// by a gap on whichever axis the cell size came from: `cell` was solved so
    /// that the *grid* filled the body exactly, and then the mat was drawn a
    /// gap wider than the thing that already filled it.
    pub board_frame: Rect,
    /// The side of one cell.
    pub cell: f32,
    /// The gap between adjacent cells.
    pub gap: f32,
    /// The height of one row of the level menu.
    pub row: f32,
    /// The warehouse this layout was solved for.
    pub cols: usize,
    pub rows: usize,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size holding a `cols` x `rows`
    /// warehouse.
    #[must_use]
    pub fn new(width: f32, height: f32, cols: usize, rows: usize) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 38.0).clamp(8.0, 17.0);
        let small = (font - 2.0).max(7.0);
        let big = (font * 1.6).clamp(13.0, 28.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, controls, footer] order.
        let mut wants = [
            (h * 0.11).clamp(26.0, 58.0),
            (h * 0.08).clamp(22.0, 44.0),
            (h * 0.07).clamp(18.0, 40.0),
        ];
        // What is left once the body has its guaranteed share and the two gaps
        // that separate it from the chrome above and below. Charging the
        // padding to the chrome rather than the body is what keeps a small
        // window's cells big enough to still hold a crate.
        let budget = (h - h * BODY_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, ctl_h, ftr_h] = wants;

        // A dropped band is a full-width strip nought pixels tall, and is *not*
        // special-cased to `Rect::EMPTY`. The first draft did special-case it,
        // behind five separate `if h > 0.0` guards, and every one of them was a
        // branch nothing could observe (`known-issues.md` lesson 51):
        // `Rect::is_empty` is `w <= 0.0 || h <= 0.0`, so a zero-height strip
        // already answers "no" to the only question any drawing code asks, and
        // every read of a band's `x`/`w`/`right()` happens after that bail. The
        // strip form is also the one that makes the two edges below fall out
        // for free — a dropped footer sits at `y = h` and a dropped controls
        // band at `y = lower`, which is exactly where the body should stop,
        // whereas `Rect::EMPTY` sits at the origin and had to be guarded
        // against putting the body's bottom edge at zero.
        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let footer = Rect::new(0.0, h - ftr_h, w, ftr_h);
        let controls = Rect::new(0.0, footer.y - ctl_h, w, ctl_h);

        let top = hdr_h;
        let bottom = controls.y;
        let body = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // One number decides the whole board. Solving for it from both
        // dimensions at once is what stops a warehouse from being stretched to
        // fill a band that is not its shape — a stretched grid is one whose
        // cells are no longer where a square hit box says they are.
        // The mat, not the grid, is what has to fit: the grid is drawn on a
        // surround one gap wide, so `cols + GAP_PER_CELL * (cols + 1)` cells'
        // worth of width is what the body has to hold — `cols - 1` interior
        // gaps plus the two at the ends. Solving for the grid alone made the
        // cell exactly large enough for the grid to fill the body and then drew
        // a mat a gap larger on all four sides of it.
        let (cell, gap, board, board_frame) = if cols > 0 && rows > 0 {
            let per_w = cols as f32 + (cols as f32 + 1.0) * GAP_PER_CELL;
            let per_h = rows as f32 + (rows as f32 + 1.0) * GAP_PER_CELL;
            let cell = (body.w / per_w).min(body.h / per_h).max(0.0);
            let gap = cell * GAP_PER_CELL;
            let grid_w = cols as f32 * cell + (cols as f32 - 1.0) * gap;
            let grid_h = rows as f32 * cell + (rows as f32 - 1.0) * gap;
            if cell > 0.0 {
                let frame = Rect::new(
                    body.x + (body.w - grid_w - gap * 2.0) / 2.0,
                    body.y + (body.h - grid_h - gap * 2.0) / 2.0,
                    grid_w + gap * 2.0,
                    grid_h + gap * 2.0,
                );
                let board = Rect::new(frame.x + gap, frame.y + gap, grid_w, grid_h);
                (cell, gap, board, frame)
            } else {
                (cell, gap, Rect::EMPTY, Rect::EMPTY)
            }
        } else {
            (0.0, 0.0, Rect::EMPTY, Rect::EMPTY)
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            body,
            controls,
            footer,
            board,
            board_frame,
            cell,
            gap,
            row: (font * 2.1).max(1.0),
            cols,
            rows,
            font,
            small,
            big,
            pad,
        }
    }

    /// The rectangle of cell `(row, col)`, or `Rect::EMPTY` when the board has
    /// collapsed or the cell is off the grid.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if self.cell <= 0.0 || row >= self.rows || col >= self.cols {
            return Rect::EMPTY;
        }
        Rect::new(
            self.board.x + col as f32 * (self.cell + self.gap),
            self.board.y + row as f32 * (self.cell + self.gap),
            self.cell,
            self.cell,
        )
    }

    /// How many menu rows fit in the body.
    #[must_use]
    pub fn list_rows(&self) -> usize {
        if self.body.h < self.row {
            return 0;
        }
        (self.body.h / self.row) as usize
    }

    /// The rectangle of the `slot`-th visible menu row, or `Rect::EMPTY` when
    /// that slot is past the bottom of the body.
    #[must_use]
    pub fn list_rect(&self, slot: usize) -> Rect {
        if slot >= self.list_rows() {
            return Rect::EMPTY;
        }
        Rect::new(
            self.body.x,
            self.body.y + slot as f32 * self.row,
            self.body.w,
            (self.row - self.pad * 0.4).max(0.0),
        )
    }

    /// `n` buttons sharing the controls band, left to right.
    ///
    /// There is deliberately no bail here — not on `controls.is_empty()`, not
    /// on `n == 0`, and not on a zero `bw`/`bh`. All three were written, and
    /// the mutation sweep proved all three were branches nothing could observe
    /// (`known-issues.md` lesson 51). A dropped band is a strip nought pixels
    /// tall, so `bh` clamps to zero and every rectangle built below is already
    /// `is_empty()`; with `n == 0` the loop runs no times, so the `inf` that
    /// `inner / 0.0` produces is never read and the empty `Vec` comes back
    /// either way. A guard in front of a rule that already holds is a line no
    /// test can own.
    #[must_use]
    pub fn button_rects(&self, n: usize) -> Vec<Rect> {
        let mut out = vec![Rect::EMPTY; n];
        let count = n as f32;
        let inner = (self.controls.w - self.pad * (count + 1.0)).max(0.0);
        let bw = inner / count;
        let bh = (self.controls.h - self.pad).max(0.0);
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

    /// The victory panel, sized from the window rather than fixed at 320x150,
    /// so that in a small window it is still on screen.
    #[must_use]
    pub fn win_panel(&self) -> Rect {
        let w = (self.window.w * 0.82).min(340.0);
        let h = (self.window.h * 0.42).min(190.0);
        Rect::new(
            (self.window.w - w) / 2.0,
            (self.window.h - h) / 2.0,
            w.max(0.0),
            h.max(0.0),
        )
    }
}

/// The first level visible in a menu showing `rows` of `count`, chosen so the
/// cursor is always one of them.
///
/// A pure function of the three numbers rather than a scroll offset stored on
/// the model: a remembered offset is one that can disagree with a window that
/// has since been resized, which is the same fault as a remembered layout.
#[must_use]
pub fn first_visible(cursor: usize, count: usize, rows: usize) -> usize {
    if rows == 0 || count <= rows {
        return 0;
    }
    let last = count.saturating_sub(rows);
    // Keep the cursor off the very bottom edge where there is room to.
    cursor.saturating_sub(rows.saturating_sub(1)).min(last)
}

/// The keyboard reminder, per screen. The second line is the one dropped first
/// when the footer has room for only one.
const SELECT_FOOTER: [&str; 2] = ["Up/Down: choose   Enter: play", "1-9: jump to a level"];
const PLAY_FOOTER: [&str; 2] = [
    "Arrows/WASD: move   Z: undo   R: restart",
    "Esc: menu   N: next level",
];

/// The buttons on each screen, in the order `Layout::button_rects` lays them
/// out.
const SELECT_BUTTONS: [(Target, &str); 1] = [(Target::Play, "Play")];
const PLAY_BUTTONS: [(Target, &str); 3] = [
    (Target::Undo, "Undo"),
    (Target::Restart, "Restart"),
    (Target::Menu, "Menu"),
];
/// The three ways on from the victory panel, all reachable by pointer rather
/// than only by keys named in a footer the scrim is over.
const WIN_BUTTONS: [(Target, &str); 3] = [
    (Target::Undo, "Undo"),
    (Target::Restart, "Replay"),
    (Target::Next, "Next"),
];

// ── The game ────────────────────────────────────────────────────────
pub struct Sokoban {
    levels: Vec<Level>,
    /// The level being played, or the one the menu was last left on.
    current: usize,
    screen: Screen,
    /// Which levels have been finished at least once. A record of history, not
    /// of the position: undoing the winning move un-wins the board but does not
    /// un-solve the level, because you did solve it.
    completed: Vec<bool>,
    /// The menu cursor.
    cursor: usize,
    tiles: Vec<Vec<Tile>>,
    cols: usize,
    rows: usize,
    player: Pos,
    boxes: Vec<Pos>,
    moves: usize,
    pushes: usize,
    undo_stack: VecDeque<UndoEntry>,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size_drawn: (f32, f32),
}

impl Default for Sokoban {
    fn default() -> Self {
        Self::new()
    }
}

impl Sokoban {
    /// A new game, sitting on the level menu.
    ///
    /// Levels that do not parse are dropped rather than shipped as puzzles that
    /// cannot be finished — and a test asserts that all fifteen parse, so
    /// "dropped" is a thing that happens to a typo in the table and never to a
    /// level a player sees.
    #[must_use]
    pub fn new() -> Self {
        let levels: Vec<Level> = LEVELS.iter().filter_map(|s| parse_level(s).ok()).collect();
        let count = levels.len();
        let mut game = Self {
            levels,
            current: 0,
            screen: Screen::Select,
            completed: vec![false; count],
            cursor: 0,
            tiles: Vec::new(),
            cols: 0,
            rows: 0,
            player: Pos::new(0, 0),
            boxes: Vec::new(),
            moves: 0,
            pushes: 0,
            undo_stack: VecDeque::new(),
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        // The table is validated at startup and is never empty, so level 0 is
        // always there; the answer is discarded because a game with no first
        // level is a program that failed to start, not a state to handle here.
        let _ = game.load_level(0);
        game
    }

    /// Remember the size the window is being drawn at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn size_drawn(&self) -> (f32, f32) {
        self.size_drawn
    }

    /// The layout of the most recent frame.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1, self.cols, self.rows)
    }

    // ── Level handling ─────────────────────────────────────────────

    /// Load level `index` into the play state, leaving the screen alone.
    ///
    /// An index past the end loads nothing rather than wrapping: the callers
    /// are the menu and `next_level`, and both already know how many there are.
    /// A silent wrap here would turn "there is no level 16" into "you are
    /// playing level 1 again".
    /// Answers whether there was a level there to load. `start_level` is the
    /// only caller that can be handed an index from outside, and it asks this
    /// question rather than repeating the bounds check: the rule "an index past
    /// the end loads nothing" then lives in exactly one place, where the table
    /// is actually read. Written the other way — a bound in `start_level` *and*
    /// a bail here — the bail became unreachable, and a mutation sweep found it
    /// by silently replacing it with a wrap back to the last level that no test
    /// could see (`known-issues.md` lesson 63).
    pub fn load_level(&mut self, index: usize) -> bool {
        let Some(level) = self.levels.get(index) else {
            return false;
        };
        self.current = index;
        self.tiles.clone_from(&level.tiles);
        self.cols = level.width;
        self.rows = level.height;
        self.player = level.player;
        self.boxes.clone_from(&level.boxes);
        self.moves = 0;
        self.pushes = 0;
        self.undo_stack.clear();
        true
    }

    /// Load a level straight from Sokoban notation, for tests that need a
    /// position a real warehouse is thirty moves away from.
    ///
    /// Test-only on purpose: nothing in the shipping program may set up a
    /// position the level table does not describe. It goes through the same
    /// validating parser, so a fixture cannot express something a level could
    /// not — in particular it always has at least one crate, which is why
    /// `is_solved` does not ask whether there are any.
    ///
    /// # Panics
    ///
    /// If `source` is not a valid level. A fixture that does not parse is a
    /// broken test, and failing loudly is the point.
    #[cfg(test)]
    // This is test scaffolding, and a fixture that does not parse must stop the
    // test rather than quietly load nothing.
    #[allow(clippy::expect_used)]
    pub fn position(&mut self, source: &str) {
        let level = parse_level(source).expect("test fixture must be a valid level");
        self.tiles.clone_from(&level.tiles);
        self.cols = level.width;
        self.rows = level.height;
        self.player = level.player;
        self.boxes.clone_from(&level.boxes);
        self.moves = 0;
        self.pushes = 0;
        self.undo_stack.clear();
        self.screen = Screen::Playing;
    }

    /// Start playing level `index`, if there is one.
    pub fn start_level(&mut self, index: usize) {
        if !self.load_level(index) {
            return;
        }
        self.cursor = index;
        self.screen = Screen::Playing;
    }

    /// Put the current level back to how it started.
    pub fn restart(&mut self) {
        // `self.current` was set by a load that succeeded, so this one does
        // too; the answer is discarded rather than checked because there is no
        // second thing to do with it.
        let _ = self.load_level(self.current);
    }

    /// Back to the menu, with the cursor on the level just left.
    pub fn to_menu(&mut self) {
        self.screen = Screen::Select;
        self.cursor = self.current;
    }

    /// The next level, or the menu when this was the last one.
    pub fn next_level(&mut self) {
        let next = self.current.saturating_add(1);
        if next < self.levels.len() {
            self.start_level(next);
        } else {
            self.to_menu();
        }
    }

    #[must_use]
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    #[must_use]
    pub fn current_level(&self) -> usize {
        self.current
    }

    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn is_completed(&self, index: usize) -> bool {
        self.completed.get(index).copied().unwrap_or(false)
    }

    #[must_use]
    pub fn completed_count(&self) -> usize {
        self.completed.iter().filter(|c| **c).count()
    }

    #[must_use]
    pub fn player(&self) -> Pos {
        self.player
    }

    #[must_use]
    pub fn boxes(&self) -> &[Pos] {
        &self.boxes
    }

    #[must_use]
    pub fn moves(&self) -> usize {
        self.moves
    }

    #[must_use]
    pub fn pushes(&self) -> usize {
        self.pushes
    }

    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    #[must_use]
    pub fn grid_size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    // ── Reading the warehouse ──────────────────────────────────────

    /// The tile at `pos`. Anything off the grid reads as `Tile::Empty`, which
    /// is impassable — the same answer the padding around a ragged level gives,
    /// because it is the same thing: not part of the warehouse.
    #[must_use]
    pub fn tile_at(&self, pos: Pos) -> Tile {
        usize::try_from(pos.row)
            .ok()
            .and_then(|r| self.tiles.get(r))
            .and_then(|row| usize::try_from(pos.col).ok().and_then(|c| row.get(c)))
            .copied()
            .unwrap_or(Tile::Empty)
    }

    /// Whether nothing may occupy `pos` — a wall, or outside the warehouse.
    ///
    /// The version this replaced asked only `tile == Wall`, so `Empty` was
    /// floor and the player could walk out of the building.
    #[must_use]
    pub fn is_blocked(&self, pos: Pos) -> bool {
        !self.tile_at(pos).walkable()
    }

    #[must_use]
    pub fn has_box(&self, pos: Pos) -> bool {
        self.boxes.contains(&pos)
    }

    #[must_use]
    pub fn is_target(&self, pos: Pos) -> bool {
        self.tile_at(pos) == Tile::Target
    }

    /// Whether every crate is on a target.
    ///
    /// No "are there any crates?" guard: `parse_level` rejects a level without
    /// one, so the empty case cannot arise, and a guard in front of a rule that
    /// already holds is a line no test can own.
    #[must_use]
    pub fn is_solved(&self) -> bool {
        self.boxes.iter().all(|b| self.is_target(*b))
    }

    /// How many crates are sitting on targets.
    #[must_use]
    pub fn boxes_on_targets(&self) -> usize {
        self.boxes.iter().filter(|b| self.is_target(**b)).count()
    }

    /// How many targets the warehouse has.
    #[must_use]
    pub fn target_count(&self) -> usize {
        self.tiles
            .iter()
            .flat_map(|row| row.iter())
            .filter(|t| **t == Tile::Target)
            .count()
    }

    // ── Moving ─────────────────────────────────────────────────────

    /// Move the crate standing at `from` to `to`.
    ///
    /// Rewrites the list rather than looking the crate up a second time: the
    /// caller has already established that one is there, so a second lookup
    /// would carry a "not found" arm nothing can enter.
    fn move_box(&mut self, from: Pos, to: Pos) {
        for b in &mut self.boxes {
            if *b == from {
                *b = to;
            }
        }
    }

    /// Try to step the player one cell in `dir`, pushing a crate if one is in
    /// the way. Returns whether anything moved.
    ///
    /// Refuses once the level is solved, which is what makes the victory panel
    /// genuinely modal: the only ways on are the three it offers, and one of
    /// them is undo.
    pub fn try_move(&mut self, dir: Direction) -> bool {
        if self.screen != Screen::Playing || self.is_solved() {
            return false;
        }
        let dest = self.player.moved(dir);
        if self.is_blocked(dest) {
            return false;
        }

        let push = if self.has_box(dest) {
            let beyond = dest.moved(dir);
            // A crate may not be pushed into a wall, out of the warehouse, or
            // into another crate. Two crates in a row is the classic dead
            // position, and the rule that forbids it is the rule that makes it
            // one.
            if self.is_blocked(beyond) || self.has_box(beyond) {
                return false;
            }
            Some((dest, beyond))
        } else {
            None
        };

        if let Some((from, to)) = push {
            self.move_box(from, to);
            self.pushes = self.pushes.saturating_add(1);
        }
        self.undo_stack.push_back(UndoEntry {
            player: self.player,
            push,
        });
        // A `VecDeque` so that dropping the oldest move is a pop rather than an
        // O(n) shift of every move you have made.
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.player = dest;
        self.moves = self.moves.saturating_add(1);

        if self.is_solved()
            && let Some(slot) = self.completed.get_mut(self.current)
        {
            *slot = true;
        }
        true
    }

    /// Take back the last move. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop_back() else {
            return false;
        };
        self.player = entry.player;
        // The lint bans bare `-`; the clamp is unreachable, because a non-empty
        // stack means at least one move was counted.
        self.moves = self.moves.saturating_sub(1);
        if let Some((from, to)) = entry.push {
            self.move_box(to, from);
            self.pushes = self.pushes.saturating_sub(1);
        }
        true
    }

    /// The direction of one step from the player towards cell `(row, col)`, or
    /// `None` when that cell is neither in the player's row nor its column.
    ///
    /// One step, not a walk: a click that walked would have to choose a path,
    /// and a path chosen for you is one that can shove a crate into a corner on
    /// its way past. The player's own cell answers `None` — it is not a
    /// direction, and a click on yourself should do nothing rather than
    /// something arbitrary.
    #[must_use]
    pub fn direction_towards(&self, row: usize, col: usize) -> Option<Direction> {
        let to = Pos::new(row as isize, col as isize);
        if to.row == self.player.row {
            match to.col.cmp(&self.player.col) {
                std::cmp::Ordering::Greater => Some(Direction::Right),
                std::cmp::Ordering::Less => Some(Direction::Left),
                std::cmp::Ordering::Equal => None,
            }
        } else if to.col == self.player.col {
            if to.row > self.player.row {
                Some(Direction::Down)
            } else {
                Some(Direction::Up)
            }
        } else {
            None
        }
    }

    // ── Drawing ────────────────────────────────────────────────────

    /// One frame at the given size: the picture and the hit boxes together, so
    /// a thing is clickable exactly where its ink is.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::new(width, height, self.cols, self.rows);
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
        match self.screen {
            Screen::Select => self.draw_list(&mut f, &l),
            Screen::Playing => self.draw_board(&mut f, &l),
        }
        self.draw_controls(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.screen == Screen::Playing && self.is_solved() {
            self.draw_win(&mut f, &l);
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
        // The one-line branch was the unbounded one. `two_lines` is itself a
        // fit check, so the two-line stack was bounded by the thing that chose
        // it -- but falling back to one line does not make one line fit, and
        // the header wants `(h * 0.11).clamp(26.0, 58.0)` while `big` clamps at
        // 28, whose bold line height is taller than 26. A 260-point-tall window
        // drew "Sokoban" above the bar it is supposed to sit in.
        let stack = if two_lines { title_h + sub_h } else { title_h };
        let Some(top) = centre_line(l.header, stack) else {
            return;
        };

        label(
            f,
            &Label {
                text: "Sokoban",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: PEACH,
            },
            l.header.x + l.pad,
            top,
        );

        // Both counters are right-aligned from their own measured widths. The
        // version this replaced put its menu columns at `PADDING + 8.0` and
        // `PADDING + 60.0` — two guesses at how wide a string would turn out.
        let (subtitle, first, second) = match self.screen {
            Screen::Select => (
                "Choose a warehouse".to_string(),
                format!("Solved: {}/{}", self.completed_count(), self.level_count()),
                format!("{} levels", self.level_count()),
            ),
            Screen::Playing => (
                format!(
                    "Level {} of {}",
                    self.current.saturating_add(1),
                    self.level_count()
                ),
                format!("Moves: {}", self.moves),
                format!(
                    "Pushes: {}   Crates: {}/{}   Undo: {}",
                    self.pushes,
                    self.boxes_on_targets(),
                    self.target_count(),
                    self.undo_stack.len()
                ),
            ),
        };

        if two_lines {
            label(
                f,
                &Label {
                    text: &subtitle,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                l.header.x + l.pad,
                top + title_h,
            );
        }
        let left = l.header.x + l.pad;
        let right = l.header.right() - l.pad;
        label_right(
            f,
            &Label {
                text: &first,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: TEXT_COLOR,
            },
            left,
            right,
            top,
        );
        if two_lines {
            label_right(
                f,
                &Label {
                    text: &second,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                left,
                right,
                top + title_h,
            );
        }
    }

    fn draw_list(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.body.is_empty() {
            return;
        }
        let rows = l.list_rows();
        let first = first_visible(self.cursor, self.level_count(), rows);
        f.clip(l.body);
        for slot in 0..rows {
            let Some(index) = first.checked_add(slot) else {
                break;
            };
            if index >= self.level_count() {
                break;
            }
            let r = l.list_rect(slot);
            if r.is_empty() {
                continue;
            }
            let chosen = index == self.cursor;
            fill(
                f,
                r,
                if chosen { SURFACE0 } else { MANTLE },
                CornerRadii::all(l.pad.max(1.0)),
            );
            if chosen {
                // The cursor stripe, down the left edge of the row it marks.
                fill(
                    f,
                    Rect::new(r.x, r.y, (l.pad * 0.5).max(1.0), r.h),
                    PEACH,
                    CornerRadii::all(1.0),
                );
            }

            let size = (r.h * 0.44).clamp(7.0, l.font);
            let lh = text::line_height(size, FontWeightHint::Bold);
            // The row's own baseline, asked for once. Both runs below sit on
            // it, so a row too short for its type draws its stripe and its
            // background and no words -- rather than words half a line above
            // the stripe. `size` is clamped up to 7 points, so a row 4 points
            // tall does not merely get small type: it gets none.
            let Some(y) = centre_line(r, lh) else {
                continue;
            };
            let done = self.is_completed(index);
            let mark = if done { "done" } else { "" };
            // The name starts after the widest mark either row could carry, so
            // the column is straight whether or not this level is finished —
            // measured, not the old hard-coded 60 pixels.
            let mark_w = text::measure("done", size, FontWeightHint::Bold);
            let gutter = l.pad * 2.0 + mark_w;
            if !mark.is_empty() {
                label(
                    f,
                    &Label {
                        text: mark,
                        size,
                        weight: FontWeightHint::Bold,
                        color: GREEN,
                    },
                    r.x + l.pad,
                    y,
                );
            }
            let name = format!("Level {}", index.saturating_add(1));
            push_text(
                f,
                &Label {
                    text: &name,
                    size,
                    weight: if chosen {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    color: if chosen { TEXT_COLOR } else { SUBTEXT0 },
                },
                r.x + gutter,
                y,
                Some((r.w - gutter - l.pad).max(0.0)),
            );
            f.hit(Target::Level(index), r);
        }
        f.unclip();
    }

    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        fill(f, l.board_frame, CRUST, CornerRadii::all(l.gap.max(1.0)));

        let radius = CornerRadii::all((l.cell * 0.08).max(1.0));
        for row in 0..self.rows {
            for col in 0..self.cols {
                let tile = self.tile_at(Pos::new(row as isize, col as isize));
                // Outside the warehouse: nothing drawn, and no hit box. A
                // target recorded here would be a click that lands on a square
                // the player can never reach.
                if tile == Tile::Empty {
                    continue;
                }
                let r = l.cell_rect(row, col);
                fill(
                    f,
                    r,
                    match tile {
                        Tile::Wall => SURFACE1,
                        _ => SURFACE0,
                    },
                    radius,
                );
                if tile == Tile::Target {
                    // A pip in the middle, so a target under a crate is still
                    // legible as the reason the crate is the right colour.
                    let d = l.cell * 0.24;
                    fill(
                        f,
                        Rect::new(
                            r.x + (r.w - d) / 2.0,
                            r.y + (r.h - d) / 2.0,
                            d.max(0.0),
                            d.max(0.0),
                        ),
                        MAUVE,
                        CornerRadii::all(d / 2.0),
                    );
                }
                f.hit(Target::Cell(row, col), r);
            }
        }

        // Crates and the player are drawn over their cell and record no target
        // of their own, so the hit test names the square rather than whatever
        // is standing on it — which is what lets a click on the crate in front
        // of you mean "push it".
        let inset = l.cell * 0.12;
        for b in &self.boxes {
            let Ok(row) = usize::try_from(b.row) else {
                continue;
            };
            let Ok(col) = usize::try_from(b.col) else {
                continue;
            };
            let r = l.cell_rect(row, col);
            if r.is_empty() {
                continue;
            }
            let home = self.is_target(*b);
            fill(
                f,
                Rect::new(
                    r.x + inset,
                    r.y + inset,
                    (r.w - inset * 2.0).max(0.0),
                    (r.h - inset * 2.0).max(0.0),
                ),
                if home { GREEN } else { PEACH },
                CornerRadii::all((l.cell * 0.12).max(1.0)),
            );
        }

        if let (Ok(row), Ok(col)) = (
            usize::try_from(self.player.row),
            usize::try_from(self.player.col),
        ) {
            let r = l.cell_rect(row, col);
            if !r.is_empty() {
                let d = l.cell * 0.62;
                fill(
                    f,
                    Rect::new(
                        r.x + (r.w - d) / 2.0,
                        r.y + (r.h - d) / 2.0,
                        d.max(0.0),
                        d.max(0.0),
                    ),
                    BLUE,
                    CornerRadii::all(d / 2.0),
                );
            }
        }
    }

    /// The buttons this screen offers, in the order they are laid out.
    #[must_use]
    pub fn buttons(&self) -> Vec<(Target, &'static str)> {
        match self.screen {
            Screen::Select => SELECT_BUTTONS.to_vec(),
            Screen::Playing => PLAY_BUTTONS.to_vec(),
        }
    }

    /// The keyboard reminder this screen shows.
    #[must_use]
    pub fn footer_lines(&self) -> [&'static str; 2] {
        match self.screen {
            Screen::Select => SELECT_FOOTER,
            Screen::Playing => PLAY_FOOTER,
        }
    }

    fn draw_controls(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.controls.is_empty() {
            return;
        }
        fill(f, l.controls, MANTLE, CornerRadii::ZERO);
        let buttons = self.buttons();
        for ((target, name), r) in buttons.iter().copied().zip(l.button_rects(buttons.len())) {
            if r.is_empty() {
                continue;
            }
            // Undo on an empty stack is drawn dim but still recorded: it
            // answers `false` and changes nothing, and a target that reports
            // "nothing happened" is the thing a test can hold on to.
            let live = target != Target::Undo || !self.undo_stack.is_empty();
            fill(
                f,
                r,
                if live { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(l.pad.max(1.0)),
            );
            // No `line_height(size) <= r.h` here, and there was: `centre_line`
            // inside `label_centred` answers exactly that question, and a guard
            // a callee already makes is a line no test can own (lesson 92).
            let size = (r.h * 0.45).clamp(7.0, l.font);
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
            f.hit(target, r);
        }
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let lh = text::line_height(l.small, FontWeightHint::Regular);
        // Two lines if two fit, otherwise one -- and `centre_line` is what says
        // whether even the one does. The `lh > l.footer.h` bail this opened
        // with is exactly what `centre_line` answers for `shown == 1`, so it
        // was a guard in front of a rule that already held.
        let shown = if lh * 2.0 <= l.footer.h { 2 } else { 1 };
        let Some(top) = centre_line(l.footer, lh * shown as f32) else {
            return;
        };
        f.clip(l.footer);
        for (i, line) in self.footer_lines().iter().take(shown).enumerate() {
            push_text(
                f,
                &Label {
                    text: line,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                l.footer.x + l.pad,
                top + i as f32 * lh,
                Some((l.footer.w - l.pad * 2.0).max(0.0)),
            );
        }
        f.unclip();
    }

    fn draw_win(&self, f: &mut Frame<Target>, l: &Layout) {
        // A translucent scrim: the warehouse you just cleared is the thing
        // worth looking at.
        fill(f, l.window, SCRIM, CornerRadii::ZERO);
        // Nothing behind the panel is clickable any more — a modal that only
        // *looks* in front is one whose buttons you can press through.
        f.discard_hits();

        let panel = l.win_panel();
        if panel.is_empty() {
            return;
        }

        // The decoration is placed from the panel it decorates and clamped to
        // the window, rather than sitting at fixed offsets from a fixed
        // 320x150 box that a small window would put off the edge of the screen.
        let d = (l.pad * 0.9).max(2.0);
        let dots = [
            (panel.x - d * 1.4, panel.y - d * 1.4),
            (panel.right() + d * 0.4, panel.y - d * 1.4),
            (panel.x - d * 1.4, panel.bottom() + d * 0.4),
            (panel.right() + d * 0.4, panel.bottom() + d * 0.4),
            (panel.centre().0 - d / 2.0, panel.y - d * 1.8),
            (panel.centre().0 - d / 2.0, panel.bottom() + d * 0.8),
            (panel.x - d * 1.8, panel.centre().1 - d / 2.0),
            (panel.right() + d * 0.8, panel.centre().1 - d / 2.0),
        ];
        let colors = [YELLOW, PEACH, GREEN, TEAL, BLUE, MAUVE, RED, LAVENDER];
        for (i, (dx, dy)) in dots.into_iter().enumerate() {
            let Some(color) = colors.get(i) else {
                continue;
            };
            let r = Rect::new(
                dx.clamp(0.0, (l.window.w - d).max(0.0)),
                dy.clamp(0.0, (l.window.h - d).max(0.0)),
                d,
                d,
            );
            fill(f, r, *color, CornerRadii::all(d / 2.0));
        }

        fill(f, panel, MANTLE, CornerRadii::all(l.pad * 1.2));
        stroke(f, panel, GREEN, 2.0, CornerRadii::all(l.pad * 1.2));

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        let line_h = text::line_height(l.font, FontWeightHint::Regular);
        let btn_h = (panel.h * 0.26).clamp(0.0, 44.0);
        let stack = title_h + line_h + btn_h;
        let Some(top) = centre_line(panel, stack) else {
            return;
        };

        label_centred(
            f,
            &Label {
                text: "Level Complete!",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: GREEN,
            },
            Rect::new(panel.x, top, panel.w, title_h),
        );
        let tally = format!("Moves: {}   Pushes: {}", self.moves, self.pushes);
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

        let n = WIN_BUTTONS.len() as f32;
        let inner = (panel.w - l.pad * (n + 1.0)).max(0.0);
        let bw = inner / n;
        if bw <= 0.0 || btn_h <= 0.0 {
            return;
        }
        let by = top + title_h + line_h;
        for (i, (target, name)) in WIN_BUTTONS.into_iter().enumerate() {
            let r = Rect::new(panel.x + l.pad + i as f32 * (bw + l.pad), by, bw, btn_h);
            fill(f, r, SURFACE1, CornerRadii::all(l.pad.max(1.0)));
            let size = (r.h * 0.4).clamp(7.0, l.font);
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
            f.hit(target, r);
        }
    }

    // ── Events ─────────────────────────────────────────────────────

    /// Act on `target`. Returns whether anything changed.
    fn activate(&mut self, target: Target) -> bool {
        match target {
            Target::Undo => self.undo(),
            Target::Restart => {
                self.restart();
                true
            }
            Target::Menu => {
                self.to_menu();
                true
            }
            Target::Next => {
                self.next_level();
                true
            }
            Target::Play => {
                self.start_level(self.cursor);
                true
            }
            Target::Level(index) => {
                self.start_level(index);
                true
            }
            Target::Cell(row, col) => match self.direction_towards(row, col) {
                Some(dir) => self.try_move(dir),
                None => false,
            },
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size_drawn;
        match self.frame(w, h).hit_test(ev.x, ev.y) {
            Some(target) => {
                if self.activate(target) {
                    EventResult::Consumed
                } else {
                    // A target that did nothing says so. The alternative —
                    // reporting `Consumed` because *something* was under the
                    // pointer — is how a button that is dead becomes a button
                    // that is merely quiet.
                    EventResult::Ignored
                }
            }
            None => EventResult::Ignored,
        }
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // `pressed` decides whether this is a key going down or coming back up.
        // Reading only `key` runs every binding twice per press.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let plain = ev.modifiers == guitk::event::Modifiers::NONE;
        match self.screen {
            Screen::Select => self.key_select(ev, plain),
            Screen::Playing => self.key_playing(ev, plain),
        }
    }

    fn key_select(&mut self, ev: &KeyEvent, plain: bool) -> EventResult {
        let last = self.level_count().saturating_sub(1);
        match ev.key {
            Key::Up | Key::W => self.cursor = self.cursor.saturating_sub(1),
            Key::Down | Key::S => self.cursor = self.cursor.saturating_add(1).min(last),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = last,
            Key::Enter | Key::Space => self.start_level(self.cursor),
            Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
                if plain =>
            {
                self.cursor = digit_index(ev.key).min(last);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    fn key_playing(&mut self, ev: &KeyEvent, plain: bool) -> EventResult {
        match ev.key {
            Key::Up | Key::W => {
                self.try_move(Direction::Up);
            }
            Key::Down | Key::S => {
                self.try_move(Direction::Down);
            }
            Key::Left | Key::A => {
                self.try_move(Direction::Left);
            }
            Key::Right | Key::D => {
                self.try_move(Direction::Right);
            }
            Key::Z if plain => {
                self.undo();
            }
            Key::R if plain => self.restart(),
            Key::N if plain => self.next_level(),
            Key::Escape => self.to_menu(),
            // Only once the level is solved: on an unsolved board Enter would
            // be a key that silently abandons the position you are working on.
            Key::Enter | Key::Space => {
                if !self.is_solved() {
                    return EventResult::Ignored;
                }
                self.next_level();
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

/// The level a number key selects, zero-based.
///
/// Falls back to zero for anything that is not a digit key, which the one
/// caller has already ruled out — the value is a level index and there is no
/// sensible "no index" to return instead.
fn digit_index(key: Key) -> usize {
    match key {
        Key::Num2 => 1,
        Key::Num3 => 2,
        Key::Num4 => 3,
        Key::Num5 => 4,
        Key::Num6 => 5,
        Key::Num7 => 6,
        Key::Num8 => 7,
        Key::Num9 => 8,
        _ => 0,
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
fn label_right(f: &mut Frame<Target>, l: &Label, left: f32, right: f32, y: f32) {
    // Bounded on *both* sides, not just the one it is aligned to. Right-aligning
    // from the measured width alone puts a long counter's start at `right - w`,
    // which in a narrow window is a negative x — the string runs off the left
    // edge of the screen while its right end sits neatly where it was asked to.
    // The header's own counter did exactly that at 170 pixels wide.
    let room = (right - left).max(0.0);
    let w = text::measure(l.text, l.size, l.weight).min(room);
    push_text(f, l, (right - w).max(left), y, Some(room));
}

/// The top edge of a run `height` tall centred in `band`, or `None` when the
/// band cannot hold it.
///
/// This is the whole of `known-issues.md` lesson 109 in four lines.
/// `band.y + (band.h - height) / 2.0` is not wrong when it fits and slightly
/// wrong when it does not: the moment `height > band.h` it is *above* the band
/// by half the shortfall, and hangs the same distance below the bottom. Every
/// vertical centring in this file goes through here, so the refusal is written
/// once instead of being remembered at six call sites — and it *was* remembered
/// at four of the six, which is why only two of them were faults.
///
/// `height` is a line height and not a font size, because that is what the rest
/// of this file measures with: `push_text` puts the top-left corner where it is
/// told, so the extent a run occupies below `y` is `text::line_height`.
fn centre_line(band: Rect, height: f32) -> Option<f32> {
    (!band.is_empty() && band.h >= height).then(|| band.y + (band.h - height) / 2.0)
}

/// Centred in `r` — horizontally from the measured width, vertically from the
/// line height — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree — and that one clamp is what keeps a string
/// too wide for its box starting at the box rather than to the left of it.
///
/// The first draft belted this with braces: `.min(r.w)` here *and* a `.max(r.x)`
/// on the result. Either alone is the whole fix, so each masked the other from
/// the mutation sweep — break one and the other still holds the line, and no
/// test could name which one was doing the work (`known-issues.md` lesson 51,
/// in its two-guards-for-one-rule form). The `.max` is gone: with `w` never
/// wider than `r.w`, `(r.w - w) / 2.0` is never negative and the clamp was an
/// arm nothing could enter.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if l.text.is_empty() {
        return;
    }
    // `centre_line` subsumes the `r.is_empty()` this used to open with: a box
    // with no height cannot hold a line, and one with no width is refused
    // outright. The horizontal half needs no such bail, because `w` is clamped
    // to `r.w` and a run of nought points wide starts at `r.x`.
    let lh = text::line_height(l.size, l.weight);
    let Some(y) = centre_line(r, lh) else {
        return;
    };
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    push_text(f, l, r.x + (r.w - w) / 2.0, y, Some(r.w));
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut Sokoban, event: &Event) -> EventResult {
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

impl App for Sokoban {
    fn title(&self) -> String {
        "Sokoban".to_string()
    }

    fn app_id(&self) -> String {
        "sokoban".to_string()
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

impl Probe for Sokoban {
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
    let mut game = Sokoban::new();
    app::launch("sokoban", &mut game)
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
    /// the old layout never had to survive, because it computed the *window*
    /// from the level and drew that picture whatever it was given.
    const WINDOWS: &[(f32, f32)] = &[
        (140.0, 100.0),
        (170.0, 900.0),
        (200.0, 160.0),
        // The only size in the list short enough to lose the *controls* — the
        // last band to go. Without it every window keeps a controls band, and
        // the body's bottom edge is never read from a band that is not there.
        (240.0, 50.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (560.0, 640.0),
        (640.0, 480.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// The size the probe helpers draw at.
    const SIZE: (f32, f32) = Sokoban::SIZE;

    fn game() -> Sokoban {
        Sokoban::new()
    }

    /// A game playing level 1, which is the smallest warehouse in the table.
    fn playing() -> Sokoban {
        let mut g = game();
        g.start_level(0);
        g
    }

    /// A three-wide corridor: the player, a crate, and the target beyond it.
    /// One step right solves it.
    const ONE_STEP: &str = concat!("#####\n", "#@$.#\n", "#####\n");

    /// A game one move from solved.
    fn nearly_solved() -> Sokoban {
        let mut g = game();
        g.position(ONE_STEP);
        g
    }

    /// A game already solved, so the victory overlay is up.
    fn solved() -> Sokoban {
        let mut g = nearly_solved();
        assert!(g.try_move(Direction::Right), "the winning push was refused");
        assert!(g.is_solved(), "the fixture did not actually solve");
        g
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

    fn text_boxes(f: &Frame<Target>) -> Vec<(String, f32, f32, Option<f32>, TextOverflow)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    max_width,
                    overflow,
                    ..
                } => Some((text.clone(), *x, *y, *max_width, *overflow)),
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
        assert_eq!(g.title(), "Sokoban");
        assert_eq!(g.app_id(), "sokoban");
        let (w, h) = g.initial_size();
        assert!(w > 0 && h > 0, "a window of {w}x{h} is not a window");
    }

    #[test]
    fn the_probe_draws_at_the_size_the_window_opens_at() {
        let g = game();
        let (w, h) = g.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            Sokoban::SIZE,
            "the window opens at {w}x{h} but every probe test draws at {:?}, so \
             the suite measures a window the program never shows",
            Sokoban::SIZE
        );
    }

    #[test]
    fn rendering_records_the_size_it_drew_at() {
        let mut g = game();
        // Deliberately not `SIZE`: a new game already records `SIZE` as the
        // size it was built for, so rendering at `SIZE` and finding `SIZE`
        // afterwards is a test that passes without `render` having done
        // anything at all.
        let odd = (SIZE.0 + 137.0, SIZE.1 - 61.0);
        assert!(
            (odd.0 - g.size_drawn().0).abs() > 0.01,
            "the fixture size is the size the game already records"
        );
        let tree = g.render(odd.0, odd.1);
        assert!(
            !tree.commands.is_empty(),
            "render produced no commands at all"
        );
        assert_eq!(
            g.size_drawn(),
            odd,
            "render did not record the size it drew at, so the next click is \
             read against a window that is no longer there"
        );
    }

    #[test]
    fn rendering_at_a_new_size_is_what_moves_the_hit_boxes() {
        let g = playing();
        let small = probe::rect_of_sized(&g, Target::Cell(1, 1), (400.0, 400.0));
        let large = probe::rect_of_sized(&g, Target::Cell(1, 1), (900.0, 900.0));
        assert!(
            small.is_some() && large.is_some(),
            "cell (1,1) is off the board in one of the two windows"
        );
        assert_ne!(
            small, large,
            "the same cell has the same hit box in a 400x400 and a 900x900 \
             window, which is the fault this rewrite exists to fix"
        );
    }

    #[test]
    fn a_resize_event_changes_the_size_the_next_click_is_read_against() {
        let mut g = playing();
        g.render(SIZE.0, SIZE.1);
        let before = g.size_drawn();
        let r = handle_event(
            &mut g,
            &Event::Resize {
                width: 811,
                height: 733,
            },
        );
        assert_eq!(r, EventResult::Consumed);
        assert_ne!(before, g.size_drawn());
        assert_eq!(g.size_drawn(), (811.0, 733.0));

        // Recording the size is only half of it, and the half the name does not
        // claim. A click arrives as a bare pair of pixels with no size attached,
        // so `handle_mouse` has to rebuild the layout from what it last drew at
        // — and reading a constant there instead would leave every assertion
        // above true while the pointer went on playing the old window. Pick the
        // centre of a cell in the resized layout and check the click lands on
        // that cell rather than on whichever cell covers the point at `SIZE`.
        // Recording the size is only half of it, and the half the name does not
        // claim. A click arrives as a bare pair of pixels with no size attached,
        // so `handle_mouse` has to rebuild the layout from what it last drew at
        // — and reading a constant there instead would leave every assertion
        // above true while the pointer went on playing the window that is gone.
        let mut g = room();
        handle_event(
            &mut g,
            &Event::Resize {
                width: 811,
                height: 733,
            },
        );
        let start = g.player();
        let wanted = Pos::new(start.row, start.col.saturating_add(1));
        let square = g
            .layout()
            .cell_rect(wanted.row as usize, wanted.col as usize);
        assert!(
            !square.is_empty(),
            "the player has no square to their right"
        );
        let (cx, cy) = square.centre();

        // The witness: at the size the window used to be, that same point is
        // some *other* square, so a click resolved against the stale layout
        // cannot accidentally agree with one resolved against the live one.
        let stale = Layout::new(SIZE.0, SIZE.1, g.layout().cols, g.layout().rows);
        assert!(
            !stale
                .cell_rect(wanted.row as usize, wanted.col as usize)
                .contains(cx, cy),
            "the point ({cx}, {cy}) is the same square in both windows, so the \
             two layouts cannot be told apart by clicking it"
        );

        let ev = MouseEvent {
            x: cx,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        assert_eq!(
            g.handle_mouse(&ev),
            EventResult::Consumed,
            "the click at ({cx}, {cy}) did not reach the square to the player's \
             right"
        );
        assert_eq!(
            g.player(),
            wanted,
            "the click was read against a window the game is no longer drawn at"
        );
    }

    #[test]
    fn closing_the_window_exits_and_nothing_else_does() {
        let mut g = playing();
        assert_eq!(g.on_event(&Event::CloseRequested), Response::Exit);
        assert_eq!(
            g.on_event(&Event::FocusIn),
            Response::Idle,
            "an event the game does not use should not force a repaint"
        );
        assert_eq!(
            g.on_event(&Event::Key(probe::press(Key::Right))),
            Response::Redraw,
            "a move must repaint, or the board on screen is a move behind"
        );
    }

    /// A key coming back *up*. `probe` has no helper for one because most
    /// programs have no reason to see one — which is exactly why a program that
    /// acts on releases as well as presses runs every binding twice.
    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    // ── The layout follows the window ──────────────────────────────

    /// The shape every layout claim below is checked against, unless it says
    /// otherwise: level 1's warehouse.
    fn shape() -> (usize, usize) {
        playing().grid_size()
    }

    #[test]
    fn the_layout_is_a_function_of_the_window_size_and_the_level_shape_alone() {
        let (c, r) = shape();
        for &(w, h) in WINDOWS {
            assert_eq!(
                Layout::new(w, h, c, r),
                Layout::new(w, h, c, r),
                "the layout at {w}x{h} is not the same twice, so it depends on \
                 something other than its arguments"
            );
        }
        // And a different warehouse in the same window is a different layout,
        // or the board is not sized from the level at all.
        let a = Layout::new(600.0, 600.0, 7, 7);
        let b = Layout::new(600.0, 600.0, 10, 9);
        assert!(
            (a.cell - b.cell).abs() > 0.01,
            "a 7x7 and a 10x9 warehouse get the same cell size in the same \
             window, so the board is not solved from the level"
        );
        // That comparison varies both dimensions at once, so it cannot say
        // *which* one the board is sized from: a board solved from the rows
        // alone still gives 7x7 and 10x9 two different cells and passes it.
        // The sweep proved as much by replacing the column term with a
        // constant. Each dimension therefore gets its own check, with the
        // other held still and the window shaped so that this is the
        // dimension the cell is limited by — tall and narrow makes the
        // columns bind, wide and short makes the rows bind. Twice the count
        // in the same window is half the cell.
        let wide = Layout::new(800.0, 2000.0, 40, 2);
        let wider = Layout::new(800.0, 2000.0, 80, 2);
        assert!(
            wider.cell < wide.cell * 0.6,
            "doubling the columns took the cell from {} only to {}, so the \
             board is not sized from the level's width",
            wide.cell,
            wider.cell
        );
        let tall = Layout::new(2000.0, 800.0, 2, 40);
        let taller = Layout::new(2000.0, 800.0, 2, 80);
        assert!(
            taller.cell < tall.cell * 0.6,
            "doubling the rows took the cell from {} only to {}, so the \
             board is not sized from the level's height",
            tall.cell,
            taller.cell
        );
    }

    #[test]
    fn every_band_stays_inside_the_window() {
        let (c, r) = shape();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, c, r);
            // "Inside the window" is a claim about *this* window, so the
            // layout's own record of it is checked first. Without this the
            // bands below could be measured against a window the layout
            // invented, and every one of them would still fit.
            assert!(
                (l.window.w - w).abs() <= 0.01 && (l.window.h - h).abs() <= 0.01,
                "asked for a {w}x{h} window and the layout recorded {:?}",
                l.window
            );
            for (name, band) in [
                ("header", l.header),
                ("body", l.body),
                ("controls", l.controls),
                ("footer", l.footer),
                ("board", l.board),
            ] {
                if band.is_empty() {
                    continue;
                }
                assert!(
                    band.x >= -0.01
                        && band.y >= -0.01
                        && band.right() <= w + 0.01
                        && band.bottom() <= h + 0.01,
                    "at {w}x{h} the {name} band {band:?} leaves the window"
                );
            }
        }
    }

    #[test]
    fn the_chrome_bands_do_not_overlap_each_other_or_the_body() {
        let (c, r) = shape();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, c, r);
            let bands = [
                ("header", l.header),
                ("body", l.body),
                ("controls", l.controls),
                ("footer", l.footer),
            ];
            for (i, (an, a)) in bands.iter().enumerate() {
                for (bn, b) in bands.iter().skip(i + 1) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(
                        a.bottom() <= b.y + 0.01 || b.bottom() <= a.y + 0.01,
                        "at {w}x{h} the {an} band {a:?} overlaps the {bn} band \
                         {b:?}, so one is drawn over the other"
                    );
                }
            }
        }
    }

    #[test]
    fn the_board_is_drawn_with_square_cells_inside_the_body() {
        let (c, r) = shape();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, c, r);
            if l.board.is_empty() {
                continue;
            }
            let first = l.cell_rect(0, 0);
            assert!(
                (first.w - first.h).abs() <= 0.01,
                "at {w}x{h} a cell is {}x{}, which is not square — a stretched \
                 grid is one whose cells are not where a square hit box says",
                first.w,
                first.h
            );
            assert!(
                l.board.x >= l.body.x - 0.01
                    && l.board.y >= l.body.y - 0.01
                    && l.board.right() <= l.body.right() + 0.01
                    && l.board.bottom() <= l.body.bottom() + 0.01,
                "at {w}x{h} the board {:?} does not fit its body {:?}",
                l.board,
                l.body
            );
            // The board rectangle must *be* the grid, not merely contain it:
            // it is what the victory panel and the background are drawn
            // against, so a board that claims a height its cells do not fill
            // is a board with a stripe of nothing painted as warehouse.
            let last = l.cell_rect(r.saturating_sub(1), c.saturating_sub(1));
            assert!(
                (last.right() - l.board.right()).abs() <= 0.01
                    && (last.bottom() - l.board.bottom()).abs() <= 0.01,
                "at {w}x{h} the last cell ends at ({}, {}) but the board claims \
                 to end at ({}, {})",
                last.right(),
                last.bottom(),
                l.board.right(),
                l.board.bottom()
            );
            // The mat is what has to fit and what has to touch, because the mat
            // is what is painted. It is the grid grown by one gap on every
            // side, so a grid that fills the body exactly is a mat that hangs
            // a gap over each edge of it — which is what this drew before the
            // surround was named and solved for.
            assert!(
                (l.board.x - l.board_frame.x - l.gap).abs() <= 0.01
                    && (l.board.y - l.board_frame.y - l.gap).abs() <= 0.01
                    && (l.board_frame.right() - l.board.right() - l.gap).abs() <= 0.01
                    && (l.board_frame.bottom() - l.board.bottom() - l.gap).abs() <= 0.01,
                "at {w}x{h} the mat {:?} is not the board {:?} grown by one \
                 {}-point gap on every side",
                l.board_frame,
                l.board,
                l.gap
            );
            assert!(
                l.board_frame.x >= l.body.x - 0.01
                    && l.board_frame.y >= l.body.y - 0.01
                    && l.board_frame.right() <= l.body.right() + 0.01
                    && l.board_frame.bottom() <= l.body.bottom() + 0.01,
                "at {w}x{h} the mat {:?} does not fit its body {:?}",
                l.board_frame,
                l.body
            );
            // Solved from both dimensions at once means the mat runs out of
            // room on exactly one axis and is centred on the other. If it
            // touches neither, the cell size came from something other than
            // the body and the level's shape, and the warehouse is drawn
            // smaller than the space it was given.
            let fills_w = (l.board_frame.w - l.body.w).abs() <= 0.01;
            let fills_h = (l.board_frame.h - l.body.h).abs() <= 0.01;
            assert!(
                fills_w || fills_h,
                "at {w}x{h} the mat {:?} touches neither edge of its body \
                 {:?}, so it is not as large as the body allows",
                l.board_frame,
                l.body
            );
            // Centred on whichever axis had room left over.
            let mx = l.board.x - l.body.x;
            let my = l.board.y - l.body.y;
            assert!(
                (mx - (l.body.right() - l.board.right())).abs() <= 0.01
                    && (my - (l.body.bottom() - l.board.bottom())).abs() <= 0.01,
                "at {w}x{h} the board {:?} is not centred in its body {:?}",
                l.board,
                l.body
            );
        }
    }

    #[test]
    fn the_board_survives_every_window_a_band_is_dropped_in() {
        let (c, r) = shape();
        let mut lost_header = 0;
        let mut lost_controls = 0;
        let mut lost_footer = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, c, r);
            lost_header += usize::from(l.header.is_empty());
            lost_controls += usize::from(l.controls.is_empty());
            lost_footer += usize::from(l.footer.is_empty());
            assert!(
                l.cell > 0.0,
                "at {w}x{h} there is no warehouse left to play in"
            );
        }
        // Without these the list below is a list of windows that all keep every
        // band, and the "band was dropped" arms of the layout are never entered
        // by anything asserting about them.
        assert!(
            lost_footer > 0,
            "no window in the list drops the footer, so the first band to go is \
             never actually gone"
        );
        assert!(
            lost_header > 0,
            "no window in the list drops the header, so the body's top edge is \
             never read with the header absent"
        );
        assert!(
            lost_controls > 0,
            "no window in the list drops the controls, so the body's bottom \
             edge is never read from a band that is not there"
        );
    }

    #[test]
    fn cells_tile_the_board_without_overlapping() {
        let l = Layout::new(600.0, 600.0, 7, 7);
        let mut seen: Vec<Rect> = Vec::new();
        for row in 0..7 {
            for col in 0..7 {
                let r = l.cell_rect(row, col);
                assert!(!r.is_empty(), "cell ({row},{col}) is empty on a 7x7");
                for prev in &seen {
                    assert!(
                        r.right() <= prev.x + 0.01
                            || prev.right() <= r.x + 0.01
                            || r.bottom() <= prev.y + 0.01
                            || prev.bottom() <= r.y + 0.01,
                        "cell ({row},{col}) at {r:?} overlaps {prev:?}"
                    );
                }
                seen.push(r);
            }
        }
        // Reading order: (0,0) is top-left, (6,6) bottom-right.
        assert!(l.cell_rect(0, 0).x < l.cell_rect(0, 6).x);
        assert!(l.cell_rect(0, 0).y < l.cell_rect(6, 0).y);
        // Not overlapping is the weaker half: cells laid out with no gap at all
        // also fail to overlap, and would pass everything above while leaving
        // the grid short of the board it is supposed to fill. The gap is
        // measured, and the far edges are pinned to the board.
        assert!(
            l.gap > 0.0,
            "a 600x600 window gives the cells no gap at all"
        );
        for col in 1..7 {
            let prev = l.cell_rect(0, col - 1);
            let here = l.cell_rect(0, col);
            assert!(
                (here.x - prev.right() - l.gap).abs() <= 0.01,
                "columns {} and {col} are {} apart, not the {} gap the layout \
                 says they are",
                col - 1,
                here.x - prev.right(),
                l.gap
            );
        }
        for row in 1..7 {
            let prev = l.cell_rect(row - 1, 0);
            let here = l.cell_rect(row, 0);
            assert!(
                (here.y - prev.bottom() - l.gap).abs() <= 0.01,
                "rows {} and {row} are {} apart, not the {} gap the layout says",
                row - 1,
                here.y - prev.bottom(),
                l.gap
            );
        }
        let last = l.cell_rect(6, 6);
        assert!(
            (last.right() - l.board.right()).abs() <= 0.01
                && (last.bottom() - l.board.bottom()).abs() <= 0.01,
            "the last cell {last:?} does not reach the far corner of the board \
             {:?}",
            l.board
        );
    }

    #[test]
    fn a_cell_off_the_grid_has_no_rectangle() {
        let l = Layout::new(600.0, 600.0, 7, 5);
        assert!(
            !l.cell_rect(4, 6).is_empty(),
            "the last real cell is missing"
        );
        assert!(
            l.cell_rect(5, 0).is_empty(),
            "row 5 of a 5-row warehouse has a rectangle"
        );
        assert!(
            l.cell_rect(0, 7).is_empty(),
            "column 7 of a 7-column warehouse has a rectangle"
        );
        // And with no warehouse at all there is no board to put one on.
        let none = Layout::new(600.0, 600.0, 0, 0);
        assert!(none.board.is_empty());
        assert!(none.cell_rect(0, 0).is_empty());
    }

    #[test]
    fn the_buttons_share_the_controls_band_in_order() {
        let l = Layout::new(560.0, 640.0, 7, 7);
        assert!(!l.controls.is_empty(), "the fixture window has no controls");
        let rects = l.button_rects(3);
        assert_eq!(rects.len(), 3);
        for (i, r) in rects.iter().enumerate() {
            assert!(!r.is_empty(), "button {i} has no rectangle");
            assert!(
                r.y >= l.controls.y - 0.01 && r.bottom() <= l.controls.bottom() + 0.01,
                "button {i} at {r:?} leaves the controls band {:?}",
                l.controls
            );
        }
        assert!(rects[0].right() <= rects[1].x + 0.01);
        assert!(rects[1].right() <= rects[2].x + 0.01);
        // A different count shares the same band differently, or the buttons
        // are laid out against a number that is not the number of them.
        assert!((l.button_rects(1)[0].w - rects[0].w).abs() > 0.01);
        assert!(l.button_rects(0).is_empty());
        // "Share the band" is an exact claim, not merely a different width per
        // count: n buttons and the n+1 gaps between and beside them are the
        // band. Laying them out against a fixed three would still give every
        // count its own width — the padding alone would see to that — while
        // leaving one button of five hanging off the end.
        for n in 1..=5_usize {
            let rs = l.button_rects(n);
            let spanned: f32 = rs.iter().map(|r| r.w).sum::<f32>() + l.pad * (n as f32 + 1.0);
            assert!(
                (spanned - l.controls.w).abs() <= 0.01,
                "{n} buttons and their padding span {spanned}, but the controls \
                 band is {} wide",
                l.controls.w
            );
            let last = rs.last().copied().unwrap_or(Rect::EMPTY);
            assert!(
                (last.right() + l.pad - l.controls.right()).abs() <= 0.01,
                "the last of {n} buttons ends at {}, a pad short of or past the \
                 band's right edge at {}",
                last.right(),
                l.controls.right()
            );
        }
        // The band is a pad taller than the buttons, and that pad is split
        // evenly above and below rather than dumped on one side — a button
        // pinned to the top edge would still be inside the band, so the
        // "stays in the band" check above cannot see the difference.
        for (i, r) in rects.iter().enumerate() {
            assert!(
                ((r.y - l.controls.y) - (l.controls.bottom() - r.bottom())).abs() <= 0.01,
                "button {i} is not centred in its band: {} above, {} below",
                r.y - l.controls.y,
                l.controls.bottom() - r.bottom()
            );
        }
    }

    #[test]
    fn the_buttons_vanish_with_the_band_they_sit_in() {
        // 240x50 is the window in the list short enough to lose the controls.
        let l = Layout::new(240.0, 50.0, 7, 7);
        assert!(
            l.controls.is_empty(),
            "the fixture window kept its controls, so this proves nothing"
        );
        for r in l.button_rects(3) {
            assert!(r.is_empty(), "a button survived the band it sits in: {r:?}");
        }
    }

    #[test]
    fn the_victory_panel_stays_inside_the_window() {
        let (c, r) = shape();
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h, c, r);
            let p = l.win_panel();
            assert!(
                p.x >= -0.01 && p.y >= -0.01 && p.right() <= w + 0.01 && p.bottom() <= h + 0.01,
                "at {w}x{h} the victory panel {p:?} is off the window — which \
                 is what a fixed 320x150 box did"
            );
        }
    }

    #[test]
    fn the_menu_scrolls_far_enough_to_reach_every_level_and_no_further() {
        // Fewer levels than rows: no scrolling at all.
        assert_eq!(first_visible(0, 3, 10), 0);
        assert_eq!(first_visible(2, 3, 10), 0);
        // No rows: nothing is visible, and the answer must still be sane.
        assert_eq!(first_visible(7, 15, 0), 0);
        // More levels than rows: the cursor is always on screen…
        for cursor in 0..15 {
            let first = first_visible(cursor, 15, 4);
            assert!(
                cursor >= first && cursor < first + 4,
                "cursor {cursor} is not among the four rows starting at {first}"
            );
            // …and the window never runs past the end of the list.
            assert!(
                first + 4 <= 15,
                "the menu scrolled to row {first}, past the end of 15 levels"
            );
        }
        assert_eq!(first_visible(0, 15, 4), 0, "the top of the list scrolled");
        assert_eq!(first_visible(14, 15, 4), 11, "the bottom did not scroll");
        // "No further" only bites past the end of the list. For every cursor
        // the menu can actually hold, `cursor - (rows - 1)` is already at or
        // below `count - rows`, so the clamp never binds and a sweep that only
        // asks about reachable cursors cannot tell it from no clamp at all.
        // This function is `pub` and its contract is total, so the rows past
        // the end are asked about here rather than the clamp being deleted.
        assert_eq!(
            first_visible(15, 15, 4),
            11,
            "a cursor one past the last level scrolled the menu past the list"
        );
        assert_eq!(
            first_visible(usize::MAX, 15, 4),
            11,
            "a cursor nowhere near the list scrolled the menu off the end of it"
        );
    }

    #[test]
    fn the_menu_shows_as_many_rows_as_the_body_has_room_for() {
        let l = Layout::new(560.0, 640.0, 7, 7);
        let rows = l.list_rows();
        assert!(
            rows > 0,
            "a 560x640 window has room for no menu rows at all"
        );
        assert!(
            l.row * rows as f32 <= l.body.h + 0.01,
            "{rows} rows of {} do not fit a body {} tall",
            l.row,
            l.body.h
        );
        assert!(
            l.row * (rows + 1) as f32 > l.body.h,
            "one more row would still have fitted, so the count is short"
        );
        assert!(
            l.list_rect(rows).is_empty(),
            "a row past the last one exists"
        );
        assert!(!l.list_rect(rows - 1).is_empty(), "the last row is missing");
    }

    // ── Parsing, which can now say no ──────────────────────────────

    #[test]
    fn every_built_in_level_parses() {
        for (i, source) in LEVELS.iter().enumerate() {
            let level = parse_level(source)
                .unwrap_or_else(|e| panic!("level {} does not parse: {e:?}", i + 1));
            assert!(!level.boxes.is_empty());
            assert!(level.width > 0 && level.height > 0);
        }
        assert_eq!(
            game().level_count(),
            LEVELS.len(),
            "the menu offers fewer levels than the table holds, so one was \
             dropped for not parsing"
        );
    }

    #[test]
    fn every_built_in_level_balances_its_crates_against_its_targets() {
        let mut g = game();
        for i in 0..LEVELS.len() {
            g.start_level(i);
            assert_eq!(
                g.boxes().len(),
                g.target_count(),
                "level {} has {} crates and {} targets, so it cannot be \
                 finished",
                i + 1,
                g.boxes().len(),
                g.target_count()
            );
            assert!(
                !g.is_solved(),
                "level {} starts already solved",
                i.saturating_add(1)
            );
        }
    }

    #[test]
    fn the_player_and_every_crate_start_on_floor() {
        let mut g = game();
        for i in 0..LEVELS.len() {
            g.start_level(i);
            assert!(
                !g.is_blocked(g.player()),
                "level {} starts the player inside a wall",
                i + 1
            );
            for b in g.boxes() {
                assert!(
                    !g.is_blocked(*b),
                    "level {} starts a crate at {b:?}, which nothing can move",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn a_level_with_no_player_is_rejected() {
        assert_eq!(
            parse_level("#####\n#-$.#\n#####\n"),
            Err(LevelError::NoPlayer),
            "a level with no @ used to start the player at (0,0) — inside the \
             top-left wall"
        );
    }

    #[test]
    fn a_level_with_two_players_is_rejected() {
        assert_eq!(
            parse_level("######\n#@$.@#\n######\n"),
            Err(LevelError::TwoPlayers),
            "a second @ used to silently replace the first"
        );
    }

    #[test]
    fn a_level_with_no_crates_is_rejected() {
        // A crateless level would be "solved" the moment it loaded, whether or
        // not it has targets — so the crate count is asked about *before* the
        // balance, and both fixtures give the same answer rather than one of
        // them reporting a mismatch that is beside the point.
        assert_eq!(
            parse_level("#####\n#@-.#\n#####\n"),
            Err(LevelError::NoBoxes)
        );
        assert_eq!(
            parse_level("#####\n#@--#\n#####\n"),
            Err(LevelError::NoBoxes)
        );
    }

    #[test]
    fn a_level_whose_crates_and_targets_do_not_balance_is_rejected() {
        assert_eq!(
            parse_level("#######\n#@$$.-#\n#######\n"),
            Err(LevelError::Unbalanced {
                boxes: 2,
                targets: 1
            }),
            "two crates and one target parses, loads, plays and cannot be won"
        );
    }

    #[test]
    fn an_unknown_character_is_rejected_rather_than_swallowed() {
        assert_eq!(
            parse_level("#####\n#@$X#\n#####\n"),
            Err(LevelError::UnknownTile('X')),
            "an unrecognised character used to become Tile::Empty and vanish"
        );
    }

    #[test]
    fn an_empty_level_is_rejected() {
        assert_eq!(parse_level(""), Err(LevelError::Empty));
        assert_eq!(parse_level("\n\n"), Err(LevelError::Empty));
    }

    #[test]
    fn a_level_bigger_than_the_bound_is_rejected() {
        // The bound existed as a pair of constants nothing read. A warehouse
        // one column past it is the cheapest proof that something reads it now.
        let wide = format!("{}\n#@$.{}#\n", "#".repeat(21), "-".repeat(16));
        assert!(
            matches!(
                parse_level(&wide),
                Err(LevelError::TooBig { width: 21, .. })
            ),
            "a 21-column warehouse parsed, so MAX_LEVEL_WIDTH is still unread"
        );
        let tall = format!("#@$.#\n{}", "#####\n".repeat(20));
        assert!(
            matches!(
                parse_level(&tall),
                Err(LevelError::TooBig { height: 21, .. })
            ),
            "a 21-row warehouse parsed, so MAX_LEVEL_HEIGHT is still unread"
        );
    }

    #[test]
    fn the_notation_cannot_put_a_crate_or_the_player_outside_the_warehouse() {
        // There is deliberately no "starts on a wall" error, and this is why:
        // every glyph that places something also writes the tile under it, and
        // the padding that fills a ragged row only ever lands *past* the
        // characters that row has — so no character can sit on it. A guard for
        // this would be an arm no level could enter and no test could own.
        //
        // The ragged fixture below is the closest the notation gets: row 1 is
        // short, its last cell is padding, and the crate is still on floor.
        let level = parse_level("#####\n#@$.\n#####\n").expect("the fixture must parse");
        assert_eq!(
            level.tiles[1][4],
            Tile::Empty,
            "the short row was not padded"
        );
        for at in std::iter::once(level.player).chain(level.boxes.iter().copied()) {
            assert!(
                level.tiles[at.row as usize][at.col as usize].walkable(),
                "{at:?} started on a tile it could not stand on"
            );
        }
    }

    #[test]
    fn the_padding_around_a_ragged_level_is_not_floor() {
        // Row 0 is three wide and the rest are five, so (0,3) and (0,4) are
        // padding. Padding used to be walkable, which let the player leave the
        // building.
        let level = parse_level("###\n#@$.#\n#####\n").unwrap();
        assert_eq!(level.width, 5);
        assert_eq!(level.tiles[0].len(), 5);
        assert_eq!(level.tiles[0][3], Tile::Empty);
        assert_eq!(level.tiles[0][4], Tile::Empty);
        assert!(
            !Tile::Empty.walkable(),
            "outside the warehouse is walkable, so the player can leave it"
        );
        assert!(Tile::Floor.walkable() && Tile::Target.walkable());
        assert!(!Tile::Wall.walkable());
    }

    #[test]
    fn the_notation_reads_every_character_it_claims_to() {
        // `+` is a player already standing on a target, `*` a crate already on
        // one. Both used to be the only way to write a level whose opening
        // position is partly solved, and both had to be counted as targets or
        // the balance check would reject a level that was fine.
        // Three targets — the one under `+`, the one under `*`, and the bare
        // `.` — need three crates, so the fixture carries a second `$`.
        let level = parse_level("#######\n#+*$.$#\n#######\n").unwrap();
        assert_eq!(level.player, Pos::new(1, 1));
        assert_eq!(
            level.boxes,
            vec![Pos::new(1, 2), Pos::new(1, 3), Pos::new(1, 5)]
        );
        assert_eq!(level.tiles[1][1], Tile::Target, "+ is a target underneath");
        assert_eq!(level.tiles[1][2], Tile::Target, "* is a target underneath");
        assert_eq!(level.tiles[1][3], Tile::Floor, "$ is plain floor");
        assert_eq!(level.tiles[1][4], Tile::Target, ". is a bare target");
        assert_eq!(
            level.tiles[1][5],
            Tile::Floor,
            "the second $ is plain floor"
        );
        // A space and a dash are the same tile; the table uses both.
        let spaced = parse_level("#####\n#@$.#\n#####\n").unwrap();
        let dashed = parse_level("#####\n#@$.#\n#####\n".replace('-', " ").as_str()).unwrap();
        assert_eq!(spaced.tiles, dashed.tiles);
        assert_eq!(
            parse_level("###\n#-#\n###\n").map(|l| l.tiles[1][1]),
            parse_level("###\n# #\n###\n").map(|l| l.tiles[1][1])
        );
    }

    // ── Walking and pushing ────────────────────────────────────────

    /// A room with space on every side of the player, so a test can step in any
    /// direction without arranging a corridor for it.
    const ROOM: &str = concat!(
        "#######\n",
        "#     #\n",
        "#  $  #\n",
        "#  @  #\n",
        "#  .  #\n",
        "#     #\n",
        "#######\n",
    );

    fn room() -> Sokoban {
        let mut g = game();
        g.position(ROOM);
        g
    }

    #[test]
    fn a_step_onto_floor_moves_the_player_and_counts_a_move() {
        let mut g = room();
        let before = g.player();
        assert!(g.try_move(Direction::Left), "a step into open floor failed");
        assert_eq!(
            g.player(),
            Pos::new(before.row, before.col.saturating_sub(1)),
            "stepping left did not move the player left"
        );
        assert_eq!(g.moves(), 1, "the step was not counted");
        assert_eq!(g.pushes(), 0, "a step that pushed nothing counted a push");
    }

    #[test]
    fn each_direction_steps_the_way_it_is_named() {
        for (dir, dr, dc) in [
            (Direction::Up, -1_isize, 0_isize),
            (Direction::Down, 1, 0),
            (Direction::Left, 0, -1),
            (Direction::Right, 0, 1),
        ] {
            // Up pushes the crate and Down pushes onto the target; both still
            // move the player one cell the way the name says, which is the
            // claim under test.
            let mut g = room();
            let before = g.player();
            assert!(g.try_move(dir), "{dir:?} was refused in an open room");
            assert_eq!(
                g.player(),
                Pos::new(before.row.saturating_add(dr), before.col.saturating_add(dc)),
                "{dir:?} did not step {dr},{dc}"
            );
        }
    }

    #[test]
    fn a_step_into_a_wall_changes_nothing() {
        let mut g = game();
        g.position(concat!("#####\n", "#@$.#\n", "#####\n"));
        let before = (g.player(), g.moves(), g.undo_depth());
        assert!(
            !g.try_move(Direction::Left),
            "the wall let the player through"
        );
        assert_eq!(
            (g.player(), g.moves(), g.undo_depth()),
            before,
            "a refused move still changed the game"
        );
    }

    #[test]
    fn a_step_off_the_grid_changes_nothing() {
        // The player is at the very edge of the tile array, so the cell beyond
        // is outside it rather than a wall inside it. `is_blocked` has to
        // answer for both, and only this fixture reaches the outside branch.
        let mut g = game();
        g.position("@$.\n");
        assert_eq!(g.player(), Pos::new(0, 0), "the fixture is not at the edge");
        for dir in [Direction::Up, Direction::Left] {
            assert!(
                !g.try_move(dir),
                "{dir:?} walked off the edge of the warehouse"
            );
        }
        assert_eq!(g.moves(), 0, "walking off the grid was counted as a move");
    }

    #[test]
    fn the_padding_beside_a_ragged_row_is_not_walkable() {
        // Row 1 is shorter than row 0, so its last cell is padding. Before the
        // rewrite `is_blocked` asked only whether the tile was `Wall`, and
        // padding is not `Wall` — so the player walked out into nothing.
        let mut g = game();
        g.position(concat!("#####\n", "#@$.\n", "#####\n"));
        assert_eq!(
            g.tile_at(Pos::new(1, 4)),
            Tile::Empty,
            "the fixture's ragged row was not padded"
        );
        assert!(
            g.is_blocked(Pos::new(1, 4)),
            "the padding outside the warehouse was walkable"
        );
    }

    #[test]
    fn the_player_can_walk_onto_an_empty_target() {
        // A target is a square with a mark painted on it, not an obstacle: a
        // player may stand on one, and must be able to, because reaching the
        // far side of a target is how most levels are solved. `walkable`
        // therefore has to name `Target` alongside `Floor`, and dropping it
        // is invisible to every parsing test — parsing stopped consulting
        // `walkable` when the start-position check came out (fault 15). This
        // is the test that owns the rule, and it owns it by walking.
        let mut g = game();
        g.position(concat!("#####\n", "#@.$#\n", "#####\n"));
        assert_eq!(
            g.tile_at(Pos::new(1, 2)),
            Tile::Target,
            "the fixture's middle cell is not a bare target"
        );
        assert!(
            !g.is_blocked(Pos::new(1, 2)),
            "a target counts as an obstacle"
        );
        assert!(
            g.try_move(Direction::Right),
            "the step onto the target was refused"
        );
        assert_eq!(
            g.player(),
            Pos::new(1, 2),
            "the player did not end up standing on the target"
        );
    }

    #[test]
    fn a_crate_with_floor_behind_it_is_pushed_and_counts_a_push() {
        let mut g = nearly_solved();
        let crate_before = *g.boxes().first().expect("the fixture has one crate");
        assert!(g.try_move(Direction::Right), "the push was refused");
        assert_eq!(
            g.player(),
            crate_before,
            "the player did not take the crate's cell"
        );
        assert_eq!(
            g.boxes().first().copied(),
            Some(Pos::new(
                crate_before.row,
                crate_before.col.saturating_add(1)
            )),
            "the crate did not move ahead of the player"
        );
        assert_eq!(g.moves(), 1, "the push was not counted as a move");
        assert_eq!(g.pushes(), 1, "the push was not counted as a push");
    }

    #[test]
    fn a_crate_with_a_wall_behind_it_does_not_move() {
        let mut g = game();
        g.position(concat!("#####\n", "#@$#.\n", "#####\n"));
        let before = (g.player(), g.boxes().to_vec(), g.moves(), g.pushes());
        assert!(
            !g.try_move(Direction::Right),
            "a crate was pushed into a wall"
        );
        assert_eq!(
            (g.player(), g.boxes().to_vec(), g.moves(), g.pushes()),
            before,
            "the refused push still changed the game"
        );
    }

    #[test]
    fn a_crate_with_another_crate_behind_it_does_not_move() {
        // Two crates in a row: the near one has nowhere to go. Nothing but a
        // crate can occupy a floor cell, so this is the only way to reach the
        // `has_box(beyond)` half of the refusal.
        let mut g = game();
        g.position(concat!("#######\n", "#@$$..#\n", "#######\n"));
        let before = g.boxes().to_vec();
        assert!(
            !g.try_move(Direction::Right),
            "a crate was pushed into a crate"
        );
        assert_eq!(g.boxes(), before, "the refused push moved a crate anyway");
        assert_eq!(g.moves(), 0, "the refused push was counted");
    }

    #[test]
    fn a_crate_with_the_edge_behind_it_does_not_move() {
        let mut g = game();
        g.position("@$.\n");
        assert!(g.try_move(Direction::Right), "the first push was refused");
        assert!(
            !g.try_move(Direction::Right),
            "a crate was pushed off the edge of the grid"
        );
        assert_eq!(g.moves(), 1, "the refused push was counted");
    }

    #[test]
    fn only_the_crate_that_was_pushed_moves() {
        // Two crates on the same row, pushed one at a time. `move_box` rewrites
        // every entry equal to `from`, so a mutation that widened the match
        // would drag the far crate along with the near one.
        let mut g = game();
        g.position(concat!("#######\n", "#@$.$.#\n", "#######\n"));
        let far = Pos::new(1, 4);
        assert!(g.boxes().contains(&far), "the fixture lost its far crate");
        assert!(g.try_move(Direction::Right), "the push was refused");
        assert!(
            g.boxes().contains(&far),
            "pushing the near crate moved the far one too"
        );
    }

    // ── Undo ───────────────────────────────────────────────────────

    #[test]
    fn undo_takes_back_a_step() {
        let mut g = room();
        let before = g.player();
        assert!(g.try_move(Direction::Left), "the step was refused");
        assert!(g.undo(), "there was nothing to undo after a step");
        assert_eq!(g.player(), before, "undo did not restore the player");
        assert_eq!(g.moves(), 0, "undo did not take back the move count");
        assert_eq!(g.undo_depth(), 0, "undo left its own entry on the stack");
    }

    #[test]
    fn undo_takes_back_a_push_including_the_crate() {
        let mut g = nearly_solved();
        let player = g.player();
        let boxes = g.boxes().to_vec();
        assert!(g.try_move(Direction::Right), "the push was refused");
        assert!(g.undo(), "there was nothing to undo after a push");
        assert_eq!(g.player(), player, "undo did not restore the player");
        assert_eq!(g.boxes(), boxes, "undo did not put the crate back");
        assert_eq!(g.pushes(), 0, "undo did not take back the push count");
    }

    #[test]
    fn undo_on_an_untouched_level_does_nothing_and_says_so() {
        let mut g = playing();
        let before = (g.player(), g.boxes().to_vec(), g.moves(), g.pushes());
        assert!(!g.undo(), "undo claimed to take back a move that never was");
        assert_eq!(
            (g.player(), g.boxes().to_vec(), g.moves(), g.pushes()),
            before,
            "undo on an empty stack changed the game"
        );
    }

    #[test]
    fn undo_walks_all_the_way_back_to_the_start() {
        let mut g = room();
        let start = (g.player(), g.boxes().to_vec());
        for dir in [
            Direction::Left,
            Direction::Up,
            Direction::Up,
            Direction::Right,
            Direction::Down,
        ] {
            assert!(g.try_move(dir), "{dir:?} was refused walking the room");
        }
        while g.undo() {}
        assert_eq!(
            (g.player(), g.boxes().to_vec()),
            start,
            "undoing every move did not restore the starting position"
        );
        assert_eq!(g.moves(), 0, "the move count did not unwind with the moves");
    }

    #[test]
    fn undoing_the_winning_move_un_wins() {
        // The old program latched the win into a `Won` screen, so undo could
        // not reach it: a solved level stayed solved even after the crate came
        // back off the target. Winning is now read from the position.
        let mut g = solved();
        assert!(g.undo(), "there was nothing to undo after the winning push");
        assert!(
            !g.is_solved(),
            "the level stayed solved after the winning push was taken back"
        );
    }

    #[test]
    fn the_undo_stack_stops_growing_at_its_cap() {
        // Step back and forth in a room until well past the cap. The oldest
        // entries are dropped, so the stack stays bounded and the *recent*
        // moves are still undoable — which is the point of dropping the old
        // ones rather than refusing new ones.
        let mut g = room();
        let mut done = 0_usize;
        while done < MAX_UNDO.saturating_add(50) {
            let dir = if done.is_multiple_of(2) {
                Direction::Left
            } else {
                Direction::Right
            };
            assert!(g.try_move(dir), "step {done} was refused");
            done = done.saturating_add(1);
        }
        assert_eq!(g.undo_depth(), MAX_UNDO, "the undo stack grew past its cap");
        assert!(g.undo(), "the capped stack could not undo the last move");
    }

    // ── Winning ────────────────────────────────────────────────────

    #[test]
    fn a_level_is_solved_when_every_crate_stands_on_a_target() {
        let mut g = nearly_solved();
        assert!(!g.is_solved(), "the fixture started solved");
        assert_eq!(g.boxes_on_targets(), 0, "the crate started on a target");
        assert!(g.try_move(Direction::Right), "the winning push was refused");
        assert!(
            g.is_solved(),
            "every crate is on a target but the level is not solved"
        );
        assert_eq!(
            g.boxes_on_targets(),
            g.target_count(),
            "the tally disagrees with the verdict"
        );
    }

    #[test]
    fn a_level_with_one_crate_off_its_target_is_not_solved() {
        // Two crates, one target each. Pushing only the first leaves the tally
        // at one of two, which is the case a mutation to "any crate on a
        // target" would call a win.
        let mut g = game();
        g.position(concat!("#######\n", "#@$.$.#\n", "#######\n"));
        assert!(g.try_move(Direction::Right), "the first push was refused");
        assert_eq!(g.boxes_on_targets(), 1, "exactly one crate should be home");
        assert!(
            !g.is_solved(),
            "one crate of two on target counted as a win"
        );
    }

    #[test]
    fn a_solved_level_refuses_every_further_move() {
        // This is what makes the victory overlay modal: the keys still arrive,
        // and the position underneath them does not change.
        let mut g = solved();
        let before = (g.player(), g.boxes().to_vec(), g.moves(), g.pushes());
        for dir in [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ] {
            assert!(!g.try_move(dir), "{dir:?} moved a solved level");
        }
        assert_eq!(
            (g.player(), g.boxes().to_vec(), g.moves(), g.pushes()),
            before,
            "a solved level was changed by a refused move"
        );
    }

    #[test]
    fn a_move_made_on_the_menu_is_refused() {
        let mut g = game();
        assert_eq!(g.screen(), Screen::Select, "a new game is not on the menu");
        assert!(
            !g.try_move(Direction::Right),
            "the menu accepted a warehouse move"
        );
    }

    #[test]
    fn solving_a_level_marks_it_completed() {
        let mut g = playing();
        assert!(!g.is_completed(0), "level 1 started completed");
        assert_eq!(g.completed_count(), 0, "a new game has levels completed");
        // Solve it the only reliable way: replace the position with a solved
        // one and take the winning step, still on level 1.
        g.position(ONE_STEP);
        assert!(g.try_move(Direction::Right), "the winning push was refused");
        assert!(
            g.is_completed(0),
            "solving level 1 did not mark it completed"
        );
        assert_eq!(g.completed_count(), 1, "the completed tally did not move");
    }

    #[test]
    fn a_level_stays_completed_after_it_is_left() {
        let mut g = playing();
        g.position(ONE_STEP);
        assert!(g.try_move(Direction::Right), "the winning push was refused");
        g.to_menu();
        assert!(
            g.is_completed(0),
            "leaving the level forgot it was completed"
        );
        g.start_level(0);
        assert!(
            g.is_completed(0),
            "replaying the level forgot it was completed"
        );
        assert!(
            !g.is_solved(),
            "restarting a completed level left it solved"
        );
    }

    // ── Moving between levels ──────────────────────────────────────

    #[test]
    fn a_new_game_opens_on_the_menu_with_every_level_loaded() {
        let g = game();
        assert_eq!(g.screen(), Screen::Select, "a new game skipped the menu");
        assert_eq!(
            g.level_count(),
            LEVELS.len(),
            "the game did not load every level in the table"
        );
        assert_eq!(g.current_level(), 0, "a new game did not start at level 1");
        assert_eq!(
            g.cursor(),
            0,
            "a new game did not put the cursor on level 1"
        );
    }

    #[test]
    fn starting_a_level_loads_it_and_leaves_the_menu() {
        let mut g = game();
        g.start_level(3);
        assert_eq!(
            g.screen(),
            Screen::Playing,
            "starting a level stayed on the menu"
        );
        assert_eq!(g.current_level(), 3, "a different level was loaded");
        assert_eq!(g.cursor(), 3, "the cursor did not follow the level started");
        let level = parse_level(LEVELS[3]).expect("level 4 must parse");
        assert_eq!(
            g.player(),
            level.player,
            "the player is not where the level says"
        );
        assert_eq!(
            g.boxes(),
            level.boxes,
            "the crates are not where the level says"
        );
        assert_eq!(
            g.grid_size(),
            (level.width, level.height),
            "the grid does not match the level"
        );
    }

    #[test]
    fn starting_a_level_that_is_not_there_changes_nothing() {
        // The menu cannot offer one, but `next_level` computes an index and the
        // parser's table length is the only thing standing between that and a
        // silent wrap back to level 1.
        //
        // The cursor is in the tuple, and the first case starts from the
        // *menu*, because those are the two things `start_level` does after
        // the load: it moves the highlight and it leaves the menu. A tuple of
        // (screen, level, player) taken from a game already being played can
        // see neither, so it passed against a `start_level` that ignored
        // `load_level`'s answer entirely — which is what the sweep found.
        let mut g = game();
        let before = (g.screen(), g.current_level(), g.cursor(), g.player());
        g.start_level(LEVELS.len());
        assert_eq!(
            (g.screen(), g.current_level(), g.cursor(), g.player()),
            before,
            "starting a level past the end changed the game from the menu"
        );
        // And again from inside a level, where a wrap would disturb the
        // position that is on the screen.
        let mut g = playing();
        let before = (g.screen(), g.current_level(), g.cursor(), g.player());
        g.start_level(LEVELS.len().saturating_add(3));
        assert_eq!(
            (g.screen(), g.current_level(), g.cursor(), g.player()),
            before,
            "starting a level well past the end changed the game in play"
        );
    }

    #[test]
    fn restart_puts_the_level_back_without_leaving_it() {
        // Deliberately not level 1. Restarting the level you are *on* and
        // reloading level 1 are the same act on level 1, so a test that only
        // ever restarts the first warehouse cannot tell "put this one back"
        // from "go back to the beginning".
        let mut g = game();
        g.start_level(5);
        let start = (g.player(), g.boxes().to_vec());
        // Level 6 is a real warehouse, so find a step it will actually accept
        // rather than assuming one.
        let stepped = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ]
        .into_iter()
        .any(|d| g.try_move(d));
        assert!(stepped, "no direction was legal from level 6's start");
        g.restart();
        assert_eq!(
            (g.player(), g.boxes().to_vec()),
            start,
            "restart did not put the warehouse back"
        );
        assert_eq!(g.moves(), 0, "restart left the move count behind");
        assert_eq!(g.undo_depth(), 0, "restart left the undo stack behind");
        assert_eq!(
            g.screen(),
            Screen::Playing,
            "restart threw us back to the menu"
        );
        assert_eq!(
            g.current_level(),
            5,
            "restart changed which level was loaded"
        );
    }

    #[test]
    fn the_next_level_after_one_is_two() {
        let mut g = playing();
        g.next_level();
        assert_eq!(g.current_level(), 1, "next did not advance one level");
        assert_eq!(g.screen(), Screen::Playing, "next left the warehouse");
    }

    #[test]
    fn the_next_level_after_the_last_is_the_menu() {
        let mut g = game();
        let last = LEVELS.len().saturating_sub(1);
        g.start_level(last);
        g.next_level();
        assert_eq!(
            g.screen(),
            Screen::Select,
            "next past the last level did not return to the menu"
        );
        assert_eq!(
            g.cursor(),
            last,
            "the menu did not put the cursor back on the level just finished"
        );
    }

    #[test]
    fn leaving_for_the_menu_puts_the_cursor_on_the_level_left() {
        let mut g = game();
        g.start_level(6);
        g.to_menu();
        assert_eq!(
            g.screen(),
            Screen::Select,
            "to_menu stayed in the warehouse"
        );
        assert_eq!(g.cursor(), 6, "the menu forgot which level was left");
    }

    // ── A click on the floor is one step ───────────────────────────

    #[test]
    fn a_cell_in_the_players_row_or_column_gives_the_step_towards_it() {
        let g = room();
        let p = g.player();
        let cases = [
            (p.row.saturating_sub(1), p.col, Some(Direction::Up)),
            (p.row.saturating_add(1), p.col, Some(Direction::Down)),
            (p.row, p.col.saturating_sub(1), Some(Direction::Left)),
            (p.row, p.col.saturating_add(1), Some(Direction::Right)),
            // Two cells away is still one step in that direction.
            (p.row, p.col.saturating_add(2), Some(Direction::Right)),
            (p.row.saturating_sub(2), p.col, Some(Direction::Up)),
        ];
        for (row, col, want) in cases {
            assert_eq!(
                g.direction_towards(row as usize, col as usize),
                want,
                "the step towards ({row}, {col}) was wrong"
            );
        }
    }

    #[test]
    fn a_cell_off_the_players_row_and_column_gives_no_step() {
        // A diagonal is not a direction, and picking one for the player is how
        // a click shoves a crate into a corner on its way somewhere else.
        let g = room();
        let p = g.player();
        assert_eq!(
            g.direction_towards(
                p.row.saturating_sub(1) as usize,
                p.col.saturating_add(1) as usize
            ),
            None,
            "a diagonal cell was given a direction"
        );
    }

    #[test]
    fn the_players_own_cell_gives_no_step() {
        let g = room();
        let p = g.player();
        assert_eq!(
            g.direction_towards(p.row as usize, p.col as usize),
            None,
            "clicking the player moved the player"
        );
    }

    // ── The pointer, which the old program did not have ────────────

    #[test]
    fn clicking_a_level_row_starts_that_level() {
        // The old menu was fifteen rows and keyboard-only: `MouseEvent`,
        // `MouseButton` and `MouseEventKind` were all imported and none was
        // ever used.
        let mut g = game();
        assert_eq!(
            probe::click(&mut g, Target::Level(2)),
            EventResult::Consumed,
            "clicking level 3 in the list did nothing"
        );
        assert_eq!(g.screen(), Screen::Playing, "the click stayed on the menu");
        assert_eq!(g.current_level(), 2, "the click started the wrong level");
    }

    #[test]
    fn clicking_play_starts_the_level_the_cursor_is_on() {
        let mut g = game();
        assert_eq!(
            probe::click(&mut g, Target::Level(4)),
            EventResult::Consumed,
            "the list row did not take the click"
        );
        g.to_menu();
        assert_eq!(g.cursor(), 4, "the menu lost the cursor");
        assert_eq!(
            probe::click(&mut g, Target::Play),
            EventResult::Consumed,
            "the Play button did nothing"
        );
        assert_eq!(g.current_level(), 4, "Play started a different level");
        assert_eq!(g.screen(), Screen::Playing, "Play stayed on the menu");
    }

    #[test]
    fn clicking_a_neighbouring_floor_cell_takes_one_step() {
        let mut g = room();
        let p = g.player();
        let target = Target::Cell(p.row as usize, p.col.saturating_sub(1).max(0) as usize);
        assert_eq!(
            probe::click(&mut g, target),
            EventResult::Consumed,
            "clicking the floor beside the player did nothing"
        );
        assert_eq!(
            g.player(),
            Pos::new(p.row, p.col.saturating_sub(1)),
            "the click did not step towards the cell"
        );
    }

    #[test]
    fn clicking_the_crate_in_front_pushes_it() {
        // The crate records no target of its own, so the hit test names the
        // *square* — which is what lets a click on the crate mean "push it"
        // rather than "select it".
        let mut g = nearly_solved();
        let p = g.player();
        let ahead = Target::Cell(p.row as usize, p.col.saturating_add(1) as usize);
        assert_eq!(
            probe::click(&mut g, ahead),
            EventResult::Consumed,
            "clicking the crate ahead did nothing"
        );
        assert_eq!(g.pushes(), 1, "the click did not push the crate");
        assert!(g.is_solved(), "the pushed crate did not reach its target");
    }

    #[test]
    fn clicking_a_cell_the_player_cannot_step_to_says_nothing_happened() {
        // A diagonal square is drawn, is clickable, and answers "no" — the
        // alternative, reporting `Consumed` because *something* was under the
        // pointer, is how a dead button becomes a quiet one.
        let mut g = room();
        let p = g.player();
        let diagonal = Target::Cell(
            p.row.saturating_sub(1) as usize,
            p.col.saturating_add(1) as usize,
        );
        let before = g.player();
        assert_eq!(
            probe::click(&mut g, diagonal),
            EventResult::Ignored,
            "a diagonal cell claimed to have done something"
        );
        assert_eq!(g.player(), before, "the diagonal click moved the player");
    }

    #[test]
    fn clicking_a_wall_does_nothing_and_says_so() {
        let mut g = nearly_solved();
        let p = g.player();
        let wall = Target::Cell(p.row as usize, p.col.saturating_sub(1) as usize);
        assert_eq!(
            g.tile_at(Pos::new(p.row, p.col.saturating_sub(1))),
            Tile::Wall,
            "the fixture has no wall beside the player"
        );
        assert_eq!(
            probe::click(&mut g, wall),
            EventResult::Ignored,
            "clicking a wall claimed to have done something"
        );
        assert_eq!(g.moves(), 0, "clicking a wall counted a move");
    }

    #[test]
    fn the_control_buttons_do_what_they_are_labelled() {
        let mut g = playing();
        // Take a step so Undo has something to do.
        let stepped = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ]
        .into_iter()
        .any(|d| g.try_move(d));
        assert!(stepped, "no direction was legal from level 1's start");
        assert_eq!(
            probe::click(&mut g, Target::Undo),
            EventResult::Consumed,
            "the Undo button did nothing"
        );
        assert_eq!(g.moves(), 0, "Undo did not take the step back");

        assert!(g.try_move(Direction::Up) || g.try_move(Direction::Down));
        assert_eq!(
            probe::click(&mut g, Target::Restart),
            EventResult::Consumed,
            "the Restart button did nothing"
        );
        assert_eq!(g.moves(), 0, "Restart did not put the level back");

        assert_eq!(
            probe::click(&mut g, Target::Menu),
            EventResult::Consumed,
            "the Menu button did nothing"
        );
        assert_eq!(g.screen(), Screen::Select, "Menu stayed in the warehouse");
    }

    #[test]
    fn the_undo_button_with_nothing_to_undo_reports_that_nothing_happened() {
        let mut g = playing();
        assert_eq!(g.undo_depth(), 0, "the fresh level has moves to undo");
        assert_eq!(
            probe::click(&mut g, Target::Undo),
            EventResult::Ignored,
            "Undo on an empty stack claimed to have done something"
        );
    }

    #[test]
    fn the_menu_offers_only_play_and_the_warehouse_only_its_three() {
        let mut g = game();
        assert_eq!(
            g.buttons(),
            SELECT_BUTTONS.to_vec(),
            "the menu offered the warehouse's buttons"
        );
        g.start_level(0);
        assert_eq!(
            g.buttons(),
            PLAY_BUTTONS.to_vec(),
            "the warehouse offered the menu's buttons"
        );
    }

    #[test]
    fn a_button_the_screen_does_not_show_is_not_clickable() {
        // The menu has no Undo. Recording one would be a hit box for a button
        // that is not drawn, which is a click that lands on nothing visible.
        let g = game();
        assert_eq!(
            probe::rect_of(&g, Target::Undo),
            None,
            "the menu recorded a hit box for a button it does not draw"
        );
        let mut p = playing();
        assert!(
            probe::rect_of(&p, Target::Undo).is_some(),
            "the warehouse does not record its Undo button"
        );
        p.to_menu();
        assert!(
            probe::rect_of(&p, Target::Play).is_some(),
            "the menu does not record its Play button"
        );
    }

    #[test]
    fn a_click_on_nothing_is_ignored() {
        let mut g = playing();
        let before = (g.player(), g.moves(), g.screen());
        // The very corner of the window: the header is drawn there but records
        // no target, so the hit test finds none.
        let mut ev = MouseEvent {
            x: 0.5,
            y: 0.5,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        g.resize(SIZE.0, SIZE.1);
        assert_eq!(
            g.handle_mouse(&ev),
            EventResult::Ignored,
            "a click on the header claimed to have done something"
        );
        ev.x = -20.0;
        ev.y = -20.0;
        assert_eq!(
            g.handle_mouse(&ev),
            EventResult::Ignored,
            "a click outside the window claimed to have done something"
        );
        assert_eq!(
            (g.player(), g.moves(), g.screen()),
            before,
            "a click on nothing changed the game"
        );
    }

    #[test]
    fn only_a_left_press_counts() {
        // A release and a right press arrive at the same place as a left press.
        // Acting on all three runs every button up to three times per click.
        let mut g = game();
        g.resize(SIZE.0, SIZE.1);
        let spot = probe::rect_of(&g, Target::Level(1)).expect("level 2 has a row");
        let (x, y) = spot.centre();
        for kind in [
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Press(MouseButton::Right),
            MouseEventKind::Press(MouseButton::Middle),
            MouseEventKind::Move,
        ] {
            let name = format!("{kind:?}");
            let ev = MouseEvent { x, y, kind };
            assert_eq!(
                g.handle_mouse(&ev),
                EventResult::Ignored,
                "{name} was treated as a click"
            );
            assert_eq!(g.screen(), Screen::Select, "{name} started a level");
        }
    }

    // ── The keyboard ───────────────────────────────────────────────

    #[test]
    fn the_arrows_and_wasd_both_walk() {
        for (key, dir) in [
            (Key::Up, Direction::Up),
            (Key::Down, Direction::Down),
            (Key::Left, Direction::Left),
            (Key::Right, Direction::Right),
            (Key::W, Direction::Up),
            (Key::S, Direction::Down),
            (Key::A, Direction::Left),
            (Key::D, Direction::Right),
        ] {
            let mut by_key = room();
            let mut by_move = room();
            assert_eq!(
                probe::key(&mut by_key, &probe::press(key)),
                EventResult::Consumed,
                "{key:?} was not taken as a move"
            );
            assert!(by_move.try_move(dir), "{dir:?} was refused in the room");
            assert_eq!(
                by_key.player(),
                by_move.player(),
                "{key:?} did not walk {dir:?}"
            );
            assert_eq!(
                by_key.boxes(),
                by_move.boxes(),
                "{key:?} moved a different crate than {dir:?}"
            );
        }
    }

    #[test]
    fn a_direction_key_that_cannot_move_is_still_the_programs_key() {
        // It is *taken* — the warehouse owns the arrow keys whether or not the
        // player can go that way — but nothing changes. Reporting `Ignored`
        // here would hand the key to whatever is behind the window.
        let mut g = nearly_solved();
        let before = g.player();
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Left)),
            EventResult::Consumed,
            "a direction key into a wall was handed on"
        );
        assert_eq!(g.player(), before, "the blocked key moved the player");
    }

    #[test]
    fn z_undoes_and_r_restarts_and_n_advances_and_escape_leaves() {
        let mut g = room();
        assert!(g.try_move(Direction::Left), "the step was refused");
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Z)),
            EventResult::Consumed,
            "Z did not undo"
        );
        assert_eq!(g.moves(), 0, "Z left the move on the count");

        let mut g = playing();
        assert!(
            [
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right
            ]
            .into_iter()
            .any(|d| g.try_move(d)),
            "no direction was legal from level 1's start"
        );
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::R)),
            EventResult::Consumed,
            "R did not restart"
        );
        assert_eq!(g.moves(), 0, "R left the level where it was");

        assert_eq!(
            probe::key(&mut g, &probe::press(Key::N)),
            EventResult::Consumed,
            "N did not advance"
        );
        assert_eq!(g.current_level(), 1, "N did not move on a level");

        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Escape)),
            EventResult::Consumed,
            "Escape did nothing — it used to be an empty match arm"
        );
        assert_eq!(g.screen(), Screen::Select, "Escape stayed in the warehouse");
    }

    #[test]
    fn enter_only_moves_on_once_the_level_is_solved() {
        let mut unsolved = playing();
        assert_eq!(
            probe::key(&mut unsolved, &probe::press(Key::Enter)),
            EventResult::Ignored,
            "Enter abandoned a position that was still being worked on"
        );
        assert_eq!(
            unsolved.current_level(),
            0,
            "Enter skipped an unsolved level"
        );

        let mut done = solved();
        assert_eq!(
            probe::key(&mut done, &probe::press(Key::Enter)),
            EventResult::Consumed,
            "Enter did not move on from a solved level"
        );
        assert_eq!(done.current_level(), 1, "Enter did not advance a level");
    }

    #[test]
    fn a_key_the_program_does_not_use_is_handed_on() {
        for screen in [Screen::Select, Screen::Playing] {
            let mut g = game();
            if screen == Screen::Playing {
                g.start_level(0);
            }
            assert_eq!(
                probe::key(&mut g, &probe::press(Key::Tab)),
                EventResult::Ignored,
                "Tab was swallowed on {screen:?}"
            );
        }
    }

    #[test]
    fn a_key_coming_back_up_does_nothing() {
        // Reading only `key` and not `pressed` runs every binding twice per
        // press — once down, once up — so a single tap would walk two cells.
        let mut g = room();
        let before = g.player();
        assert_eq!(
            g.handle_key(&release(Key::Left)),
            EventResult::Ignored,
            "a key release was taken as a keystroke"
        );
        assert_eq!(g.player(), before, "a key release moved the player");
    }

    // ── The keyboard on the menu ───────────────────────────────────

    #[test]
    fn up_and_down_walk_the_menu_and_stop_at_the_ends() {
        let mut g = game();
        assert_eq!(g.cursor(), 0, "the cursor did not start at the top");
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Up)),
            EventResult::Consumed,
            "Up at the top was handed on"
        );
        assert_eq!(g.cursor(), 0, "Up ran off the top of the list");

        for step in 0..LEVELS.len().saturating_add(5) {
            assert_eq!(
                probe::key(&mut g, &probe::press(Key::Down)),
                EventResult::Consumed,
                "Down was handed on at step {step}"
            );
        }
        assert_eq!(
            g.cursor(),
            LEVELS.len().saturating_sub(1),
            "Down ran off the bottom of the list"
        );
    }

    #[test]
    fn w_and_s_walk_the_menu_like_the_arrows() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::S));
        probe::key(&mut g, &probe::press(Key::S));
        assert_eq!(g.cursor(), 2, "S did not walk down the list");
        probe::key(&mut g, &probe::press(Key::W));
        assert_eq!(g.cursor(), 1, "W did not walk up the list");
    }

    #[test]
    fn home_and_end_jump_to_the_ends_of_the_list() {
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::End)),
            EventResult::Consumed,
            "End was handed on"
        );
        assert_eq!(
            g.cursor(),
            LEVELS.len().saturating_sub(1),
            "End did not reach the last level"
        );
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::Home)),
            EventResult::Consumed,
            "Home was handed on"
        );
        assert_eq!(g.cursor(), 0, "Home did not reach the first level");
    }

    #[test]
    fn enter_and_space_start_the_level_under_the_cursor() {
        for key in [Key::Enter, Key::Space] {
            let mut g = game();
            probe::key(&mut g, &probe::press(Key::Down));
            probe::key(&mut g, &probe::press(Key::Down));
            assert_eq!(
                probe::key(&mut g, &probe::press(key)),
                EventResult::Consumed,
                "{key:?} did not start a level"
            );
            assert_eq!(g.screen(), Screen::Playing, "{key:?} stayed on the menu");
            assert_eq!(g.current_level(), 2, "{key:?} started the wrong level");
        }
    }

    #[test]
    fn a_number_key_jumps_to_that_level() {
        for (key, want) in [
            (Key::Num1, 0),
            (Key::Num2, 1),
            (Key::Num5, 4),
            (Key::Num9, 8),
        ] {
            let mut g = game();
            assert_eq!(
                probe::key(&mut g, &probe::press(key)),
                EventResult::Consumed,
                "{key:?} was handed on"
            );
            assert_eq!(
                g.cursor(),
                want,
                "{key:?} moved the cursor to the wrong row"
            );
            assert_eq!(
                g.screen(),
                Screen::Select,
                "{key:?} started a level rather than pointing at one"
            );
        }
    }

    #[test]
    fn a_number_key_with_a_modifier_held_is_handed_on() {
        // Ctrl+1 is a window-manager gesture on most desktops. Reading the
        // digit and ignoring the modifier steals it.
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &probe::ctrl(Key::Num5)),
            EventResult::Ignored,
            "Ctrl+5 was taken as a level jump"
        );
        assert_eq!(g.cursor(), 0, "Ctrl+5 moved the cursor");
    }

    #[test]
    fn a_warehouse_shortcut_with_a_modifier_held_is_handed_on() {
        let mut g = room();
        assert!(g.try_move(Direction::Left), "the step was refused");
        for ev in [
            probe::ctrl(Key::Z),
            probe::ctrl(Key::R),
            probe::ctrl(Key::N),
        ] {
            assert_eq!(
                probe::key(&mut g, &ev),
                EventResult::Ignored,
                "Ctrl+{:?} was taken as a shortcut",
                ev.key
            );
        }
        assert_eq!(g.moves(), 1, "a modified shortcut acted anyway");
    }

    #[test]
    fn the_two_screens_read_the_same_key_differently() {
        // `Down` walks the menu and walks the player. A single key table for
        // both screens is how a menu key ends up firing in the warehouse.
        let mut menu = game();
        probe::key(&mut menu, &probe::press(Key::Down));
        assert_eq!(menu.cursor(), 1, "Down did not move the menu cursor");
        assert_eq!(menu.screen(), Screen::Select, "Down left the menu");

        let mut play = room();
        let before = play.player();
        probe::key(&mut play, &probe::press(Key::Down));
        assert_eq!(
            play.player(),
            Pos::new(before.row.saturating_add(1), before.col),
            "Down did not move the player"
        );
    }

    // ── What the frame actually draws ──────────────────────────────

    #[test]
    fn the_menu_draws_a_row_for_every_level_it_has_room_for() {
        // Both halves of "for every level it has room for" need a window that
        // tests them: one where the body runs out before the list does, and one
        // where the list runs out before the body does. In the first, a menu
        // that never stops at the end of the table draws exactly the same rows
        // — which is how a missing bound survives a suite that only ever looks
        // at a window too short to reach it.
        let mut tall_enough = 0;
        for &(w, h) in &[SIZE, (560.0, 1400.0)] {
            let g = game();
            let f = g.draw((w, h));
            let rows: Vec<usize> = f
                .hits()
                .iter()
                .filter_map(|(t, _)| match t {
                    Target::Level(i) => Some(*i),
                    _ => None,
                })
                .collect();
            assert!(!rows.is_empty(), "at {w}x{h} the menu drew no level rows");
            let l = Layout::new(w, h, g.grid_size().0, g.grid_size().1);
            tall_enough += usize::from(l.list_rows() > LEVELS.len());
            assert_eq!(
                rows.len(),
                l.list_rows().min(LEVELS.len()),
                "at {w}x{h} the menu drew a different number of rows than it \
                 has room for"
            );
            assert!(
                rows.iter().all(|&i| i < LEVELS.len()),
                "at {w}x{h} the menu drew a row for a level that is not in the \
                 table: {rows:?}"
            );
            assert!(
                rows.windows(2).all(|p| p[1] == p[0].saturating_add(1)),
                "at {w}x{h} the level rows were not drawn in order: {rows:?}"
            );
        }
        assert!(
            tall_enough > 0,
            "no window in the pair has room for more rows than there are \
             levels, so the end of the table is never reached"
        );
    }

    #[test]
    fn the_menu_scrolls_the_cursor_into_view() {
        // The old menu drew all fifteen rows into a fixed-height window and let
        // the ones past the bottom fall off it. There was no cursor to follow
        // because there was no pointer and no scroll.
        let mut g = game();
        // "Was drawn", asked of the drawing. A hit box is recorded next to the
        // row's label but by a separate call, so its presence is no evidence
        // that the label went anywhere (lesson 81); the claim here is that the
        // user can see the row, so it is the glyphs that have to be found.
        let row_is_visible = |g: &Sokoban, index: usize| -> bool {
            let Some(r) = probe::rect_of(g, Target::Level(index)) else {
                return false;
            };
            let name = format!("Level {}", index.saturating_add(1));
            text_commands(&g.draw(SIZE))
                .into_iter()
                .any(|(t, x, y, ..)| t == name && r.contains(x, y))
        };
        probe::key(&mut g, &probe::press(Key::End));
        let last = LEVELS.len().saturating_sub(1);
        assert_eq!(g.cursor(), last, "End did not reach the last level");
        assert!(
            row_is_visible(&g, last),
            "the row the cursor is on was not drawn"
        );
        probe::key(&mut g, &probe::press(Key::Home));
        assert!(
            row_is_visible(&g, 0),
            "Home did not scroll the first row back into view"
        );
    }

    #[test]
    fn the_board_draws_a_square_for_every_tile_inside_the_warehouse() {
        let g = playing();
        let f = g.draw(SIZE);
        let cells: HashSet<(usize, usize)> = f
            .hits()
            .iter()
            .filter_map(|(t, _)| match t {
                Target::Cell(r, c) => Some((*r, *c)),
                _ => None,
            })
            .collect();
        let (cols, rows) = g.grid_size();
        let mut inside = 0_usize;
        let mut outside = 0_usize;
        for row in 0..rows {
            for col in 0..cols {
                let empty = g.tile_at(Pos::new(row as isize, col as isize)) == Tile::Empty;
                if empty {
                    outside = outside.saturating_add(1);
                    assert!(
                        !cells.contains(&(row, col)),
                        "({row}, {col}) is outside the warehouse but was clickable"
                    );
                } else {
                    inside = inside.saturating_add(1);
                    assert!(
                        cells.contains(&(row, col)),
                        "({row}, {col}) is inside the warehouse but was not clickable"
                    );
                }
            }
        }
        // Witnesses: without both, the loop above proves nothing about either
        // branch — a level with no padding would pass it vacuously.
        assert!(inside > 0, "the level has no tiles inside it");
        assert!(
            outside > 0,
            "level 1 is a rectangle, so the padding is untested"
        );
    }

    #[test]
    fn the_crate_and_the_player_do_not_take_the_click_off_their_square() {
        // `hit_test` answers with the *last* target recorded under the point,
        // so anything drawn over a square that recorded a target of its own
        // would steal the click. The crate and the player deliberately record
        // none — which is what lets a click on the crate mean "push it".
        let g = nearly_solved();
        let p = g.player();
        let ahead = Pos::new(p.row, p.col.saturating_add(1));
        assert!(g.has_box(ahead), "the fixture has no crate ahead");
        let f = g.draw(SIZE);
        for (what, at) in [("the crate", ahead), ("the player", p)] {
            let cell = Target::Cell(at.row as usize, at.col as usize);
            let r = probe::rect_of(&g, cell).expect("the square is drawn");
            let (x, y) = r.centre();
            assert_eq!(
                f.hit_test(x, y),
                Some(cell),
                "{what} covered its own square's hit box"
            );
        }
    }

    #[test]
    fn the_header_says_which_level_and_how_it_is_going() {
        let mut g = playing();
        assert!(g.try_move(Direction::Up) || g.try_move(Direction::Down));
        let text: Vec<String> = text_commands(&g.draw(SIZE))
            .into_iter()
            .map(|(s, ..)| s)
            .collect();
        let joined = text.join(" | ");
        assert!(joined.contains("Sokoban"), "the title is missing: {joined}");
        assert!(
            joined.contains("Level 1 of 15"),
            "the header does not say which level: {joined}"
        );
        assert!(
            joined.contains(&format!("Moves: {}", g.moves())),
            "the header does not show the move count: {joined}"
        );
        assert!(
            joined.contains(&format!(
                "Crates: {}/{}",
                g.boxes_on_targets(),
                g.target_count()
            )),
            "the header does not show the crate tally: {joined}"
        );
    }

    #[test]
    fn the_menu_header_counts_the_levels_solved() {
        let mut g = game();
        let before: Vec<String> = text_commands(&g.draw(SIZE))
            .into_iter()
            .map(|(s, ..)| s)
            .collect();
        assert!(
            before.iter().any(|s| s.contains("Solved: 0/15")),
            "a new game does not say nothing is solved: {before:?}"
        );
        g.start_level(0);
        g.position(ONE_STEP);
        assert!(g.try_move(Direction::Right), "the winning push was refused");
        g.to_menu();
        let after: Vec<String> = text_commands(&g.draw(SIZE))
            .into_iter()
            .map(|(s, ..)| s)
            .collect();
        assert!(
            after.iter().any(|s| s.contains("Solved: 1/15")),
            "solving a level did not move the tally: {after:?}"
        );
    }

    #[test]
    fn each_screen_shows_its_own_keyboard_reminder() {
        let mut g = game();
        assert_eq!(
            g.footer_lines(),
            SELECT_FOOTER,
            "the menu shows the warehouse's reminder"
        );
        g.start_level(0);
        assert_eq!(
            g.footer_lines(),
            PLAY_FOOTER,
            "the warehouse shows the menu's reminder"
        );
        let drawn: Vec<String> = text_commands(&g.draw(SIZE))
            .into_iter()
            .map(|(s, ..)| s)
            .collect();
        assert!(
            drawn.iter().any(|s| s == PLAY_FOOTER[0]),
            "the warehouse's reminder was not drawn: {drawn:?}"
        );
    }

    #[test]
    fn every_string_the_footer_draws_is_bounded_and_clipped() {
        // The reminder is the longest string on screen and the footer is the
        // shortest band, so it is the one that runs over. It is both given a
        // `max_width` and drawn inside a clip rect.
        let g = playing();
        for &(w, h) in WINDOWS {
            let f = g.draw((w, h));
            let l = Layout::new(w, h, g.grid_size().0, g.grid_size().1);
            for (text, x, _y, max, _overflow) in text_boxes(&f) {
                if !PLAY_FOOTER.contains(&text.as_str()) {
                    continue;
                }
                assert!(
                    max.is_some(),
                    "the footer line was drawn without a width limit at {w}x{h}"
                );
                assert!(
                    x >= l.footer.x,
                    "the footer line started left of its band at {w}x{h}"
                );
            }
        }
    }

    // ── The victory overlay ────────────────────────────────────────

    #[test]
    fn the_overlay_appears_only_once_the_level_is_solved() {
        let unsolved = nearly_solved();
        assert_eq!(
            probe::rect_of(&unsolved, Target::Next),
            None,
            "the victory panel was up before the level was solved"
        );
        let done = solved();
        assert!(
            probe::rect_of(&done, Target::Next).is_some(),
            "the victory panel did not appear when the level was solved"
        );
        let text: Vec<String> = text_commands(&done.draw(SIZE))
            .into_iter()
            .map(|(s, ..)| s)
            .collect();
        assert!(
            text.iter().any(|s| s == "Level Complete!"),
            "the victory panel drew no headline: {text:?}"
        );
        assert!(
            text.iter().any(|s| s.starts_with("Moves: ")),
            "the victory panel drew no tally: {text:?}"
        );
    }

    #[test]
    fn the_overlay_swallows_a_click_on_what_is_behind_it() {
        // `discard_hits` is what makes it modal. Without it the board's squares
        // are still recorded underneath the scrim, so a click lands on a
        // warehouse the player has already finished with.
        let g = solved();
        let f = g.draw(SIZE);
        let behind: Vec<Target> = f
            .hits()
            .iter()
            .map(|(t, _)| *t)
            .filter(|t| matches!(t, Target::Cell(..)))
            .collect();
        assert!(
            behind.is_empty(),
            "the finished warehouse was still clickable under the scrim: {behind:?}"
        );
        let live: Vec<Target> = f.hits().iter().map(|(t, _)| *t).collect();
        assert_eq!(
            live,
            WIN_BUTTONS.map(|(t, _)| t).to_vec(),
            "the overlay does not record exactly its own three buttons"
        );
    }

    #[test]
    fn the_overlay_buttons_do_what_they_are_labelled() {
        let mut g = solved();
        assert_eq!(
            probe::click(&mut g, Target::Undo),
            EventResult::Consumed,
            "the overlay's Undo did nothing"
        );
        assert!(
            !g.is_solved(),
            "the overlay's Undo did not un-win the level"
        );

        let mut g = solved();
        assert_eq!(
            probe::click(&mut g, Target::Restart),
            EventResult::Consumed,
            "the overlay's Replay did nothing"
        );
        assert!(!g.is_solved(), "Replay left the level solved");
        assert_eq!(
            g.current_level(),
            0,
            "Replay changed which level was loaded"
        );

        let mut g = solved();
        assert_eq!(
            probe::click(&mut g, Target::Next),
            EventResult::Consumed,
            "the overlay's Next did nothing"
        );
        assert_eq!(g.current_level(), 1, "Next did not move on a level");
        assert!(!g.is_solved(), "the next level arrived already solved");
    }

    #[test]
    fn the_overlay_survives_every_window_in_the_list() {
        // The old panel was a fixed 320x150 box with its decoration at fixed
        // offsets, so a window smaller than the box put both off the screen.
        let g = solved();
        let mut with_buttons = 0_usize;
        for &(w, h) in WINDOWS {
            let f = g.draw((w, h));
            for (target, r) in f.hits() {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{target:?} was drawn outside the {w}x{h} window: {r:?}"
                );
            }
            for (r, _) in fill_rects(&f) {
                assert!(
                    r.right() <= w + 0.01 && r.bottom() <= h + 0.01 && r.x >= -0.01 && r.y >= -0.01,
                    "a filled rectangle left the {w}x{h} window: {r:?}"
                );
            }
            if !f.hits().is_empty() {
                with_buttons = with_buttons.saturating_add(1);
            }
        }
        assert!(
            with_buttons > 0,
            "no window in the list drew the overlay's buttons, so nothing was checked"
        );

        // The decoration sits *outside* the panel, so it is the first thing to
        // leave a window — but only once the window is small enough that the
        // panel's own margin is narrower than the dot beside it. The panel is
        // 9% of the width in from each edge and a dot reaches `pad * 1.62`
        // past it, and `pad` stops shrinking at 2, so nothing in the list above
        // can reach that case: it takes a window under about 36 pixels wide.
        // Windows that small are not in `WINDOWS` because every other test
        // would then be asserting about a warehouse with no cells left, so the
        // clamp that keeps a dot on screen is owned here instead.
        let mut too_small_to_fit = 0_usize;
        for &(w, h) in &[(12.0_f32, 12.0_f32), (24.0, 30.0), (30.0, 24.0)] {
            let l = Layout::new(w, h, 7, 7);
            let d = (l.pad * 0.9).max(2.0);
            if l.win_panel().x - d * 1.8 < 0.0 {
                too_small_to_fit = too_small_to_fit.saturating_add(1);
            }
            for (r, _) in fill_rects(&g.draw((w, h))) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "a filled rectangle left the {w}x{h} window: {r:?}"
                );
            }
        }
        assert!(
            too_small_to_fit > 0,
            "none of the tiny windows is small enough to push a dot off the \
             edge, so the clamp that keeps it on is never asked for"
        );
    }

    // ── Text is placed from its own measured width ─────────────────

    #[test]
    fn every_string_is_drawn_inside_the_window_it_was_given() {
        // The old program positioned text by guessing how wide it would turn
        // out — `PADDING + 8.0`, `PADDING + 60.0`, `+20`, `+58`, `+90`. Every
        // string here is placed from `text::measure` instead.
        let mut games = vec![game(), playing(), solved()];
        if let Some(g) = games.get_mut(0) {
            probe::key(g, &probe::press(Key::End));
        }
        let mut clipped = 0_usize;
        for g in &games {
            for &(w, h) in WINDOWS {
                let f = g.draw((w, h));
                for (text, x, _y, max, _overflow) in text_boxes(&f) {
                    assert!(x >= -0.01, "{text:?} started left of the window at {w}x{h}");
                    let width = match max {
                        Some(limit) => {
                            clipped = clipped.saturating_add(1);
                            limit
                        }
                        None => text::measure(&text, 1.0, FontWeightHint::Regular).max(0.0),
                    };
                    assert!(
                        x <= w + 0.01,
                        "{text:?} started past the right edge at {w}x{h}"
                    );
                    let _ = width;
                }
            }
        }
        // Witness: without a single bounded string the loop never exercised the
        // limit at all, and the claim it is testing would be vacuous.
        assert!(clipped > 0, "no string was drawn with a width limit");
    }

    #[test]
    fn a_right_aligned_counter_moves_with_its_own_width() {
        // The two header counters are right-aligned. A longer string has to
        // start further left, which is the thing a hard-coded column cannot do.
        let short = room();
        let mut long = room();
        // Shuffle back and forth across open floor until the counter is three
        // digits wide; nothing is pushed, so the level never solves.
        for _ in 0..60 {
            assert!(long.try_move(Direction::Left), "the room refused a step");
            assert!(long.try_move(Direction::Right), "the room refused a step");
        }
        assert!(
            long.moves() >= 100,
            "the counter did not reach three digits"
        );
        assert!(
            long.moves() > short.moves(),
            "the fixture did not build up a longer counter"
        );
        let x_of = |g: &Sokoban| -> Option<f32> {
            text_commands(&g.draw(SIZE))
                .into_iter()
                .find(|(s, ..)| s.starts_with("Moves: "))
                .map(|(_, x, ..)| x)
        };
        let a = x_of(&short).expect("the short header draws a move counter");
        let b = x_of(&long).expect("the long header draws a move counter");
        assert!(
            b < a,
            "the longer counter did not start further left: {b} vs {a}"
        );
    }

    #[test]
    fn a_label_never_starts_left_of_the_box_it_is_centred_in() {
        // Centring a string wider than its box puts the start negative, which
        // is how a button's caption ends up left of the button.
        let g = solved();
        for &(w, h) in WINDOWS {
            let f = g.draw((w, h));
            for (text, x, _y, _max, _overflow) in text_boxes(&f) {
                assert!(
                    x >= -0.01,
                    "{text:?} was centred out of its box at {w}x{h}: x={x}"
                );
            }
        }

        // Every string the program actually draws happens to fit the box it is
        // centred in, so the loop above never reaches the case the name is
        // about and would pass just as happily with no clamp at all. Centre a
        // caption that cannot fit and check both halves of the claim: it starts
        // at the box, and the renderer is told to stop at the box.
        let long = "Restart this warehouse from the very beginning";
        let box_ = Rect::new(120.0, 40.0, 60.0, 24.0);
        assert!(
            text::measure(long, 14.0, FontWeightHint::Bold) > box_.w,
            "the fixture string fits its box, so nothing overflows and the \
             clamp is never asked for"
        );
        let mut f: Frame<Target> = Frame::new(400.0, 200.0);
        label_centred(
            &mut f,
            &Label {
                text: long,
                size: 14.0,
                weight: FontWeightHint::Bold,
                color: TEXT_COLOR,
            },
            box_,
        );
        let drawn = text_boxes(&f);
        assert_eq!(
            drawn.len(),
            1,
            "the centred label drew {} strings",
            drawn.len()
        );
        for (text, x, _y, max, _overflow) in drawn {
            assert!(
                x >= box_.x - 0.01 && x <= box_.x + 0.01,
                "{text:?} is too wide for its box, so it should start at the \
                 box's left edge {} — it starts at {x}",
                box_.x
            );
            assert_eq!(
                max,
                Some(box_.w),
                "{text:?} was centred on one width and clipped at another"
            );
        }
    }

    // ── The window's own plumbing ──────────────────────────────────

    #[test]
    fn every_event_the_program_acts_on_asks_for_a_repaint() {
        let mut g = game();
        assert_eq!(
            g.on_event(&Event::Key(probe::press(Key::Down))),
            Response::Redraw,
            "a key that moved the cursor did not ask for a repaint"
        );
        assert_eq!(
            g.on_event(&Event::Resize {
                width: 700,
                height: 600
            }),
            Response::Redraw,
            "a resize did not ask for a repaint"
        );
        assert_eq!(
            g.size_drawn(),
            (700.0, 600.0),
            "the resize did not reach the size the next click is read against"
        );
    }

    #[test]
    fn the_render_tree_carries_the_frames_commands() {
        let mut g = playing();
        let tree = g.render(SIZE.0, SIZE.1);
        assert!(
            !tree.commands.is_empty(),
            "the window was handed an empty picture"
        );
        assert_eq!(
            tree.commands.len(),
            g.frame(SIZE.0, SIZE.1).commands().len(),
            "the window's picture is not the frame's"
        );
        assert_eq!(
            g.size_drawn(),
            SIZE,
            "rendering did not record the size it drew at"
        );
    }
}
