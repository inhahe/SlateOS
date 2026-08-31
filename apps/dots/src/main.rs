//! Slate OS Dots and Boxes -- classic pencil-and-paper strategy game.
//!
//! Two players take turns drawing lines between adjacent dots on a grid.
//! When a player completes the fourth side of a box, that box is claimed and
//! the player gets another turn. The game ends when all boxes are filled.
//! Supports human-vs-AI and two-player modes, configurable grid sizes
//! (3x3, 4x4, 5x5 dots), keyboard and mouse input, and a greedy AI opponent.
//!
//! # The window
//!
//! [`Layout::solve`] derives every rectangle in the frame from the width and
//! height the compositor hands `render`. There is deliberately no
//! `window_width()` or `window_height()` any more -- there were, and they were
//! the drawing pass telling the window how big to be: the footer bar was
//! painted at `window_height() - FOOTER_HEIGHT`, which is the bottom of the
//! window the app *wanted*, so in every other window it floated somewhere in
//! the middle with unpainted canvas beneath it.
//!
//! The lattice ignored the window entirely. `dot_pos` was
//! `PADDING + DOT_RADIUS + col * DOT_SPACING`, three compile-time constants,
//! so a 5x5 board was 340 pixels wide in a 1200-pixel window and 340 pixels
//! wide in a 300-pixel one -- in the first it sat in the top-left corner with
//! most of the canvas empty, and in the second it ran off the right edge.
//! The spacing is now what the smaller of the two free dimensions can pay for.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
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
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Player colors ───────────────────────────────────────────────────
const PLAYER1_COLOR: Color = BLUE;
const PLAYER2_COLOR: Color = RED;
const PLAYER1_BOX_COLOR: Color = Color::from_hex(0x2A3A5E);
const PLAYER2_BOX_COLOR: Color = Color::from_hex(0x5E2A3A);

// ── The window ──────────────────────────────────────────────────────
/// The size the window opens at. Nothing is measured from it: every
/// rectangle in the frame comes from [`Layout::solve`], which is given the
/// live window each time `render` is called.
const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The title, drawn and measured from one place.
const TITLE: &str = "Dots & Boxes";

/// The footer's list of keys.
const FOOTER_HELP: &str =
    "Arrows: move | Tab: toggle H/V | Enter: draw | N: new | M: mode | 3/4/5: size";

/// The largest a cell may grow to, in pixels.
///
/// Without a cap a 2000-pixel window draws a 3x3 board with 900 pixels between
/// its dots: two dots and a line, filling a monitor. The board stops growing
/// here and is centred in what is left.
const MAX_SPACING: f32 = 90.0;

/// The dot's radius as a fraction of the spacing, and the two together as a
/// fraction of the board's span.
///
/// The lattice reaches half a dot past the outermost centres on every side, so
/// a board of `n` cells spans `n * spacing + 2 * radius`. Writing the radius as
/// a fraction of the spacing lets `solve` invert that exactly rather than
/// guessing and then discovering the discs do not fit -- which is what the old
/// `board_pixel_span` had to correct for after the fact.
const DOT_FRACTION: f32 = 0.12;

/// How many lines bound a box. A box is complete when it has this many.
const SIDES_PER_BOX: usize = 4;

/// Default grid size: 4x4 dots = 3x3 boxes.
const DEFAULT_GRID_SIZE: usize = 4;
const MIN_GRID_SIZE: usize = 3;
const MAX_GRID_SIZE: usize = 5;

// ── Randomness ─────────────────────────────────────────────────────
//
// This file used to carry its own copy of the LCG that `guitk::rng` exists to
// replace, reduced with `val % bound` and seeded with a literal `42`. The AI
// picks a safe line at random when several are equally good, so every match
// against the computer played out the same way from the same position.

/// The seed a session falls back to when the kernel has no entropy to give.
///
/// The AI's tie-breaking may be predictable -- the worst outcome is that the
/// computer plays the same game twice. Refusing to start would be the worse
/// failure; see [`guitk::rng::seeded_from_system`]. "DOTSBOX!" in ASCII.
const FALLBACK_SEED: u64 = 0x444F_5453_424F_5821;

// ── Layout ──────────────────────────────────────────────────────────
/// Every rectangle in the frame, derived from the window and the grid size.
///
/// The board is square and centred in what the header and the footer leave.
/// Nothing here is a constant offset: the old code laid the lattice out from
/// `PADDING + DOT_RADIUS + col * DOT_SPACING` and the footer from a
/// `window_height()` the window had never agreed to.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    header: Rect,
    footer: Rect,
    /// The square the lattice occupies, discs included.
    board: Rect,
    pad: f32,
    spacing: f32,
    dot_radius: f32,
    line_w: f32,
    cursor_w: f32,
    /// How far from a line a click may land and still count.
    reach: f32,
    title: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32, grid_size: usize) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);
        let pad = (w.min(h) * 0.035).clamp(4.0, 20.0);
        let font = (h / 34.0).clamp(9.0, 18.0);
        let title = (font * 1.35).clamp(12.0, 24.0);
        let small = (font - 3.0).max(8.0);

        let header = Rect::new(0.0, 0.0, w, (title + small + pad * 1.4).min(h));
        let footer_h = (small * 2.2).min((h - header.h).max(0.0));
        let footer = Rect::new(0.0, h - footer_h, w, footer_h);

        // What the two bars leave, less a pad on every side.
        let free = Rect::new(
            pad,
            header.bottom() + pad,
            (w - pad * 2.0).max(0.0),
            (footer.y - pad - (header.bottom() + pad)).max(0.0),
        );

        // A board of `cells` cells spans `cells * spacing + 2 * radius`, and
        // the radius is `DOT_FRACTION * spacing`, so the span the free square
        // can pay for inverts exactly.
        #[expect(
            clippy::cast_precision_loss,
            reason = "grid_size is clamped to 3..=5; exact in f32"
        )]
        let cells = grid_size.max(2).saturating_sub(1) as f32;
        let span = free.w.min(free.h);
        let spacing = (span / DOT_FRACTION.mul_add(2.0, cells)).clamp(0.0, MAX_SPACING);
        let dot_radius = spacing * DOT_FRACTION;
        let board_span = dot_radius.mul_add(2.0, cells * spacing);
        let board = Rect::new(
            free.x + (free.w - board_span).max(0.0) / 2.0,
            free.y + (free.h - board_span).max(0.0) / 2.0,
            board_span,
            board_span,
        );

        Self {
            window,
            header,
            footer,
            board,
            pad,
            spacing,
            dot_radius,
            line_w: (spacing * 0.07).clamp(1.0, 6.0),
            cursor_w: (spacing * 0.11).clamp(2.0, 9.0),
            reach: spacing * 0.2,
            title,
            font,
            small,
        }
    }

    /// Centre of the dot at grid coordinates `(row, col)`, in pixels.
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid_size is clamped to 3..=5; exact in f32"
    )]
    fn dot_pos(&self, row: usize, col: usize) -> (f32, f32) {
        (
            self.dot_radius
                .mul_add(1.0, self.board.x + col as f32 * self.spacing),
            self.dot_radius
                .mul_add(1.0, self.board.y + row as f32 * self.spacing),
        )
    }

    /// The two dots a line runs between, in pixels.
    ///
    /// A horizontal line joins `(row, col)` to the dot on its right; a
    /// vertical one joins it to the dot below. Painting and hit-testing both
    /// read this, so a line cannot be drawn between one pair of dots and
    /// clicked between another.
    fn line_endpoints(&self, line: LineId) -> ((f32, f32), (f32, f32)) {
        let start = self.dot_pos(line.row, line.col);
        let end = match line.orientation {
            Orientation::Horizontal => (
                self.dot_pos(line.row, line.col.saturating_add(1)).0,
                start.1,
            ),
            Orientation::Vertical => (
                start.0,
                self.dot_pos(line.row.saturating_add(1), line.col).1,
            ),
        };
        (start, end)
    }

    /// The rectangle a click on `line` must land in.
    ///
    /// It is the line's own band, inset at both ends by `reach`, so the bands
    /// of a horizontal and a vertical line that meet at a dot do not overlap
    /// and "which line did I click" is never a tie to be broken. The old code
    /// had no boxes at all: it walked every line measuring the perpendicular
    /// distance and kept the nearest, an inverse of the drawing mapping that
    /// only agreed with the picture because both called `line_endpoints`.
    /// Now the pass that paints a line records the box, so it cannot.
    fn line_box(&self, line: LineId) -> Rect {
        let ((x1, y1), (x2, y2)) = self.line_endpoints(line);
        let r = self.reach;
        match line.orientation {
            Orientation::Horizontal => {
                Rect::new(x1 + r, y1 - r, (x2 - x1 - r * 2.0).max(0.0), r * 2.0)
            }
            Orientation::Vertical => {
                Rect::new(x1 - r, y1 + r, r * 2.0, (y2 - y1 - r * 2.0).max(0.0))
            }
        }
    }
}

// ── Line orientation ────────────────────────────────────────────────
/// A line can be horizontal (connecting dots in the same row) or vertical
/// (connecting dots in the same column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    /// The other one.
    const fn toggled(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

// ── Line identifier ─────────────────────────────────────────────────
/// Identifies a line segment between two adjacent dots.
///
/// For horizontal lines: `(row, col)` is the left dot; the right dot is at `(row, col+1)`.
/// For vertical lines: `(row, col)` is the top dot; the bottom dot is at `(row+1, col)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LineId {
    orientation: Orientation,
    row: usize,
    col: usize,
}

impl LineId {
    const fn new(orientation: Orientation, row: usize, col: usize) -> Self {
        Self {
            orientation,
            row,
            col,
        }
    }

    fn horizontal(row: usize, col: usize) -> Self {
        Self::new(Orientation::Horizontal, row, col)
    }

    fn vertical(row: usize, col: usize) -> Self {
        Self::new(Orientation::Vertical, row, col)
    }
}

// ── Player ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Player {
    One,
    Two,
}

impl Player {
    fn other(self) -> Self {
        match self {
            Player::One => Player::Two,
            Player::Two => Player::One,
        }
    }

    fn color(self) -> Color {
        match self {
            Player::One => PLAYER1_COLOR,
            Player::Two => PLAYER2_COLOR,
        }
    }

    fn box_color(self) -> Color {
        match self {
            Player::One => PLAYER1_BOX_COLOR,
            Player::Two => PLAYER2_BOX_COLOR,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Player::One => "Player 1",
            Player::Two => "Player 2",
        }
    }
}

// ── Game mode ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameMode {
    /// Human vs AI.
    VsAi,
    /// Two human players.
    TwoPlayer,
}

impl GameMode {
    fn label(self) -> &'static str {
        match self {
            GameMode::VsAi => "vs AI",
            GameMode::TwoPlayer => "2-Player",
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    Playing,
    GameOver,
}

// ── Cursor for keyboard navigation ─────────────────────────────────
//
// There is no `Cursor` type. There was, and its three fields were
// `orientation`, `row` and `col` -- the three fields of `LineId`, declared a
// second time with a `to_line_id` to convert one into the other. The cursor
// *is* a line; a second name for it only creates the possibility that the line
// the cursor is on and the line it converts to are not the same one.

// ── What a click can land on ────────────────────────────────────────
/// Everything in the frame a pointer can press.
///
/// Recorded by the pass that paints each thing, so there is no second mapping
/// from pixels back to meaning that could disagree with the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// One of the lines between two adjacent dots.
    Line(LineId),
    NewGame,
    Mode,
    Size(usize),
}

