//! Slate OS Gomoku (five in a row) -- a 15x15 Go-style board against a
//! minimax opponent, in a real window.
//!
//! Stones sit *on* the intersections rather than inside the cells, so every
//! part of the board -- lines, stones, star points, coordinate labels, the
//! cursor, the win-line highlight -- is placed relative to a point, and a
//! click has to be resolved to the nearest point rather than to a cell it
//! falls inside. [`Layout`] owns that mapping and its inverse, derived from
//! the window the compositor actually gave us, and the drawing pass records
//! a hit box on every intersection it draws, so a stone is clickable exactly
//! where its ink is.
//!
//! The opponent searches on a clock rather than inside the event handler:
//! placing a stone leaves the game in [`GamePhase::Thinking`], which is a
//! state a frame can render, and the reply arrives on the next tick. Done
//! synchronously, as it was, "White is thinking" was a string no frame ever
//! showed, because the search ran to completion before the handler returned.

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

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Board colors ────────────────────────────────────────────────────
const BOARD_BG: Color = Color::from_hex(0xD4A867);
const BOARD_BORDER: Color = Color::from_hex(0x8B6914);
const GRID_LINE_COLOR: Color = Color::from_hex(0x2A2A2A);
const STAR_POINT_COLOR: Color = Color::from_hex(0x2A2A2A);
const CURSOR_COLOR: Color = Color::from_hex(0x89B4FA);
const BLACK_STONE: Color = Color::from_hex(0x1A1A2E);
const WHITE_STONE: Color = Color::from_hex(0xE8E8E8);
const BLACK_STONE_BORDER: Color = Color::from_hex(0x000000);
const WHITE_STONE_BORDER: Color = Color::from_hex(0xBBBBBB);
const WIN_HIGHLIGHT: Color = Color::rgba(243, 139, 168, 150);
const LAST_MOVE_MARKER: Color = Color::from_hex(0xF38BA8);

// ── Board geometry ─────────────────────────────────────────
const BOARD_SIZE: usize = 15;

/// Index of the last row and of the last column.
const LAST_INDEX: i32 = BOARD_SIZE as i32 - 1;

/// The window the program asks for, and the size its tests draw at.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 640.0;

// ── The text the panel and the header hold ─────────────────
//
// Named, because `Layout::solve` has to measure these to decide whether a
// panel is worth drawing at all and where the turn indicator starts, while
// `draw_panel` and `draw_header` are the code that actually draws them. A
// string measured in one function and drawn in another is a column sized for a
// line it does not contain: this layout measured `"Draws: 88"` and nothing
// else, so the panel's real widest lines -- `"New game (N)"` and
// `"\u{25CF} Black: 0"` -- were never part of the decision at all
// (known-issues lesson 93).

/// The program's name, drawn at the left of the header and measured to place
/// the turn indicator past it.
const TITLE_TEXT: &str = "Gomoku";

const GAME_INFO_HEADING: &str = "Game Info";
const SCORES_HEADING: &str = "Scores";

/// The panel's headings, drawn bold at [`Layout::font`].
const PANEL_HEADINGS: [&str; 2] = [GAME_INFO_HEADING, SCORES_HEADING];

const MOVES_STEM: &str = "Moves: ";
const TURN_STEM: &str = "Turn: ";
const BLACK_SCORE_STEM: &str = "\u{25CF} Black: ";
const WHITE_SCORE_STEM: &str = "\u{25CB} White: ";
const DRAWS_STEM: &str = "Draws: ";

/// Each of the panel's lines as the layout must measure it: the stem
/// `draw_panel` draws, followed by the widest tail the panel is sized to hold.
///
/// A game longer than 999 moves, or a scoreline past 99, draws a line wider
/// than the column was sized for; it is elided by the `max_width` every panel
/// line is drawn with rather than painted over the board.
const PANEL_LINES: [(&str, &str); 5] = [
    (MOVES_STEM, "888"),
    (TURN_STEM, "Black"),
    (BLACK_SCORE_STEM, "88"),
    (WHITE_SCORE_STEM, "88"),
    (DRAWS_STEM, "88"),
];

const NEW_GAME_LABEL: &str = "New game (N)";
const UNDO_LABEL: &str = "Undo (Z)";

/// The panel's buttons, drawn centred at [`Layout::small`].
const PANEL_BUTTONS: [&str; 2] = [NEW_GAME_LABEL, UNDO_LABEL];

/// What the pointer can land on. The drawing pass records one of these for
/// every intersection it draws and for every button, so a click is answered by
/// the picture rather than by arithmetic over constants the picture may not
/// have been drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// An intersection, by row and column.
    Point(usize, usize),
    NewGame,
    Undo,
}

/// Every rectangle and type size the frame is drawn from, solved from the
/// window the compositor gave us.
///
/// This replaced eleven constants -- `CELL_SIZE = 36.0`, `BOARD_OFFSET_X`,
/// `BOARD_OFFSET_Y`, `PANEL_X` and four font sizes among them. `render` took
/// no width and no height at all, so the program drew the same 800x600 picture
/// into whatever window it was given, while `handle_mouse` sent the click
/// through `intersection_near`, which answered from the same constants: in any
/// other window the board was drawn in one place and clicked in another.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    /// The title and the turn indicator.
    header: Rect,
    /// The square the board is drawn in, including the margin the coordinate
    /// labels need outside the outermost lines.
    board: Rect,
    /// The information column beside the board.
    panel: Rect,
    /// The game-over message, when there is one.
    status: Rect,
    /// The top-left intersection, and the distance between two of them.
    origin: (f32, f32),
    cell: f32,
    stone: f32,
    pad: f32,
    title: f32,
    font: f32,
    label: f32,
    small: f32,
}

impl Layout {
    /// Solve the layout for a window of `w` by `h`.
    ///
    /// The board is square and takes whatever is left after the header and the
    /// panel, so a window of any shape gets a playable board rather than a
    /// 15x15 grid drawn at 36 px per cell off the bottom of a short one. The
    /// panel is dropped whole rather than drawn illegibly narrow when the
    /// window cannot pay for it, which is the same rule the other wired apps
    /// use for their chrome.
    fn solve(w: f32, h: f32) -> Self {
        let window = Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0));
        let font = (h / 34.0).clamp(9.0, 17.0);
        let title = (font * 1.5).clamp(13.0, 26.0);
        let label = (font - 2.0).max(7.0);
        let small = (font - 4.0).max(6.0);
        let pad = (w.min(h) * 0.025).clamp(3.0, 16.0);

        let hdr_h = (h * 0.09).clamp(0.0, 48.0);
        let header = Rect::new(0.0, 0.0, w, hdr_h);

        // The panel is worth having only if it can hold its widest line. Below
        // that it is dropped and the board takes the whole width, rather than
        // squeezing the board to make room for a column too narrow to read.
        //
        // Every line is measured, in the weight and size it is drawn at. This
        // used to measure `"Draws: 88"` alone -- a string the panel never
        // draws, and narrower than several it does.
        let mut widest: f32 = 0.0;
        for heading in PANEL_HEADINGS {
            widest = widest.max(text::measure(heading, font, FontWeightHint::Bold));
        }
        for (stem, tail) in PANEL_LINES {
            let mut line = String::from(stem);
            line.push_str(tail);
            widest = widest.max(text::measure(&line, font, FontWeightHint::Regular));
        }
        for button in PANEL_BUTTONS {
            widest = widest.max(text::measure(button, small, FontWeightHint::Regular));
        }
        let panel_w_min = widest + pad * 2.0;
        // `clamp` panics when the low bound is above the high one, and a large
        // font can carry the minimum past 220.
        let want_panel = (w * 0.26).clamp(panel_w_min, panel_w_min.max(220.0));
        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {
            want_panel
        } else {
            0.0
        };

        let status_h = (font * 2.2).clamp(0.0, 44.0);
        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h.min(h));

        let free_w = (w - panel_w).max(0.0);
        let free_h = (status.y - header.bottom()).max(0.0);
        let side = free_w.min(free_h).max(0.0);
        let board = Rect::new(
            ((free_w - side) / 2.0).max(0.0),
            header.bottom() + ((free_h - side) / 2.0).max(0.0),
            side,
            side,
        );
        let panel = Rect::new(free_w, header.bottom(), panel_w, free_h);

        // The labels live outside the outermost lines, so the grid itself is
        // inset by one label's worth on every side.
        let margin = (label * 1.6).min(side / 6.0);
        let grid = (side - margin * 2.0).max(0.0);
        let cell = grid / (BOARD_SIZE - 1) as f32;
        let origin = (board.x + margin, board.y + margin);

        Self {
            window,
            header,
            board,
            panel,
            status,
            origin,
            cell,
            // Stones must not touch: half a cell is where they meet exactly,
            // so this is a little under it.
            stone: cell * 0.44,
            pad,
            title,
            font,
            label,
            small,
        }
    }

    /// Screen coordinates of the intersection at `(row, col)`.
    ///
    /// Row 0 is the top of the window. The board is symmetric, so unlike chess
    /// there is no flip here and nothing an inverse has to undo.
    fn intersection(&self, row: i32, col: i32) -> (f32, f32) {
        (
            self.origin.0 + col as f32 * self.cell,
            self.origin.1 + row as f32 * self.cell,
        )
    }

    /// The rectangle a stone at `(row, col)` occupies, which is also the hit
    /// box the drawing pass records for it.
    ///
    /// Slightly smaller than a cell, so the boxes do not overlap and a click
    /// lands on the intersection whose stone it is nearest -- and so that a
    /// click in the middle of a cell, aimed at no intersection at all, does
    /// nothing rather than dropping a stone half a cell from where it was
    /// aimed.
    fn stone_rect(&self, row: i32, col: i32) -> Rect {
        let (x, y) = self.intersection(row, col);
        Rect::new(
            x - self.stone,
            y - self.stone,
            self.stone * 2.0,
            self.stone * 2.0,
        )
    }
}

// ── AI search depth ─────────────────────────────────────────────────
const AI_DEPTH: i32 = 3;

// ── Win condition ───────────────────────────────────────────────────
const WIN_COUNT: usize = 5;

// ── Directions for win checking (row_delta, col_delta) ──────────────
// Horizontal, vertical, diagonal-down-right, diagonal-down-left
const DIRECTIONS: [(i32, i32); 4] = [
    (0, 1),  // horizontal
    (1, 0),  // vertical
    (1, 1),  // diagonal \
    (1, -1), // diagonal /
];

// ── Star points on a 15x15 board ────────────────────────────────────
// Traditional Go-style star points: corners at (3,3), center at (7,7),
// and side midpoints.
const STAR_POINTS: [(usize, usize); 5] = [(3, 3), (3, 11), (7, 7), (11, 3), (11, 11)];

// ── AI evaluation scores ────────────────────────────────────────────
// Pattern scores for the AI evaluator. Higher scores = more important patterns.
const SCORE_FIVE: i32 = 1_000_000;
const SCORE_OPEN_FOUR: i32 = 100_000;
const SCORE_HALF_OPEN_FOUR: i32 = 10_000;
const SCORE_OPEN_THREE: i32 = 5_000;
const SCORE_HALF_OPEN_THREE: i32 = 500;
const SCORE_OPEN_TWO: i32 = 200;
const SCORE_HALF_OPEN_TWO: i32 = 50;
const SCORE_ONE: i32 = 10;

// ── Cell state ──────────────────────────────────────────────────────

/// Represents what occupies a board intersection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Empty,
    Black,
    White,
}

impl Cell {
    /// Return the opponent's color. Empty returns Empty.
    fn opponent(self) -> Self {
        match self {
            Cell::Black => Cell::White,
            Cell::White => Cell::Black,
            Cell::Empty => Cell::Empty,
        }
    }
}

// ── Move record (for undo) ──────────────────────────────────────────

/// A single placed stone, recording who placed it and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MoveRecord {
    row: usize,
    col: usize,
    stone: Cell,
}

// ── Win line ────────────────────────────────────────────────────────

/// Describes a winning line of five stones.
#[derive(Clone, Debug, PartialEq, Eq)]
struct WinLine {
    positions: Vec<(usize, usize)>,
}

// ── Game phase ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Waiting for Black -- the human -- to place a stone.
    Playing,
    /// Black has moved and White's search has not run yet.
    ///
    /// This phase exists so that "White is thinking" is a state a frame can
    /// be drawn in. The search used to run inside `place_stone`, which is
    /// inside the event handler, so it finished before the handler returned
    /// and the message it set was never on screen for a single frame. Now the
    /// handler leaves the game here, the frame renders, and the reply arrives
    /// on the next tick.
    Thinking,
    Won,
    Draw,
}

// ── Board ───────────────────────────────────────────────────────────

/// The 15x15 Gomoku board.
#[derive(Clone, Debug)]
struct Board {
    cells: [[Cell; BOARD_SIZE]; BOARD_SIZE],
}

impl Board {
    /// Create an empty board.
    fn new() -> Self {
        Self {
            cells: [[Cell::Empty; BOARD_SIZE]; BOARD_SIZE],
        }
    }

    /// The cell at `(row, col)`, or `None` if that is not on the board.
    ///
    /// Every read of the array goes through here. The search walks lines in
    /// eight directions and runs off the edge on purpose -- "the run ends
    /// because the board ends" is the answer it wants -- so a bounds check
    /// that returns `None` is the accessor, not a guard bolted onto one.
    fn get(&self, row: i32, col: i32) -> Option<Cell> {
        let row = usize::try_from(row).ok()?;
        let col = usize::try_from(col).ok()?;
        self.cells.get(row)?.get(col).copied()
    }

