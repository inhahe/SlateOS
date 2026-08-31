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
        let panel_w_min =
            text::measure("Draws: 88", font, FontWeightHint::Regular).max(font * 6.0) + pad * 2.0;
        let want_panel = (w * 0.26).clamp(panel_w_min, 220.0);
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

    /// The layout of the window as it stands.
    fn layout(&self) -> Layout {
        Layout::solve(self.width, self.height)
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
    fn think(&mut self) {
        if self.phase != GamePhase::Thinking {
            return;
        }
        let Some((r, c)) = find_best_move(&self.board, Cell::White) else {
            // No legal reply and the board is not full: nothing to do but
            // hand the turn back, rather than sit in Thinking forever.
            self.current_turn = Cell::Black;
            self.phase = GamePhase::Playing;
            return;
        };
        self.cursor_row = r as i32;
        self.cursor_col = c as i32;
        self.place_stone(r, c);
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
            Event::Tick { .. } if self.phase == GamePhase::Thinking => {
                self.think();
                EventResult::Consumed
            }
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
            "Gomoku",
            x,
            l.header.y + (l.header.h - l.title) / 2.0,
            l.title,
            FontWeightHint::Bold,
            TEXT_COLOR,
            Some(l.header.w - x - l.pad),
        );

        // The turn indicator is placed past the measured width of the title
        // rather than at a hand-tuned offset from it.
        let used = text::measure("Gomoku", l.title, FontWeightHint::Bold);
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

        heading(f, "Game Info", &mut y);
        for line in [
            format!("Moves: {}", self.move_count),
            format!(
                "Turn: {}",
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
        heading(f, "Scores", &mut y);
        for (line, color) in [
            (format!("\u{25CF} Black: {}", self.scores.0), TEXT_COLOR),
            (format!("\u{25CB} White: {}", self.scores.1), TEXT_COLOR),
            (format!("Draws: {}", self.scores.2), SUBTEXT0),
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
            ("New game (N)", Target::NewGame, true),
            ("Undo (Z)", Target::Undo, !self.move_history.is_empty()),
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
        String::from("Gomoku")
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