// ── Board ───────────────────────────────────────────────────────────
/// The game board tracking all lines and box ownership.
///
/// For an NxN dot grid, there are:
/// - N*(N-1) horizontal lines
/// - (N-1)*N vertical lines
/// - (N-1)*(N-1) boxes
struct Board {
    /// Number of dots per side (e.g. 4 for a 3x3 box grid).
    grid_size: usize,
    /// Horizontal lines: `h_lines[row][col]` is true if the horizontal line
    /// from dot (row, col) to dot (row, col+1) has been drawn.
    h_lines: Vec<Vec<bool>>,
    /// Vertical lines: `v_lines[row][col]` is true if the vertical line
    /// from dot (row, col) to dot (row+1, col) has been drawn.
    v_lines: Vec<Vec<bool>>,
    /// Box ownership: `boxes[row][col]` is the player who completed the box
    /// at grid position (row, col), where the box is bounded by dots
    /// (row, col), (row, col+1), (row+1, col), (row+1, col+1).
    boxes: Vec<Vec<Option<Player>>>,
}

impl Board {
    /// Create a new empty board with the given grid size (number of dots per side).
    fn new(grid_size: usize) -> Self {
        let boxes_per_side = grid_size.saturating_sub(1);
        Self {
            grid_size,
            h_lines: vec![vec![false; boxes_per_side]; grid_size],
            v_lines: vec![vec![false; grid_size]; boxes_per_side],
            boxes: vec![vec![None; boxes_per_side]; boxes_per_side],
        }
    }

    /// Number of boxes per side.
    fn boxes_per_side(&self) -> usize {
        self.grid_size.saturating_sub(1)
    }

    /// The grid of lines of one orientation.
    fn lines(&self, orientation: Orientation) -> &Vec<Vec<bool>> {
        match orientation {
            Orientation::Horizontal => &self.h_lines,
            Orientation::Vertical => &self.v_lines,
        }
    }

    /// Whether the line at these coordinates is drawn; `false` off the board.
    ///
    /// Every read of a line goes through here.  `is_box_complete`,
    /// `box_side_count` and `available_lines` each used to index the two
    /// grids directly, so each carried its own bounds reasoning and its own
    /// way of being wrong at the edge.
    fn drawn(&self, orientation: Orientation, row: usize, col: usize) -> bool {
        self.lines(orientation)
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(false)
    }

    /// Count how many lines have been drawn.
    fn drawn_line_count(&self) -> usize {
        let count = |rows: &Vec<Vec<bool>>| rows.iter().flatten().filter(|&&d| d).count();
        count(&self.h_lines).saturating_add(count(&self.v_lines))
    }

    /// Check if a line has been drawn.
    fn is_line_drawn(&self, line: LineId) -> bool {
        self.drawn(line.orientation, line.row, line.col)
    }

    /// Check if a line ID is valid for this board.
    fn is_valid_line(&self, line: LineId) -> bool {
        let bps = self.boxes_per_side();
        match line.orientation {
            Orientation::Horizontal => line.row < self.grid_size && line.col < bps,
            Orientation::Vertical => line.row < bps && line.col < self.grid_size,
        }
    }

    /// Whether `line` is a line of this board that has not been drawn yet.
    ///
    /// This is the one rule that decides whether a click, a keypress or the
    /// AI may play a line, and `is_valid_line(line) && !is_line_drawn(line)`
    /// used to be spelled out three times over: in `draw_line`, in
    /// `try_place_line` and in the pass that records the hit boxes.
    /// `try_place_line`'s copy stood in front of `draw_line`'s, so nothing
    /// could reach the latter -- deleting either half of it changed no
    /// behaviour any test could observe (lesson 92).
    fn is_available(&self, line: LineId) -> bool {
        self.is_valid_line(line) && !self.is_line_drawn(line)
    }

    /// Draw a line for `player` and claim any box it completes.
    ///
    /// `None` if there was no such line to draw -- off the board, or already
    /// drawn -- and `Some(n)` for a move that completed `n` boxes. The two
    /// answers used to be the same `0`, so a caller could not tell a refused
    /// move from a move that merely captured nothing, which is the difference
    /// between keeping the turn and passing it.
    fn draw_line(&mut self, line: LineId, player: Player) -> Option<usize> {
        if !self.is_available(line) {
            return None;
        }

        let grid = match line.orientation {
            Orientation::Horizontal => &mut self.h_lines,
            Orientation::Vertical => &mut self.v_lines,
        };
        if let Some(cell) = grid.get_mut(line.row).and_then(|r| r.get_mut(line.col)) {
            *cell = true;
        }

        let mut completed = 0usize;
        for (br, bc) in self.adjacent_boxes(line) {
            if !self.is_box_complete(br, bc) {
                continue;
            }
            let Some(cell) = self.boxes.get_mut(br).and_then(|r| r.get_mut(bc)) else {
                continue;
            };
            if cell.is_none() {
                *cell = Some(player);
                completed = completed.saturating_add(1);
            }
        }
        Some(completed)
    }

    /// Get the box coordinates adjacent to a line.
    ///
    /// A horizontal line at (row, col) borders:
    /// - box (row-1, col) above (if row > 0)
    /// - box (row, col) below (if row < boxes_per_side)
    ///
    /// A vertical line at (row, col) borders:
    /// - box (row, col-1) to the left (if col > 0)
    /// - box (row, col) to the right (if col < boxes_per_side)
    fn adjacent_boxes(&self, line: LineId) -> Vec<(usize, usize)> {
        let bps = self.boxes_per_side();
        let mut result = Vec::new();
        match line.orientation {
            Orientation::Horizontal => {
                if line.row > 0 {
                    let br = line.row.saturating_sub(1);
                    if br < bps && line.col < bps {
                        result.push((br, line.col));
                    }
                }
                if line.row < bps && line.col < bps {
                    result.push((line.row, line.col));
                }
            }
            Orientation::Vertical => {
                if line.col > 0 {
                    let bc = line.col.saturating_sub(1);
                    if line.row < bps && bc < bps {
                        result.push((line.row, bc));
                    }
                }
                if line.row < bps && line.col < bps {
                    result.push((line.row, line.col));
                }
            }
        }
        result
    }

    /// Check if all four sides of a box are drawn.
    ///
    /// This is `box_side_count == 4` and nothing else.  It used to be its own
    /// copy of the same four reads, so "which four lines bound this box" was
    /// written down twice and the two copies could drift apart.
    fn is_box_complete(&self, box_row: usize, box_col: usize) -> bool {
        self.box_side_count(box_row, box_col) == SIDES_PER_BOX
    }

    /// Count sides drawn for a specific box.
    fn box_side_count(&self, box_row: usize, box_col: usize) -> usize {
        let bps = self.boxes_per_side();
        if box_row >= bps || box_col >= bps {
            return 0;
        }
        let sides = [
            // Top, bottom, left, right.
            self.drawn(Orientation::Horizontal, box_row, box_col),
            self.drawn(Orientation::Horizontal, box_row.saturating_add(1), box_col),
            self.drawn(Orientation::Vertical, box_row, box_col),
            self.drawn(Orientation::Vertical, box_row, box_col.saturating_add(1)),
        ];
        sides.into_iter().filter(|&s| s).count()
    }

    /// Return all valid lines that haven't been drawn yet.
    fn available_lines(&self) -> Vec<LineId> {
        let mut lines = Vec::new();
        let bps = self.boxes_per_side();
        for row in 0..self.grid_size {
            for col in 0..bps {
                if !self.drawn(Orientation::Horizontal, row, col) {
                    lines.push(LineId::horizontal(row, col));
                }
            }
        }
        for row in 0..bps {
            for col in 0..self.grid_size {
                if !self.drawn(Orientation::Vertical, row, col) {
                    lines.push(LineId::vertical(row, col));
                }
            }
        }
        lines
    }

    /// Check if all lines have been drawn (game over condition).
    fn all_lines_drawn(&self) -> bool {
        for row in &self.h_lines {
            for &drawn in row {
                if !drawn {
                    return false;
                }
            }
        }
        for row in &self.v_lines {
            for &drawn in row {
                if !drawn {
                    return false;
                }
            }
        }
        true
    }

    /// Count boxes owned by a given player.
    fn score(&self, player: Player) -> usize {
        self.boxes
            .iter()
            .flatten()
            .filter(|cell| **cell == Some(player))
            .count()
    }
}

// ── AI ──────────────────────────────────────────────────────────────
/// Greedy AI strategy:
/// 1. If any line completes a box, take it.
/// 2. Avoid lines that would give the opponent a box (lines that leave a box with 3 sides).
/// 3. Otherwise, pick a random safe line.
/// 4. If forced, pick a line that gives away the fewest boxes.
fn ai_choose_line(board: &Board, rng: &mut SeededRng) -> Option<LineId> {
    let available = board.available_lines();
    if available.is_empty() {
        return None;
    }

    // How many of a line's boxes already have exactly `sides` sides drawn.
    // Phase 1 counts boxes at three sides -- the ones this line completes.
    // Phases 2 and 3 count boxes at two -- the ones this line hands over.
    let boxes_at = |line: LineId, sides: usize| {
        board
            .adjacent_boxes(line)
            .into_iter()
            .filter(|&(br, bc)| board.box_side_count(br, bc) == sides)
            .count()
    };

    // Phase 1: take the line that completes the most boxes.
    let best_capture = available
        .iter()
        .map(|&line| (boxes_at(line, SIDES_PER_BOX.saturating_sub(1)), line))
        .filter(|&(count, _)| count > 0)
        .max_by_key(|&(count, _)| count);
    if let Some((_, line)) = best_capture {
        return Some(line);
    }

    // Phase 2: a safe line hands no box its third side. Pick one at random.
    let safe: Vec<LineId> = available
        .iter()
        .copied()
        .filter(|&line| boxes_at(line, SIDES_PER_BOX.saturating_sub(2)) == 0)
        .collect();
    if !safe.is_empty() {
        return safe.get(rng.below(safe.len())).copied();
    }

    // Phase 3: every move is dangerous. Give away the fewest boxes.
    available
        .iter()
        .copied()
        .min_by_key(|&line| boxes_at(line, SIDES_PER_BOX.saturating_sub(2)))
}

// ── Main app struct ─────────────────────────────────────────────────
struct DotsAndBoxes {
    board: Board,
    current_player: Player,
    phase: GamePhase,
    mode: GameMode,
    /// The line the keyboard is pointing at. It *is* a line, not a second
    /// three-field type that converts into one.
    cursor: LineId,
    /// The window the last frame was drawn at, so a click resolves against
    /// what the player is looking at rather than against a constant.
    size: (f32, f32),
    // There are no `score_p1`, `score_p2` or `total_moves` fields. There
    // were, raised by hand in `try_place_line` and again in `do_ai_move`,
    // while `Board::score` and `Board::drawn_line_count` computed the same
    // two answers from the grid. Four assignments maintaining a fact the
    // board already holds is four chances for the readout and the board to
    // disagree; `score` and `moves_made` below derive them instead.
    rng: SeededRng,
    /// Accumulated time for AI delay.
    ai_delay_ms: u64,
    /// Whether the AI is "thinking" (short delay before move).
    ai_pending: bool,
}

/// AI thinking delay in milliseconds.
const AI_DELAY: u64 = 400;

impl DotsAndBoxes {
    fn new() -> Self {
        Self::with_config(
            DEFAULT_GRID_SIZE,
            GameMode::VsAi,
            seed_from_system(FALLBACK_SEED),
        )
    }

    fn with_config(grid_size: usize, mode: GameMode, seed: u64) -> Self {
        let clamped_size = grid_size.clamp(MIN_GRID_SIZE, MAX_GRID_SIZE);
        Self {
            board: Board::new(clamped_size),
            current_player: Player::One,
            phase: GamePhase::Playing,
            mode,
            cursor: LineId::horizontal(0, 0),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            rng: SeededRng::new(seed),
            ai_delay_ms: 0,
            ai_pending: false,
        }
    }

    /// Start a new game with the current grid size and mode.
    fn new_game(&mut self) {
        let grid = self.board.grid_size;
        self.new_game_with_size(grid);
    }

    /// Start a new game with a specific grid size.
    ///
    /// The window survives it. `*self = Self::with_config(..)` replaces every
    /// field, and one of them is the size the last frame was drawn at -- so
    /// without carrying it across, pressing New in a resized window sent the
    /// next click back to the 560x620 the app opened at, and the board the
    /// player could see stopped answering the pointer.
    fn new_game_with_size(&mut self, grid_size: usize) {
        let mode = self.mode;
        let seed = self.rng.next_u64();
        let window = self.size;
        *self = Self::with_config(grid_size, mode, seed);
        self.size = window;
    }