    /// Put `cell` at `(row, col)`, reporting whether that was on the board.
    fn set(&mut self, row: usize, col: usize, cell: Cell) -> bool {
        let Some(slot) = self.cells.get_mut(row).and_then(|r| r.get_mut(col)) else {
            return false;
        };
        *slot = cell;
        true
    }

    /// Whether the intersection at `(row, col)` is empty.
    ///
    /// An intersection that is not on the board is not empty: it is not an
    /// intersection, and the one caller asks this to decide whether a stone
    /// may be placed there.
    fn is_empty(&self, row: usize, col: usize) -> bool {
        let (Ok(r), Ok(c)) = (i32::try_from(row), i32::try_from(col)) else {
            return false;
        };
        self.get(r, c) == Some(Cell::Empty)
    }

    /// Count the number of occupied intersections.
    fn stone_count(&self) -> usize {
        self.cells
            .iter()
            .flatten()
            .filter(|&&c| c != Cell::Empty)
            .count()
    }

    /// Whether the board is completely full (draw condition).
    fn is_full(&self) -> bool {
        self.stone_count() == BOARD_SIZE * BOARD_SIZE
    }

    /// Check if a specific player has five in a row starting from (row, col)
    /// in the given direction. Returns the winning positions if found.
    fn check_line_from(
        &self,
        row: i32,
        col: i32,
        dr: i32,
        dc: i32,
        stone: Cell,
    ) -> Option<Vec<(usize, usize)>> {
        let mut positions = Vec::new();
        for i in 0..WIN_COUNT as i32 {
            let r = row.saturating_add(dr.saturating_mul(i));
            let c = col.saturating_add(dc.saturating_mul(i));
            if self.get(r, c) != Some(stone) {
                return None;
            }
            let (Ok(ru), Ok(cu)) = (usize::try_from(r), usize::try_from(c)) else {
                return None;
            };
            positions.push((ru, cu));
        }
        Some(positions)
    }

    /// Check if the given stone color has won. Returns the winning line if so.
    fn check_winner(&self, stone: Cell) -> Option<WinLine> {
        for row in 0..BOARD_SIZE as i32 {
            for col in 0..BOARD_SIZE as i32 {
                for &(dr, dc) in &DIRECTIONS {
                    if let Some(positions) = self.check_line_from(row, col, dr, dc, stone) {
                        return Some(WinLine { positions });
                    }
                }
            }
        }
        None
    }

    /// Count consecutive stones of the given color starting from (row, col)
    /// in the direction (dr, dc), not counting the starting position.
    fn count_direction(&self, row: i32, col: i32, dr: i32, dc: i32, stone: Cell) -> i32 {
        let mut count = 0i32;
        let mut r = row.saturating_add(dr);
        let mut c = col.saturating_add(dc);
        while self.get(r, c) == Some(stone) {
            count = count.saturating_add(1);
            r = r.saturating_add(dr);
            c = c.saturating_add(dc);
        }
        count
    }

    /// Check what is at the end of a consecutive run of `stone` in direction
    /// (dr, dc) starting from (row, col). Returns true if the end is empty
    /// (open end), false if blocked (edge or opponent stone).
    fn is_open_end(&self, row: i32, col: i32, dr: i32, dc: i32, stone: Cell) -> bool {
        let mut r = row.saturating_add(dr);
        let mut c = col.saturating_add(dc);
        while self.get(r, c) == Some(stone) {
            r = r.saturating_add(dr);
            c = c.saturating_add(dc);
        }
        self.get(r, c) == Some(Cell::Empty)
    }

    /// Evaluate a single line pattern through (row, col) in a given direction
    /// for the specified stone color. Returns a score based on the pattern
    /// (how many in a row, open/half-open ends).
    fn evaluate_line_pattern(&self, row: i32, col: i32, dr: i32, dc: i32, stone: Cell) -> i32 {
        let (br, bc) = (dr.saturating_neg(), dc.saturating_neg());
        let count_fwd = self.count_direction(row, col, dr, dc, stone);
        let count_bwd = self.count_direction(row, col, br, bc, stone);
        // +1 for the stone at (row, col) itself.
        let total = count_fwd.saturating_add(count_bwd).saturating_add(1);

        if total >= WIN_COUNT as i32 {
            return SCORE_FIVE;
        }

        let open_fwd = self.is_open_end(row, col, dr, dc, stone);
        let open_bwd = self.is_open_end(row, col, br, bc, stone);
        let open_ends = i32::from(open_fwd).saturating_add(i32::from(open_bwd));

        match (total, open_ends) {
            (4, 2) => SCORE_OPEN_FOUR,
            (4, 1) => SCORE_HALF_OPEN_FOUR,
            (3, 2) => SCORE_OPEN_THREE,
            (3, 1) => SCORE_HALF_OPEN_THREE,
            (2, 2) => SCORE_OPEN_TWO,
            (2, 1) => SCORE_HALF_OPEN_TWO,
            (1, _) => SCORE_ONE,
            _ => 0,
        }
    }

    /// Evaluate the entire board from the perspective of `stone`.
    /// Positive score = advantage for `stone`, negative = disadvantage.
    fn evaluate(&self, stone: Cell) -> i32 {
        let mut score = 0i32;
        let opponent = stone.opponent();
        let center = BOARD_SIZE as i32 / 2;

        for row in 0..BOARD_SIZE as i32 {
            for col in 0..BOARD_SIZE as i32 {
                let Some(cell) = self.get(row, col) else {
                    continue;
                };
                if cell == Cell::Empty {
                    continue;
                }
                for &(dr, dc) in &DIRECTIONS {
                    let line_score = self.evaluate_line_pattern(row, col, dr, dc, cell);
                    if cell == stone {
                        score = score.saturating_add(line_score);
                    } else if cell == opponent {
                        score = score.saturating_sub(line_score);
                    }
                }

                // Center control bonus: stones closer to the middle are
                // slightly better. Counted in the same pass as the patterns
                // -- it used to be a second walk of the same 225 cells.
                let dist = row
                    .saturating_sub(center)
                    .saturating_abs()
                    .saturating_add(col.saturating_sub(center).saturating_abs());
                let center_bonus = (LAST_INDEX.saturating_sub(dist)).max(0).saturating_mul(2);
                if cell == stone {
                    score = score.saturating_add(center_bonus);
                } else {
                    score = score.saturating_sub(center_bonus);
                }
            }
        }

        score
    }

    /// Generate candidate moves for the AI. Only considers intersections
    /// near existing stones (within a radius of 2) to keep the search
    /// space manageable.
    fn candidate_moves(&self) -> Vec<(usize, usize)> {
        let mut seen = [[false; BOARD_SIZE]; BOARD_SIZE];
        let mut moves = Vec::new();
        let radius = 2i32;
        let center = BOARD_SIZE as i32 / 2;

        if self.stone_count() == 0 {
            // First move: play the centre.
            return vec![(BOARD_SIZE / 2, BOARD_SIZE / 2)];
        }

        for row in 0..BOARD_SIZE as i32 {
            for col in 0..BOARD_SIZE as i32 {
                if self.get(row, col) == Some(Cell::Empty) || self.get(row, col).is_none() {
                    continue;
                }
                for dr in radius.saturating_neg()..=radius {
                    for dc in radius.saturating_neg()..=radius {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = row.saturating_add(dr);
                        let nc = col.saturating_add(dc);
                        if self.get(nr, nc) != Some(Cell::Empty) {
                            continue;
                        }
                        let (Ok(nru), Ok(ncu)) = (usize::try_from(nr), usize::try_from(nc)) else {
                            continue;
                        };
                        let Some(mark) = seen.get_mut(nru).and_then(|r| r.get_mut(ncu)) else {
                            continue;
                        };
                        if !*mark {
                            *mark = true;
                            moves.push((nru, ncu));
                        }
                    }
                }
            }
        }

        // Sort candidates by a quick heuristic: prefer moves closer to center
        moves.sort_by_key(|&(r, c)| {
            let r = i32::try_from(r).unwrap_or(LAST_INDEX);
            let c = i32::try_from(c).unwrap_or(LAST_INDEX);
            r.saturating_sub(center)
                .saturating_abs()
                .saturating_add(c.saturating_sub(center).saturating_abs())
        });

        moves
    }

    /// Check if placing a stone creates an immediate threat (four in a row
    /// or win). Used to prioritize moves in the AI search.
    fn is_threat_move(&self, row: usize, col: usize, stone: Cell) -> bool {
        let (Ok(r), Ok(c)) = (i32::try_from(row), i32::try_from(col)) else {
            return false;
        };
        for &(dr, dc) in &DIRECTIONS {
            let fwd = self.count_direction(r, c, dr, dc, stone);
            let bwd = self.count_direction(r, c, dr.saturating_neg(), dc.saturating_neg(), stone);
            let total = fwd.saturating_add(bwd).saturating_add(1);
            if total >= 4 {
                return true;
            }
        }
        false
    }
}

// -- AI (minimax with alpha-beta pruning) ----------------------------

/// Minimax search with alpha-beta pruning for the AI player.
fn minimax(
    board: &mut Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
    ai_stone: Cell,
) -> i32 {
    // Terminal checks
    if board.check_winner(ai_stone).is_some() {
        return SCORE_FIVE.saturating_add(depth); // Prefer faster wins
    }
    if board.check_winner(ai_stone.opponent()).is_some() {
        return SCORE_FIVE.saturating_add(depth).saturating_neg(); // Opponent won
    }
    if depth == 0 || board.is_full() {
        return board.evaluate(ai_stone);
    }

    let candidates = board.candidate_moves();
    if candidates.is_empty() {
        return board.evaluate(ai_stone);
    }

    let next = depth.saturating_sub(1);
    if maximizing {
        let mut best = i32::MIN;
        for (r, c) in candidates {
            board.set(r, c, ai_stone);
            let val = minimax(board, next, alpha, beta, false, ai_stone);
            board.set(r, c, Cell::Empty);
            if val > best {
                best = val;
            }
            if best > alpha {
                alpha = best;
            }
            if beta <= alpha {
                break;
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        let opponent = ai_stone.opponent();
        for (r, c) in candidates {
            board.set(r, c, opponent);
            let val = minimax(board, next, alpha, beta, true, ai_stone);
            board.set(r, c, Cell::Empty);
            if val < best {
                best = val;
            }
            if best < beta {
                beta = best;
            }
            if beta <= alpha {
                break;
            }
        }
        best
    }
}

/// Find the best move for the AI player using minimax with alpha-beta pruning.
/// Uses deeper search (depth 4) when there are threats on the board.
fn find_best_move(board: &Board, ai_stone: Cell) -> Option<(usize, usize)> {
    let candidates = board.candidate_moves();
    let mut best_move = *candidates.first()?;

    // Check for immediate wins first
    for &(r, c) in &candidates {
        let mut test = board.clone();
        test.set(r, c, ai_stone);
        if test.check_winner(ai_stone).is_some() {
            return Some((r, c));
        }
    }

    // Check for immediate blocks (opponent about to win)
    let opponent = ai_stone.opponent();
    for &(r, c) in &candidates {
        let mut test = board.clone();
        test.set(r, c, opponent);
        if test.check_winner(opponent).is_some() {
            return Some((r, c));
        }
    }

    // Determine search depth: use deeper search when threats exist
    let has_threats = candidates.iter().any(|&(r, c)| {
        board.is_threat_move(r, c, ai_stone) || board.is_threat_move(r, c, opponent)
    });
    let depth = if has_threats {
        AI_DEPTH.saturating_add(1)
    } else {
        AI_DEPTH
    };

    let mut best_score = i32::MIN;
    for (r, c) in candidates {
        let mut test = board.clone();
        test.set(r, c, ai_stone);
        let score = minimax(
            &mut test,
            depth.saturating_sub(1),
            i32::MIN,
            i32::MAX,
            false,
            ai_stone,
        );
        if score > best_score {
            best_score = score;
            best_move = (r, c);
        }
    }

    Some(best_move)
}

// ── Main application struct ─────────────────────────────────────────

/// The Gomoku application state.
struct GomokuApp {
    board: Board,
    phase: GamePhase,
    current_turn: Cell,
    cursor_row: i32,
    cursor_col: i32,
    move_history: Vec<MoveRecord>,
    move_count: usize,
    win_line: Option<WinLine>,
    winner: Cell,
    /// Score tracking across games: (black_wins, white_wins, draws).
    scores: (u32, u32, u32),
    /// The last stone placed (for the marker dot).
    last_move: Option<(usize, usize)>,
    /// The size the window is now, which is the size the last frame was drawn
    /// at and so the size the next click has to be read against.
    width: f32,
    height: f32,
}

impl GomokuApp {
    /// Create a new Gomoku app in its initial state.
    fn new() -> Self {
        Self {
            board: Board::new(),
            phase: GamePhase::Playing,
            current_turn: Cell::Black,
            cursor_row: BOARD_SIZE as i32 / 2,
            cursor_col: BOARD_SIZE as i32 / 2,
            move_history: Vec::new(),
            move_count: 0,
            win_line: None,
            winner: Cell::Empty,
            scores: (0, 0, 0),
            last_move: None,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Remember the size the window is now.
    fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// One frame: the boxes and the paint come out of the same pass, so a
    /// stone cannot be drawn in one place and clicked in another.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::solve(width, height);
        let mut f = Frame::new(width, height);
        self.draw(&mut f, &l);
        f
    }

    /// Start a new game, preserving scores.
    fn new_game(&mut self) {
        self.board = Board::new();
        self.phase = GamePhase::Playing;
        self.current_turn = Cell::Black;
        self.cursor_row = BOARD_SIZE as i32 / 2;
        self.cursor_col = BOARD_SIZE as i32 / 2;
        self.move_history.clear();
        self.move_count = 0;
        self.win_line = None;
        self.winner = Cell::Empty;
        self.last_move = None;
    }

    /// Attempt to place a stone at the cursor position.
    /// Returns true if the stone was placed successfully.
    fn try_place_stone(&mut self) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }

        let row = self.cursor_row as usize;
        let col = self.cursor_col as usize;

        if !self.board.is_empty(row, col) {
            return false;
        }

        self.place_stone(row, col);
        true
    }

    /// Place `stone`'s stone at `(row, col)` and work out what that did to
    /// the game.
    ///
    /// Both colours come through here. They used to not: `place_stone` and
    /// `ai_move` each had their own copy of "did that win, did that fill the
    /// board, credit the score", and the White copy had drifted -- it credited
    /// `scores.1` directly instead of matching on the stone, which is the same
    /// answer only for as long as nothing else ever calls it.
    fn place_stone(&mut self, row: usize, col: usize) {
        let stone = self.current_turn;
        self.board.set(row, col, stone);
        self.move_history.push(MoveRecord { row, col, stone });
        self.move_count = self.move_count.saturating_add(1);
        self.last_move = Some((row, col));

        if let Some(win_line) = self.board.check_winner(stone) {
            self.phase = GamePhase::Won;
            self.winner = stone;
            self.win_line = Some(win_line);
            match stone {
                Cell::Black => self.scores.0 = self.scores.0.saturating_add(1),
                Cell::White => self.scores.1 = self.scores.1.saturating_add(1),
                Cell::Empty => {}
            }
            return;
        }

        if self.board.is_full() {
            self.phase = GamePhase::Draw;
            self.scores.2 = self.scores.2.saturating_add(1);
            return;
        }

        self.current_turn = stone.opponent();
        // Black's move hands over to a search that has not run yet, and that
        // is a state rather than a step: the frame drawn between here and the
        // next tick is the one that says "White is thinking".
        self.phase = if self.current_turn == Cell::White {
            GamePhase::Thinking
        } else {
            GamePhase::Playing
        };
    }

    /// Run White's search and play its answer.
    ///
    /// Called from the tick rather than from the event handler, which is what
    /// makes [`GamePhase::Thinking`] a phase a frame can be drawn in.
    /// Returns whether there was a search to run, which is what the tick is
    /// answered with.
    ///
    /// The phase is tested here and nowhere else. It used to be tested twice
    /// -- once by the `Event::Tick` arm, to decide `Consumed` against
    /// `Ignored`, and once at the top of this function -- and a condition
    /// written down twice is a condition one of whose copies cannot be
    /// reached. The mutation sweep proved it: deleting the guard from this
    /// function changed no behaviour at all, because the call site had
    /// already established what it was checking for.
    fn think(&mut self) -> bool {
        if self.phase != GamePhase::Thinking {
            return false;
        }
        let Some((r, c)) = find_best_move(&self.board, Cell::White) else {
            // No legal reply and the board is not full: nothing to do but
            // hand the turn back, rather than sit in Thinking forever.
            self.current_turn = Cell::Black;
            self.phase = GamePhase::Playing;
            return true;
        };
        self.cursor_row = r as i32;
        self.cursor_col = c as i32;
        self.place_stone(r, c);
        true
    }

    /// Undo the last move(s). If the last move was by the AI (White),
    /// undo both the AI move and the preceding player move.
    fn undo(&mut self) {
        if self.move_history.is_empty() {
            return;
        }

        // If the game is over, undo the score it awarded as well as the
        // move. A search that has not run yet awarded nothing, so Thinking
        // just goes back to Playing.
        if self.phase == GamePhase::Thinking {
            self.phase = GamePhase::Playing;
        } else if self.phase != GamePhase::Playing {
            self.phase = GamePhase::Playing;
            self.win_line = None;
            // Take back the credit this game awarded. `Cell::Empty` is the
            // draw, which is the only reason a colourless stone names a
            // score at all.
            match self.winner {
                Cell::Black => self.scores.0 = self.scores.0.saturating_sub(1),
                Cell::White => self.scores.1 = self.scores.1.saturating_sub(1),
                Cell::Empty => self.scores.2 = self.scores.2.saturating_sub(1),
            }
            self.winner = Cell::Empty;
        }

        // Take back White's reply if it made one, then Black's move, so
        // that one press of Z gives the board back to the player rather than
        // handing them a position the opponent is about to answer.
        self.take_back(Cell::White);
        self.take_back(Cell::Black);

        // Update current turn and last_move
        self.current_turn = Cell::Black;
        self.last_move = self.move_history.last().map(|m| (m.row, m.col));
    }

    /// Lift the last stone from the board if it was `stone`'s.
    fn take_back(&mut self, stone: Cell) {
        if self.move_history.last().map(|m| m.stone) != Some(stone) {
            return;
        }
        let Some(record) = self.move_history.pop() else {
            return;
        };
        self.board.set(record.row, record.col, Cell::Empty);
        self.move_count = self.move_count.saturating_sub(1);
    }

    /// Handle keyboard input.
    ///
    /// Releases are ignored. They were not: every `match` arm below tested
    /// only `key`, so each keystroke arrived twice and did its work twice --
    /// the arrows stepped two intersections, which on a 15x15 board leaves
    /// every other row and column unreachable from the keyboard entirely,
    /// `N` dealt two games and `Z` took back two moves.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }
        match event {
            // Arrow key movement
            KeyEvent { key: Key::Up, .. } if self.cursor_row > 0 => {
                self.cursor_row = self.cursor_row.saturating_sub(1);
            }
            KeyEvent { key: Key::Down, .. } if self.cursor_row < LAST_INDEX => {
                self.cursor_row = self.cursor_row.saturating_add(1);
            }
            KeyEvent { key: Key::Left, .. } if self.cursor_col > 0 => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
            }
            KeyEvent {
                key: Key::Right, ..
            } if self.cursor_col < LAST_INDEX => {
                self.cursor_col = self.cursor_col.saturating_add(1);
            }

            // Place stone
            KeyEvent {
                key: Key::Enter, ..
            }
            | KeyEvent {
                key: Key::Space, ..
            } => {
                self.try_place_stone();
            }

            // New game
            KeyEvent { key: Key::N, .. } => {
                self.new_game();
            }

            // Undo
            KeyEvent { key: Key::Z, .. } => {
                self.undo();
            }

            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Handle mouse clicks.
    ///
    /// The click is resolved against the frame the window was last drawn at,
    /// so a stone is clickable exactly where its ink is. It used to be
    /// resolved by `intersection_near`, a free function of `CELL_SIZE` and
    /// two offset constants: in any window that was not 800x600 the board was
    /// drawn in one place and clicked in another.
    ///
    /// It also used to return early unless it was Black's turn in a live
    /// game, which made New game and Undo keyboard-only -- so the pointer
    /// could not start a second game after the first one ended.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return EventResult::Ignored;
        };
        let frame = self.frame(self.width, self.height);
        match frame.hit_test(event.x, event.y) {
            Some(Target::NewGame) => {
                self.new_game();
                EventResult::Consumed
            }
            Some(Target::Undo) => {
                self.undo();
                EventResult::Consumed
            }
            Some(Target::Point(row, col)) => {
                if self.phase != GamePhase::Playing || self.current_turn != Cell::Black {
                    return EventResult::Ignored;
                }
                self.cursor_row = i32::try_from(row).unwrap_or(self.cursor_row);
                self.cursor_col = i32::try_from(col).unwrap_or(self.cursor_col);
                // Consumed either way: even a click on an occupied point
                // moves the cursor, so the picture changed and the window has
                // to repaint.
                self.try_place_stone();
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    /// Handle a general event.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            // The tick is answered by whether a search actually ran, rather
            // than by re-deciding here whether one should have.
            Event::Tick { .. } if self.think() => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }

    /// Draw the whole window into `f`.
    ///
    /// Every rectangle comes from `l`, and every intersection and button
    /// records a hit box, so the click handler answers from the picture rather
    /// than from arithmetic over constants the picture may not have been drawn
    /// from. `render` used to take no width and no height at all.
    fn draw(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, BASE, CornerRadii::all(0.0));
        self.draw_header(f, l);
        self.draw_board(f, l);
        self.draw_panel(f, l);
        self.draw_status(f, l);
    }

    /// The title, and beside it whose turn it is.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        let x = l.board.x.max(l.pad);
        text_at(
            f,
            TITLE_TEXT,
            x,
            l.header.y + (l.header.h - l.title) / 2.0,
            l.title,
            FontWeightHint::Bold,
            TEXT_COLOR,
            Some(l.header.w - x - l.pad),
        );

        // The turn indicator is placed past the measured width of the title
        // rather than at a hand-tuned offset from it.
        let used = text::measure(TITLE_TEXT, l.title, FontWeightHint::Bold);
        let (turn_text, turn_color) = match self.phase {
            GamePhase::Won if self.winner == Cell::Black => ("Black wins", GREEN),
            GamePhase::Won => ("White wins", RED),
            GamePhase::Draw => ("Draw", YELLOW),
            GamePhase::Thinking => ("White is thinking", LAVENDER),
            GamePhase::Playing if self.current_turn == Cell::Black => ("Black to play", BLUE),
            GamePhase::Playing => ("White to play", SUBTEXT0),
        };
        let tx = x + used + l.pad * 2.0;
        let room = l.header.right() - l.pad - tx;
        if room > l.font * 3.0 {
            text_at(
                f,
                turn_text,
                tx,
                l.header.y + (l.header.h - l.font) / 2.0,
                l.font,
                FontWeightHint::Regular,
                turn_color,
                Some(room),
            );
        }
    }

    /// The board: its wood, its border, the grid, the star points, the
    /// coordinate labels, the win line, the stones and the cursor.
    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.board.is_empty() || l.cell <= 0.0 {
            return;
        }
        fill(f, l.board, BOARD_BG, CornerRadii::all(l.pad * 0.4));
        f.push(RenderCommand::StrokeRect {
            x: l.board.x,
            y: l.board.y,
            width: l.board.w,
            height: l.board.h,
            color: BOARD_BORDER,
            line_width: (l.cell * 0.08).clamp(1.0, 3.0),
            corner_radii: CornerRadii::all(l.pad * 0.4),
        });

        let line_w = (l.cell * 0.04).max(1.0);
        let (fx, fy) = l.intersection(0, 0);
        let (lx, ly) = l.intersection(LAST_INDEX, LAST_INDEX);
        for i in 0..BOARD_SIZE as i32 {
            let (_, y) = l.intersection(i, 0);
            f.push(RenderCommand::Line {
                x1: fx,
                y1: y,
                x2: lx,
                y2: y,
                color: GRID_LINE_COLOR,
                width: line_w,
            });
            let (x, _) = l.intersection(0, i);
            f.push(RenderCommand::Line {
                x1: x,
                y1: fy,
                x2: x,
                y2: ly,
                color: GRID_LINE_COLOR,
                width: line_w,
            });
        }

        let star_r = (l.cell * 0.11).max(1.0);
        for &(r, c) in &STAR_POINTS {
            let (x, y) = l.intersection(r as i32, c as i32);
            fill(
                f,
                Rect::new(x - star_r, y - star_r, star_r * 2.0, star_r * 2.0),
                STAR_POINT_COLOR,
                CornerRadii::all(star_r),
            );
        }

        self.draw_coordinates(f, l);
        self.draw_win_line(f, l);
        self.draw_stones(f, l);
        self.draw_cursor(f, l);
    }

    /// Column letters above and below, row numbers left and right.
    ///
    /// Each label is centred on the line it names, measured rather than nudged
    /// by a constant, and dropped whole when the margin is too small to hold
    /// it -- which is what stops a squeezed board printing its labels over its
    /// own outermost stones.
    fn draw_coordinates(&self, f: &mut Frame<Target>, l: &Layout) {
        let gap = l.origin.1 - l.board.y;
        if gap < l.label * 1.1 {
            return;
        }
        for i in 0..BOARD_SIZE as i32 {
            let letter = char::from(b'A'.saturating_add(i as u8)).to_string();
            let number = (BOARD_SIZE as i32).saturating_sub(i).to_string();
            let (x, y) = l.intersection(i, i);
            let half_l = text::measure(&letter, l.label, FontWeightHint::Regular) / 2.0;
            let half_n = text::measure(&number, l.label, FontWeightHint::Regular) / 2.0;

            for ly in [
                l.board.y + (gap - l.label) / 2.0,
                l.board.bottom() - gap + (gap - l.label) / 2.0,
            ] {
                text_at(
                    f,
                    &letter,
                    x - half_l,
                    ly,
                    l.label,
                    FontWeightHint::Regular,
                    SUBTEXT0,
                    None,
                );
            }
            for lx in [
                l.board.x + gap / 2.0 - half_n,
                l.board.right() - gap / 2.0 - half_n,
            ] {
                text_at(
                    f,
                    &number,
                    lx,
                    y - l.label / 2.0,
                    l.label,
                    FontWeightHint::Regular,
                    SUBTEXT0,
                    None,
                );
            }
        }
    }

    /// The five stones that won, marked behind them.
    fn draw_win_line(&self, f: &mut Frame<Target>, l: &Layout) {
        let Some(win) = self.win_line.as_ref() else {
            return;
        };
        for &(r, c) in &win.positions {
            let rect = l.stone_rect(r as i32, c as i32);
            fill(f, rect, WIN_HIGHLIGHT, CornerRadii::all(l.stone));
        }
    }

    /// Every stone on the board, and a hit box on every intersection --
    /// occupied or not, because an empty one is where the next stone goes.
    fn draw_stones(&self, f: &mut Frame<Target>, l: &Layout) {
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                let rect = l.stone_rect(row as i32, col as i32);
                f.hit(Target::Point(row, col), rect);
                let Some(cell) = self.board.get(row as i32, col as i32) else {
                    continue;
                };
                let (body, border) = match cell {
                    Cell::Black => (BLACK_STONE, BLACK_STONE_BORDER),
                    Cell::White => (WHITE_STONE, WHITE_STONE_BORDER),
                    Cell::Empty => continue,
                };
                fill(f, rect, body, CornerRadii::all(l.stone));
                f.push(RenderCommand::StrokeRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.w,
                    height: rect.h,
                    color: border,
                    line_width: (l.stone * 0.12).max(1.0),
                    corner_radii: CornerRadii::all(l.stone),
                });
                if self.last_move == Some((row, col)) {
                    let r = (l.stone * 0.3).max(1.0);
                    let (x, y) = l.intersection(row as i32, col as i32);
                    fill(
                        f,
                        Rect::new(x - r, y - r, r * 2.0, r * 2.0),
                        LAST_MOVE_MARKER,
                        CornerRadii::all(r),
                    );
                }
            }
        }
    }

    /// The keyboard cursor, drawn only while there is a move to make with it.
    fn draw_cursor(&self, f: &mut Frame<Target>, l: &Layout) {
        if self.phase != GamePhase::Playing || self.current_turn != Cell::Black {
            return;
        }
        let rect = l.stone_rect(self.cursor_row, self.cursor_col);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: CURSOR_COLOR,
            line_width: (l.stone * 0.16).max(1.0),
            corner_radii: CornerRadii::all(l.stone * 0.3),
        });
    }

    /// The information column: the move count, the scores, and the two
    /// buttons that used to be keyboard-only.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.is_empty() {
            return;
        }
        fill(f, l.panel, MANTLE, CornerRadii::all(0.0));
        let x = l.panel.x + l.pad;
        let w = (l.panel.w - l.pad * 2.0).max(0.0);
        let mut y = l.panel.y + l.pad;
        let step = l.font * 1.6;

        let heading = |f: &mut Frame<Target>, s: &str, y: &mut f32| {
            text_at(f, s, x, *y, l.font, FontWeightHint::Bold, LAVENDER, Some(w));
            *y += l.font * 1.2;
            f.push(RenderCommand::Line {
                x1: x,
                y1: *y,
                x2: x + w,
                y2: *y,
                color: SURFACE1,
                width: 1.0,
            });
            *y += l.pad * 0.6;
        };

        heading(f, GAME_INFO_HEADING, &mut y);
        for line in [
            format!("{MOVES_STEM}{}", self.move_count),
            format!(
                "{TURN_STEM}{}",
                match self.current_turn {
                    Cell::Black => "Black",
                    Cell::White => "White",
                    Cell::Empty => "-",
                }
            ),
        ] {
            text_at(
                f,
                &line,
                x,
                y,
                l.font,
                FontWeightHint::Regular,
                TEXT_COLOR,
                Some(w),
            );
            y += step;
        }

        y += l.pad;
        heading(f, SCORES_HEADING, &mut y);
        for (line, color) in [
            (format!("{BLACK_SCORE_STEM}{}", self.scores.0), TEXT_COLOR),
            (format!("{WHITE_SCORE_STEM}{}", self.scores.1), TEXT_COLOR),
            (format!("{DRAWS_STEM}{}", self.scores.2), SUBTEXT0),
        ] {
            text_at(
                f,
                &line,
                x,
                y,
                l.font,
                FontWeightHint::Regular,
                color,
                Some(w),
            );
            y += step;
        }

        // The buttons. Undo and new game were reachable only from the keyboard,
        // and the footer text was the only record that those keys existed.
        y += l.pad;
        let bh = (l.font * 2.0).max(1.0);
        for (label, target, enabled) in [
            (NEW_GAME_LABEL, Target::NewGame, true),
            (UNDO_LABEL, Target::Undo, !self.move_history.is_empty()),
        ] {
            if y + bh > l.panel.bottom() {
                break;
            }
            let r = Rect::new(x, y, w, bh);
            fill(
                f,
                r,
                if enabled { SURFACE0 } else { MANTLE },
                CornerRadii::all(l.pad * 0.4),
            );
            if enabled {
                f.hit(target, r);
            }
            let tw = text::measure(label, l.small, FontWeightHint::Regular);
            text_at(
                f,
                label,
                r.x + (r.w - tw) / 2.0,
                r.y + (r.h - l.small) / 2.0,
                l.small,
                FontWeightHint::Regular,
                if enabled { TEXT_COLOR } else { OVERLAY0 },
                Some(r.w),
            );
            y += bh + l.pad * 0.6;
        }
    }

    /// The band along the bottom: what the keys do, or how the game ended.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.status.is_empty() {
            return;
        }
        let (msg, color) = match self.phase {
            GamePhase::Won if self.winner == Cell::Black => {
                ("Black wins! Z to take it back, N for a new game", GREEN)
            }
            GamePhase::Won => ("White wins. Z to take it back, N for a new game", RED),
            GamePhase::Draw => ("A draw. N for a new game", YELLOW),
            GamePhase::Thinking => ("White is thinking...", LAVENDER),
            GamePhase::Playing => (
                "Arrows move, Enter places, Z undoes, N starts again",
                SUBTEXT0,
            ),
        };
        fill(f, l.status, MANTLE, CornerRadii::all(0.0));
        let tw = text::measure(msg, l.small, FontWeightHint::Regular);
        text_at(
            f,
            msg,
            l.status.x + ((l.status.w - tw) / 2.0).max(l.pad),
            l.status.y + (l.status.h - l.small) / 2.0,
            l.small,
            FontWeightHint::Regular,
            color,
            Some((l.status.w - l.pad * 2.0).max(0.0)),
        );
    }
}