    /// Grid size (number of dots per side).
    fn grid_size(&self) -> usize {
        self.board.grid_size
    }

    /// Number of boxes per side.
    fn boxes_per_side(&self) -> usize {
        self.board.boxes_per_side()
    }

    /// Try to place a line at the given position. Returns true if a line was placed.
    fn try_place_line(&mut self, line: LineId) -> bool {
        if !self.accepts_moves() {
            return false;
        }

        // Whether the line can be played is `draw_line`'s rule, and asking it
        // by trying is what keeps the answer in one place.
        let Some(completed) = self.board.draw_line(line, self.current_player) else {
            return false;
        };

        if self.board.all_lines_drawn() {
            self.phase = GamePhase::GameOver;
            return true;
        }

        // If the player completed a box, they get another turn.
        if completed == 0 {
            self.current_player = self.current_player.other();
            // If it's now the AI's turn, start the delay.
            if self.mode == GameMode::VsAi && self.current_player == Player::Two {
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            }
        } else {
            // Player completed a box and gets another turn.
            // If the current player is AI, start another AI move.
            if self.mode == GameMode::VsAi && self.current_player == Player::Two {
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            }
        }
        true
    }

    /// Execute the AI's move.
    fn do_ai_move(&mut self) {
        if self.phase != GamePhase::Playing {
            return;
        }
        if let Some(line) = ai_choose_line(&self.board, &mut self.rng) {
            // `ai_choose_line` picks from `available_lines`, so the draw
            // cannot be refused; treating a refusal as "captured nothing"
            // would pass the turn, which is the safe reading either way.
            let completed = self.board.draw_line(line, Player::Two).unwrap_or(0);

            if self.board.all_lines_drawn() {
                self.phase = GamePhase::GameOver;
                self.ai_pending = false;
                return;
            }

            if completed > 0 {
                // AI gets another turn.
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            } else {
                self.current_player = Player::One;
                self.ai_pending = false;
            }
        } else {
            self.ai_pending = false;
        }
    }

    /// How many boxes a player has claimed, read off the board.
    fn score(&self, player: Player) -> usize {
        self.board.score(player)
    }

    /// How many lines have been drawn, read off the board.
    fn moves_made(&self) -> usize {
        self.board.drawn_line_count()
    }

    /// Get the winner, if any. Returns None for a draw.
    fn winner(&self) -> Option<Player> {
        let (one, two) = (self.score(Player::One), self.score(Player::Two));
        match one.cmp(&two) {
            std::cmp::Ordering::Greater => Some(Player::One),
            std::cmp::Ordering::Less => Some(Player::Two),
            std::cmp::Ordering::Equal => None,
        }
    }

    // ── Cursor navigation ──────────────────────────────────────────

    /// The extent of the line grid of one orientation, as `(rows, columns)`.
    ///
    /// Horizontal lines run `grid_size` rows of `boxes_per_side` columns;
    /// vertical lines are the transpose. `move_cursor` used to spell this out
    /// once per direction per orientation -- eight arms of the same rule --
    /// and `toggle_cursor_orientation` spelled it out twice more.
    fn cursor_extent(&self, orientation: Orientation) -> (usize, usize) {
        let bps = self.boxes_per_side();
        let gs = self.grid_size();
        match orientation {
            Orientation::Horizontal => (gs, bps),
            Orientation::Vertical => (bps, gs),
        }
    }

    /// Move the cursor in the given direction, wrapping around.
    ///
    /// Left and right wrap within the row. Up and down step off the top or the
    /// bottom into the *other* orientation's grid, entering it from the far
    /// side, with the column clamped to what that grid actually has.
    fn move_cursor(&mut self, key: Key) {
        let (rows, cols) = self.cursor_extent(self.cursor.orientation);
        let other = self.cursor.orientation.toggled();
        let (other_rows, other_cols) = self.cursor_extent(other);
        let flip_to = |cursor: &mut LineId, row: usize| {
            cursor.orientation = other;
            cursor.row = row;
            cursor.col = cursor.col.min(other_cols.saturating_sub(1));
        };
        match key {
            Key::Left => {
                self.cursor.col = if self.cursor.col > 0 {
                    self.cursor.col.saturating_sub(1)
                } else {
                    cols.saturating_sub(1)
                };
            }
            Key::Right => {
                let next = self.cursor.col.saturating_add(1);
                self.cursor.col = if next < cols { next } else { 0 };
            }
            Key::Up => {
                if self.cursor.row > 0 {
                    self.cursor.row = self.cursor.row.saturating_sub(1);
                } else {
                    flip_to(&mut self.cursor, other_rows.saturating_sub(1));
                }
            }
            Key::Down => {
                let next = self.cursor.row.saturating_add(1);
                if next < rows {
                    self.cursor.row = next;
                } else {
                    flip_to(&mut self.cursor, 0);
                }
            }
            _ => {}
        }
    }

    /// Toggle cursor orientation between horizontal and vertical.
    fn toggle_cursor_orientation(&mut self) {
        let other = self.cursor.orientation.toggled();
        let (rows, cols) = self.cursor_extent(other);
        self.cursor.orientation = other;
        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(cols.saturating_sub(1));
    }

    // ── Board geometry ─────────────────────────────────────────────
    //
    // There used to be two hit tests. `handle_mouse` called
    // `hit_test_line_precise`, which measures the perpendicular distance to
    // each line *segment*, so the whole 70px length of a line was clickable.
    // `hit_test_line` measured the distance to each segment's *midpoint*
    // instead, so only a 12px disc around the middle of a line answered at all
    // and the two ends were dead. They were not two spellings of one rule --
    // they were two different rules -- and the app ran one of them while three
    // of the tests were pointed at the other. A test aimed at a function
    // nothing calls is green about code that does not ship; see
    // design-decisions.md §486. The midpoint version is gone.
    //
    // What is left of that story now lives in `Layout`: the endpoints of a
    // line are written once, and the *only* way a click reaches a line is the
    // hit box the drawing pass records for it. There is no `hit_test_line` at
    // all -- an inverse of the drawing mapping is a second mapping, and two
    // mappings agree with each other in every window and with the screen in
    // one.

    /// Every line on the board, drawn or not.
    fn all_lines(&self) -> impl Iterator<Item = LineId> + use<> {
        let gs = self.grid_size();
        let bps = self.boxes_per_side();
        let horizontals =
            (0..gs).flat_map(move |row| (0..bps).map(move |col| LineId::horizontal(row, col)));
        let verticals =
            (0..bps).flat_map(move |row| (0..gs).map(move |col| LineId::vertical(row, col)));
        horizontals.chain(verticals)
    }