/// Fill `r`, unless there is nothing of it to fill.
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

/// Draw `s` at `(x, y)`, cut to `max_width` where there is one.
///
/// The width that is passed here is the width the renderer is told to stop at,
/// so a label cannot be positioned against one limit and drawn against another.
fn text_at(
    f: &mut Frame<Target>,
    s: &str,
    x: f32,
    y: f32,
    font_size: f32,
    font_weight: FontWeightHint,
    color: Color,
    max_width: Option<f32>,
) {
    if s.is_empty() || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width,
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

impl App for GomokuApp {
    fn title(&self) -> String {
        String::from(TITLE_TEXT)
    }

    fn app_id(&self) -> String {
        String::from("gomoku")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// The opponent's search runs on this tick rather than inside the event
    /// handler. Without an interval here the game would enter
    /// [`GamePhase::Thinking`] after Black's first stone and stay there.
    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(60))
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
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for GomokuApp {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }

    fn scroll_at(
        &mut self,
        _x: f32,
        _y: f32,
        _dy: f32,
        _size: (f32, f32),
    ) -> Option<Self::Outcome> {
        // Nothing scrolls: the board is sized to the window rather than
        // panned inside it.
        None
    }
}

fn main() -> ExitCode {
    let mut app = GomokuApp::new();
    app::launch("gomoku", &mut app)
}

// ── Tests ───────────────────────────────────────────────────────────

// -- Tests -----------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that panics on bad data is a test that failed, which is the point"
)]
mod tests {
    use super::*;
    use guitk::probe;

    // =======================================================================
    // Scaffolding
    // =======================================================================

    /// The window every test that does not say otherwise is read against.
    const W: (f32, f32) = GomokuApp::SIZE;

    /// The sizes a claim about the layout has to hold at.
    ///
    /// A narrow one, a short one, a tall one and a very large one. Two sizes
    /// cannot separate a formula that scales from one that is merely
    /// proportional to the one number both happen to share (lesson 86).
    const SIZES: [(f32, f32); 6] = [
        (320.0, 240.0),
        (400.0, 900.0),
        (640.0, 560.0),
        (900.0, 400.0),
        (1280.0, 800.0),
        (1920.0, 1080.0),
    ];

    fn key_of(key: Key) -> KeyEvent {
        probe::press(key)
    }

    /// Put a stone on the board without going through the game, so that a
    /// position can be built without the opponent answering it.
    fn place_raw(board: &mut Board, row: usize, col: usize, stone: Cell) {
        assert!(
            board.set(row, col, stone),
            "({row}, {col}) is off the board"
        );
    }

    /// An app whose board is `stones`, with nothing else touched.
    fn app_with(stones: &[(usize, usize, Cell)]) -> GomokuApp {
        let mut app = GomokuApp::new();
        for &(r, c, stone) in stones {
            place_raw(&mut app.board, r, c, stone);
        }
        app
    }

    /// Play `n` stones for Black by hand, letting White answer each one.
    ///
    /// The reply arrives on a tick rather than inside the placement, which is
    /// the whole point of [`GamePhase::Thinking`], so the tick is sent here
    /// exactly as the window would send it.
    fn play_moves(app: &mut GomokuApp, moves: &[(i32, i32)]) {
        for &(r, c) in moves {
            app.cursor_row = r;
            app.cursor_col = c;
            app.handle_key(&key_of(Key::Enter));
            app.handle_event(&Event::Tick { elapsed_ms: 60 });
        }
    }

    /// True when `body` is painted with its origin inside `r`.
    ///
    /// A hit box says a click lands somewhere; this says the user can see what
    /// they are aiming at (lesson 81).
    fn text_inside(frame: &Frame<Target>, body: &str, r: Rect) -> bool {
        frame.commands().iter().any(|c| {
            matches!(c, RenderCommand::Text { text, x, y, .. }
                if text.as_str() == body && r.contains(*x, *y))
        })
    }

    /// Every string the frame paints.
    fn texts(frame: &Frame<Target>) -> Vec<String> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// True when some painted string contains `needle`.
    fn says(frame: &Frame<Target>, needle: &str) -> bool {
        texts(frame).iter().any(|t| t.contains(needle))
    }

    /// True when some string painted with its origin inside `r` contains
    /// `needle`.
    ///
    /// [`says`] cannot tell two bands apart, and gomoku has two that say the
    /// same thing: the header and the status band both report "White is
    /// thinking". A mutation that stopped the status band saying it survived a
    /// suite that only asked whether the *frame* said it, because the header
    /// still did. Any claim about a particular band has to name the band.
    fn says_in(frame: &Frame<Target>, needle: &str, r: Rect) -> bool {
        frame.commands().iter().any(|c| {
            matches!(c, RenderCommand::Text { text, x, y, .. }
                if text.contains(needle) && r.contains(*x, *y))
        })
    }