    // ── Event handling ─────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(ke) if ke.pressed => self.handle_key(ke.key),
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: Key) -> EventResult {
        // The verbs a key and a button share are `activate`d rather than
        // spelled a second time here: `N` and the New button run the same
        // line, so the two cannot drift apart.
        match key {
            Key::N => self.activate(Target::NewGame),
            Key::M => self.activate(Target::Mode),
            Key::Num3 => self.activate(Target::Size(3)),
            Key::Num4 => self.activate(Target::Size(4)),
            Key::Num5 => self.activate(Target::Size(5)),
            Key::Left | Key::Right | Key::Up | Key::Down if self.accepts_moves() => {
                self.move_cursor(key);
                EventResult::Consumed
            }
            Key::Tab if self.accepts_moves() => {
                self.toggle_cursor_orientation();
                EventResult::Consumed
            }
            Key::Enter | Key::Space if self.accepts_moves() => {
                let line = self.cursor;
                self.activate(Target::Line(line))
            }
            _ => EventResult::Ignored,
        }
    }

    /// Whether the board is taking input from the player right now.
    ///
    /// Written once. It was written five times -- `phase == Playing &&
    /// !ai_pending` in four key arms and again in `handle_mouse` -- and
    /// `try_place_line` holds a sixth copy that is the one that actually
    /// protects the board.
    fn accepts_moves(&self) -> bool {
        self.phase == GamePhase::Playing && !self.ai_pending
    }

    /// Do what a target does, whether a key or a click asked for it.
    fn activate(&mut self, target: Target) -> EventResult {
        match target {
            Target::Line(line) => {
                if self.try_place_line(line) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::NewGame => {
                self.new_game();
                EventResult::Consumed
            }
            Target::Mode => {
                self.mode = match self.mode {
                    GameMode::VsAi => GameMode::TwoPlayer,
                    GameMode::TwoPlayer => GameMode::VsAi,
                };
                self.new_game();
                EventResult::Consumed
            }
            Target::Size(n) => {
                self.new_game_with_size(n);
                EventResult::Consumed
            }
        }
    }

    fn handle_mouse(&mut self, me: &MouseEvent) -> EventResult {
        let MouseEventKind::Press(MouseButton::Left) = me.kind else {
            return EventResult::Ignored;
        };
        let (w, h) = self.size;
        match self.frame(w, h).hit_test(me.x, me.y) {
            Some(target) => self.activate(target),
            None => EventResult::Ignored,
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) -> EventResult {
        if self.ai_pending && self.phase == GamePhase::Playing {
            self.ai_delay_ms = self.ai_delay_ms.saturating_add(elapsed_ms);
            if self.ai_delay_ms >= AI_DELAY {
                self.ai_pending = false;
                self.do_ai_move();
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    // ── Rendering ──────────────────────────────────────────────────

    /// The frame, with a hit box on every line and every verb.
    ///
    /// `render` used to take the window and use it for four things -- the
    /// background, the header bar's width, the footer bar's width and the
    /// game-over wash -- while the board, the fonts and the footer's *position*
    /// came from constants. Everything is measured from `Layout` now.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h, self.grid_size());
        let mut f = Frame::new(w, h);

        fill(&mut f, l.window, BASE, 6.0);
        self.draw_header(&mut f, &l);
        self.draw_boxes(&mut f, &l);
        self.draw_lines(&mut f, &l);
        if self.accepts_moves() {
            self.draw_cursor(&mut f, &l);
        }
        self.draw_dots(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.phase == GamePhase::GameOver {
            self.draw_game_over(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, 0.0);
        if l.header.is_empty() {
            return;
        }

        text_at(
            f,
            l.pad,
            l.pad * 0.4,
            TITLE,
            LAVENDER,
            l.title,
            FontWeightHint::Bold,
        );
        let mode_text = format!(
            "{} | {1}x{1} | {2} moves",
            self.mode.label(),
            self.grid_size(),
            self.moves_made()
        );
        text_at(
            f,
            l.pad,
            l.pad.mul_add(0.4, l.title),
            &mode_text,
            SUBTEXT0,
            l.small,
            FontWeightHint::Regular,
        );

        // The scores and the turn indicator were drawn at `win_width - 200.0`
        // and `win_width - 100.0`: a width used as a coordinate, with an
        // offset that was right for one set of words. They are measured and
        // laid out from the right edge inwards now, and a window too narrow to
        // hold one drops it rather than drawing it over the title.
        let right = l.header.right() - l.pad;
        let turn = self.turn_text();
        let turn_w = text::measure(&turn, l.small, FontWeightHint::Bold);
        let scores = [
            (self.player_one_label(), PLAYER1_COLOR),
            (self.player_two_label(), PLAYER2_COLOR),
        ];
        let score_w = scores.iter().fold(0.0f32, |acc, (s, _)| {
            acc.max(text::measure(s, l.small, FontWeightHint::Bold))
        });

        let title_end = l.pad
            + text::measure(TITLE, l.title, FontWeightHint::Bold).max(text::measure(
                &mode_text,
                l.small,
                FontWeightHint::Regular,
            ));
        let turn_x = right - turn_w;
        if turn_x > title_end {
            text_at(
                f,
                turn_x,
                (l.header.h - l.small) / 2.0,
                &turn,
                self.current_player.color(),
                l.small,
                FontWeightHint::Bold,
            );
        }
        let score_x = turn_x - l.pad - score_w;
        if score_x > title_end {
            for (i, (s, colour)) in scores.iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "two scores")]
                let row = i as f32;
                text_at(
                    f,
                    score_x,
                    l.pad.mul_add(0.4, row * l.small * 1.5),
                    s,
                    *colour,
                    l.small,
                    FontWeightHint::Bold,
                );
            }
        }
    }

    /// What the header says about whose turn it is.
    fn turn_text(&self) -> String {
        if self.phase == GamePhase::GameOver {
            return String::from("Game Over!");
        }
        if self.ai_pending {
            return String::from("AI thinking...");
        }
        let name = match (self.mode, self.current_player) {
            (GameMode::VsAi, Player::One) => "Your turn",
            (GameMode::VsAi, Player::Two) => "AI's turn",
            (GameMode::TwoPlayer, Player::One) => "P1's turn",
            (GameMode::TwoPlayer, Player::Two) => "P2's turn",
        };
        String::from(name)
    }

    fn player_one_label(&self) -> String {
        if self.mode == GameMode::VsAi {
            format!("You: {}", self.score(Player::One))
        } else {
            format!("{}: {}", Player::One.label(), self.score(Player::One))
        }
    }

    fn player_two_label(&self) -> String {
        if self.mode == GameMode::VsAi {
            format!("AI: {}", self.score(Player::Two))
        } else {
            format!("{}: {}", Player::Two.label(), self.score(Player::Two))
        }
    }

    /// The single letter a claimed box carries.
    fn box_initial(&self, player: Player) -> &'static str {
        match (self.mode, player) {
            (GameMode::VsAi, Player::One) => "Y",
            (GameMode::VsAi, Player::Two) => "A",
            (GameMode::TwoPlayer, Player::One) => "1",
            (GameMode::TwoPlayer, Player::Two) => "2",
        }
    }

    fn draw_boxes(&self, f: &mut Frame<Target>, l: &Layout) {
        let bps = self.boxes_per_side();
        let margin = l.spacing * 0.06;
        for row in 0..bps {
            for col in 0..bps {
                let Some(Some(player)) = self.board.boxes.get(row).map(|r| r.get(col).copied())
                else {
                    continue;
                };
                let Some(player) = player else { continue };
                let (x1, y1) = l.dot_pos(row, col);
                let (x2, y2) = l.dot_pos(row.saturating_add(1), col.saturating_add(1));
                let cell = Rect::new(
                    x1 + margin,
                    y1 + margin,
                    (x2 - x1 - margin * 2.0).max(0.0),
                    (y2 - y1 - margin * 2.0).max(0.0),
                );
                fill(f, cell, player.box_color(), l.spacing * 0.06);

                // The initial was centred by subtracting 5 and 8 -- half of
                // one glyph at one font size. It is measured now.
                let label = self.box_initial(player);
                let size = (l.spacing * 0.28).clamp(6.0, 18.0);
                let (cx, cy) = cell.centre();
                text_at(
                    f,
                    cx - text::measure(label, size, FontWeightHint::Bold) / 2.0,
                    cy - size / 2.0,
                    label,
                    player.color(),
                    size,
                    FontWeightHint::Bold,
                );
            }
        }
    }

    /// Every line, painted -- and every line, recorded as a hit box.
    ///
    /// The two happen in the same loop deliberately. A line that is drawn
    /// somewhere the player cannot click, or clickable somewhere nothing is
    /// drawn, would need the two to be written apart.
    fn draw_lines(&self, f: &mut Frame<Target>, l: &Layout) {
        for line in self.all_lines() {
            let ((x1, y1), (x2, y2)) = l.line_endpoints(line);
            let drawn = self.board.is_line_drawn(line);
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: if drawn { LAVENDER } else { SURFACE0 },
                width: if drawn {
                    l.line_w
                } else {
                    (l.line_w * 0.4).max(1.0)
                },
            });
            if !drawn && self.accepts_moves() {
                f.hit(Target::Line(line), l.line_box(line));
            }
        }
    }

    fn draw_cursor(&self, f: &mut Frame<Target>, l: &Layout) {
        let line = self.cursor;
        if !self.board.is_available(line) {
            return;
        }
        let ((x1, y1), (x2, y2)) = l.line_endpoints(line);
        f.push(RenderCommand::Line {
            x1,
            y1,
            x2,
            y2,
            color: self.current_player.color(),
            width: l.cursor_w,
        });
    }

    fn draw_dots(&self, f: &mut Frame<Target>, l: &Layout) {
        let gs = self.grid_size();
        let r = l.dot_radius;
        for row in 0..gs {
            for col in 0..gs {
                let (x, y) = l.dot_pos(row, col);
                fill(f, Rect::new(x - r, y - r, r * 2.0, r * 2.0), TEXT_COLOR, r);
            }
        }
    }

    /// The footer, and the verbs in it.
    ///
    /// It used to be one line of text naming six keys -- a label describing
    /// keystrokes where buttons would do -- painted at
    /// `window_height() - FOOTER_HEIGHT`, the bottom of a window the app had
    /// decided on rather than the one it was given. Every verb is a button
    /// now, and the bar sits at the bottom of the actual window.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, 0.0);
        if l.footer.is_empty() {
            return;
        }

        let size = (l.small * 0.95).max(7.0);
        let gap = l.pad * 0.4;
        let mut buttons: Vec<(String, Target, bool)> = vec![
            (String::from("New"), Target::NewGame, false),
            (String::from(self.mode.label()), Target::Mode, false),
        ];
        for n in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            buttons.push((format!("{n}x{n}"), Target::Size(n), n == self.grid_size()));
        }

        // Laid out from the left, and any button that would not fit whole is
        // left out rather than drawn off the edge.
        let mut x = l.pad;
        let h = (l.footer.h - gap).max(0.0);
        let y = l.footer.y + (l.footer.h - h) / 2.0;
        for (label, target, on) in &buttons {
            let w = text::measure(label, size, FontWeightHint::Bold) + l.pad;
            if x + w > l.footer.right() - l.pad {
                break;
            }
            let r = Rect::new(x, y, w, h);
            fill(f, r, if *on { SURFACE1 } else { SURFACE0 }, h * 0.25);
            let (cx, cy) = r.centre();
            text_at(
                f,
                cx - text::measure(label, size, FontWeightHint::Bold) / 2.0,
                cy - size / 2.0,
                label,
                if *on { TEXT_COLOR } else { SUBTEXT0 },
                size,
                FontWeightHint::Bold,
            );
            f.hit(*target, r);
            x += w + gap;
        }

        // The list of keys, if what is left can hold it.
        let help_w = text::measure(FOOTER_HELP, size * 0.9, FontWeightHint::Regular);
        if x + help_w <= l.footer.right() - l.pad {
            text_at(
                f,
                l.footer.right() - l.pad - help_w,
                y + (h - size * 0.9) / 2.0,
                FOOTER_HELP,
                OVERLAY0,
                size * 0.9,
                FontWeightHint::Regular,
            );
        }
    }

    /// The end-of-game card.
    ///
    /// It was a fixed 260x140 box with its four lines at fixed offsets inside
    /// it, so in a window narrower than 260 it hung off both edges and in a
    /// short one it hung off the bottom. It is sized from the lines it holds
    /// and clamped to the window now.
    fn draw_game_over(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, CRUST, 0.0);

        let result = match self.winner() {
            Some(Player::One) if self.mode == GameMode::VsAi => String::from("You win!"),
            Some(Player::One) => String::from("Player 1 wins!"),
            Some(Player::Two) if self.mode == GameMode::VsAi => String::from("AI wins!"),
            Some(Player::Two) => String::from("Player 2 wins!"),
            None => String::from("It's a draw!"),
        };
        let score = format!(
            "Score: {} - {}",
            self.score(Player::One),
            self.score(Player::Two)
        );
        let lines: [(&str, f32, Color, FontWeightHint); 4] = [
            ("Game Over!", l.title, YELLOW, FontWeightHint::Bold),
            (&result, l.font, TEXT_COLOR, FontWeightHint::Regular),
            (&score, l.small, SUBTEXT0, FontWeightHint::Regular),
            (
                "Press N or the New button",
                l.small,
                OVERLAY0,
                FontWeightHint::Regular,
            ),
        ];

        let widest = lines.iter().fold(0.0f32, |acc, (s, size, _, weight)| {
            acc.max(text::measure(s, *size, *weight))
        });
        let text_h = lines
            .iter()
            .fold(0.0f32, |acc, (_, size, _, _)| acc + size * 1.9);
        let card_w = (widest + l.pad * 2.0).min(l.window.w);
        let card_h = (text_h + l.pad * 2.0).min(l.window.h);
        let card = Rect::new(
            (l.window.w - card_w) / 2.0,
            (l.window.h - card_h) / 2.0,
            card_w,
            card_h,
        );
        fill(f, card, SURFACE0, l.pad * 0.4);

        let mut y = card.y + l.pad;
        for (s, size, colour, weight) in lines {
            text_at(
                f,
                card.x + (card.w - text::measure(s, size, weight)) / 2.0,
                y,
                s,
                colour,
                size,
                weight,
            );
            y += size * 1.9;
        }
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A filled rectangle, skipped when there is nothing to fill.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {
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

/// One run of text.
fn text_at(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

// There is no `point_to_segment_distance`. It was the arithmetic behind the
// nearest-line hit test: project the click onto every one of the board's 24 to
// 60 segments, keep the closest within a threshold. A geometry routine that
// decides which line was clicked has to agree with the geometry routine that
// decides where the line was drawn, and nothing made them agree -- the drawn
// line came from `line_endpoints`, the click came from here, and the reach was
// a constant. The drawing pass now records a hit box for each line as it
// paints it, so a click resolves against the rectangle the line was actually
// put in, and this had no remaining caller.

// ── The app ─────────────────────────────────────────────────────────

impl App for DotsAndBoxes {
    fn title(&self) -> String {
        String::from(TITLE)
    }

    fn app_id(&self) -> String {
        String::from("dots")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // The AI waits `AI_DELAY` before it moves, so that "AI thinking..." is
        // a state a frame can be drawn in rather than a string that is true
        // only for the duration of a function call.
        Some(std::time::Duration::from_millis(40))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for DotsAndBoxes {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(&mut self, _x: f32, _y: f32, _dy: f32, _size: (f32, f32)) -> Option<EventResult> {
        None
    }
}

fn main() -> ExitCode {
    let mut app = DotsAndBoxes::new();
    app::launch("dots", &mut app)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![expect(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::panic,
        clippy::float_cmp,
        reason = "test code: a panic is a diagnosis"
    )]

    use super::*;
    use guitk::probe::{click_sized, is_visible_sized, press, rect_of_sized};

    // ── Fixtures ───────────────────────────────────────────────────

    /// The windows every geometric claim is checked at.
    ///
    /// The board is square, so the *shorter* side is the one that pays for it.
    /// A list of near-4:3 windows would let width and height take turns being
    /// the binding constraint without ever making that visible -- so this list
    /// deliberately spans both orders, both extremes and the degenerate cases:
    /// wider than tall, taller than wide, exactly square, one so short the
    /// header and the footer have already spent the height, and one so narrow
    /// the footer cannot hold a single button.
    const SIZES: [(f32, f32); 12] = [
        (560.0, 620.0),
        (320.0, 240.0),
        (240.0, 320.0),
        (400.0, 400.0),
        (1280.0, 800.0),
        (800.0, 1280.0),
        (1920.0, 1080.0),
        (200.0, 900.0),
        (900.0, 200.0),
        (140.0, 140.0),
        (900.0, 60.0),
        (60.0, 900.0),
    ];

    fn test_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(4, GameMode::VsAi, 12345)
    }

    fn two_player_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(4, GameMode::TwoPlayer, 12345)
    }

    fn small_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(3, GameMode::TwoPlayer, 99)
    }

    /// Draw `line` while setting a fixture up, insisting it was really drawn.
    ///
    /// `draw_line` answers `None` for a line the board refused -- off the
    /// board, or already there -- so a fixture with a typo in it used to set
    /// up a position other than the one it described and the test went on to
    /// pass about the wrong board (lesson 90). This is where that becomes a
    /// failure instead.
    fn set(b: &mut Board, line: LineId) {
        assert!(
            b.draw_line(line, Player::One).is_some(),
            "the fixture could not draw {line:?}"
        );
    }

    /// Every rectangle the frame recorded a hit box for.
    fn hit_boxes(app: &DotsAndBoxes, size: (f32, f32)) -> Vec<(Target, Rect)> {
        app.draw(size).hits().to_vec()
    }

    /// Every rectangle the frame filled.
    fn painted_rects(app: &DotsAndBoxes, size: (f32, f32)) -> Vec<Rect> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(x, y, width, height)),
                _ => None,
            })
            .collect()
    }

    /// Every line segment the frame painted, as `((x1, y1), (x2, y2))`.
    fn painted_lines(app: &DotsAndBoxes, size: (f32, f32)) -> Vec<((f32, f32), (f32, f32))> {
        app.draw(size)
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::Line { x1, y1, x2, y2, .. } => Some(((x1, y1), (x2, y2))),
                _ => None,
            })
            .collect()
    }

    // ── The layout follows the window ──────────────────────────────

    #[test]
    fn the_layout_follows_the_window_rather_than_a_constant() {
        // Each pair has to sit in the band where the quantity being compared
        // is free to move, or the comparison is between two *clamped* values
        // and passes against a program that ignores the window entirely. The
        // first pair written here was 400 against 1200 and read 291.6 against
        // 291.6: `spacing` saturates `MAX_SPACING` above about 385px square.
        // `font` is clamped to 9 below about 306px tall and to 18 above 612,
        // so no one pair can exercise both.

        // The board grows with the window, below the cap.
        let small = Layout::solve(150.0, 150.0, 4);
        let large = Layout::solve(300.0, 300.0, 4);
        assert!(
            small.spacing < MAX_SPACING && large.spacing < MAX_SPACING,
            "the board fixture saturated the cap: {} and {}",
            small.spacing,
            large.spacing
        );
        assert!(
            large.board.w > small.board.w * 1.8,
            "a window twice as wide drew a board {} wide against {}",
            large.board.w,
            small.board.w
        );

        // The type grows with the window, inside its own clamp.
        let short = Layout::solve(400.0, 340.0, 4);
        let tall = Layout::solve(400.0, 600.0, 4);
        assert!(
            short.font > 9.0 && tall.font < 18.0,
            "the type fixture hit a clamp: {} and {}",
            short.font,
            tall.font
        );
        assert!(
            tall.font > short.font * 1.5,
            "the type did not grow with the window: {} vs {}",
            tall.font,
            short.font
        );

        assert_eq!(small.window, Rect::new(0.0, 0.0, 150.0, 150.0));
    }

    #[test]
    fn the_board_is_square_at_every_window_shape() {
        for (w, h) in SIZES {
            for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
                let l = Layout::solve(w, h, gs);
                assert!(
                    (l.board.w - l.board.h).abs() < 0.01,
                    "{w}x{h} grid {gs}: the board is {}x{}, not square",
                    l.board.w,
                    l.board.h
                );
            }
        }
    }

    #[test]
    fn the_board_is_centred_in_what_the_two_bars_leave() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, 4);
            if l.board.w <= 0.0 {
                continue;
            }
            let left = l.board.x;
            let right = w - l.board.right();
            assert!(
                (left - right).abs() < 0.01,
                "{w}x{h}: {left} of slack on the left and {right} on the right"
            );
            let above = l.board.y - l.header.bottom();
            let below = l.footer.y - l.board.bottom();
            assert!(
                (above - below).abs() < 0.01,
                "{w}x{h}: {above} above the board and {below} below it"
            );
        }
    }

    #[test]
    fn every_part_of_the_layout_stays_inside_the_window() {
        for (w, h) in SIZES {
            for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
                let l = Layout::solve(w, h, gs);
                for (name, r) in [
                    ("header", l.header),
                    ("footer", l.footer),
                    ("board", l.board),
                ] {
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{w}x{h} grid {gs}: the {name} is {r:?}, outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_header_the_board_and_the_footer_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, 5);
            if l.board.is_empty() {
                continue;
            }
            assert!(
                l.board.y >= l.header.bottom() - 0.01,
                "{w}x{h}: the board starts at {} , above the header's bottom {}",
                l.board.y,
                l.header.bottom()
            );
            assert!(
                l.board.bottom() <= l.footer.y + 0.01,
                "{w}x{h}: the board ends at {} , below the footer's top {}",
                l.board.bottom(),
                l.footer.y
            );
        }
    }

    #[test]
    fn the_dots_stop_growing_before_they_become_saucers() {
        // Without the cap a 4K window would draw five dinner plates joined by
        // beams. `MAX_SPACING` bounds the lattice; the board then keeps the
        // centring slack instead of spending it.
        let l = Layout::solve(4000.0, 4000.0, 3);
        assert!(
            l.spacing <= MAX_SPACING + 0.01,
            "spacing ran to {} in a 4000px window",
            l.spacing
        );
        assert!(
            l.board.w < 4000.0 * 0.2,
            "the board took {} of a 4000px window",
            l.board.w
        );
    }

    #[test]
    fn a_window_with_no_room_for_a_board_draws_none_and_offers_no_lines() {
        // The header and the footer have spent the height; there is nothing
        // left. The board must be empty rather than negative, and no line may
        // claim a hit box in a board that was never drawn.
        //
        // 900x60 is *not* one of these: 60px of height still leaves 8.6px
        // between the bars, so the program draws a real -- microscopic --
        // lattice with 1.6px hit boxes. That is small, not absent, and the
        // containment tests already cover it. Asserting emptiness there would
        // be asserting a threshold the program does not have.
        for size in [(30.0, 30.0), (10.0, 10.0), (0.0, 0.0)] {
            let l = Layout::solve(size.0, size.1, 4);
            assert!(
                l.board.w >= 0.0 && l.board.h >= 0.0,
                "{size:?}: the board is {:?}",
                l.board
            );
            let app = test_app();
            for (target, r) in hit_boxes(&app, size) {
                if let Target::Line(_) = target {
                    assert!(
                        r.is_empty(),
                        "{size:?}: {target:?} kept a hit box {r:?} in a window with no board"
                    );
                }
            }
        }
    }

    // ── The lattice ────────────────────────────────────────────────

    #[test]
    fn the_dots_are_an_even_lattice_that_fills_the_board_square() {
        for (w, h) in SIZES {
            for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
                let l = Layout::solve(w, h, gs);
                if l.spacing <= 0.0 {
                    continue;
                }
                // Adjacent dots are one spacing apart, in both axes.
                for row in 0..gs {
                    for col in 1..gs {
                        let a = l.dot_pos(row, col - 1);
                        let b = l.dot_pos(row, col);
                        assert!(
                            (b.0 - a.0 - l.spacing).abs() < 0.01,
                            "{w}x{h} grid {gs}: dots {col} and {} of row {row} are {} apart, not {}",
                            col - 1,
                            b.0 - a.0,
                            l.spacing
                        );
                        assert_eq!(a.1, b.1, "{w}x{h} grid {gs}: row {row} is not level");
                    }
                }
                // The lattice reaches exactly one radius inside each edge.
                let first = l.dot_pos(0, 0);
                let last = l.dot_pos(gs - 1, gs - 1);
                assert!(
                    (first.0 - l.board.x - l.dot_radius).abs() < 0.01,
                    "{w}x{h} grid {gs}: the first dot sits {} from the board's left edge, not {}",
                    first.0 - l.board.x,
                    l.dot_radius
                );
                assert!(
                    (l.board.right() - last.0 - l.dot_radius).abs() < 0.01,
                    "{w}x{h} grid {gs}: the last dot sits {} from the board's right edge, not {}",
                    l.board.right() - last.0,
                    l.dot_radius
                );
            }
        }
    }

    #[test]
    fn every_line_is_painted_between_the_two_dots_it_joins() {
        let app = test_app();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, app.grid_size());
            if l.spacing <= 0.0 {
                continue;
            }
            let painted = painted_lines(&app, (w, h));
            for line in app.all_lines() {
                let a = l.dot_pos(line.row, line.col);
                let b = match line.orientation {
                    Orientation::Horizontal => l.dot_pos(line.row, line.col + 1),
                    Orientation::Vertical => l.dot_pos(line.row + 1, line.col),
                };
                assert!(
                    painted.iter().any(|&(p, q)| (p.0 - a.0).abs() < 0.01
                        && (p.1 - a.1).abs() < 0.01
                        && (q.0 - b.0).abs() < 0.01
                        && (q.1 - b.1).abs() < 0.01),
                    "{w}x{h}: {line:?} was not painted from {a:?} to {b:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_is_painted_outside_the_window() {
        let mut app = two_player_app();
        // A claimed box, a cursor and a game-over card all draw; check the
        // ordinary board and the end card both stay inside.
        app.try_place_line(LineId::horizontal(0, 0));
        app.try_place_line(LineId::vertical(0, 0));
        app.try_place_line(LineId::vertical(0, 1));
        app.try_place_line(LineId::horizontal(1, 0));
        for (w, h) in SIZES {
            for r in painted_rects(&app, (w, h)) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{w}x{h}: painted {r:?} outside the window"
                );
            }
            for (p, q) in painted_lines(&app, (w, h)) {
                for (x, y) in [p, q] {
                    assert!(
                        x >= -0.01 && y >= -0.01 && x <= w + 0.01 && y <= h + 0.01,
                        "{w}x{h}: painted a line endpoint at ({x}, {y})"
                    );
                }
            }
        }
    }

    // ── The hit boxes ──────────────────────────────────────────────

    #[test]
    fn every_undrawn_line_has_a_hit_box_and_it_is_on_the_line() {
        let app = test_app();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, app.grid_size());
            if l.spacing <= 0.0 {
                continue;
            }
            for line in app.all_lines() {
                let r = rect_of_sized(&app, Target::Line(line), (w, h))
                    .unwrap_or_else(|| panic!("{w}x{h}: {line:?} has no hit box"));
                let ((x1, y1), (x2, y2)) = l.line_endpoints(line);
                // The box straddles the segment it belongs to: its centre is
                // the segment's midpoint, to a pixel.
                let (cx, cy) = r.centre();
                assert!(
                    (cx - f32::midpoint(x1, x2)).abs() < 0.01
                        && (cy - f32::midpoint(y1, y2)).abs() < 0.01,
                    "{w}x{h}: {line:?} is painted {:?}..{:?} but its box centres on {:?}",
                    (x1, y1),
                    (x2, y2),
                    (cx, cy)
                );
                // And it never reaches past the ends of the segment -- with
                // `reach` of slop across the line, which is the band's whole
                // width, but none along it.
                assert!(
                    r.x >= x1.min(x2) - l.reach - 0.01
                        && r.right() <= x1.max(x2) + l.reach + 0.01
                        && r.y >= y1.min(y2) - l.reach - 0.01
                        && r.bottom() <= y1.max(y2) + l.reach + 0.01,
                    "{w}x{h}: {line:?} claims {r:?}, past its own endpoints"
                );
            }
        }
    }

    #[test]
    fn no_two_lines_claim_the_same_pixel() {
        // The old hit test walked every segment and kept the nearest, so a
        // click at a dot -- where four segments meet -- was resolved by
        // whichever comparison happened to win. The boxes are inset at both
        // ends so that question never arises.
        let app = test_app();
        for (w, h) in SIZES {
            let boxes: Vec<(Target, Rect)> = hit_boxes(&app, (w, h))
                .into_iter()
                .filter(|(t, r)| matches!(t, Target::Line(_)) && !r.is_empty())
                .collect();
            for (i, (a_t, a)) in boxes.iter().enumerate() {
                for (b_t, b) in boxes.iter().skip(i + 1) {
                    let overlap = a.intersect(*b).filter(|o| o.w > 0.01 && o.h > 0.01);
                    assert!(
                        overlap.is_none(),
                        "{w}x{h}: {a_t:?} at {a:?} and {b_t:?} at {b:?} overlap in {overlap:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_in_a_lines_box_draws_that_line_and_no_other() {
        for (w, h) in SIZES {
            let probe = test_app();
            let l = Layout::solve(w, h, probe.grid_size());
            if l.spacing <= 1.0 {
                continue;
            }
            for line in probe.all_lines() {
                let mut app = two_player_app();
                let r = rect_of_sized(&app, Target::Line(line), (w, h)).unwrap();
                let (cx, cy) = r.centre();
                let out = click_sized(&mut app, Target::Line(line), MouseButton::Left, (w, h));
                assert_eq!(
                    out,
                    EventResult::Consumed,
                    "{w}x{h}: a click at ({cx}, {cy}) on {line:?} was ignored"
                );
                assert!(
                    app.board.is_line_drawn(line),
                    "{w}x{h}: clicking {line:?} drew something else"
                );
                assert_eq!(
                    app.moves_made(),
                    1,
                    "{w}x{h}: clicking {line:?} drew {} lines",
                    app.moves_made()
                );
            }
        }
    }

    #[test]
    fn a_click_where_no_line_is_does_nothing() {
        let mut app = two_player_app();
        let size = (560.0, 620.0);
        let l = Layout::solve(size.0, size.1, app.grid_size());
        // The centre of the first box: as far from any of its four sides as a
        // point on the board can be.
        let (x1, y1) = l.dot_pos(0, 0);
        let (x2, y2) = l.dot_pos(1, 1);
        let out = app.click_at(
            f32::midpoint(x1, x2),
            f32::midpoint(y1, y2),
            MouseButton::Left,
            size,
        );
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(app.moves_made(), 0);
    }

    #[test]
    fn a_line_already_drawn_stops_offering_a_hit_box() {
        let mut app = two_player_app();
        let size = (560.0, 620.0);
        let line = LineId::horizontal(1, 1);
        assert!(is_visible_sized(&app, Target::Line(line), size));
        assert!(app.try_place_line(line));
        assert!(
            !is_visible_sized(&app, Target::Line(line), size),
            "a drawn line still answered the pointer"
        );
    }

    #[test]
    fn the_board_stops_answering_the_pointer_while_the_ai_thinks() {
        let mut app = test_app();
        let size = (560.0, 620.0);
        assert!(app.try_place_line(LineId::horizontal(0, 0)));
        assert!(app.ai_pending, "the AI's turn did not begin");
        assert!(
            hit_boxes(&app, size)
                .iter()
                .all(|(t, _)| !matches!(t, Target::Line(_))),
            "lines were still clickable while the AI was thinking"
        );
        // ...and the buttons still are, so the player can start a new game.
        assert!(is_visible_sized(&app, Target::NewGame, size));
    }

    #[test]
    fn the_click_resolves_against_the_window_the_player_can_see() {
        // `handle_mouse` hit-tests the frame at `self.size`, which `render`
        // last set. Resizing and then clicking where a line now is must draw
        // that line -- the fault this replaces was a click resolved against
        // the 560x620 the app opened at, wherever the window had got to.
        let big = (1280.0, 900.0);
        let mut app = two_player_app();
        app.render(big.0, big.1);
        let line = LineId::vertical(1, 2);
        let l = Layout::solve(big.0, big.1, app.grid_size());
        let (cx, cy) = l.line_box(line).centre();
        let out = app.handle_event(&Event::Mouse(MouseEvent {
            x: cx,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(out, EventResult::Consumed);
        assert!(app.board.is_line_drawn(line));
    }

    #[test]
    fn a_new_game_keeps_the_window_the_player_resized_to() {
        let big = (1280.0, 900.0);
        let mut app = two_player_app();
        app.render(big.0, big.1);
        app.activate(Target::NewGame);
        assert_eq!(
            app.size, big,
            "New reset the window back to the size the app opened at"
        );
        app.activate(Target::Size(5));
        assert_eq!(app.size, big, "the size buttons reset the window");
    }

    // ── The buttons ────────────────────────────────────────────────

    #[test]
    fn every_footer_button_is_drawn_inside_the_footer() {
        let app = test_app();
        for (w, h) in SIZES {
            let l = Layout::solve(w, h, app.grid_size());
            for (target, r) in hit_boxes(&app, (w, h)) {
                if matches!(target, Target::Line(_)) {
                    continue;
                }
                assert!(
                    r.x >= l.footer.x - 0.01
                        && r.y >= l.footer.y - 0.01
                        && r.right() <= l.footer.right() + 0.01
                        && r.bottom() <= l.footer.bottom() + 0.01,
                    "{w}x{h}: {target:?} at {r:?} is outside the footer {:?}",
                    l.footer
                );
            }
        }
    }

    #[test]
    fn the_buttons_and_the_keys_do_the_same_thing() {
        let size = (800.0, 700.0);
        for (key, target) in [
            (Key::N, Target::NewGame),
            (Key::M, Target::Mode),
            (Key::Num3, Target::Size(3)),
            (Key::Num5, Target::Size(5)),
        ] {
            let mut by_key = DotsAndBoxes::with_config(4, GameMode::VsAi, 7);
            let mut by_click = DotsAndBoxes::with_config(4, GameMode::VsAi, 7);
            by_key.key_at(&press(key), size);
            click_sized(&mut by_click, target, MouseButton::Left, size);
            assert_eq!(
                (by_key.grid_size(), by_key.mode),
                (by_click.grid_size(), by_click.mode),
                "{key:?} and {target:?} disagree"
            );
        }
    }

    #[test]
    fn the_size_buttons_set_the_size_they_are_labelled_with() {
        let size = (800.0, 700.0);
        for n in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let mut app = test_app();
            click_sized(&mut app, Target::Size(n), MouseButton::Left, size);
            assert_eq!(
                app.grid_size(),
                n,
                "the {n}x{n} button gave a different board"
            );
            assert_eq!(app.board.boxes.len(), n - 1);
        }
    }

    #[test]
    fn the_mode_button_swaps_the_opponent_and_starts_over() {
        let size = (800.0, 700.0);
        let mut app = two_player_app();
        assert!(app.try_place_line(LineId::horizontal(0, 0)));
        click_sized(&mut app, Target::Mode, MouseButton::Left, size);
        assert_eq!(app.mode, GameMode::VsAi);
        assert_eq!(app.moves_made(), 0, "the mode button kept the old board");
        click_sized(&mut app, Target::Mode, MouseButton::Left, size);
        assert_eq!(app.mode, GameMode::TwoPlayer);
    }

    #[test]
    fn a_footer_too_narrow_for_a_button_leaves_it_out_rather_than_off_the_edge() {
        // Five buttons and a help line do not fit in a 60px-wide footer.
        // Whatever is drawn must still be whole and inside. (140px was the
        // first width tried here and all five fit: at that width the type is
        // small enough that "New", "vs AI" and three "NxN"s come to 100px.)
        let app = test_app();
        let size = (60.0, 900.0);
        let l = Layout::solve(size.0, size.1, app.grid_size());
        let buttons: Vec<(Target, Rect)> = hit_boxes(&app, size)
            .into_iter()
            .filter(|(t, _)| !matches!(t, Target::Line(_)))
            .collect();
        assert!(
            buttons.len() < 5,
            "all five buttons claimed to fit in 140px: {buttons:?}"
        );
        for (target, r) in buttons {
            assert!(
                r.right() <= l.footer.right() - l.pad + 0.01,
                "{target:?} at {r:?} runs past the footer's right margin"
            );
        }
    }

    // ── The board ──────────────────────────────────────────────────

    #[test]
    fn a_board_has_the_lines_and_boxes_its_grid_size_calls_for() {
        for n in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let b = Board::new(n);
            assert_eq!(b.grid_size, n);
            assert_eq!(b.boxes_per_side(), n - 1);
            assert_eq!(b.h_lines.len(), n, "{n}: horizontal rows");
            assert_eq!(b.h_lines[0].len(), n - 1, "{n}: horizontal columns");
            assert_eq!(b.v_lines.len(), n - 1, "{n}: vertical rows");
            assert_eq!(b.v_lines[0].len(), n, "{n}: vertical columns");
            assert_eq!(b.boxes.len(), n - 1);
            assert_eq!(b.available_lines().len(), 2 * n * (n - 1));
            assert_eq!(b.drawn_line_count(), 0);
            assert!(!b.all_lines_drawn());
        }
    }

    #[test]
    fn a_line_is_drawn_once_and_only_the_lines_that_exist_are() {
        let mut b = Board::new(4);
        let line = LineId::horizontal(1, 2);
        assert!(b.is_available(line));
        assert_eq!(b.draw_line(line, Player::One), Some(0));
        assert!(b.is_line_drawn(line));
        assert_eq!(b.drawn_line_count(), 1);
        // A refusal is `None`, not `Some(0)`: "there was no line to draw" and
        // "the line completed no box" are the difference between passing the
        // turn and keeping it, and they used to be the same answer.
        assert!(!b.is_available(line));
        assert_eq!(b.draw_line(line, Player::Two), None);
        assert_eq!(b.drawn_line_count(), 1);
        for bad in [
            LineId::horizontal(0, 3),
            LineId::horizontal(4, 0),
            LineId::vertical(3, 0),
            LineId::vertical(0, 4),
        ] {
            assert!(!b.is_valid_line(bad), "{bad:?} was accepted");
            assert!(!b.is_available(bad), "{bad:?} was offered");
            assert_eq!(b.draw_line(bad, Player::One), None, "{bad:?} was drawn");
        }
        assert_eq!(b.drawn_line_count(), 1);
        // Every line the board does have is available on an empty board, and
        // none of them is once it has been played.
        let mut full = Board::new(4);
        let all = full.available_lines();
        assert_eq!(all.len(), 24, "a 4x4 board has 24 lines");
        for line in &all {
            assert!(full.is_available(*line), "{line:?} was not on offer");
            assert!(
                full.draw_line(*line, Player::One).is_some(),
                "{line:?} was refused"
            );
            assert!(!full.is_available(*line), "{line:?} was still on offer");
        }
        assert!(full.available_lines().is_empty());
    }

    #[test]
    fn a_box_is_complete_when_and_only_when_it_has_four_sides() {
        let mut b = Board::new(3);
        let sides = [
            LineId::horizontal(0, 0),
            LineId::horizontal(1, 0),
            LineId::vertical(0, 0),
            LineId::vertical(0, 1),
        ];
        for (i, line) in sides.iter().enumerate() {
            assert_eq!(b.box_side_count(0, 0), i, "before side {i}");
            assert!(!b.is_box_complete(0, 0), "complete after {i} sides");
            let completed = b.draw_line(*line, Player::One);
            assert_eq!(
                completed,
                Some(usize::from(i == SIDES_PER_BOX - 1)),
                "side {i} reported {completed:?} boxes"
            );
        }
        assert_eq!(b.box_side_count(0, 0), SIDES_PER_BOX);
        assert!(b.is_box_complete(0, 0));
        assert_eq!(b.boxes[0][0], Some(Player::One));
        assert_eq!(b.score(Player::One), 1);
        assert_eq!(b.score(Player::Two), 0);
    }

    #[test]
    fn a_box_off_the_board_has_no_sides_and_is_never_complete() {
        // On an *empty* board every side reads false wherever you ask, so the
        // bail that rejects an off-board box and the absence of it give the
        // same answer and the test proves nothing (lesson 90). Every line is
        // drawn here, so a box one past the edge borrows real sides from the
        // rows and columns that do exist: (2, 0) would pick up the bottom
        // wall and (0, 2) the right-hand one, and each counts 1 without it.
        let mut b = Board::new(3);
        for line in b.available_lines() {
            set(&mut b, line);
        }
        assert!(b.all_lines_drawn());
        assert_eq!(b.box_side_count(1, 1), SIDES_PER_BOX, "the last real box");
        for (r, c) in [(2, 0), (0, 2), (2, 2), (9, 9)] {
            assert_eq!(b.box_side_count(r, c), 0, "({r}, {c})");
            assert!(!b.is_box_complete(r, c), "({r}, {c})");
        }
    }

    #[test]
    fn one_line_can_complete_two_boxes_at_once() {
        // Both orientations, because a line's two boxes are found by two
        // separate arms: a horizontal line looks above and below, a vertical
        // one left and right. A test that plays only the vertical case calls
        // the rule covered while never once entering the other arm -- which
        // is how deleting "the box above a horizontal line" survived a sweep.
        let mut vertical = Board::new(3);
        // Both boxes of the top row, everything but the wall between them.
        for line in [
            LineId::horizontal(0, 0),
            LineId::horizontal(0, 1),
            LineId::horizontal(1, 0),
            LineId::horizontal(1, 1),
            LineId::vertical(0, 0),
            LineId::vertical(0, 2),
        ] {
            assert_eq!(vertical.draw_line(line, Player::One), Some(0), "{line:?}");
        }
        assert_eq!(
            vertical.draw_line(LineId::vertical(0, 1), Player::Two),
            Some(2)
        );
        assert_eq!(vertical.score(Player::Two), 2);

        let mut horizontal = Board::new(3);
        // Both boxes of the left column, everything but the floor between them.
        for line in [
            LineId::horizontal(0, 0),
            LineId::horizontal(2, 0),
            LineId::vertical(0, 0),
            LineId::vertical(0, 1),
            LineId::vertical(1, 0),
            LineId::vertical(1, 1),
        ] {
            assert_eq!(horizontal.draw_line(line, Player::One), Some(0), "{line:?}");
        }
        assert_eq!(
            horizontal.draw_line(LineId::horizontal(1, 0), Player::Two),
            Some(2)
        );
        assert_eq!(horizontal.score(Player::Two), 2);
    }

    #[test]
    fn a_line_borders_the_boxes_on_either_side_of_it_and_no_others() {
        let b = Board::new(4);
        assert_eq!(b.adjacent_boxes(LineId::horizontal(0, 0)), vec![(0, 0)]);
        assert_eq!(
            b.adjacent_boxes(LineId::horizontal(1, 1)),
            vec![(0, 1), (1, 1)]
        );
        assert_eq!(b.adjacent_boxes(LineId::horizontal(3, 2)), vec![(2, 2)]);
        assert_eq!(b.adjacent_boxes(LineId::vertical(0, 0)), vec![(0, 0)]);
        assert_eq!(
            b.adjacent_boxes(LineId::vertical(1, 1)),
            vec![(1, 0), (1, 1)]
        );
        assert_eq!(b.adjacent_boxes(LineId::vertical(2, 3)), vec![(2, 2)]);
    }

    #[test]
    fn every_box_is_claimed_by_the_time_every_line_is_drawn() {
        let mut app = two_player_app();
        let lines: Vec<LineId> = app.all_lines().collect();
        for line in lines {
            app.try_place_line(line);
        }
        assert!(app.board.all_lines_drawn());
        assert_eq!(app.phase, GamePhase::GameOver);
        let boxes = app.boxes_per_side() * app.boxes_per_side();
        assert_eq!(
            app.score(Player::One) + app.score(Player::Two),
            boxes,
            "{} boxes went unclaimed",
            boxes - app.score(Player::One) - app.score(Player::Two)
        );
    }

    // ── Turns ──────────────────────────────────────────────────────

    #[test]
    fn a_move_that_completes_nothing_passes_the_turn() {
        let mut app = two_player_app();
        assert_eq!(app.current_player, Player::One);
        assert!(app.try_place_line(LineId::horizontal(0, 0)));
        assert_eq!(app.current_player, Player::Two);
        assert!(app.try_place_line(LineId::horizontal(2, 2)));
        assert_eq!(app.current_player, Player::One);
    }

    #[test]
    fn a_move_that_completes_a_box_keeps_the_turn() {
        let mut app = two_player_app();
        for line in [
            LineId::horizontal(0, 0),
            LineId::horizontal(2, 2),
            LineId::horizontal(1, 0),
            LineId::horizontal(2, 1),
            LineId::vertical(0, 0),
        ] {
            app.try_place_line(line);
        }
        // Five moves in, whoever is to play completes the first box with the
        // sixth. Which of the two that is depends on the count, so the claim
        // is about the *mover* rather than about Player::One.
        let mover = app.current_player;
        assert!(app.try_place_line(LineId::vertical(0, 1)));
        assert_eq!(app.score(mover), 1);
        assert_eq!(
            app.current_player, mover,
            "completing a box handed the turn over"
        );
    }

    #[test]
    fn the_board_takes_no_moves_once_the_game_is_over() {
        let mut app = two_player_app();
        let lines: Vec<LineId> = app.all_lines().collect();
        for line in &lines {
            app.try_place_line(*line);
        }
        assert_eq!(app.phase, GamePhase::GameOver);
        let mut fresh = two_player_app();
        fresh.phase = GamePhase::GameOver;
        assert!(!fresh.try_place_line(LineId::horizontal(0, 0)));
        assert_eq!(fresh.moves_made(), 0);
        assert!(!fresh.accepts_moves());
    }

    #[test]
    fn the_winner_is_whoever_has_the_most_boxes() {
        let mut app = two_player_app();
        assert_eq!(app.winner(), None, "an empty board has a winner");
        app.board.boxes[0][0] = Some(Player::One);
        assert_eq!(app.winner(), Some(Player::One));
        app.board.boxes[0][1] = Some(Player::Two);
        app.board.boxes[1][0] = Some(Player::Two);
        assert_eq!(app.winner(), Some(Player::Two));
        app.board.boxes[1][1] = Some(Player::One);
        assert_eq!(app.winner(), None, "two apiece is not a draw");
    }

    #[test]
    fn the_readouts_are_the_board_and_not_a_second_copy_of_it() {
        // `score_p1`, `score_p2` and `total_moves` were fields, raised by hand
        // in two places. They are derived now, so a board reached by *any*
        // route reports itself correctly -- including one edited directly,
        // which no assignment site would ever have seen.
        let mut app = two_player_app();
        set(&mut app.board, LineId::horizontal(0, 0));
        app.board.boxes[1][1] = Some(Player::Two);
        assert_eq!(app.moves_made(), 1);
        assert_eq!(app.score(Player::Two), 1);
        assert_eq!(app.score(Player::One), 0);
        // The move count is both grids added together, and a fixture that
        // draws one horizontal line cannot tell that from either grid alone.
        set(&mut app.board, LineId::vertical(0, 0));
        assert_eq!(app.moves_made(), 2, "the vertical grid was not counted");
        set(&mut app.board, LineId::vertical(1, 2));
        set(&mut app.board, LineId::horizontal(2, 1));
        assert_eq!(app.moves_made(), 4);
        // Whatever has been drawn, drawn plus available is every line there is.
        let total = app
            .grid_size()
            .saturating_mul(app.boxes_per_side())
            .saturating_mul(2);
        assert_eq!(
            app.moves_made()
                .saturating_add(app.board.available_lines().len()),
            total
        );
    }

    // ── The cursor ─────────────────────────────────────────────────

    #[test]
    fn the_cursor_is_a_line_and_always_a_line_that_exists() {
        let mut app = test_app();
        assert_eq!(app.cursor, LineId::horizontal(0, 0));
        // Walk it a long way in every direction and check it never leaves the
        // board. The wrap and the orientation flip are the two places it could.
        for key in [Key::Right, Key::Down, Key::Left, Key::Up] {
            for _ in 0..40 {
                app.move_cursor(key);
                assert!(
                    app.board.is_valid_line(app.cursor),
                    "{key:?} walked the cursor to {:?}",
                    app.cursor
                );
            }
        }
        for _ in 0..8 {
            app.toggle_cursor_orientation();
            assert!(app.board.is_valid_line(app.cursor));
        }
    }

    #[test]
    fn the_cursor_wraps_within_its_row_and_flips_between_the_two_grids() {
        let mut app = test_app(); // 4 dots: h is 4x3, v is 3x4
        app.cursor = LineId::horizontal(0, 2);
        app.move_cursor(Key::Right);
        assert_eq!(app.cursor, LineId::horizontal(0, 0), "right did not wrap");
        app.move_cursor(Key::Left);
        assert_eq!(app.cursor, LineId::horizontal(0, 2), "left did not wrap");
        // Up off the top of the horizontal grid enters the vertical one at its
        // bottom row, with the column clamped to what that grid has.
        app.move_cursor(Key::Up);
        assert_eq!(app.cursor, LineId::vertical(2, 2));
        // ...and down off the bottom of the vertical grid re-enters the
        // horizontal one at row 0.
        app.move_cursor(Key::Down);
        assert_eq!(app.cursor, LineId::horizontal(0, 2));
        // The clamp on that flip only binds from the wider grid to the
        // narrower one, and the walk above never visits a column the
        // destination lacks: vertical is four columns wide and horizontal
        // three, so the crossing has to start at vertical column 3 for the
        // clamp to do anything at all.
        app.cursor = LineId::vertical(2, 3);
        app.move_cursor(Key::Down);
        assert_eq!(
            app.cursor,
            LineId::horizontal(0, 2),
            "the cursor kept a column the horizontal grid does not have"
        );
        assert!(app.board.is_valid_line(app.cursor));
        app.cursor = LineId::vertical(0, 3);
        app.move_cursor(Key::Up);
        assert_eq!(app.cursor, LineId::horizontal(3, 2));
        assert!(app.board.is_valid_line(app.cursor));
    }

    #[test]
    fn a_toggle_clamps_the_cursor_into_the_grid_it_lands_in() {
        let mut app = test_app();
        // Horizontal row 3 exists; vertical row 3 does not.
        app.cursor = LineId::horizontal(3, 0);
        app.toggle_cursor_orientation();
        assert_eq!(app.cursor, LineId::vertical(2, 0));
        // Vertical column 3 exists; horizontal column 3 does not.
        app.cursor = LineId::vertical(0, 3);
        app.toggle_cursor_orientation();
        assert_eq!(app.cursor, LineId::horizontal(0, 2));
    }

    #[test]
    fn the_cursor_does_not_move_when_the_board_is_not_taking_moves() {
        let size = (560.0, 620.0);
        for freeze in [GamePhase::GameOver, GamePhase::Playing] {
            let mut app = test_app();
            app.phase = freeze;
            app.ai_pending = freeze == GamePhase::Playing;
            assert!(!app.accepts_moves());
            let before = app.cursor;
            for key in [Key::Right, Key::Down, Key::Tab, Key::Enter] {
                assert_eq!(
                    app.key_at(&press(key), size),
                    EventResult::Ignored,
                    "{key:?} was taken with the board frozen"
                );
            }
            assert_eq!(app.cursor, before);
            assert_eq!(app.moves_made(), 0);
        }
    }

    #[test]
    fn enter_and_space_draw_the_line_the_cursor_points_at() {
        let size = (560.0, 620.0);
        for key in [Key::Enter, Key::Space] {
            let mut app = two_player_app();
            app.cursor = LineId::vertical(1, 2);
            assert_eq!(app.key_at(&press(key), size), EventResult::Consumed);
            assert!(
                app.board.is_line_drawn(LineId::vertical(1, 2)),
                "{key:?} drew nothing"
            );
            // A second press on the same line changes nothing.
            assert_eq!(app.key_at(&press(key), size), EventResult::Ignored);
            assert_eq!(app.moves_made(), 1);
        }
    }

    #[test]
    fn a_key_release_is_not_a_key_press() {
        let mut app = two_player_app();
        let mut release = press(Key::Enter);
        release.pressed = false;
        assert_eq!(app.handle_event(&Event::Key(release)), EventResult::Ignored);
        assert_eq!(app.moves_made(), 0);
    }

    // ── The AI ─────────────────────────────────────────────────────

    #[test]
    fn the_ai_takes_a_box_it_can_complete() {
        let mut b = Board::new(3);
        for line in [
            LineId::horizontal(0, 0),
            LineId::horizontal(1, 0),
            LineId::vertical(0, 0),
        ] {
            set(&mut b, line);
        }
        // Every seed, not one. Completing a box is also a *safe* move -- it
        // takes a box to four sides, not to three -- so the line that captures
        // is in the pool the fallback picks at random from, and an AI with no
        // capture phase at all lands on it often enough that a single lucky
        // seed reads exactly like a working one.
        for seed in 0..30u64 {
            let mut rng = SeededRng::new(seed);
            assert_eq!(
                ai_choose_line(&b, &mut rng),
                Some(LineId::vertical(0, 1)),
                "seed {seed}: the AI left a box on three sides"
            );
        }
    }

    #[test]
    fn the_ai_prefers_the_line_that_completes_two_boxes() {
        let mut b = Board::new(3);
        // Both top boxes on three sides, sharing the wall between them --
        // and a *third* box on three sides of its own, so that there are two
        // captures to choose between. With only the pair, the one line that
        // captures anything is both the most and the least of them, and
        // "prefers the larger" is a claim the position cannot test at all.
        for line in [
            LineId::horizontal(0, 0),
            LineId::horizontal(0, 1),
            LineId::horizontal(1, 0),
            LineId::horizontal(1, 1),
            LineId::vertical(0, 0),
            LineId::vertical(0, 2),
            LineId::vertical(1, 0),
            LineId::vertical(1, 1),
        ] {
            set(&mut b, line);
        }
        let two = LineId::vertical(0, 1);
        let one = LineId::horizontal(2, 0);
        assert_eq!(b.box_side_count(0, 0), 3, "the first of the pair");
        assert_eq!(b.box_side_count(0, 1), 3, "the second of the pair");
        assert_eq!(b.box_side_count(1, 0), 3, "the single box");
        for seed in 0..30u64 {
            let mut rng = SeededRng::new(seed);
            assert_eq!(
                ai_choose_line(&b, &mut rng),
                Some(two),
                "seed {seed}: the AI took {one:?}, which is worth one box"
            );
        }
    }

    #[test]
    fn the_ai_does_not_hand_over_a_box_while_a_safe_line_is_left() {
        let mut b = Board::new(4);
        // Put one box on two sides. Its two remaining sides are the gift.
        set(&mut b, LineId::horizontal(0, 0));
        set(&mut b, LineId::vertical(0, 0));
        let gifts = [LineId::horizontal(1, 0), LineId::vertical(0, 1)];
        for seed in 0..20u64 {
            let mut rng = SeededRng::new(seed);
            let choice = ai_choose_line(&b, &mut rng).unwrap();
            assert!(
                !gifts.contains(&choice),
                "seed {seed}: the AI played {choice:?}, handing over box (0, 0)"
            );
        }
    }

    #[test]
    fn the_ai_has_nothing_to_say_about_a_board_with_no_lines_left() {
        let mut b = Board::new(3);
        let all: Vec<LineId> = {
            let bps = b.boxes_per_side();
            (0..b.grid_size)
                .flat_map(|r| (0..bps).map(move |c| LineId::horizontal(r, c)))
                .chain((0..bps).flat_map(|r| (0..b.grid_size).map(move |c| LineId::vertical(r, c))))
                .collect()
        };
        for line in all {
            set(&mut b, line);
        }
        let mut rng = SeededRng::new(5);
        assert_eq!(ai_choose_line(&b, &mut rng), None);
    }

    #[test]
    fn the_ai_waits_before_it_moves_so_that_thinking_is_a_frame_and_not_a_word() {
        let mut app = test_app();
        assert!(app.try_place_line(LineId::horizontal(0, 0)));
        assert!(app.ai_pending);
        assert_eq!(app.moves_made(), 1);
        // Short of the delay, nothing happens.
        assert_eq!(
            app.handle_tick(AI_DELAY - 1),
            EventResult::Ignored,
            "the AI moved early"
        );
        assert_eq!(app.moves_made(), 1);
        assert!(app.ai_pending, "the wait ended without a move");
        // At the delay, it plays.
        assert_eq!(app.handle_tick(1), EventResult::Consumed);
        assert_eq!(app.moves_made(), 2);
    }

    #[test]
    fn a_tick_with_no_ai_waiting_changes_nothing() {
        let mut app = two_player_app();
        assert_eq!(app.handle_tick(10_000), EventResult::Ignored);
        assert_eq!(app.moves_made(), 0);
        assert_eq!(app.current_player, Player::One);
    }

    #[test]
    fn the_ai_keeps_playing_while_it_keeps_completing_boxes() {
        let mut app = test_app();
        // Leave box (0, 0) on three sides with the AI to move.
        set(&mut app.board, LineId::horizontal(0, 0));
        set(&mut app.board, LineId::horizontal(1, 0));
        set(&mut app.board, LineId::vertical(0, 0));
        app.current_player = Player::Two;
        app.ai_pending = true;
        assert_eq!(app.handle_tick(AI_DELAY), EventResult::Consumed);
        assert_eq!(app.score(Player::Two), 1);
        assert!(
            app.ai_pending,
            "the AI completed a box and handed the turn back anyway"
        );
    }

    #[test]
    fn a_whole_game_against_the_ai_ends_with_every_line_drawn() {
        let mut app = test_app();
        for _ in 0..500 {
            if app.phase == GamePhase::GameOver {
                break;
            }
            if app.ai_pending {
                app.handle_tick(AI_DELAY);
                continue;
            }
            let Some(line) = app.all_lines().find(|l| !app.board.is_line_drawn(*l)) else {
                break;
            };
            app.try_place_line(line);
        }
        assert_eq!(app.phase, GamePhase::GameOver, "the game never finished");
        assert!(app.board.all_lines_drawn());
        assert_eq!(
            app.score(Player::One) + app.score(Player::Two),
            app.boxes_per_side() * app.boxes_per_side()
        );
    }

    // ── The seed ───────────────────────────────────────────────────

    #[test]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        // The file used to carry its own LCG seeded with `42`, so every match
        // against the computer played out identically from the same position.
        //
        // On a host with no kernel entropy source `seed_from_system` returns
        // the fallback, so this cannot assert that two fresh games differ --
        // it asserts the seed is the crate's fallback and *not* the literal
        // this replaces.
        let fresh = DotsAndBoxes::new().rng.next_u64();
        assert_eq!(
            fresh,
            SeededRng::new(FALLBACK_SEED).next_u64(),
            "a fresh game did not use the crate's fallback seed"
        );
        assert_ne!(
            fresh,
            SeededRng::new(42).next_u64(),
            "a fresh game is still seeded by the old hardcoded literal"
        );
    }

    #[test]
    fn a_new_game_does_not_replay_the_last_one() {
        // The claim is that *successive* games differ, which is not what
        // "the new game differs from the old" asserts: seeding every new game
        // from one constant satisfies the latter (the constant is not the
        // seed the session started with) while making every game after the
        // first identical, which is the exact bug being guarded against.
        let mut app = test_app();
        let mut seen = std::collections::BTreeSet::new();
        seen.insert(app.rng.clone().next_u64());
        for round in 0..5 {
            app.new_game();
            assert!(
                seen.insert(app.rng.clone().next_u64()),
                "new game {round} dealt a game already played"
            );
        }
        // The size buttons deal too, and from the same stream.
        for round in 0..3 {
            app.new_game_with_size(MIN_GRID_SIZE);
            assert!(
                seen.insert(app.rng.clone().next_u64()),
                "resize {round} dealt a game already played"
            );
        }
    }

    #[test]
    fn the_ai_breaks_ties_differently_under_different_seeds() {
        let b = Board::new(5);
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..30u64 {
            let mut rng = SeededRng::new(seed);
            if let Some(line) = ai_choose_line(&b, &mut rng) {
                seen.insert((
                    line.orientation == Orientation::Vertical,
                    line.row,
                    line.col,
                ));
            }
        }
        assert!(
            seen.len() > 1,
            "thirty seeds all opened with the same line: {seen:?}"
        );
    }

    // ── New game ───────────────────────────────────────────────────

    #[test]
    fn a_new_game_clears_the_board_and_keeps_the_settings() {
        let mut app = two_player_app();
        app.try_place_line(LineId::horizontal(0, 0));
        app.phase = GamePhase::GameOver;
        app.new_game();
        assert_eq!(app.moves_made(), 0);
        assert_eq!(app.score(Player::One), 0);
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_player, Player::One);
        assert_eq!(app.grid_size(), 4, "New changed the board size");
        assert_eq!(app.mode, GameMode::TwoPlayer, "New changed the opponent");
        assert!(!app.ai_pending);
    }

    #[test]
    fn the_grid_size_is_clamped_to_the_sizes_that_exist() {
        assert_eq!(
            DotsAndBoxes::with_config(1, GameMode::VsAi, 1).grid_size(),
            MIN_GRID_SIZE
        );
        assert_eq!(
            DotsAndBoxes::with_config(99, GameMode::VsAi, 1).grid_size(),
            MAX_GRID_SIZE
        );
    }

    // ── The end of the game ────────────────────────────────────────

    #[test]
    fn the_game_over_card_fits_the_window_it_is_drawn_in() {
        let mut app = small_app();
        let lines: Vec<LineId> = app.all_lines().collect();
        for line in lines {
            app.try_place_line(line);
        }
        assert_eq!(app.phase, GamePhase::GameOver);
        for (w, h) in SIZES {
            for r in painted_rects(&app, (w, h)) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{w}x{h}: the end card painted {r:?} outside the window"
                );
            }
        }
    }

    #[test]
    fn the_end_card_says_who_won_in_the_words_of_the_mode_being_played() {
        for (mode, one, two) in [
            (GameMode::VsAi, "You win!", "AI wins!"),
            (GameMode::TwoPlayer, "Player 1 wins!", "Player 2 wins!"),
        ] {
            for (winner, expected) in [(Player::One, one), (Player::Two, two)] {
                let mut app = DotsAndBoxes::with_config(3, mode, 1);
                app.phase = GamePhase::GameOver;
                app.board.boxes[0][0] = Some(winner);
                let said = app
                    .draw((560.0, 620.0))
                    .commands()
                    .iter()
                    .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == expected));
                assert!(
                    said,
                    "{mode:?} with {winner:?} ahead did not say {expected:?}"
                );
            }
        }
    }

    #[test]
    fn a_claimed_box_is_labelled_in_the_words_of_the_mode_being_played() {
        for (mode, one, two) in [(GameMode::VsAi, "Y", "A"), (GameMode::TwoPlayer, "1", "2")] {
            let app = DotsAndBoxes::with_config(3, mode, 1);
            assert_eq!(app.box_initial(Player::One), one);
            assert_eq!(app.box_initial(Player::Two), two);
        }
    }

    #[test]
    fn the_two_players_are_told_apart_by_colour() {
        assert_ne!(Player::One.color(), Player::Two.color());
        assert_ne!(Player::One.box_color(), Player::Two.box_color());
        assert_eq!(Player::One.other(), Player::Two);
        assert_eq!(Player::Two.other(), Player::One);
    }

    // ── The window ─────────────────────────────────────────────────

    #[test]
    fn the_app_opens_a_window_of_the_size_it_asks_for() {
        let app = test_app();
        assert_eq!(app.title(), TITLE);
        assert_eq!(app.app_id(), "dots");
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
        assert!(
            app.tick_interval().is_some(),
            "without a tick the AI never moves"
        );
    }

    #[test]
    fn a_close_request_closes_the_window() {
        let mut app = test_app();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn a_move_asks_for_a_redraw_and_a_dead_key_does_not() {
        let mut app = two_player_app();
        assert!(matches!(
            app.on_event(&Event::Key(press(Key::Enter))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(press(Key::Q))),
            Response::Idle
        ));
    }

    #[test]
    fn render_records_the_window_it_was_given() {
        let mut app = test_app();
        let tree = app.render(1000.0, 700.0);
        assert_eq!(app.size, (1000.0, 700.0));
        assert!(!tree.commands.is_empty(), "the frame was empty");
    }
}