    /// Every filled rectangle of `color`.
    fn fills_of(frame: &Frame<Target>, color: Color) -> Vec<Rect> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color: got,
                    ..
                } if *got == color => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    /// Every stroked rectangle of `color`.
    fn strokes_of(frame: &Frame<Target>, color: Color) -> Vec<Rect> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color: got,
                    ..
                } if *got == color => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    // =======================================================================
    // The window
    // =======================================================================

    /// The frame is painted to the edges of whatever window it is given, not
    /// to the edges of the one the program was written for.
    ///
    /// `render` took no width and no height at all: it drew a background of
    /// `PANEL_X + 220.0` by a constant height into every window there was.
    #[test]
    fn the_background_covers_the_window_at_every_size() {
        for size in SIZES {
            let app = GomokuApp::new();
            let frame = app.frame(size.0, size.1);
            assert!(
                frame.commands().iter().any(|c| matches!(c,
                    RenderCommand::FillRect { x, y, width, height, .. }
                        if *x == 0.0 && *y == 0.0
                            && (*width - size.0).abs() < 0.01
                            && (*height - size.1).abs() < 0.01)),
                "no background covering {size:?}"
            );
        }
    }

    /// Nothing the app draws is placed outside the window it was given.
    ///
    /// Unclipped, deliberately: `Frame::hit` drops a box that is empty after
    /// clipping, so testing hit boxes here would be asking a question whose
    /// answer the clip already guaranteed (lesson 80). This walks the paint.
    #[test]
    fn nothing_is_painted_outside_the_window() {
        for size in SIZES {
            let mut app = app_with(&[
                (7, 7, Cell::Black),
                (7, 8, Cell::White),
                (0, 0, Cell::Black),
                (LAST_INDEX as usize, LAST_INDEX as usize, Cell::White),
            ]);
            app.last_move = Some((7, 8));
            app.move_history.push(MoveRecord {
                row: 7,
                col: 7,
                stone: Cell::Black,
            });
            for phase in [
                GamePhase::Playing,
                GamePhase::Thinking,
                GamePhase::Won,
                GamePhase::Draw,
            ] {
                app.phase = phase;
                for cmd in app.frame(size.0, size.1).commands() {
                    let (x, y) = match cmd {
                        RenderCommand::FillRect { x, y, .. }
                        | RenderCommand::Text { x, y, .. }
                        | RenderCommand::StrokeRect { x, y, .. } => (*x, *y),
                        RenderCommand::Line { x1, y1, .. } => (*x1, *y1),
                        _ => continue,
                    };
                    assert!(
                        x >= -1.5 && y >= -1.5 && x <= size.0 && y <= size.1,
                        "{phase:?} draws at ({x}, {y}), outside a {size:?} window"
                    );
                }
            }
        }
    }

    // =======================================================================
    // The layout
    // =======================================================================

    /// The bands stack down the window without overlapping each other.
    #[test]
    fn the_bands_stack_without_overlapping() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            assert!(l.header.y >= -0.01, "the header starts above the window");
            assert!(
                l.board.y >= l.header.bottom() - 0.01,
                "the board at {:?} runs into the header at {:?} ({size:?})",
                l.board,
                l.header
            );
            assert!(
                l.board.bottom() <= l.status.y + 0.01,
                "the board at {:?} runs into the status band at {:?} ({size:?})",
                l.board,
                l.status
            );
            assert!(
                l.panel.is_empty() || l.panel.x >= l.board.right() - 0.01,
                "the panel at {:?} sits on the board at {:?} ({size:?})",
                l.panel,
                l.board
            );
            assert!(
                l.status.bottom() <= size.1 + 0.01,
                "the status band runs off the bottom at {size:?}"
            );
        }
    }

    /// The board is square, whatever shape the window is.
    ///
    /// A 15x15 grid drawn at 36 px per cell into a 400 px-tall window put its
    /// bottom five rows below the window, which is what a fixed `CELL_SIZE`
    /// does to a shape it was not written for.
    #[test]
    fn the_board_is_square_and_fits_the_window() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            assert!(
                (l.board.w - l.board.h).abs() < 0.01,
                "the board is {}x{} at {size:?}",
                l.board.w,
                l.board.h
            );
            assert!(l.board.w > 0.0, "no board at all at {size:?}");
            assert!(
                l.board.right() <= size.0 + 0.01 && l.board.bottom() <= size.1 + 0.01,
                "the board {:?} leaves a {size:?} window",
                l.board
            );
        }
    }

    /// A bigger window gets a bigger board, and both dimensions can be the one
    /// that makes it bigger.
    ///
    /// Two windows cannot tell a formula that scales from one that is merely
    /// proportional to the single number they share (lesson 86). The board is
    /// square, so only the smaller of the two free dimensions can bind: each
    /// pair below therefore grows the dimension that is actually binding, and
    /// the pairs bind on opposite axes. Growing 600x500 to 1400x500 would
    /// prove nothing -- the height still binds and the board is right to stay
    /// the size it was.
    #[test]
    fn the_board_grows_with_the_window() {
        let (short, tall) = (Layout::solve(900.0, 400.0), Layout::solve(900.0, 600.0));
        assert!(
            tall.board.h > short.board.h * 1.2,
            "a window 200 px taller drew a {} px board where 400 px drew {}",
            tall.board.h,
            short.board.h
        );

        let (narrow, wide) = (Layout::solve(400.0, 900.0), Layout::solve(500.0, 900.0));
        assert!(
            wide.board.w > narrow.board.w * 1.1,
            "a window 100 px wider drew a {} px board where 400 px drew {}",
            wide.board.w,
            narrow.board.w
        );

        // And the cell grew with the board rather than the grid gaining rows.
        assert!(tall.cell > short.cell && wide.cell > narrow.cell);
    }

    /// Every intersection is inside the board, and consecutive ones are one
    /// cell apart.
    #[test]
    fn the_grid_is_evenly_spaced_inside_the_board() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            for row in 0..BOARD_SIZE as i32 {
                for col in 0..BOARD_SIZE as i32 {
                    let (x, y) = l.intersection(row, col);
                    assert!(
                        x >= l.board.x - 0.01
                            && x <= l.board.right() + 0.01
                            && y >= l.board.y - 0.01
                            && y <= l.board.bottom() + 0.01,
                        "({row}, {col}) is at ({x}, {y}), outside the board {:?} at {size:?}",
                        l.board
                    );
                    if col > 0 {
                        let (px, _) = l.intersection(row, col - 1);
                        assert!(
                            (x - px - l.cell).abs() < 0.01,
                            "column {col} is not one cell from {} at {size:?}",
                            col - 1
                        );
                    }
                }
            }
        }
    }

    /// The outermost lines are inset by the margin the labels live in, so the
    /// grid does not run to the very edge of the wood.
    #[test]
    fn the_grid_is_inset_from_the_edge_of_the_board() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            let (x0, y0) = l.intersection(0, 0);
            let (x1, y1) = l.intersection(LAST_INDEX, LAST_INDEX);
            assert!(
                x0 > l.board.x && y0 > l.board.y,
                "the first line is on the edge of the board at {size:?}"
            );
            assert!(
                x1 < l.board.right() && y1 < l.board.bottom(),
                "the last line is on the edge of the board at {size:?}"
            );
            assert!(
                (x0 - l.board.x - (l.board.right() - x1)).abs() < 0.01,
                "the margins are not equal at {size:?}"
            );
        }
    }

    /// No two stones can touch, whatever the window.
    ///
    /// Half a cell is exactly where two stones on neighbouring intersections
    /// meet, so the radius has to be under it.
    #[test]
    fn stones_on_neighbouring_points_do_not_overlap() {
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            let a = l.stone_rect(7, 7);
            let b = l.stone_rect(7, 8);
            let c = l.stone_rect(8, 7);
            assert!(
                a.right() < b.x,
                "stones on the same row overlap at {size:?}: {a:?} and {b:?}"
            );
            assert!(
                a.bottom() < c.y,
                "stones in the same column overlap at {size:?}"
            );
            assert!(
                a.w > l.cell * 0.5,
                "a stone is smaller than half a cell at {size:?}, which is a \
                 board of dots rather than of stones"
            );
        }
    }

    /// The panel is dropped whole rather than drawn too narrow to read.
    ///
    /// Both halves are asserted, because a rule that only ever fires one way
    /// is indistinguishable from a constant (lesson 90): a window with room
    /// for the column keeps it, and one without loses it entirely rather than
    /// squeezing the board to make space for an illegible strip.
    #[test]
    fn a_window_with_no_room_for_the_panel_drops_it_whole() {
        let wide = Layout::solve(1000.0, 600.0);
        assert!(
            !wide.panel.is_empty(),
            "a 1000x600 window has room for the panel and did not get one"
        );
        assert!(
            wide.panel.w >= wide.font * 6.0,
            "the panel is narrower than its own text at 1000x600"
        );

        let narrow = Layout::solve(400.0, 900.0);
        assert!(
            narrow.panel.is_empty(),
            "a 400x900 window kept a {} px panel beside a board that needs \
             the width",
            narrow.panel.w
        );
        assert!(
            narrow.board.w > wide.board.w * 0.5,
            "dropping the panel did not give the width to the board"
        );
    }

    /// A window with the panel dropped draws no panel, rather than drawing one
    /// of zero width somewhere.
    #[test]
    fn a_dropped_panel_is_not_painted() {
        let app = GomokuApp::new();
        let frame = app.frame(400.0, 900.0);
        assert!(
            !says(&frame, "Scores"),
            "the panel was dropped from the layout and drawn anyway"
        );
        assert!(
            probe::rect_of_sized(&app, Target::NewGame, (400.0, 900.0)).is_none(),
            "a button in a panel that is not there is still clickable"
        );
    }

    /// Every line the panel draws fits between its padding, at every window
    /// size that draws a panel at all.
    ///
    /// The layout decides whether a panel is worth having by measuring its
    /// widest line, and the widest line it measured used to be `"Draws: 88"` --
    /// a string the panel never draws, and narrower than `"New game (N)"` and
    /// `"\u{25CF} Black: 0"`, which it does (known-issues lesson 93). The test
    /// iterates the same named constants the layout measures and `draw_panel`
    /// draws, so a hint edited in one place cannot fall out of step with the
    /// other two.
    ///
    /// Asked of the default window alone it would prove nothing: at 900 px the
    /// panel is a quarter of the width and comfortably wide whatever was
    /// measured. The measurement decides something only in the windows near
    /// the limit, which are exactly the ones a single-size test never visits --
    /// so sweep them, and assert the sweep actually found panels, or a layout
    /// change that dropped the panel everywhere would turn this into a loop
    /// that runs zero times and passes.
    #[test]
    fn the_panel_is_wide_enough_for_the_lines_it_holds() {
        let mut checked = 0_u32;
        for w in (280_u16..=1600).step_by(20) {
            for h in [400.0_f32, 640.0, 900.0] {
                let l = Layout::solve(f32::from(w), h);
                if l.panel.is_empty() {
                    continue;
                }
                checked = checked.saturating_add(1);
                let room = l.panel.w - l.pad * 2.0;
                let check = |line: &str, got: f32| {
                    assert!(
                        got <= room + 0.01,
                        "{line:?} wants {got} px of the {room} px the panel leaves at {w}x{h}"
                    );
                };
                for heading in PANEL_HEADINGS {
                    check(
                        heading,
                        text::measure(heading, l.font, FontWeightHint::Bold),
                    );
                }
                for (stem, tail) in PANEL_LINES {
                    let line = format!("{stem}{tail}");
                    let got = text::measure(&line, l.font, FontWeightHint::Regular);
                    check(&line, got);
                }
                for button in PANEL_BUTTONS {
                    check(
                        button,
                        text::measure(button, l.small, FontWeightHint::Regular),
                    );
                }
            }
        }
        assert!(checked > 50, "only {checked} of those windows drew a panel");
    }

    /// The coordinate labels are drawn when there is room for them and dropped
    /// when there is not, rather than printed over the outermost stones.
    #[test]
    fn the_coordinate_labels_are_dropped_when_they_do_not_fit() {
        let app = GomokuApp::new();
        let roomy = app.frame(W.0, W.1);
        assert!(
            says(&roomy, "A") && says(&roomy, "15"),
            "a 900x640 window has room for the coordinates and drew none"
        );

        let l = Layout::solve(60.0, 60.0);
        assert!(
            l.origin.1 - l.board.y < l.label * 1.1,
            "60x60 was chosen because its margin is too small for a label, \
             and it is not: margin {} against label {}",
            l.origin.1 - l.board.y,
            l.label
        );
        let cramped = app.frame(60.0, 60.0);
        assert!(
            !says(&cramped, "15"),
            "the labels were drawn into a margin too small to hold them"
        );
    }

    /// A label is centred on the line it names rather than nudged by a
    /// constant.
    #[test]
    fn a_coordinate_label_is_centred_on_its_line() {
        let app = GomokuApp::new();
        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);
        let mut checked = 0;
        for (i, letter) in ["A", "H", "O"].iter().enumerate() {
            let col = [0, 7, 14][i];
            let (x, _) = l.intersection(0, col);
            let half = text::measure(letter, l.label, FontWeightHint::Regular) / 2.0;
            assert!(
                frame.commands().iter().any(|c| matches!(c,
                    RenderCommand::Text { text, x: tx, .. }
                        if text == letter && (*tx + half - x).abs() < 0.6)),
                "the label {letter} is not centred on column {col}"
            );
            checked += 1;
        }
        assert_eq!(checked, 3);
    }

    // =======================================================================
    // Placing a stone, and the reply that arrives on a tick
    // =======================================================================

    #[test]
    fn a_fresh_game_is_blacks_to_play() {
        let app = GomokuApp::new();
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(app.move_count, 0);
        assert_eq!(app.winner, Cell::Empty);
        assert!(app.win_line.is_none());
        assert!(app.last_move.is_none());
        assert_eq!(app.cursor_row, BOARD_SIZE as i32 / 2);
        assert_eq!(app.cursor_col, BOARD_SIZE as i32 / 2);
    }

    #[test]
    fn a_stone_lands_where_the_cursor_is() {
        let mut app = GomokuApp::new();
        app.cursor_row = 4;
        app.cursor_col = 9;
        assert!(app.try_place_stone());
        assert_eq!(app.board.get(4, 9), Some(Cell::Black));
        assert_eq!(app.move_count, 1);
        assert_eq!(app.last_move, Some((4, 9)));
    }

    #[test]
    fn a_stone_cannot_be_placed_on_a_stone() {
        let mut app = GomokuApp::new();
        app.cursor_row = 4;
        app.cursor_col = 9;
        assert!(app.try_place_stone());
        // Back to Black's turn without letting White answer, so that the
        // refusal is about the occupied point and not about the turn.
        app.phase = GamePhase::Playing;
        app.current_turn = Cell::Black;
        assert!(!app.try_place_stone(), "a second stone landed on the first");
        assert_eq!(app.move_count, 1);
    }

    #[test]
    fn nothing_can_be_placed_after_the_game_is_over() {
        let mut app = GomokuApp::new();
        app.phase = GamePhase::Won;
        app.winner = Cell::Black;
        app.cursor_row = 2;
        app.cursor_col = 2;
        assert!(!app.try_place_stone());
        assert_eq!(app.board.get(2, 2), Some(Cell::Empty));
    }

    /// Black's stone leaves the game *waiting* for White rather than already
    /// answered by White.
    ///
    /// This is the whole reason [`GamePhase::Thinking`] exists. The search
    /// used to run inside `place_stone`, which runs inside the event handler,
    /// so it finished before the handler returned: there was no moment at
    /// which the game was Black-has-moved-and-White-has-not, and so no frame
    /// in which "White is thinking" could be drawn.
    #[test]
    fn blacks_move_leaves_white_thinking_rather_than_answered() {
        let mut app = GomokuApp::new();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&key_of(Key::Enter));
        assert_eq!(app.phase, GamePhase::Thinking);
        assert_eq!(app.current_turn, Cell::White);
        assert_eq!(app.move_count, 1, "White replied inside the event handler");
    }

    /// And the reply arrives on the next tick.
    #[test]
    fn the_tick_is_what_makes_white_move() {
        let mut app = GomokuApp::new();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&key_of(Key::Enter));
        assert_eq!(app.move_count, 1);
        app.handle_event(&Event::Tick { elapsed_ms: 60 });
        assert_eq!(app.move_count, 2, "the tick did not run White's search");
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(
            app.move_history.last().map(|m| m.stone),
            Some(Cell::White),
            "the second stone was not White's"
        );
    }

    /// A tick when nobody is thinking changes nothing and asks for no repaint.
    #[test]
    fn a_tick_with_nothing_to_think_about_is_ignored() {
        let mut app = GomokuApp::new();
        let before = app.move_count;
        assert_eq!(
            app.handle_event(&Event::Tick { elapsed_ms: 60 }),
            EventResult::Ignored
        );
        assert_eq!(app.move_count, before);
    }

    /// White replies to a real threat rather than playing anywhere legal.
    ///
    /// Four Black stones in a row with both ends open: any reply that is not
    /// one of the two ends loses on Black's next move.
    #[test]
    fn white_blocks_a_four_it_is_about_to_lose_to() {
        let mut app = GomokuApp::new();
        for c in 4..8 {
            place_raw(&mut app.board, 7, c, Cell::Black);
        }
        place_raw(&mut app.board, 2, 2, Cell::White);
        app.current_turn = Cell::White;
        app.phase = GamePhase::Thinking;
        app.think();
        let played = app.last_move.expect("White played");
        assert!(
            played == (7, 3) || played == (7, 8),
            "White played {played:?} and let Black's open four become five"
        );
    }

    /// And it takes a win in front of it rather than blocking.
    #[test]
    fn white_takes_a_five_when_it_has_one() {
        let mut app = GomokuApp::new();
        for c in 4..8 {
            place_raw(&mut app.board, 7, c, Cell::White);
        }
        for c in 4..8 {
            place_raw(&mut app.board, 9, c, Cell::Black);
        }
        app.current_turn = Cell::White;
        app.phase = GamePhase::Thinking;
        app.think();
        assert_eq!(app.phase, GamePhase::Won, "White did not take its five");
        assert_eq!(app.winner, Cell::White);
    }

    /// A full board with no five is a draw, and the draw is credited once.
    #[test]
    fn a_board_that_fills_without_a_five_is_a_draw() {
        let mut app = GomokuApp::new();
        for r in 0..BOARD_SIZE {
            for c in 0..BOARD_SIZE {
                if (r, c) == (0, 0) {
                    continue;
                }
                let stone = if (r / 2 + c / 3) % 2 == 0 {
                    Cell::Black
                } else {
                    Cell::White
                };
                place_raw(&mut app.board, r, c, stone);
            }
        }
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.current_turn = if app.board.get(0, 1) == Some(Cell::Black) {
            Cell::Black
        } else {
            Cell::White
        };
        assert!(app.try_place_stone());
        assert_eq!(app.phase, GamePhase::Draw, "a full board was not a draw");
        assert_eq!(app.scores.2, 1);
    }

    // =======================================================================
    // Scores
    // =======================================================================

    /// A win is credited to the colour that made it, whichever colour that is.
    ///
    /// Both halves matter: White's credit used to be written a second time in
    /// `ai_move`, hard-coded to `scores.1` rather than matched on the stone,
    /// so the two copies agreed only for as long as nobody changed one.
    #[test]
    fn a_win_is_credited_to_the_colour_that_won() {
        let mut checked = 0;
        for stone in [Cell::Black, Cell::White] {
            let mut app = GomokuApp::new();
            for c in 4..8 {
                place_raw(&mut app.board, 7, c, stone);
            }
            app.current_turn = stone;
            app.cursor_row = 7;
            app.cursor_col = 8;
            assert!(app.try_place_stone());
            assert_eq!(app.phase, GamePhase::Won);
            assert_eq!(app.winner, stone);
            let (b, w, d) = app.scores;
            match stone {
                Cell::Black => assert_eq!((b, w, d), (1, 0, 0)),
                Cell::White => assert_eq!((b, w, d), (0, 1, 0)),
                Cell::Empty => panic!("no such game"),
            }
            checked += 1;
        }
        assert_eq!(checked, 2, "only one colour's score was checked");
    }

    #[test]
    fn a_new_game_clears_the_board_and_keeps_the_scores() {
        let mut app = GomokuApp::new();
        app.scores = (3, 2, 1);
        app.cursor_row = 1;
        app.cursor_col = 1;
        assert!(app.try_place_stone());
        app.new_game();
        assert_eq!(app.board.stone_count(), 0);
        assert_eq!(app.move_count, 0);
        assert!(app.move_history.is_empty());
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert!(app.last_move.is_none());
        assert!(app.win_line.is_none());
        assert_eq!(app.scores, (3, 2, 1), "a new game reset the scoreboard");
    }

    // =======================================================================
    // Undo
    // =======================================================================

    /// Z gives the board back to the player: White's reply and Black's move,
    /// so the position that comes back is one Black can play from.
    #[test]
    fn undo_takes_back_the_pair_not_just_the_reply() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7)]);
        assert_eq!(app.move_count, 2);
        app.undo();
        assert_eq!(app.move_count, 0, "undo left White's reply on the board");
        assert_eq!(app.board.stone_count(), 0);
        assert_eq!(app.current_turn, Cell::Black);
        assert!(app.last_move.is_none());
    }

    #[test]
    fn undo_on_an_empty_board_does_nothing() {
        let mut app = GomokuApp::new();
        app.undo();
        assert_eq!(app.move_count, 0);
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.scores, (0, 0, 0));
    }

    /// Undoing a won game takes the credit back with the stone.
    ///
    /// Otherwise Z after a win is a way to add a point to the scoreboard for
    /// every time it is pressed.
    #[test]
    fn undoing_a_win_takes_back_the_point_it_scored() {
        let mut checked = 0;
        for stone in [Cell::Black, Cell::White] {
            let mut app = GomokuApp::new();
            for c in 4..8 {
                place_raw(&mut app.board, 7, c, stone);
            }
            app.current_turn = stone;
            app.cursor_row = 7;
            app.cursor_col = 8;
            assert!(app.try_place_stone());
            assert_ne!(app.scores, (0, 0, 0), "the win scored nothing to undo");
            app.undo();
            assert_eq!(app.scores, (0, 0, 0), "{stone:?}'s point survived the undo");
            assert_eq!(app.phase, GamePhase::Playing);
            assert_eq!(app.winner, Cell::Empty);
            assert!(app.win_line.is_none(), "the win line outlived the win");
            checked += 1;
        }
        assert_eq!(checked, 2);
    }

    /// Undoing a draw takes back the draw's point too.
    #[test]
    fn undoing_a_draw_takes_back_the_point_it_scored() {
        let mut app = GomokuApp::new();
        app.phase = GamePhase::Draw;
        app.winner = Cell::Empty;
        app.scores = (0, 0, 1);
        app.move_history.push(MoveRecord {
            row: 0,
            col: 0,
            stone: Cell::Black,
        });
        app.move_count = 1;
        place_raw(&mut app.board, 0, 0, Cell::Black);
        app.undo();
        assert_eq!(app.scores, (0, 0, 0), "the draw's point survived the undo");
    }

    /// Undo during White's search cancels it rather than scoring anything.
    #[test]
    fn undo_while_white_is_thinking_gives_the_move_back() {
        let mut app = GomokuApp::new();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&key_of(Key::Enter));
        assert_eq!(app.phase, GamePhase::Thinking);
        app.undo();
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(app.move_count, 0);
        assert_eq!(app.scores, (0, 0, 0), "an unfinished game scored a point");
    }

    /// Undoing a win Black finished gives back Black's stone and not White's
    /// too.
    ///
    /// `undo` calls `take_back(White)` then `take_back(Black)`, and each only
    /// lifts a stone of its own colour. Deleting that colour check survived
    /// the whole suite, because every undo fixture had a history that ended
    /// in White or held a single move -- and on those, "lift the top stone
    /// twice" and "lift White's then Black's" agree. They part only where a
    /// history of two or more ends in Black, which is what a win by Black is
    /// (lesson 90: the rule was never exercised in the regime it governs).
    #[test]
    fn undoing_blacks_win_does_not_also_take_back_whites_last_reply() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(0, 0)]);
        assert_eq!(app.move_count, 2, "the opening pair was not played");

        // A row of four for Black well clear of the opening pair, then the
        // fifth stone played for real so the win goes through `place_stone`.
        for c in 4..8 {
            place_raw(&mut app.board, 7, c, Cell::Black);
        }
        app.current_turn = Cell::Black;
        app.cursor_row = 7;
        app.cursor_col = 8;
        assert!(app.try_place_stone());
        assert_eq!(app.phase, GamePhase::Won);
        assert_eq!(app.move_count, 3);

        app.undo();
        assert_eq!(
            app.move_count, 2,
            "undoing Black's win took White's reply back with it"
        );
        assert_eq!(
            app.move_history.last().map(|m| m.stone),
            Some(Cell::White),
            "White's reply should still be on the board"
        );
    }

    #[test]
    fn undo_can_be_pressed_back_to_the_opening_board() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7), (5, 5), (9, 9)]);
        assert!(app.move_count >= 6, "the game did not get going");
        for _ in 0..4 {
            app.undo();
        }
        assert_eq!(app.move_count, 0);
        assert_eq!(app.board.stone_count(), 0);
        assert!(app.move_history.is_empty());
    }

    // =======================================================================
    // The keyboard
    // =======================================================================

    /// A key does its work once per press.
    ///
    /// It did it twice: no arm read `pressed`, so the release that follows
    /// every press ran the same arm again. On a 15x15 board that made every
    /// other row and column unreachable by arrow key, dealt two games per N
    /// and took back two pairs of stones per Z.
    #[test]
    fn a_key_acts_on_the_press_and_not_again_on_the_release() {
        let mut app = GomokuApp::new();
        let start = app.cursor_row;
        assert_eq!(
            app.handle_key(&probe::press(Key::Up)),
            EventResult::Consumed
        );
        assert_eq!(app.cursor_row, start - 1, "one press did not move one row");
        assert_eq!(
            app.handle_key(&probe::release(Key::Up)),
            EventResult::Ignored,
            "the release was acted on"
        );
        assert_eq!(app.cursor_row, start - 1, "the release moved the cursor");
    }

    /// Every intersection is reachable from the keyboard.
    ///
    /// The direct statement of the damage the double-fire did: stepping two
    /// at a time from the centre, the cursor can only ever land on rows and
    /// columns of the same parity as the one it started on.
    #[test]
    fn the_arrows_can_reach_every_intersection() {
        for (row, col) in [(0, 0), (0, LAST_INDEX), (LAST_INDEX, 0), (6, 7), (7, 6)] {
            let mut app = GomokuApp::new();
            for _ in 0..BOARD_SIZE {
                if app.cursor_row > row {
                    app.handle_key(&probe::press(Key::Up));
                }
                if app.cursor_row < row {
                    app.handle_key(&probe::press(Key::Down));
                }
                if app.cursor_col > col {
                    app.handle_key(&probe::press(Key::Left));
                }
                if app.cursor_col < col {
                    app.handle_key(&probe::press(Key::Right));
                }
            }
            assert_eq!(
                (app.cursor_row, app.cursor_col),
                (row, col),
                "the arrows cannot reach ({row}, {col})"
            );
        }
    }

    #[test]
    fn the_cursor_stops_at_the_edges() {
        let mut app = GomokuApp::new();
        for _ in 0..BOARD_SIZE * 2 {
            app.handle_key(&probe::press(Key::Up));
            app.handle_key(&probe::press(Key::Left));
        }
        assert_eq!((app.cursor_row, app.cursor_col), (0, 0));
        for _ in 0..BOARD_SIZE * 2 {
            app.handle_key(&probe::press(Key::Down));
            app.handle_key(&probe::press(Key::Right));
        }
        assert_eq!(
            (app.cursor_row, app.cursor_col),
            (LAST_INDEX, LAST_INDEX),
            "the cursor walked off the board"
        );
    }

    /// An arrow at the edge that cannot move asks for no repaint.
    #[test]
    fn a_key_that_changes_nothing_is_ignored() {
        let mut app = GomokuApp::new();
        app.cursor_row = 0;
        assert_eq!(
            app.handle_key(&probe::press(Key::Up)),
            EventResult::Ignored,
            "an arrow that could not move still asked for a repaint"
        );
        assert_eq!(
            app.handle_key(&probe::press(Key::Q)),
            EventResult::Ignored,
            "a key the game does not use asked for a repaint"
        );
    }

    #[test]
    fn space_places_a_stone_as_enter_does() {
        let mut app = GomokuApp::new();
        app.cursor_row = 3;
        app.cursor_col = 3;
        assert_eq!(
            app.handle_key(&probe::press(Key::Space)),
            EventResult::Consumed
        );
        assert_eq!(app.board.get(3, 3), Some(Cell::Black));
    }

    #[test]
    fn n_deals_one_new_game_and_z_takes_back_one_pair() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7), (6, 6)]);
        let before = app.move_count;
        assert!(before >= 4);
        app.handle_key(&probe::press(Key::Z));
        assert_eq!(app.move_count, before - 2, "Z took back the wrong number");
        app.scores = (1, 1, 0);
        app.handle_key(&probe::press(Key::N));
        assert_eq!(app.move_count, 0);
        assert_eq!(app.scores, (1, 1, 0));
    }

    // =======================================================================
    // The pointer
    // =======================================================================

    /// A click on an intersection puts a stone on that intersection, in a
    /// window of any size.
    ///
    /// This is the fault the whole rewrite turns on. `render` drew from
    /// `CELL_SIZE` and two offset constants and `handle_mouse` resolved the
    /// click through `intersection_near`, a free function of the same three:
    /// the arithmetic agreed with itself in every window, and with the picture
    /// in exactly one.
    #[test]
    fn a_click_lands_on_the_intersection_it_was_aimed_at() {
        for size in SIZES {
            for (row, col) in [(0, 0), (7, 7), (LAST_INDEX as usize, LAST_INDEX as usize)] {
                let mut app = GomokuApp::new();
                let outcome =
                    probe::click_sized(&mut app, Target::Point(row, col), MouseButton::Left, size);
                assert_eq!(outcome, EventResult::Consumed);
                assert_eq!(
                    app.board.get(row as i32, col as i32),
                    Some(Cell::Black),
                    "a click aimed at ({row}, {col}) in a {size:?} window put \
                     the stone somewhere else"
                );
                assert_eq!(app.last_move, Some((row, col)));
            }
        }
    }

    /// The hit box of an intersection is centred on the intersection it names.
    ///
    /// The click above could pass while every box was one place to the left,
    /// because it aims by name. This measures the box against the geometry the
    /// grid lines are drawn from.
    #[test]
    fn every_intersection_is_clickable_where_it_is_drawn() {
        for size in SIZES {
            let app = GomokuApp::new();
            let l = Layout::solve(size.0, size.1);
            let mut checked = 0;
            for row in 0..BOARD_SIZE {
                for col in 0..BOARD_SIZE {
                    let r = probe::rect_of_sized(&app, Target::Point(row, col), size)
                        .unwrap_or_else(|| panic!("({row}, {col}) is not clickable at {size:?}"));
                    let (x, y) = l.intersection(row as i32, col as i32);
                    let c = r.centre();
                    assert!(
                        (c.0 - x).abs() < 0.01 && (c.1 - y).abs() < 0.01,
                        "({row}, {col}) is drawn at ({x}, {y}) and clicked at {c:?} \
                         in a {size:?} window"
                    );
                    checked += 1;
                }
            }
            assert_eq!(checked, BOARD_SIZE * BOARD_SIZE);
        }
    }

    /// A click on an occupied point moves the cursor there and places nothing.
    #[test]
    fn a_click_on_a_stone_does_not_place_another() {
        let mut app = app_with(&[(4, 4, Cell::White)]);
        probe::click(&mut app, Target::Point(4, 4));
        assert_eq!(app.board.get(4, 4), Some(Cell::White));
        assert_eq!(app.move_count, 0);
        assert_eq!((app.cursor_row, app.cursor_col), (4, 4));
    }

    /// A click on the board while White is thinking is refused.
    #[test]
    fn a_click_during_whites_search_is_refused() {
        let mut app = GomokuApp::new();
        app.phase = GamePhase::Thinking;
        app.current_turn = Cell::White;
        let outcome = probe::click(&mut app, Target::Point(3, 3));
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.board.get(3, 3), Some(Cell::Empty));
    }

    /// A click on the wood between the lines does nothing.
    ///
    /// Dropping a stone half a cell from where the player aimed is worse than
    /// a click that does nothing, so the boxes deliberately do not tile.
    #[test]
    fn a_click_between_the_lines_places_nothing() {
        let mut app = GomokuApp::new();
        let l = Layout::solve(W.0, W.1);
        let (x0, y0) = l.intersection(7, 7);
        let (x, y) = (x0 + l.cell / 2.0, y0 + l.cell / 2.0);
        let outcome = app.click_at(x, y, MouseButton::Left, W);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.move_count, 0, "a click on the wood placed a stone");
    }

    /// New game and undo answer the pointer.
    ///
    /// `handle_mouse` returned early unless it was Black's turn in a live
    /// game, so after a win -- the one moment a player most wants to start
    /// again -- the button did nothing at all and the keyboard was the only
    /// way out.
    #[test]
    fn the_buttons_answer_the_pointer_after_the_game_is_over() {
        let mut app = GomokuApp::new();
        for c in 4..8 {
            place_raw(&mut app.board, 7, c, Cell::Black);
        }
        app.cursor_row = 7;
        app.cursor_col = 8;
        assert!(app.try_place_stone());
        assert_eq!(app.phase, GamePhase::Won);

        let outcome = probe::click(&mut app, Target::NewGame);
        assert_eq!(outcome, EventResult::Consumed);
        assert_eq!(app.phase, GamePhase::Playing, "New game did not start one");
        assert_eq!(app.board.stone_count(), 0);
        assert_eq!(
            app.scores.0, 1,
            "the button that starts a game cleared the score"
        );
    }

    #[test]
    fn the_undo_button_takes_back_a_move() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7)]);
        assert_eq!(app.move_count, 2);
        let outcome = probe::click(&mut app, Target::Undo);
        assert_eq!(outcome, EventResult::Consumed);
        assert_eq!(app.move_count, 0);
    }

    /// With nothing to undo the button is drawn but not clickable, so a click
    /// on it is not silently swallowed.
    #[test]
    fn undo_is_not_clickable_with_nothing_to_undo() {
        let app = GomokuApp::new();
        assert!(
            probe::rect_of(&app, Target::NewGame).is_some(),
            "New game is not clickable on a fresh board"
        );
        assert!(
            probe::rect_of(&app, Target::Undo).is_none(),
            "Undo is clickable with an empty move history"
        );

        let mut played = GomokuApp::new();
        play_moves(&mut played, &[(7, 7)]);
        assert!(
            probe::rect_of(&played, Target::Undo).is_some(),
            "Undo is still not clickable after a move has been made"
        );
    }

    /// Both buttons are drawn where they are clickable, and inside the panel.
    #[test]
    fn each_button_is_labelled_where_it_is_clickable() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7)]);
        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);
        let mut checked = 0;
        for (target, label) in [
            (Target::NewGame, "New game (N)"),
            (Target::Undo, "Undo (Z)"),
        ] {
            let r = probe::rect_of(&app, target).expect("a button");
            assert!(
                r.x >= l.panel.x - 0.01 && r.right() <= l.panel.right() + 0.01,
                "{label} at {r:?} leaves the panel {:?}",
                l.panel
            );
            assert!(
                text_inside(&frame, label, r),
                "{label} is clickable at {r:?} and its text is drawn elsewhere"
            );
            checked += 1;
        }
        assert_eq!(checked, 2);
    }

    /// A click on nothing in particular is ignored rather than treated as a
    /// move.
    #[test]
    fn a_click_on_the_background_does_nothing() {
        let mut app = GomokuApp::new();
        let (x, y) = probe::bare_point(&app, W).expect("a point that hits nothing");
        let outcome = app.click_at(x, y, MouseButton::Left, W);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.move_count, 0);
    }

    /// A click is read against the window the last frame was drawn at.
    ///
    /// The app is rendered small and then clicked at the coordinates that are
    /// right for the small window; if the click were read against the size the
    /// app was born with, it would land on a different intersection.
    #[test]
    fn a_click_is_read_against_the_window_that_was_drawn() {
        let small = (420.0, 380.0);
        let mut app = GomokuApp::new();
        app.render(small.0, small.1);
        let l = Layout::solve(small.0, small.1);
        let (x, y) = l.intersection(2, 11);
        app.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(
            app.board.get(2, 11),
            Some(Cell::Black),
            "the click was read against a window that is not the one on screen"
        );
    }

    /// Only the left button plays.
    #[test]
    fn the_right_button_does_not_place_a_stone() {
        let mut app = GomokuApp::new();
        let outcome = probe::click_with(&mut app, Target::Point(5, 5), MouseButton::Right);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.move_count, 0);
    }

    /// Nothing scrolls, and saying so is not the same as forgetting to handle
    /// it: a scroll over the board leaves the game exactly as it was.
    ///
    /// [`probe::scroll_at_point`] is deliberately not used here -- it panics
    /// on a `None`, because a program that has a wheel must not answer one by
    /// accident. This is the other case: a program with no wheel at all, whose
    /// `None` is the correct and only answer.
    #[test]
    fn a_scroll_changes_nothing() {
        let mut app = GomokuApp::new();
        let l = Layout::solve(W.0, W.1);
        let (x, y) = l.intersection(7, 7);
        assert!(
            app.scroll_at(x, y, -3.0, W).is_none(),
            "the wheel was answered by a game that has nothing to scroll"
        );
        assert_eq!(app.move_count, 0);
        assert_eq!((app.cursor_row, app.cursor_col), (7, 7));
    }

    // =======================================================================
    // The paint
    // =======================================================================

    /// True when some fill of `color` is centred on ({x}, {y}).
    fn fill_centred_on(frame: &Frame<Target>, color: Color, x: f32, y: f32) -> bool {
        fills_of(frame, color).iter().any(|r| {
            let c = r.centre();
            (c.0 - x).abs() < 0.01 && (c.1 - y).abs() < 0.01
        })
    }

    /// Each stone is painted in its own colour, on its own intersection.
    ///
    /// Centred on [`Layout::intersection`] rather than compared to
    /// [`Layout::stone_rect`], so that a stone drawn a whole cell away from
    /// the line it sits on cannot pass by agreeing with the same wrong sum
    /// twice (lesson 84).
    #[test]
    fn every_stone_is_painted_on_the_point_it_sits_on() {
        let black = [(0, 0), (7, 7), (LAST_INDEX as usize, 3)];
        let white = [(1, 2), (8, 8)];
        let mut stones: Vec<(usize, usize, Cell)> =
            black.iter().map(|&(r, c)| (r, c, Cell::Black)).collect();
        stones.extend(white.iter().map(|&(r, c)| (r, c, Cell::White)));
        let app = app_with(&stones);
        for size in SIZES {
            let l = Layout::solve(size.0, size.1);
            let frame = app.frame(size.0, size.1);
            for (points, color, name) in [
                (&black[..], BLACK_STONE, "black"),
                (&white[..], WHITE_STONE, "white"),
            ] {
                for &(row, col) in points {
                    let (x, y) = l.intersection(row as i32, col as i32);
                    assert!(
                        fill_centred_on(&frame, color, x, y),
                        "the {name} stone on ({row}, {col}) is not painted at \
                         ({x}, {y}) in a {size:?} window"
                    );
                }
                assert_eq!(
                    fills_of(&frame, color).len(),
                    points.len(),
                    "{} {name} stones are on the board and {} were painted at {size:?}",
                    points.len(),
                    fills_of(&frame, color).len()
                );
            }
        }
    }

    /// An empty intersection is a hit box and nothing else.
    ///
    /// The box is recorded on all 225 points, because an empty one is where
    /// the next stone goes; the paint must not follow it.
    #[test]
    fn an_empty_intersection_is_clickable_but_not_painted() {
        let app = GomokuApp::new();
        let frame = app.frame(W.0, W.1);
        assert!(
            probe::rect_of(&app, Target::Point(7, 7)).is_some(),
            "an empty point on an empty board is not clickable"
        );
        assert!(
            fills_of(&frame, BLACK_STONE).is_empty() && fills_of(&frame, WHITE_STONE).is_empty(),
            "the opening board is empty and stones were painted on it"
        );
    }

    /// A stone is drawn with an edge, so a black stone is not a hole in the
    /// wood and a white one is not a hole in the light.
    #[test]
    fn a_stone_is_drawn_with_an_edge() {
        let app = app_with(&[(3, 3, Cell::Black), (4, 4, Cell::White)]);
        let frame = app.frame(W.0, W.1);
        let l = Layout::solve(W.0, W.1);
        for (row, col, color) in [(3, 3, BLACK_STONE_BORDER), (4, 4, WHITE_STONE_BORDER)] {
            let want = l.stone_rect(row, col);
            assert!(
                strokes_of(&frame, color)
                    .iter()
                    .any(|r| (r.x - want.x).abs() < 0.01 && (r.y - want.y).abs() < 0.01),
                "the stone on ({row}, {col}) has no edge"
            );
        }
    }

    /// The last stone played is marked, and no other one is.
    ///
    /// Both halves matter: a marker on every stone would satisfy "the last
    /// move is marked" while telling the player nothing (lesson 90).
    #[test]
    fn only_the_last_stone_played_is_marked() {
        let mut app = GomokuApp::new();
        play_moves(&mut app, &[(7, 7), (3, 4)]);
        let last = app.last_move.expect("a last move");
        assert_eq!(app.board.stone_count(), 4, "two pairs were not played");

        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);
        let marks = fills_of(&frame, LAST_MOVE_MARKER);
        assert_eq!(
            marks.len(),
            1,
            "four stones are on the board and {} of them are marked as last",
            marks.len()
        );
        let (x, y) = l.intersection(last.0 as i32, last.1 as i32);
        let c = marks[0].centre();
        assert!(
            (c.0 - x).abs() < 0.01 && (c.1 - y).abs() < 0.01,
            "the last move is {last:?} and the marker is at {c:?}, not ({x}, {y})"
        );
    }

    /// An opening board has nothing to mark.
    #[test]
    fn an_empty_board_marks_no_last_move() {
        let app = GomokuApp::new();
        assert!(fills_of(&app.frame(W.0, W.1), LAST_MOVE_MARKER).is_empty());
    }

    /// The five stones that won are marked, and only those five.
    #[test]
    fn the_win_is_marked_on_the_five_stones_that_made_it() {
        let mut app = GomokuApp::new();
        for c in 4..8 {
            place_raw(&mut app.board, 7, c, Cell::Black);
        }
        let before = app.frame(W.0, W.1);
        assert!(
            fills_of(&before, WIN_HIGHLIGHT).is_empty(),
            "four in a row is not a win and was highlighted as one"
        );

        app.cursor_row = 7;
        app.cursor_col = 8;
        assert!(app.try_place_stone());
        assert_eq!(app.phase, GamePhase::Won);

        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);
        let marks = fills_of(&frame, WIN_HIGHLIGHT);
        assert_eq!(marks.len(), WIN_COUNT, "the win line is not five stones");
        for col in 4..9 {
            let (x, y) = l.intersection(7, col);
            assert!(
                fill_centred_on(&frame, WIN_HIGHLIGHT, x, y),
                "(7, {col}) won the game and is not marked"
            );
        }
    }

    /// The cursor is drawn where the cursor is, and follows the arrows.
    #[test]
    fn the_cursor_is_drawn_where_the_arrows_left_it() {
        let mut app = GomokuApp::new();
        let l = Layout::solve(W.0, W.1);
        for _ in 0..3 {
            app.handle_key(&key_of(Key::Right));
        }
        app.handle_key(&key_of(Key::Up));
        assert_eq!((app.cursor_row, app.cursor_col), (6, 10));

        let frame = app.frame(W.0, W.1);
        let want = l.stone_rect(6, 10);
        assert!(
            strokes_of(&frame, CURSOR_COLOR)
                .iter()
                .any(|r| (r.x - want.x).abs() < 0.01 && (r.y - want.y).abs() < 0.01),
            "the cursor is on (6, 10) and is drawn somewhere else"
        );
    }

    /// The cursor is drawn only while there is a move to make with it.
    ///
    /// A cursor on a finished game, or one shown while White is searching,
    /// invites a key that will be refused.
    #[test]
    fn the_cursor_is_hidden_when_there_is_no_move_to_make() {
        let mut app = GomokuApp::new();
        let mut checked = 0;
        for (phase, turn, want) in [
            (GamePhase::Playing, Cell::Black, true),
            (GamePhase::Playing, Cell::White, false),
            (GamePhase::Thinking, Cell::White, false),
            (GamePhase::Won, Cell::Black, false),
            (GamePhase::Draw, Cell::Black, false),
        ] {
            app.phase = phase;
            app.current_turn = turn;
            let drawn = !strokes_of(&app.frame(W.0, W.1), CURSOR_COLOR).is_empty();
            assert_eq!(
                drawn,
                want,
                "the cursor is {} in {phase:?} with {turn:?} to play",
                if drawn { "drawn" } else { "hidden" }
            );
            checked += 1;
        }
        assert_eq!(checked, 5);
    }

    /// The status band says what the game is doing, in every phase.
    ///
    /// `GamePhase::Thinking` is the one that could not be drawn before the
    /// rewrite: the search ran inside the placement, so the frame that would
    /// have said so was never painted.
    #[test]
    fn the_status_band_says_what_the_game_is_doing() {
        let mut app = GomokuApp::new();
        let band = Layout::solve(W.0, W.1).status;
        let mut checked = 0;
        for (phase, winner, needle) in [
            (GamePhase::Playing, Cell::Empty, "Arrows move"),
            (GamePhase::Thinking, Cell::Empty, "White is thinking"),
            (GamePhase::Won, Cell::Black, "Black wins!"),
            (GamePhase::Won, Cell::White, "White wins."),
            (GamePhase::Draw, Cell::Empty, "A draw"),
        ] {
            app.phase = phase;
            app.winner = winner;
            let frame = app.frame(W.0, W.1);
            assert!(
                says_in(&frame, needle, band),
                "{phase:?} with {winner:?} winning does not say {needle:?}; it says {:?}",
                texts(&frame)
            );
            checked += 1;
        }
        assert_eq!(checked, 5);
    }

    /// A real game reaches a frame that says White is thinking.
    ///
    /// The test above sets the phase by hand, which cannot tell a message
    /// that is reachable from one that is dead (lesson 90). This plays a move
    /// and looks at the frame that follows it, before any tick.
    #[test]
    fn the_frame_after_blacks_move_says_white_is_thinking() {
        let mut app = GomokuApp::new();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&key_of(Key::Enter));
        assert_eq!(app.phase, GamePhase::Thinking);
        let l = Layout::solve(W.0, W.1);
        let frame = app.frame(W.0, W.1);
        assert!(
            says_in(&frame, "White is thinking", l.status),
            "White is searching and the status band does not say so"
        );
        assert!(
            says_in(&frame, "White is thinking", l.header),
            "White is searching and the header still names a turn to play"
        );
    }

    /// The header names the game and whose turn it is.
    #[test]
    fn the_header_names_the_turn() {
        let mut app = GomokuApp::new();
        let head = Layout::solve(W.0, W.1).header;
        let frame = app.frame(W.0, W.1);
        assert!(
            says_in(&frame, "Gomoku", head),
            "the window does not name the game"
        );
        assert!(says_in(&frame, "Black to play", head));

        app.current_turn = Cell::White;
        assert!(says_in(&app.frame(W.0, W.1), "White to play", head));
    }

    /// The panel counts the moves and keeps the score.
    #[test]
    fn the_panel_counts_the_moves_and_the_scores() {
        let mut app = GomokuApp::new();
        assert!(says(&app.frame(W.0, W.1), "Moves: 0"));

        play_moves(&mut app, &[(7, 7)]);
        assert!(
            says(&app.frame(W.0, W.1), "Moves: 2"),
            "a pair was played and the panel did not count it"
        );

        app.scores = (3, 2, 1);
        let frame = app.frame(W.0, W.1);
        assert!(says(&frame, "Black: 3"), "{:?}", texts(&frame));
        assert!(says(&frame, "White: 2"));
        assert!(says(&frame, "Draws: 1"));
    }

    // =======================================================================
    // The window the OS opens
    // =======================================================================

    /// The app names itself for the title bar and the taskbar.
    #[test]
    fn the_window_names_itself() {
        let app = GomokuApp::new();
        assert_eq!(app.title(), "Gomoku");
        assert_eq!(app.app_id(), "gomoku");
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
    }

    /// The app asks to be ticked, which is the only thing that makes White
    /// move.
    ///
    /// `tick_interval` returning `None` would leave the game in
    /// [`GamePhase::Thinking`] for ever after Black's first stone, with the
    /// board frozen and the status band honestly reporting a search that
    /// nothing would ever run.
    #[test]
    fn the_app_asks_for_the_tick_that_makes_white_move() {
        let app = GomokuApp::new();
        let interval = app.tick_interval().expect("a tick interval");
        assert!(
            interval <= Duration::from_millis(250) && interval > Duration::ZERO,
            "White thinks on a {interval:?} clock, which is not a pace a \
             player would sit through"
        );
    }

    /// Rendering remembers the size, so the next click is read against the
    /// window that is actually on screen.
    #[test]
    fn rendering_remembers_the_size_the_click_will_be_read_against() {
        let mut app = GomokuApp::new();
        let _ = app.render(500.0, 700.0);
        assert_eq!(
            (app.width, app.height),
            (500.0, 700.0),
            "the app was drawn at 500x700 and remembers a different window"
        );
    }

    /// A window of no size at all is laid out for something rather than
    /// dividing by it.
    #[test]
    fn a_window_with_no_size_does_not_divide_by_zero() {
        let mut app = GomokuApp::new();
        let _ = app.render(0.0, 0.0);
        assert!(app.width > 0.0 && app.height > 0.0);
        let l = Layout::solve(app.width, app.height);
        assert!(
            l.cell.is_finite(),
            "a zero-sized window gave a {} px cell",
            l.cell
        );
    }

    /// A close request is answered by closing, and every other event by
    /// whether it changed the picture.
    #[test]
    fn the_window_is_told_when_to_repaint_and_when_to_close() {
        let mut app = GomokuApp::new();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
        assert_eq!(
            app.on_event(&Event::Key(key_of(Key::Right))),
            Response::Redraw,
            "the cursor moved and the window was not asked to repaint"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::release(Key::Right))),
            Response::Idle,
            "a key release changed nothing and asked for a repaint"
        );
    }

    /// The tree handed to the compositor is the frame that was measured.
    ///
    /// `render` returns a [`RenderTree`], and a frame converted to one must
    /// keep its paint: a window that hit-tests against commands it never
    /// handed over would be back where the rewrite started.
    #[test]
    fn the_tree_the_window_gets_is_the_frame_that_was_drawn() {
        let mut app = app_with(&[(7, 7, Cell::Black)]);
        let commands = app.frame(W.0, W.1).commands().len();
        let tree = app.render(W.0, W.1);
        assert_eq!(
            tree.commands.len(),
            commands,
            "the frame has {commands} commands and the window was handed {}",
            tree.commands.len()
        );
    }

    // =======================================================================
    // The board and the rules
    // =======================================================================

    #[test]
    fn a_new_board_is_empty() {
        let board = Board::new();
        assert_eq!(board.stone_count(), 0);
        assert!(!board.is_full());
        for r in 0..BOARD_SIZE {
            for c in 0..BOARD_SIZE {
                assert!(board.is_empty(r, c), "({r}, {c}) is not empty");
            }
        }
    }

    /// `get` answers `None` off the board rather than panicking or wrapping.
    ///
    /// The search walks lines until they leave the board, so "off the board"
    /// is an answer it needs and not an error it avoids.
    #[test]
    fn reading_off_the_board_says_so() {
        let board = Board::new();
        assert_eq!(board.get(0, 0), Some(Cell::Empty));
        assert_eq!(board.get(LAST_INDEX, LAST_INDEX), Some(Cell::Empty));
        for (r, c) in [
            (-1, 0),
            (0, -1),
            (BOARD_SIZE as i32, 0),
            (0, BOARD_SIZE as i32),
            (i32::MIN, i32::MIN),
            (i32::MAX, i32::MAX),
        ] {
            assert_eq!(board.get(r, c), None, "({r}, {c}) answered as on-board");
        }
    }

    #[test]
    fn writing_off_the_board_is_refused_rather_than_done_somewhere_else() {
        let mut board = Board::new();
        assert!(board.set(0, 0, Cell::Black));
        assert!(!board.set(BOARD_SIZE, 0, Cell::White));
        assert!(!board.set(0, BOARD_SIZE, Cell::White));
        assert_eq!(board.stone_count(), 1, "a refused write still landed");
    }

    #[test]
    fn an_intersection_off_the_board_is_not_empty() {
        let board = Board::new();
        assert!(board.is_empty(0, 0));
        assert!(
            !board.is_empty(BOARD_SIZE, 0),
            "a point that is not on the board was offered as a place to play"
        );
    }

    #[test]
    fn a_full_board_is_a_draw_and_a_nearly_full_one_is_not() {
        let mut board = Board::new();
        for r in 0..BOARD_SIZE {
            for c in 0..BOARD_SIZE {
                // Alternate in a pattern with no five in a row, so that
                // "full" is the only thing being tested here.
                let stone = if (r / 2 + c / 3) % 2 == 0 {
                    Cell::Black
                } else {
                    Cell::White
                };
                place_raw(&mut board, r, c, stone);
            }
        }
        assert!(board.is_full());
        board.set(7, 7, Cell::Empty);
        assert!(!board.is_full(), "224 of 225 stones counted as full");
        assert_eq!(board.stone_count(), BOARD_SIZE * BOARD_SIZE - 1);
    }

    #[test]
    fn a_colour_has_an_opponent_and_an_empty_point_does_not() {
        assert_eq!(Cell::Black.opponent(), Cell::White);
        assert_eq!(Cell::White.opponent(), Cell::Black);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
    }

    /// Five in a row wins in every direction, and four does not.
    ///
    /// One test over all four directions and both colours, counting the cases
    /// it checked, rather than eight tests each of which could quietly stop
    /// running.
    #[test]
    fn five_in_a_row_wins_in_every_direction() {
        let mut checked = 0;
        for &(dr, dc) in &DIRECTIONS {
            for stone in [Cell::Black, Cell::White] {
                // A start far enough from every edge that five fit in either
                // sense of the direction.
                let (r0, c0) = (5i32, 5i32);
                let mut four = Board::new();
                for i in 0..4 {
                    place_raw(
                        &mut four,
                        (r0 + dr * i) as usize,
                        (c0 + dc * i) as usize,
                        stone,
                    );
                }
                assert!(
                    four.check_winner(stone).is_none(),
                    "four in a row at {dr},{dc} for {stone:?} was called a win"
                );

                let mut five = four.clone();
                place_raw(
                    &mut five,
                    (r0 + dr * 4) as usize,
                    (c0 + dc * 4) as usize,
                    stone,
                );
                let win = five
                    .check_winner(stone)
                    .unwrap_or_else(|| panic!("five in a row at {dr},{dc} did not win"));
                assert_eq!(win.positions.len(), WIN_COUNT);
                assert!(
                    five.check_winner(stone.opponent()).is_none(),
                    "{stone:?}'s five won the game for the other colour"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 8, "not every direction and colour was checked");
    }

    /// A run of five that leaves the board does not win.
    ///
    /// The line walker stops at the edge rather than wrapping onto the far
    /// side, which is the fault a flat array indexed by `row * 15 + col`
    /// invites and which this board's `get` is bounds-checked to avoid.
    #[test]
    fn a_run_does_not_wrap_around_the_edge() {
        let mut board = Board::new();
        // The last three of row 3 and the first two of row 4: contiguous in a
        // flat array, five apart in nothing on a real board.
        for c in 12..BOARD_SIZE {
            place_raw(&mut board, 3, c, Cell::Black);
        }
        for c in 0..2 {
            place_raw(&mut board, 4, c, Cell::Black);
        }
        assert!(
            board.check_winner(Cell::Black).is_none(),
            "a run that crossed the right edge onto the next row was a win"
        );
    }

    #[test]
    fn a_win_names_the_five_stones_that_made_it() {
        let mut board = Board::new();
        for c in 3..8 {
            place_raw(&mut board, 6, c, Cell::White);
        }
        let win = board.check_winner(Cell::White).expect("five in a row");
        assert_eq!(
            win.positions,
            vec![(6, 3), (6, 4), (6, 5), (6, 6), (6, 7)],
            "the win line does not name the stones that won"
        );
    }

    #[test]
    fn a_mixed_line_of_five_wins_for_nobody() {
        let mut board = Board::new();
        for c in 3..7 {
            place_raw(&mut board, 6, c, Cell::Black);
        }
        place_raw(&mut board, 6, 7, Cell::White);
        assert!(board.check_winner(Cell::Black).is_none());
        assert!(board.check_winner(Cell::White).is_none());
    }
}
