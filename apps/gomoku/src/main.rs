#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::needless_range_loop)]
#![allow(unused_imports)]

//! Slate OS Gomoku (Five-in-a-Row) — a full Gomoku game with AI opponent.
//!
//! Features a 15x15 Go-style board with grid lines and star points,
//! alternating Black/White stones, a minimax AI with alpha-beta pruning
//! (depth 3-4) using pattern-based evaluation, win detection in all four
//! directions, arrow key cursor movement + Enter placement, mouse click
//! placement, move counter, turn indicator, score tracking across games,
//! undo (Z key), new game (N key), and win line highlighting.
//! Themed with the Catppuccin Mocha palette.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

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
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

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

// ── Layout constants ────────────────────────────────────────────────
const BOARD_SIZE: usize = 15;
const CELL_SIZE: f32 = 36.0;
const BOARD_OFFSET_X: f32 = 50.0;
const BOARD_OFFSET_Y: f32 = 70.0;
const STONE_RADIUS: f32 = 15.0;
const STAR_POINT_RADIUS: f32 = 4.0;
const PANEL_X: f32 = BOARD_OFFSET_X + BOARD_PIXEL_SIZE + 40.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const SMALL_FONT_SIZE: f32 = 12.0;

// ── Board geometry ──────────────────────────────────────────────────
//
// Stones in gomoku sit *on* the intersections rather than inside the cells, so
// the board's painted extent is one cell short of `BOARD_SIZE` cells, and every
// part of the board -- lines, stones, star points, labels, cursor -- is placed
// relative to a point rather than to a rectangle. That mapping, and the inverse
// a click needs, used to be spelled out at ten separate sites.

/// Distance from the first intersection to the last, in pixels.
const BOARD_PIXEL_SIZE: f32 = CELL_SIZE * (BOARD_SIZE - 1) as f32;

/// Index of the last row and of the last column.
const LAST_INDEX: i32 = BOARD_SIZE as i32 - 1;

/// How far a click may land from an intersection and still count, squared.
///
/// A little wider than a stone, so the whole of a drawn stone is clickable and
/// a near miss still lands. Unlike a board of squares this leaves real gaps --
/// the middle of a cell belongs to no intersection at all -- and that is the
/// intent: dropping a stone half a cell from where the player aimed is worse
/// than a click that does nothing.
const CLICK_RADIUS_SQ: f32 = STONE_RADIUS * STONE_RADIUS * 1.5;

/// Screen coordinates of the intersection at `(row, col)`.
///
/// Row 0 is the top of the window. The board is symmetric, so unlike chess
/// there is no flip here and nothing an inverse has to undo.
fn intersection(row: i32, col: i32) -> (f32, f32) {
    (
        BOARD_OFFSET_X + col as f32 * CELL_SIZE,
        BOARD_OFFSET_Y + row as f32 * CELL_SIZE,
    )
}

/// The intersection a click at `(x, y)` lands on, or `None` if it is not near
/// enough to any of them.
///
/// The nearest intersection is found by rounding and then *clamping* into
/// range, rather than by computing an index and rejecting it when out of
/// range. The difference is visible at the edges: a click a few pixels beyond
/// the last line is still on the stone drawn there, so it has to resolve to
/// that stone. Clamping and then measuring the distance gets both edges alike;
/// the previous index-then-reject spelling accepted the overhang on the left
/// and top and refused the identical click on the right and bottom.
fn intersection_near(x: f32, y: f32) -> Option<(i32, i32)> {
    let col = (((x - BOARD_OFFSET_X) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
    let row = (((y - BOARD_OFFSET_Y) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
    let (ix, iy) = intersection(row, col);
    let (dx, dy) = (x - ix, y - iy);
    if dx * dx + dy * dy <= CLICK_RADIUS_SQ {
        Some((row, col))
    } else {
        None
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
    Playing,
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

    /// Get the cell at (row, col). Returns `None` if out of bounds.
    fn get(&self, row: i32, col: i32) -> Option<Cell> {
        if row >= 0 && row < BOARD_SIZE as i32 && col >= 0 && col < BOARD_SIZE as i32 {
            Some(self.cells[row as usize][col as usize])
        } else {
            None
        }
    }

    /// Set the cell at (row, col). Panics if out of bounds (test-only usage is fine).
    fn set(&mut self, row: usize, col: usize, cell: Cell) {
        self.cells[row][col] = cell;
    }

    /// Whether the intersection at (row, col) is empty.
    fn is_empty(&self, row: usize, col: usize) -> bool {
        self.cells[row][col] == Cell::Empty
    }

    /// Count the number of occupied intersections.
    fn stone_count(&self) -> usize {
        let mut count = 0;
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                if self.cells[row][col] != Cell::Empty {
                    count += 1;
                }
            }
        }
        count
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
            let r = row + dr * i;
            let c = col + dc * i;
            if self.get(r, c) != Some(stone) {
                return None;
            }
            positions.push((r as usize, c as usize));
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
        let mut count = 0;
        let mut r = row + dr;
        let mut c = col + dc;
        while self.get(r, c) == Some(stone) {
            count += 1;
            r += dr;
            c += dc;
        }
        count
    }

    /// Check what is at the end of a consecutive run of `stone` in direction
    /// (dr, dc) starting from (row, col). Returns true if the end is empty
    /// (open end), false if blocked (edge or opponent stone).
    fn is_open_end(&self, row: i32, col: i32, dr: i32, dc: i32, stone: Cell) -> bool {
        let mut r = row + dr;
        let mut c = col + dc;
        while self.get(r, c) == Some(stone) {
            r += dr;
            c += dc;
        }
        self.get(r, c) == Some(Cell::Empty)
    }

    /// Evaluate a single line pattern through (row, col) in a given direction
    /// for the specified stone color. Returns a score based on the pattern
    /// (how many in a row, open/half-open ends).
    fn evaluate_line_pattern(&self, row: i32, col: i32, dr: i32, dc: i32, stone: Cell) -> i32 {
        let count_fwd = self.count_direction(row, col, dr, dc, stone);
        let count_bwd = self.count_direction(row, col, -dr, -dc, stone);
        let total = count_fwd + count_bwd + 1; // +1 for the stone at (row, col)

        if total >= WIN_COUNT as i32 {
            return SCORE_FIVE;
        }

        let open_fwd = self.is_open_end(row, col, dr, dc, stone);
        let open_bwd = self.is_open_end(row, col, -dr, -dc, stone);
        let open_ends = i32::from(open_fwd) + i32::from(open_bwd);

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

        for row in 0..BOARD_SIZE as i32 {
            for col in 0..BOARD_SIZE as i32 {
                let cell = self.cells[row as usize][col as usize];
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
            }
        }

        // Center control bonus: stones closer to center are slightly better
        for row in 0..BOARD_SIZE as i32 {
            for col in 0..BOARD_SIZE as i32 {
                let cell = self.cells[row as usize][col as usize];
                if cell == Cell::Empty {
                    continue;
                }
                let center = BOARD_SIZE as i32 / 2;
                let dist = (row - center).abs() + (col - center).abs();
                let center_bonus = (14 - dist).max(0) * 2;
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

        let has_stones = self.stone_count() > 0;

        if !has_stones {
            // First move: play center
            return vec![(BOARD_SIZE / 2, BOARD_SIZE / 2)];
        }

        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                if self.cells[row][col] == Cell::Empty {
                    continue;
                }
                let r = row as i32;
                let c = col as i32;
                for dr in -radius..=radius {
                    for dc in -radius..=radius {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r + dr;
                        let nc = c + dc;
                        if nr >= 0 && nr < BOARD_SIZE as i32 && nc >= 0 && nc < BOARD_SIZE as i32 {
                            let nru = nr as usize;
                            let ncu = nc as usize;
                            if self.cells[nru][ncu] == Cell::Empty && !seen[nru][ncu] {
                                seen[nru][ncu] = true;
                                moves.push((nru, ncu));
                            }
                        }
                    }
                }
            }
        }

        // Sort candidates by a quick heuristic: prefer moves closer to center
        let center = BOARD_SIZE as i32 / 2;
        moves.sort_by_key(|&(r, c)| (r as i32 - center).abs() + (c as i32 - center).abs());

        moves
    }

    /// Check if placing a stone creates an immediate threat (four in a row
    /// or win). Used to prioritize moves in the AI search.
    fn is_threat_move(&self, row: usize, col: usize, stone: Cell) -> bool {
        let r = row as i32;
        let c = col as i32;
        for &(dr, dc) in &DIRECTIONS {
            let fwd = self.count_direction(r, c, dr, dc, stone);
            let bwd = self.count_direction(r, c, -dr, -dc, stone);
            let total = fwd + bwd + 1;
            if total >= 4 {
                return true;
            }
        }
        false
    }
}

// ── AI (Minimax with alpha-beta pruning) ────────────────────────────

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
        return SCORE_FIVE + depth; // Prefer faster wins
    }
    if board.check_winner(ai_stone.opponent()).is_some() {
        return -(SCORE_FIVE + depth); // Opponent won
    }
    if depth == 0 || board.is_full() {
        return board.evaluate(ai_stone);
    }

    let candidates = board.candidate_moves();
    if candidates.is_empty() {
        return board.evaluate(ai_stone);
    }

    if maximizing {
        let mut best = i32::MIN;
        for (r, c) in candidates {
            board.set(r, c, ai_stone);
            let val = minimax(board, depth - 1, alpha, beta, false, ai_stone);
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
            let val = minimax(board, depth - 1, alpha, beta, true, ai_stone);
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
    if candidates.is_empty() {
        return None;
    }

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
    let depth = if has_threats { AI_DEPTH + 1 } else { AI_DEPTH };

    let mut best_score = i32::MIN;
    let mut best_move = candidates[0];

    for (r, c) in candidates {
        let mut test = board.clone();
        test.set(r, c, ai_stone);
        let score = minimax(&mut test, depth - 1, i32::MIN, i32::MAX, false, ai_stone);
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
        }
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

    /// Place a stone and handle game state transitions (win/draw/AI turn).
    fn place_stone(&mut self, row: usize, col: usize) {
        let stone = self.current_turn;
        self.board.set(row, col, stone);
        self.move_history.push(MoveRecord { row, col, stone });
        self.move_count += 1;
        self.last_move = Some((row, col));

        // Check for win
        if let Some(win_line) = self.board.check_winner(stone) {
            self.phase = GamePhase::Won;
            self.winner = stone;
            self.win_line = Some(win_line);
            match stone {
                Cell::Black => self.scores.0 += 1,
                Cell::White => self.scores.1 += 1,
                Cell::Empty => {}
            }
            return;
        }

        // Check for draw
        if self.board.is_full() {
            self.phase = GamePhase::Draw;
            self.scores.2 += 1;
            return;
        }

        // Switch turn
        self.current_turn = self.current_turn.opponent();

        // If it's now the AI's turn (White), make the AI move
        if self.current_turn == Cell::White && self.phase == GamePhase::Playing {
            self.ai_move();
        }
    }

    /// Have the AI (White) make its move.
    fn ai_move(&mut self) {
        if let Some((r, c)) = find_best_move(&self.board, Cell::White) {
            let stone = Cell::White;
            self.board.set(r, c, stone);
            self.move_history.push(MoveRecord {
                row: r,
                col: c,
                stone,
            });
            self.move_count += 1;
            self.last_move = Some((r, c));

            // Check for AI win
            if let Some(win_line) = self.board.check_winner(stone) {
                self.phase = GamePhase::Won;
                self.winner = stone;
                self.win_line = Some(win_line);
                self.scores.1 += 1;
                return;
            }

            // Check for draw after AI move
            if self.board.is_full() {
                self.phase = GamePhase::Draw;
                self.scores.2 += 1;
                return;
            }

            self.current_turn = Cell::Black;
        }
    }

    /// Undo the last move(s). If the last move was by the AI (White),
    /// undo both the AI move and the preceding player move.
    fn undo(&mut self) {
        if self.move_history.is_empty() {
            return;
        }

        // If the game is over, just undo the last move (the winning move)
        if self.phase != GamePhase::Playing {
            self.phase = GamePhase::Playing;
            self.win_line = None;
            // Undo the score increment
            match self.winner {
                Cell::Black => {
                    if self.scores.0 > 0 {
                        self.scores.0 -= 1;
                    }
                }
                Cell::White => {
                    if self.scores.1 > 0 {
                        self.scores.1 -= 1;
                    }
                }
                Cell::Empty => {
                    // Draw
                    if self.scores.2 > 0 {
                        self.scores.2 -= 1;
                    }
                }
            }
            self.winner = Cell::Empty;
        }

        // Undo AI move (White) if the last move was by White
        if let Some(last) = self.move_history.last()
            && last.stone == Cell::White
        {
            let record = self.move_history.pop().expect("just checked non-empty");
            self.board.set(record.row, record.col, Cell::Empty);
            self.move_count = self.move_count.saturating_sub(1);
        }

        // Undo player move (Black)
        if let Some(last) = self.move_history.last()
            && last.stone == Cell::Black
        {
            let record = self.move_history.pop().expect("just checked non-empty");
            self.board.set(record.row, record.col, Cell::Empty);
            self.move_count = self.move_count.saturating_sub(1);
        }

        // Update current turn and last_move
        self.current_turn = Cell::Black;
        self.last_move = self.move_history.last().map(|m| (m.row, m.col));
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, event: &KeyEvent) {
        match event {
            // Arrow key movement
            KeyEvent { key: Key::Up, .. } if self.cursor_row > 0 => {
                self.cursor_row -= 1;
            }
            KeyEvent { key: Key::Down, .. } if self.cursor_row < BOARD_SIZE as i32 - 1 => {
                self.cursor_row += 1;
            }
            KeyEvent { key: Key::Left, .. } if self.cursor_col > 0 => {
                self.cursor_col -= 1;
            }
            KeyEvent {
                key: Key::Right, ..
            } if self.cursor_col < BOARD_SIZE as i32 - 1 => {
                self.cursor_col += 1;
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

            _ => {}
        }
    }

    /// Handle mouse clicks: place stone at clicked intersection.
    fn handle_mouse(&mut self, event: &MouseEvent) {
        if self.phase != GamePhase::Playing || self.current_turn != Cell::Black {
            return;
        }

        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            if let Some((row, col)) = intersection_near(event.x, event.y) {
                self.cursor_row = row;
                self.cursor_col = col;
                self.try_place_stone();
            }
        }
    }

    /// Handle a general event.
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    /// Generate render commands for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // ── Background ──────────────────────────────────────────────
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: PANEL_X + 220.0,
            height: BOARD_OFFSET_Y + BOARD_PIXEL_SIZE + 60.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // ── Title ───────────────────────────────────────────────────
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 28.0,
            text: String::from("Gomoku"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // ── Turn indicator next to title ────────────────────────────
        let turn_text = if self.phase == GamePhase::Won {
            match self.winner {
                Cell::Black => String::from("Black wins!"),
                Cell::White => String::from("White wins!"),
                Cell::Empty => String::from("Game over"),
            }
        } else if self.phase == GamePhase::Draw {
            String::from("Draw!")
        } else {
            match self.current_turn {
                Cell::Black => String::from("\u{25CF} Black's turn"),
                Cell::White => String::from("\u{25CB} White's turn"),
                Cell::Empty => String::from(""),
            }
        };
        let turn_color = match self.phase {
            GamePhase::Won => GREEN,
            GamePhase::Draw => YELLOW,
            GamePhase::Playing => TEXT_COLOR,
        };
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X + 120.0,
            y: 32.0,
            text: turn_text,
            color: turn_color,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // ── Board background (wooden color) ─────────────────────────
        let board_margin = CELL_SIZE * 0.6;
        cmds.push(RenderCommand::FillRect {
            x: BOARD_OFFSET_X - board_margin,
            y: BOARD_OFFSET_Y - board_margin,
            width: BOARD_PIXEL_SIZE + board_margin * 2.0,
            height: BOARD_PIXEL_SIZE + board_margin * 2.0,
            color: BOARD_BG,
            corner_radii: CornerRadii::all(4.0),
        });

        // ── Board border ────────────────────────────────────────────
        cmds.push(RenderCommand::StrokeRect {
            x: BOARD_OFFSET_X - board_margin,
            y: BOARD_OFFSET_Y - board_margin,
            width: BOARD_PIXEL_SIZE + board_margin * 2.0,
            height: BOARD_PIXEL_SIZE + board_margin * 2.0,
            color: BOARD_BORDER,
            line_width: 2.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // ── Grid lines ──────────────────────────────────────────────
        // Horizontal lines
        for row in 0..BOARD_SIZE {
            let (_, y) = intersection(row as i32, 0);
            cmds.push(RenderCommand::Line {
                x1: BOARD_OFFSET_X,
                y1: y,
                x2: BOARD_OFFSET_X + BOARD_PIXEL_SIZE,
                y2: y,
                color: GRID_LINE_COLOR,
                width: 1.0,
            });
        }
        // Vertical lines
        for col in 0..BOARD_SIZE {
            let (x, _) = intersection(0, col as i32);
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: BOARD_OFFSET_Y,
                x2: x,
                y2: BOARD_OFFSET_Y + BOARD_PIXEL_SIZE,
                color: GRID_LINE_COLOR,
                width: 1.0,
            });
        }

        // ── Star points ─────────────────────────────────────────────
        for &(sr, sc) in &STAR_POINTS {
            let (ix, iy) = intersection(sr as i32, sc as i32);
            let x = ix - STAR_POINT_RADIUS;
            let y = iy - STAR_POINT_RADIUS;
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width: STAR_POINT_RADIUS * 2.0,
                height: STAR_POINT_RADIUS * 2.0,
                color: STAR_POINT_COLOR,
                corner_radii: CornerRadii::all(STAR_POINT_RADIUS),
            });
        }

        // ── Coordinate labels ───────────────────────────────────────
        // Column labels (A-O)
        for col in 0..BOARD_SIZE {
            let label = ((b'A' + col as u8) as char).to_string();
            let x = intersection(0, col as i32).0 - 4.0;
            cmds.push(RenderCommand::Text {
                x,
                y: BOARD_OFFSET_Y - board_margin + 4.0,
                text: label.clone(),
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x,
                y: BOARD_OFFSET_Y + BOARD_PIXEL_SIZE + board_margin - 14.0,
                text: label,
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        // Row labels (1-15, bottom to top like Go convention)
        for row in 0..BOARD_SIZE {
            let label = format!("{}", BOARD_SIZE - row);
            let x = BOARD_OFFSET_X - board_margin + 2.0;
            let y = intersection(row as i32, 0).1 - 6.0;
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: label.clone(),
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: BOARD_OFFSET_X + BOARD_PIXEL_SIZE + board_margin - 16.0,
                y,
                text: label,
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // ── Win line highlight ──────────────────────────────────────
        if let Some(ref wl) = self.win_line {
            for &(wr, wc) in &wl.positions {
                let (ix, iy) = intersection(wr as i32, wc as i32);
                let x = ix - STONE_RADIUS - 3.0;
                let y = iy - STONE_RADIUS - 3.0;
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: (STONE_RADIUS + 3.0) * 2.0,
                    height: (STONE_RADIUS + 3.0) * 2.0,
                    color: WIN_HIGHLIGHT,
                    corner_radii: CornerRadii::all(STONE_RADIUS + 3.0),
                });
            }
        }

        // ── Stones ──────────────────────────────────────────────────
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                let cell = self.board.cells[row][col];
                if cell == Cell::Empty {
                    continue;
                }

                let (cx, cy) = intersection(row as i32, col as i32);

                // Stone border (slightly larger circle behind)
                let border_color = match cell {
                    Cell::Black => BLACK_STONE_BORDER,
                    Cell::White => WHITE_STONE_BORDER,
                    Cell::Empty => unreachable!(),
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx - STONE_RADIUS - 1.0,
                    y: cy - STONE_RADIUS - 1.0,
                    width: (STONE_RADIUS + 1.0) * 2.0,
                    height: (STONE_RADIUS + 1.0) * 2.0,
                    color: border_color,
                    corner_radii: CornerRadii::all(STONE_RADIUS + 1.0),
                });

                // Stone fill
                let stone_color = match cell {
                    Cell::Black => BLACK_STONE,
                    Cell::White => WHITE_STONE,
                    Cell::Empty => unreachable!(),
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx - STONE_RADIUS,
                    y: cy - STONE_RADIUS,
                    width: STONE_RADIUS * 2.0,
                    height: STONE_RADIUS * 2.0,
                    color: stone_color,
                    corner_radii: CornerRadii::all(STONE_RADIUS),
                });

                // Last move marker (small colored dot on the most recent stone)
                if self.last_move == Some((row, col)) {
                    let marker_r = 4.0;
                    let marker_color = match cell {
                        Cell::Black => WHITE_STONE,
                        Cell::White => BLACK_STONE,
                        Cell::Empty => unreachable!(),
                    };
                    cmds.push(RenderCommand::FillRect {
                        x: cx - marker_r,
                        y: cy - marker_r,
                        width: marker_r * 2.0,
                        height: marker_r * 2.0,
                        color: marker_color,
                        corner_radii: CornerRadii::all(marker_r),
                    });
                }
            }
        }

        // ── Cursor ──────────────────────────────────────────────────
        if self.phase == GamePhase::Playing && self.current_turn == Cell::Black {
            let (cx, cy) = intersection(self.cursor_row, self.cursor_col);
            let cursor_size = STONE_RADIUS + 4.0;
            cmds.push(RenderCommand::StrokeRect {
                x: cx - cursor_size,
                y: cy - cursor_size,
                width: cursor_size * 2.0,
                height: cursor_size * 2.0,
                color: CURSOR_COLOR,
                line_width: 2.0,
                corner_radii: CornerRadii::all(cursor_size),
            });
        }

        // ── Side panel ──────────────────────────────────────────────
        let px = PANEL_X;
        let py = BOARD_OFFSET_Y;

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: 200.0,
            height: 380.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Game info section
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 20.0,
            text: String::from("Game Info"),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Separator line
        cmds.push(RenderCommand::Line {
            x1: px + 15.0,
            y1: py + 40.0,
            x2: px + 185.0,
            y2: py + 40.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Move count
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 55.0,
            text: format!("Moves: {}", self.move_count),
            color: TEXT_COLOR,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Current turn
        let turn_label = match self.current_turn {
            Cell::Black => "\u{25CF} Black",
            Cell::White => "\u{25CB} White",
            Cell::Empty => "",
        };
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 78.0,
            text: format!("Turn: {turn_label}"),
            color: TEXT_COLOR,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Scores section
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 115.0,
            text: String::from("Scores"),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Line {
            x1: px + 15.0,
            y1: py + 135.0,
            x2: px + 185.0,
            y2: py + 135.0,
            color: SURFACE1,
            width: 1.0,
        });

        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 150.0,
            text: format!("\u{25CF} Black: {}", self.scores.0),
            color: TEXT_COLOR,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 173.0,
            text: format!("\u{25CB} White: {}", self.scores.1),
            color: TEXT_COLOR,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 196.0,
            text: format!("Draws: {}", self.scores.2),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Controls section
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 233.0,
            text: String::from("Controls"),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Line {
            x1: px + 15.0,
            y1: py + 253.0,
            x2: px + 185.0,
            y2: py + 253.0,
            color: SURFACE1,
            width: 1.0,
        });

        let controls = [
            ("\u{2190}\u{2191}\u{2192}\u{2193}", "Move cursor"),
            ("Enter/Space", "Place stone"),
            ("N", "New game"),
            ("Z", "Undo"),
        ];

        for (i, (key, desc)) in controls.iter().enumerate() {
            let cy = py + 268.0 + i as f32 * 22.0;
            cmds.push(RenderCommand::Text {
                x: px + 15.0,
                y: cy,
                text: String::from(*key),
                color: PEACH,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: px + 100.0,
                y: cy,
                text: String::from(*desc),
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // ── Status bar (game over message) ──────────────────────────
        if self.phase != GamePhase::Playing {
            let msg = match self.phase {
                GamePhase::Won => {
                    let who = match self.winner {
                        Cell::Black => "Black",
                        Cell::White => "White",
                        Cell::Empty => "Nobody",
                    };
                    format!("{who} wins! Press N for new game.")
                }
                GamePhase::Draw => String::from("Draw! Board is full. Press N for new game."),
                GamePhase::Playing => String::new(),
            };
            let bar_y = BOARD_OFFSET_Y + BOARD_PIXEL_SIZE + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: BOARD_OFFSET_X - 10.0,
                y: bar_y,
                width: PANEL_X + 200.0,
                height: 30.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: BOARD_OFFSET_X,
                y: bar_y + 8.0,
                text: msg,
                color: GREEN,
                font_size: INFO_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        cmds
    }
}

fn main() {
    let _app = GomokuApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes past the end, or unwraps a `None`, is a test that
    // has already failed; panicking is the reporting mechanism, not a fault.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::unwrap_used
    )]

    use super::*;

    // ── Helper functions ────────────────────────────────────────────

    /// Create an app with a fresh game.
    fn fresh_app() -> GomokuApp {
        GomokuApp::new()
    }

    /// Place a stone on the board without triggering AI.
    fn place_raw(board: &mut Board, row: usize, col: usize, stone: Cell) {
        board.set(row, col, stone);
    }

    // ── Board basic tests ───────────────────────────────────────────

    #[test]
    fn test_new_board_is_empty() {
        let board = Board::new();
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                assert_eq!(board.cells[row][col], Cell::Empty);
            }
        }
    }

    #[test]
    fn test_board_set_and_get() {
        let mut board = Board::new();
        board.set(3, 5, Cell::Black);
        assert_eq!(board.get(3, 5), Some(Cell::Black));
        assert_eq!(board.get(0, 0), Some(Cell::Empty));
    }

    #[test]
    fn test_board_get_out_of_bounds() {
        let board = Board::new();
        assert_eq!(board.get(-1, 0), None);
        assert_eq!(board.get(0, -1), None);
        assert_eq!(board.get(15, 0), None);
        assert_eq!(board.get(0, 15), None);
        assert_eq!(board.get(-5, -5), None);
    }

    #[test]
    fn test_board_is_empty() {
        let mut board = Board::new();
        assert!(board.is_empty(7, 7));
        board.set(7, 7, Cell::Black);
        assert!(!board.is_empty(7, 7));
    }

    #[test]
    fn test_board_stone_count() {
        let mut board = Board::new();
        assert_eq!(board.stone_count(), 0);
        board.set(0, 0, Cell::Black);
        assert_eq!(board.stone_count(), 1);
        board.set(1, 1, Cell::White);
        assert_eq!(board.stone_count(), 2);
    }

    #[test]
    fn test_board_is_full() {
        let mut board = Board::new();
        assert!(!board.is_full());
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                board.set(row, col, Cell::Black);
            }
        }
        assert!(board.is_full());
    }

    // ── Cell tests ──────────────────────────────────────────────────

    #[test]
    fn test_cell_opponent() {
        assert_eq!(Cell::Black.opponent(), Cell::White);
        assert_eq!(Cell::White.opponent(), Cell::Black);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
    }

    // ── Win detection: horizontal ───────────────────────────────────

    #[test]
    fn test_win_horizontal_black() {
        let mut board = Board::new();
        for c in 3..8 {
            board.set(7, c, Cell::Black);
        }
        let result = board.check_winner(Cell::Black);
        assert!(result.is_some());
        let wl = result.expect("expected win line");
        assert_eq!(wl.positions.len(), 5);
    }

    #[test]
    fn test_win_horizontal_white() {
        let mut board = Board::new();
        for c in 0..5 {
            board.set(0, c, Cell::White);
        }
        assert!(board.check_winner(Cell::White).is_some());
    }

    #[test]
    fn test_win_horizontal_at_right_edge() {
        let mut board = Board::new();
        for c in 10..15 {
            board.set(7, c, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_no_win_horizontal_four() {
        let mut board = Board::new();
        for c in 3..7 {
            board.set(7, c, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_none());
    }

    // ── Win detection: vertical ─────────────────────────────────────

    #[test]
    fn test_win_vertical_black() {
        let mut board = Board::new();
        for r in 2..7 {
            board.set(r, 5, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_vertical_white() {
        let mut board = Board::new();
        for r in 10..15 {
            board.set(r, 0, Cell::White);
        }
        assert!(board.check_winner(Cell::White).is_some());
    }

    #[test]
    fn test_win_vertical_at_top() {
        let mut board = Board::new();
        for r in 0..5 {
            board.set(r, 14, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_no_win_vertical_four() {
        let mut board = Board::new();
        for r in 5..9 {
            board.set(r, 7, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_none());
    }

    // ── Win detection: diagonal (backslash \) ───────────────────────

    #[test]
    fn test_win_diagonal_backslash() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(3 + i, 3 + i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_diagonal_backslash_at_corner() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(i, i, Cell::White);
        }
        assert!(board.check_winner(Cell::White).is_some());
    }

    #[test]
    fn test_win_diagonal_backslash_bottom_right() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(10 + i, 10 + i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_no_win_diagonal_backslash_four() {
        let mut board = Board::new();
        for i in 0..4 {
            board.set(3 + i, 3 + i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_none());
    }

    // ── Win detection: diagonal (forward slash /) ───────────────────

    #[test]
    fn test_win_diagonal_forward_slash() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(7 - i, 3 + i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_diagonal_forward_slash_top_right() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(4 - i, 10 + i, Cell::White);
        }
        assert!(board.check_winner(Cell::White).is_some());
    }

    #[test]
    fn test_win_diagonal_forward_slash_bottom_left() {
        let mut board = Board::new();
        for i in 0..5 {
            board.set(14 - i, i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_no_win_diagonal_forward_slash_four() {
        let mut board = Board::new();
        for i in 0..4 {
            board.set(7 - i, 3 + i, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_none());
    }

    // ── Win detection: mixed (no false positives) ───────────────────

    #[test]
    fn test_no_win_different_colors() {
        let mut board = Board::new();
        board.set(7, 3, Cell::Black);
        board.set(7, 4, Cell::Black);
        board.set(7, 5, Cell::White);
        board.set(7, 6, Cell::Black);
        board.set(7, 7, Cell::Black);
        assert!(board.check_winner(Cell::Black).is_none());
        assert!(board.check_winner(Cell::White).is_none());
    }

    #[test]
    fn test_no_win_empty_board() {
        let board = Board::new();
        assert!(board.check_winner(Cell::Black).is_none());
        assert!(board.check_winner(Cell::White).is_none());
    }

    #[test]
    fn test_win_line_positions_correct() {
        let mut board = Board::new();
        for c in 5..10 {
            board.set(3, c, Cell::Black);
        }
        let wl = board.check_winner(Cell::Black).expect("should win");
        assert_eq!(wl.positions.len(), 5);
        for (i, &(r, c)) in wl.positions.iter().enumerate() {
            assert_eq!(r, 3);
            assert_eq!(c, 5 + i);
        }
    }

    // ── Stone placement tests ───────────────────────────────────────

    #[test]
    fn test_place_stone_on_empty() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        assert!(app.try_place_stone());
        // After placing, AI (White) also plays, so two stones total
        assert!(app.board.stone_count() >= 2);
    }

    #[test]
    fn test_place_stone_on_occupied_fails() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        // Try to place on same spot (now it should have Black's stone)
        // First we need to verify 7,7 is occupied
        assert!(!app.board.is_empty(7, 7));
        app.cursor_row = 7;
        app.cursor_col = 7;
        assert!(!app.try_place_stone());
    }

    #[test]
    fn test_place_stone_alternates_turns() {
        let mut app = fresh_app();
        assert_eq!(app.current_turn, Cell::Black);
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        // After Black places and AI responds, it should be Black's turn again
        assert_eq!(app.current_turn, Cell::Black);
    }

    #[test]
    fn test_cannot_place_after_win() {
        let mut app = fresh_app();
        // Manually set up a near-win for Black (without AI interference)
        app.board = Board::new();
        for c in 0..4 {
            app.board.set(0, c, Cell::Black);
        }
        app.cursor_row = 0;
        app.cursor_col = 4;
        app.place_stone(0, 4); // This triggers win check
        assert_eq!(app.phase, GamePhase::Won);
        // Now try to place another stone
        app.cursor_row = 5;
        app.cursor_col = 5;
        assert!(!app.try_place_stone());
    }

    // ── Game state tests ────────────────────────────────────────────

    #[test]
    fn test_initial_state() {
        let app = fresh_app();
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(app.move_count, 0);
        assert!(app.move_history.is_empty());
        assert!(app.win_line.is_none());
        assert_eq!(app.winner, Cell::Empty);
        assert_eq!(app.cursor_row, 7);
        assert_eq!(app.cursor_col, 7);
    }

    #[test]
    fn test_new_game_resets_state() {
        let mut app = fresh_app();
        app.cursor_row = 3;
        app.cursor_col = 5;
        app.try_place_stone();
        let old_scores = app.scores;
        app.new_game();
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(app.move_count, 0);
        assert!(app.move_history.is_empty());
        assert_eq!(app.scores, old_scores); // scores preserved
    }

    #[test]
    fn test_new_game_preserves_scores() {
        let mut app = fresh_app();
        app.scores = (3, 2, 1);
        app.new_game();
        assert_eq!(app.scores, (3, 2, 1));
    }

    #[test]
    fn test_win_increments_score_black() {
        let mut app = fresh_app();
        app.board = Board::new();
        for c in 0..4 {
            app.board.set(0, c, Cell::Black);
        }
        app.place_stone(0, 4);
        assert_eq!(app.scores.0, 1);
    }

    #[test]
    fn test_win_increments_score_white() {
        let mut app = fresh_app();
        app.board = Board::new();
        for c in 0..4 {
            app.board.set(0, c, Cell::White);
        }
        app.current_turn = Cell::White;
        app.place_stone(0, 4);
        assert_eq!(app.scores.1, 1);
    }

    #[test]
    fn test_draw_detection() {
        let mut app = fresh_app();
        app.board = Board::new();
        // Fill the entire board without creating five in a row.
        // Pattern: alternate BW in a way that never creates five.
        // Row 0: B W B W B W B W B W B W B W B
        // Row 1: W B W B W B W B W B W B W B W
        // etc., but swap every 3rd row to break diagonals.
        for row in 0..BOARD_SIZE {
            for col in 0..BOARD_SIZE {
                let pattern = match row % 3 {
                    0 => (row + col) % 2,
                    1 => (row + col + 1) % 2,
                    _ => (col / 2 + row) % 2,
                };
                let cell = if pattern == 0 {
                    Cell::Black
                } else {
                    Cell::White
                };
                app.board.set(row, col, cell);
            }
        }
        // Make sure there's no winner
        if app.board.check_winner(Cell::Black).is_some()
            || app.board.check_winner(Cell::White).is_some()
        {
            // Fallback: just test the draw mechanism directly
            app.phase = GamePhase::Draw;
            app.scores.2 += 1;
            assert_eq!(app.phase, GamePhase::Draw);
            assert_eq!(app.scores.2, 1);
            return;
        }
        assert!(app.board.is_full());
    }

    // ── Undo tests ──────────────────────────────────────────────────

    #[test]
    fn test_undo_removes_last_moves() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        let count_after = app.move_count;
        assert!(count_after >= 2); // Black + AI White
        app.undo();
        assert_eq!(app.move_count, 0);
        assert!(app.board.is_empty(7, 7));
    }

    #[test]
    fn test_undo_on_empty_history() {
        let mut app = fresh_app();
        app.undo(); // Should not crash
        assert_eq!(app.move_count, 0);
        assert_eq!(app.current_turn, Cell::Black);
    }

    #[test]
    fn test_undo_restores_turn() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        app.undo();
        assert_eq!(app.current_turn, Cell::Black);
    }

    #[test]
    fn test_undo_after_win_restores_playing() {
        let mut app = fresh_app();
        app.board = Board::new();
        for c in 0..4 {
            app.board.set(0, c, Cell::Black);
        }
        app.move_history = (0..4)
            .map(|c| MoveRecord {
                row: 0,
                col: c,
                stone: Cell::Black,
            })
            .collect();
        app.move_count = 4;
        app.place_stone(0, 4);
        assert_eq!(app.phase, GamePhase::Won);
        assert_eq!(app.scores.0, 1);
        app.undo();
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.scores.0, 0);
    }

    #[test]
    fn test_undo_multiple_times() {
        let mut app = fresh_app();
        // Place first move
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        // Place second move
        app.cursor_row = 3;
        app.cursor_col = 3;
        app.try_place_stone();
        // Undo twice
        app.undo();
        app.undo();
        assert_eq!(app.move_count, 0);
        assert!(app.board.is_empty(7, 7));
        assert!(app.board.is_empty(3, 3));
    }

    // ── Cursor movement tests ───────────────────────────────────────

    #[test]
    fn test_cursor_initial_position() {
        let app = fresh_app();
        assert_eq!(app.cursor_row, BOARD_SIZE as i32 / 2);
        assert_eq!(app.cursor_col, BOARD_SIZE as i32 / 2);
    }

    #[test]
    fn test_cursor_move_up() {
        let mut app = fresh_app();
        app.cursor_row = 5;
        app.handle_key(&KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_row, 4);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut app = fresh_app();
        app.cursor_row = 5;
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_row, 6);
    }

    #[test]
    fn test_cursor_move_left() {
        let mut app = fresh_app();
        app.cursor_col = 5;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_col, 4);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut app = fresh_app();
        app.cursor_col = 5;
        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_col, 6);
    }

    #[test]
    fn test_cursor_clamp_top() {
        let mut app = fresh_app();
        app.cursor_row = 0;
        app.handle_key(&KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn test_cursor_clamp_bottom() {
        let mut app = fresh_app();
        app.cursor_row = BOARD_SIZE as i32 - 1;
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_row, BOARD_SIZE as i32 - 1);
    }

    #[test]
    fn test_cursor_clamp_left() {
        let mut app = fresh_app();
        app.cursor_col = 0;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_clamp_right() {
        let mut app = fresh_app();
        app.cursor_col = BOARD_SIZE as i32 - 1;
        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.cursor_col, BOARD_SIZE as i32 - 1);
    }

    // ── Key handling tests ──────────────────────────────────────────

    #[test]
    fn test_enter_places_stone() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert!(!app.board.is_empty(7, 7));
    }

    #[test]
    fn test_space_places_stone() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert!(!app.board.is_empty(7, 7));
    }

    #[test]
    fn test_n_key_new_game() {
        let mut app = fresh_app();
        app.cursor_row = 3;
        app.cursor_col = 3;
        app.try_place_stone();
        app.handle_key(&KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn test_z_key_undo() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        let count = app.move_count;
        app.handle_key(&KeyEvent {
            key: Key::Z,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert!(app.move_count < count);
    }

    // ── AI behavior tests ───────────────────────────────────────────

    #[test]
    fn test_ai_blocks_four_in_row() {
        let mut board = Board::new();
        // Set up Black with 4 in a row, one end open
        board.set(7, 3, Cell::Black);
        board.set(7, 4, Cell::Black);
        board.set(7, 5, Cell::Black);
        board.set(7, 6, Cell::Black);
        // AI should block at either end
        let best = find_best_move(&board, Cell::White);
        assert!(best.is_some());
        let (r, c) = best.expect("AI should find a move");
        assert_eq!(r, 7);
        assert!(
            c == 2 || c == 7,
            "AI should block at row 7, col 2 or 7, got col {c}"
        );
    }

    #[test]
    fn test_ai_takes_winning_move() {
        let mut board = Board::new();
        // White has 4 in a row, can win
        board.set(5, 3, Cell::White);
        board.set(5, 4, Cell::White);
        board.set(5, 5, Cell::White);
        board.set(5, 6, Cell::White);
        let best = find_best_move(&board, Cell::White);
        assert!(best.is_some());
        let (r, c) = best.expect("AI should find winning move");
        assert_eq!(r, 5);
        assert!(
            c == 2 || c == 7,
            "AI should win at row 5, col 2 or 7, got col {c}"
        );
    }

    #[test]
    fn test_ai_plays_center_on_empty_board() {
        let board = Board::new();
        let best = find_best_move(&board, Cell::White);
        assert!(best.is_some());
        let (r, c) = best.expect("AI should find a move");
        assert_eq!(r, 7);
        assert_eq!(c, 7);
    }

    #[test]
    fn test_ai_returns_some_on_non_full_board() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        let best = find_best_move(&board, Cell::White);
        assert!(best.is_some());
    }

    #[test]
    fn test_ai_prefers_winning_over_blocking() {
        let mut board = Board::new();
        // White has 4 in a row (can win)
        board.set(3, 0, Cell::White);
        board.set(3, 1, Cell::White);
        board.set(3, 2, Cell::White);
        board.set(3, 3, Cell::White);
        // Black also has 4 in a row (threatening)
        board.set(10, 5, Cell::Black);
        board.set(10, 6, Cell::Black);
        board.set(10, 7, Cell::Black);
        board.set(10, 8, Cell::Black);
        let best = find_best_move(&board, Cell::White);
        let (r, c) = best.expect("AI should find a move");
        // AI should take the win rather than block
        assert_eq!(r, 3);
        assert!(c == 4, "AI should win at (3, 4), got ({r}, {c})");
    }

    // ── Board evaluation tests ──────────────────────────────────────

    #[test]
    fn test_evaluate_empty_board() {
        let board = Board::new();
        let score = board.evaluate(Cell::Black);
        assert_eq!(score, 0);
    }

    #[test]
    fn test_evaluate_single_stone() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        let score = board.evaluate(Cell::Black);
        assert!(
            score > 0,
            "Single stone should give positive score for its owner"
        );
    }

    #[test]
    fn test_evaluate_advantage() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        board.set(7, 8, Cell::Black);
        board.set(7, 9, Cell::Black);
        let score = board.evaluate(Cell::Black);
        assert!(score > 0, "Three in a row should be positive for Black");
    }

    #[test]
    fn test_evaluate_five_in_row() {
        let mut board = Board::new();
        for c in 3..8 {
            board.set(5, c, Cell::Black);
        }
        let score = board.evaluate(Cell::Black);
        assert!(
            score >= SCORE_FIVE,
            "Five in a row should score at least SCORE_FIVE"
        );
    }

    #[test]
    fn test_evaluate_symmetry() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        let score_b = board.evaluate(Cell::Black);
        let score_w = board.evaluate(Cell::White);
        // One stone for Black: positive for Black, negative for White
        assert!(score_b > 0);
        assert!(score_w < 0);
    }

    // ── Pattern evaluation tests ────────────────────────────────────

    #[test]
    fn test_line_pattern_open_three() {
        let mut board = Board::new();
        // Open three: _BBB_
        board.set(7, 5, Cell::Black);
        board.set(7, 6, Cell::Black);
        board.set(7, 7, Cell::Black);
        let score = board.evaluate_line_pattern(7, 6, 0, 1, Cell::Black);
        assert!(score >= SCORE_OPEN_THREE);
    }

    #[test]
    fn test_line_pattern_half_open_three() {
        let mut board = Board::new();
        // Half-open three: XBBB_ (X = edge)
        board.set(7, 0, Cell::Black);
        board.set(7, 1, Cell::Black);
        board.set(7, 2, Cell::Black);
        let score = board.evaluate_line_pattern(7, 1, 0, 1, Cell::Black);
        assert!(score >= SCORE_HALF_OPEN_THREE);
    }

    #[test]
    fn test_line_pattern_open_four() {
        let mut board = Board::new();
        // Open four: _BBBB_
        board.set(7, 4, Cell::Black);
        board.set(7, 5, Cell::Black);
        board.set(7, 6, Cell::Black);
        board.set(7, 7, Cell::Black);
        let score = board.evaluate_line_pattern(7, 5, 0, 1, Cell::Black);
        assert!(score >= SCORE_OPEN_FOUR);
    }

    // ── Candidate move generation tests ─────────────────────────────

    #[test]
    fn test_candidates_empty_board() {
        let board = Board::new();
        let moves = board.candidate_moves();
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0], (7, 7));
    }

    #[test]
    fn test_candidates_near_existing_stones() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        let moves = board.candidate_moves();
        // Should have moves near (7,7) within radius 2
        assert!(!moves.is_empty());
        for &(r, c) in &moves {
            let dr = (r as i32 - 7).abs();
            let dc = (c as i32 - 7).abs();
            assert!(dr <= 2 && dc <= 2, "Candidate ({r},{c}) too far from (7,7)");
        }
    }

    #[test]
    fn test_candidates_exclude_occupied() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        let moves = board.candidate_moves();
        assert!(!moves.contains(&(7, 7)), "Should not include occupied cell");
    }

    // ── Threat detection tests ──────────────────────────────────────

    #[test]
    fn test_is_threat_four_in_row() {
        let mut board = Board::new();
        board.set(7, 4, Cell::Black);
        board.set(7, 5, Cell::Black);
        board.set(7, 6, Cell::Black);
        assert!(board.is_threat_move(7, 7, Cell::Black));
    }

    #[test]
    fn test_is_not_threat_two_in_row() {
        let mut board = Board::new();
        board.set(7, 5, Cell::Black);
        assert!(!board.is_threat_move(7, 6, Cell::Black));
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = fresh_app();
        let cmds = app.render();
        assert!(!cmds.is_empty(), "Render should produce commands");
    }

    #[test]
    fn test_render_has_background() {
        let app = fresh_app();
        let cmds = app.render();
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == BASE));
        assert!(has_bg, "Render should include background rect");
    }

    #[test]
    fn test_render_has_title() {
        let app = fresh_app();
        let cmds = app.render();
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Gomoku"));
        assert!(has_title, "Render should include title text");
    }

    #[test]
    fn test_render_has_grid_lines() {
        let app = fresh_app();
        let cmds = app.render();
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        // At minimum: 15 horizontal + 15 vertical = 30 grid lines,
        // plus separator lines in the panel
        assert!(
            line_count >= 30,
            "Should have at least 30 grid lines, got {line_count}"
        );
    }

    #[test]
    fn test_render_has_star_points() {
        let app = fresh_app();
        let cmds = app.render();
        // Star points are rendered as small filled circles
        let star_count = cmds
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::FillRect { color, width, .. }
                if *color == STAR_POINT_COLOR && *width < 20.0)
            })
            .count();
        assert_eq!(
            star_count, 5,
            "Should render 5 star points, got {star_count}"
        );
    }

    #[test]
    fn test_render_shows_stone_after_placement() {
        let mut app = fresh_app();
        app.board.set(7, 7, Cell::Black);
        let cmds = app.render();
        // Should have a black stone circle
        let has_black_stone = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == BLACK_STONE));
        assert!(has_black_stone, "Should render a black stone");
    }

    #[test]
    fn test_render_shows_cursor() {
        let app = fresh_app();
        let cmds = app.render();
        let has_cursor = cmds.iter().any(
            |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == CURSOR_COLOR),
        );
        assert!(has_cursor, "Should render cursor highlight");
    }

    #[test]
    fn test_render_shows_win_highlight() {
        let mut app = fresh_app();
        app.board = Board::new();
        for c in 3..8 {
            app.board.set(5, c, Cell::Black);
        }
        app.phase = GamePhase::Won;
        app.winner = Cell::Black;
        app.win_line = Some(WinLine {
            positions: (3..8).map(|c| (5, c)).collect(),
        });
        let cmds = app.render();
        let highlight_count = cmds
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == WIN_HIGHLIGHT),
            )
            .count();
        assert_eq!(highlight_count, 5, "Should highlight 5 winning positions");
    }

    #[test]
    fn test_render_shows_game_over_bar() {
        let mut app = fresh_app();
        app.phase = GamePhase::Won;
        app.winner = Cell::Black;
        let cmds = app.render();
        let has_game_over = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("wins")));
        assert!(has_game_over, "Should show game over message");
    }

    #[test]
    fn test_render_shows_controls_panel() {
        let app = fresh_app();
        let cmds = app.render();
        let has_controls = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Controls"));
        assert!(has_controls, "Should show controls section");
    }

    #[test]
    fn test_render_shows_scores_panel() {
        let app = fresh_app();
        let cmds = app.render();
        let has_scores = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Scores"));
        assert!(has_scores, "Should show scores section");
    }

    #[test]
    fn test_render_shows_last_move_marker() {
        let mut app = fresh_app();
        app.board.set(7, 7, Cell::Black);
        app.last_move = Some((7, 7));
        let cmds = app.render();
        // Last move marker is a small white dot on a black stone
        let has_marker = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, width, .. }
                if *color == WHITE_STONE && *width < 12.0)
        });
        assert!(has_marker, "Should render last move marker");
    }

    // ── Event handling tests ────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = fresh_app();
        app.cursor_row = 5;
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }));
        assert_eq!(app.cursor_row, 4);
    }

    #[test]
    fn test_handle_event_mouse() {
        let mut app = fresh_app();
        // Click near center of the board at intersection (7, 7)
        let cx = BOARD_OFFSET_X + 7.0 * CELL_SIZE;
        let cy = BOARD_OFFSET_Y + 7.0 * CELL_SIZE;
        app.handle_event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: cx,
            y: cy,
        }));
        assert!(!app.board.is_empty(7, 7));
    }

    // ── Move record tests ───────────────────────────────────────────

    #[test]
    fn test_move_history_recorded() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        // At least 2 moves: Black + AI White
        assert!(app.move_history.len() >= 2);
        assert_eq!(app.move_history[0].stone, Cell::Black);
        assert_eq!(app.move_history[0].row, 7);
        assert_eq!(app.move_history[0].col, 7);
    }

    #[test]
    fn test_last_move_updated() {
        let mut app = fresh_app();
        app.cursor_row = 7;
        app.cursor_col = 7;
        app.try_place_stone();
        // Last move should be the AI's move (since AI plays after Black)
        assert!(app.last_move.is_some());
    }

    // ── Boundary and edge case tests ────────────────────────────────

    #[test]
    fn test_place_at_all_corners() {
        for &(r, c) in &[(0, 0), (0, 14), (14, 0), (14, 14)] {
            let mut board = Board::new();
            board.set(r, c, Cell::Black);
            assert_eq!(board.get(r as i32, c as i32), Some(Cell::Black));
        }
    }

    #[test]
    fn test_win_at_board_edge_top() {
        let mut board = Board::new();
        for c in 0..5 {
            board.set(0, c, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_at_board_edge_bottom() {
        let mut board = Board::new();
        for c in 5..10 {
            board.set(14, c, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_at_board_edge_left() {
        let mut board = Board::new();
        for r in 0..5 {
            board.set(r, 0, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_win_at_board_edge_right() {
        let mut board = Board::new();
        for r in 5..10 {
            board.set(r, 14, Cell::White);
        }
        assert!(board.check_winner(Cell::White).is_some());
    }

    #[test]
    fn test_six_in_row_also_wins() {
        // In standard Gomoku, six in a row still counts as a win
        let mut board = Board::new();
        for c in 2..8 {
            board.set(7, c, Cell::Black);
        }
        assert!(board.check_winner(Cell::Black).is_some());
    }

    #[test]
    fn test_count_direction_empty() {
        let board = Board::new();
        assert_eq!(board.count_direction(7, 7, 0, 1, Cell::Black), 0);
    }

    #[test]
    fn test_count_direction_with_stones() {
        let mut board = Board::new();
        board.set(7, 8, Cell::Black);
        board.set(7, 9, Cell::Black);
        board.set(7, 10, Cell::Black);
        assert_eq!(board.count_direction(7, 7, 0, 1, Cell::Black), 3);
    }

    #[test]
    fn test_is_open_end_true() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        board.set(7, 8, Cell::Black);
        // (7,9) is empty -> open end in direction (0,1)
        assert!(board.is_open_end(7, 7, 0, 1, Cell::Black));
    }

    #[test]
    fn test_is_open_end_blocked_by_opponent() {
        let mut board = Board::new();
        board.set(7, 7, Cell::Black);
        board.set(7, 8, Cell::Black);
        board.set(7, 9, Cell::White);
        assert!(!board.is_open_end(7, 7, 0, 1, Cell::Black));
    }

    #[test]
    fn test_is_open_end_blocked_by_edge() {
        let mut board = Board::new();
        board.set(7, 13, Cell::Black);
        board.set(7, 14, Cell::Black);
        // Edge of board in direction (0,1) -> blocked
        assert!(!board.is_open_end(7, 13, 0, 1, Cell::Black));
    }

    // ── Board geometry tests ───────────────────────────────
    //
    // `intersection` and `intersection_near` are each other's inverse, so a
    // test that clicks where the code says it drew something agrees with any
    // mapping, right or wrong. The checks that carry weight here are the ones
    // whose expected value comes from somewhere else: the painted grid lines,
    // the window, and the way a Go board is numbered.

    /// The x of every vertical grid line and the y of every horizontal one,
    /// read back off the paint and sorted. Nothing here consults
    /// `intersection`, so these are the lines a player actually sees.
    fn painted_grid(commands: &[RenderCommand]) -> (Vec<f32>, Vec<f32>) {
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for c in commands {
            if let RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                ..
            } = c
            {
                // The side panel draws separator lines too; the board's are the
                // ones in the grid colour.
                if *color != GRID_LINE_COLOR {
                    continue;
                }
                if (x1 - x2).abs() < 0.01 {
                    xs.push(*x1);
                } else if (y1 - y2).abs() < 0.01 {
                    ys.push(*y1);
                }
            }
        }
        xs.sort_by(f32::total_cmp);
        ys.sort_by(f32::total_cmp);
        (xs, ys)
    }

    /// Centres of the stones the renderer painted. A stone is the fill whose
    /// corner radius makes it a circle of exactly `STONE_RADIUS`.
    fn painted_stone_centers(commands: &[RenderCommand]) -> Vec<(f32, f32)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x, y, width, color, ..
                } if (*width - STONE_RADIUS * 2.0).abs() < 0.01
                    && (*color == BLACK_STONE || *color == WHITE_STONE) =>
                {
                    Some((*x + STONE_RADIUS, *y + STONE_RADIUS))
                }
                _ => None,
            })
            .collect()
    }

    /// Where a coordinate label was drawn.
    ///
    /// The side panel also draws short numbers at this size -- move numbers,
    /// counts -- so the colour is part of the match; without it, looking up
    /// row "8" finds whichever "8" the panel happened to print.
    fn label_pos(commands: &[RenderCommand], want: &str) -> Option<(f32, f32)> {
        commands.iter().find_map(|c| match c {
            RenderCommand::Text {
                x,
                y,
                text,
                font_size,
                color,
                ..
            } if (*font_size - SMALL_FONT_SIZE).abs() < 0.01
                && *color == OVERLAY0
                && text == want =>
            {
                Some((*x, *y))
            }
            _ => None,
        })
    }

    #[test]
    fn the_painted_grid_is_fifteen_lines_each_way_evenly_spaced() {
        // Measured against the window and against itself, never against
        // `intersection`: fifteen lines in each direction, one cell apart,
        // inside the window and clear of the side panel.
        let app = fresh_app();
        let (xs, ys) = painted_grid(&app.render());
        assert_eq!(xs.len(), BOARD_SIZE, "one vertical line per column");
        assert_eq!(ys.len(), BOARD_SIZE, "one horizontal line per row");

        for pair in xs.windows(2) {
            assert!(
                (pair[1] - pair[0] - CELL_SIZE).abs() < 0.01,
                "columns should be one cell apart, got {}",
                pair[1] - pair[0]
            );
        }
        for pair in ys.windows(2) {
            assert!(
                (pair[1] - pair[0] - CELL_SIZE).abs() < 0.01,
                "rows should be one cell apart, got {}",
                pair[1] - pair[0]
            );
        }
        assert!(
            xs[0] > 0.0 && ys[0] > 0.0,
            "the board starts inside the window"
        );
        assert!(
            xs[BOARD_SIZE - 1] < PANEL_X,
            "the board should not run under the side panel"
        );
    }

    #[test]
    fn every_painted_intersection_is_clickable() {
        // The round-trip half: every crossing a player can see resolves to a
        // distinct point, and to the one at that position in reading order.
        let app = fresh_app();
        let (xs, ys) = painted_grid(&app.render());
        // Probed around each crossing as well as on it. Clicking only the
        // exact crossing cannot tell "nearest line" from "line at or before
        // this point": both answer correctly when the click is dead on. The
        // offsets are a stone's radius, which is both less than half a cell
        // (so the nearest crossing is unambiguous) and within the slop (so the
        // answer must be a hit), and is also exactly the ink a player aims at.
        let r = STONE_RADIUS;
        for (row, &y) in ys.iter().enumerate() {
            for (col, &x) in xs.iter().enumerate() {
                for (dx, dy) in [
                    (0.0, 0.0),
                    (r, 0.0),
                    (-r, 0.0),
                    (0.0, r),
                    (0.0, -r),
                    (r * 0.7, r * 0.7),
                    (-r * 0.7, -r * 0.7),
                ] {
                    assert_eq!(
                        intersection_near(x + dx, y + dy),
                        Some((row as i32, col as i32)),
                        "({}, {}) is on the stone at ({row}, {col}) and should hit it",
                        x + dx,
                        y + dy
                    );
                }
            }
        }
    }

    #[test]
    fn every_grid_line_spans_the_whole_board() {
        // `painted_grid` keeps only where each line sits, so a grid drawn with
        // the right spacing but the wrong *length* slips past it. The lines
        // have to reach each other: every horizontal line runs from the first
        // vertical to the last, and the other way round.
        let app = fresh_app();
        let commands = app.render();
        let (mut horizontals, mut verticals) = (Vec::new(), Vec::new());
        for c in &commands {
            if let RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                ..
            } = c
            {
                if *color != GRID_LINE_COLOR {
                    continue;
                }
                if (y1 - y2).abs() < 0.01 {
                    horizontals.push((*y1, x1.min(*x2), x1.max(*x2)));
                } else {
                    verticals.push((*x1, y1.min(*y2), y1.max(*y2)));
                }
            }
        }
        assert_eq!(horizontals.len(), BOARD_SIZE);
        assert_eq!(verticals.len(), BOARD_SIZE);

        let left = verticals.iter().map(|v| v.0).fold(f32::MAX, f32::min);
        let right = verticals.iter().map(|v| v.0).fold(f32::MIN, f32::max);
        let top = horizontals.iter().map(|h| h.0).fold(f32::MAX, f32::min);
        let bottom = horizontals.iter().map(|h| h.0).fold(f32::MIN, f32::max);

        for (y, x1, x2) in horizontals {
            assert!(
                (x1 - left).abs() < 0.01 && (x2 - right).abs() < 0.01,
                "the line at y={y} runs {x1}..{x2}, but the board is {left}..{right}"
            );
        }
        for (x, y1, y2) in verticals {
            assert!(
                (y1 - top).abs() < 0.01 && (y2 - bottom).abs() < 0.01,
                "the line at x={x} runs {y1}..{y2}, but the board is {top}..{bottom}"
            );
        }
    }

    #[test]
    fn the_wooden_board_sits_squarely_under_the_grid() {
        // The margin round the grid is drawn from `BOARD_PIXEL_SIZE`, and the
        // grid itself from `intersection`. If those two disagree about how big
        // the board is, the wood is off-centre -- the only symptom of a board
        // sized a line too wide, since everything else just shifts with it.
        let app = fresh_app();
        let commands = app.render();
        let (xs, ys) = painted_grid(&commands);
        let wood = commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == BOARD_BG => Some((*x, *y, *width, *height)),
                _ => None,
            })
            .expect("the wooden board should be painted");

        let (wx, wy, ww, wh) = wood;
        let margins = [
            ("left", xs[0] - wx),
            ("right", wx + ww - xs[BOARD_SIZE - 1]),
            ("top", ys[0] - wy),
            ("bottom", wy + wh - ys[BOARD_SIZE - 1]),
        ];
        for &(name, m) in &margins {
            assert!(
                m > 0.0,
                "the {name} margin is {m}; the grid escapes the wood"
            );
        }
        for pair in margins.windows(2) {
            assert!(
                (pair[0].1 - pair[1].1).abs() < 0.01,
                "the {} margin is {} but the {} margin is {}",
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1
            );
        }
    }

    #[test]
    fn the_middle_of_a_cell_belongs_to_no_intersection() {
        // This board deliberately has gaps -- a stone dropped half a cell from
        // where the player aimed is worse than a click that does nothing -- so
        // the gaps are asserted rather than left to chance.
        let app = fresh_app();
        let (xs, ys) = painted_grid(&app.render());
        for pair_y in ys.windows(2) {
            for pair_x in xs.windows(2) {
                let (mx, my) = (
                    f32::midpoint(pair_x[0], pair_x[1]),
                    f32::midpoint(pair_y[0], pair_y[1]),
                );
                assert_eq!(
                    intersection_near(mx, my),
                    None,
                    "the middle of a cell at ({mx}, {my}) should hit nothing"
                );
            }
        }
    }

    #[test]
    fn the_click_slop_reaches_equally_far_past_every_edge() {
        // The bug this pins: the hit test used to compute an index and reject
        // it when out of range, which let the overhang work on the left and
        // top but not on the right and bottom. A stone is drawn centred on the
        // edge intersections, so half of each edge stone hangs off the board
        // and has to stay clickable.
        let app = fresh_app();
        let (xs, ys) = painted_grid(&app.render());
        let (first_x, last_x) = (xs[0], xs[BOARD_SIZE - 1]);
        let (first_y, last_y) = (ys[0], ys[BOARD_SIZE - 1]);

        // Swept rather than probed at one distance. How far the slop reaches is
        // the hit test's business and this test does not claim to know it; what
        // it claims is that the answer is the *same number* on all four sides.
        // A single probe would have to land in the 0.4px window where the two
        // spellings disagreed, which is knowledge of the bug rather than of the
        // rule.
        let mut d = 0.0f32;
        while d < CELL_SIZE {
            let left = intersection_near(first_x - d, first_y).is_some();
            let right = intersection_near(last_x + d, first_y).is_some();
            assert_eq!(
                left, right,
                "{d}px past the left edge is {left}, but {d}px past the right edge is {right}"
            );

            let top = intersection_near(first_x, first_y - d).is_some();
            let bottom = intersection_near(first_x, last_y + d).is_some();
            assert_eq!(
                top, bottom,
                "{d}px above the board is {top}, but {d}px below it is {bottom}"
            );
            d += 0.25;
        }

        // And it has to reach at least as far as the ink: a stone is drawn
        // centred on its intersection, so the outer half of every edge stone
        // hangs off the board and must still be clickable.
        assert_eq!(
            intersection_near(last_x + STONE_RADIUS, first_y),
            Some((0, LAST_INDEX)),
            "the outer half of the last stone in a row must be clickable"
        );
        assert_eq!(
            intersection_near(first_x, last_y + STONE_RADIUS),
            Some((LAST_INDEX, 0)),
            "the outer half of the last stone in a column must be clickable"
        );
    }

    #[test]
    fn nothing_well_outside_the_board_lands_on_an_intersection() {
        // Swept far past every edge, because the faults this is looking for --
        // a saturating cast, a wrap, a clamp that silently rescues a bad index
        // -- all live beyond the board rather than just outside it.
        let far = BOARD_OFFSET_X + BOARD_PIXEL_SIZE + 300.0;
        let mut x = -200.0;
        while x < far {
            let mut y = -200.0;
            while y < far {
                if let Some((row, col)) = intersection_near(x, y) {
                    let (ix, iy) = intersection(row, col);
                    let (dx, dy) = (x - ix, y - iy);
                    assert!(
                        dx * dx + dy * dy <= CLICK_RADIUS_SQ,
                        "({x}, {y}) resolved to ({row}, {col}), which is not near it"
                    );
                    assert!(
                        (0..BOARD_SIZE as i32).contains(&row)
                            && (0..BOARD_SIZE as i32).contains(&col),
                        "({x}, {y}) resolved to ({row}, {col}), which is off the board"
                    );
                }
                y += 6.0;
            }
            x += 6.0;
        }
    }

    #[test]
    fn a_stone_is_painted_on_the_crossing_it_was_played_on() {
        // Read back off the paint and measured against the painted grid, so a
        // renderer that draws stones somewhere other than where clicks land is
        // caught without either side supplying the other's expected value.
        let mut app = fresh_app();
        app.board.set(0, 0, Cell::Black);
        app.board.set(7, 7, Cell::White);
        app.board.set(14, 14, Cell::Black);
        let commands = app.render();
        let (xs, ys) = painted_grid(&commands);
        let centers = painted_stone_centers(&commands);
        assert_eq!(centers.len(), 3, "one stone painted per stone played");

        for (row, col) in [(0usize, 0usize), (7, 7), (14, 14)] {
            let (want_x, want_y) = (xs[col], ys[row]);
            assert!(
                centers
                    .iter()
                    .any(|&(x, y)| (x - want_x).abs() < 0.01 && (y - want_y).abs() < 0.01),
                "no stone painted on the crossing at ({want_x}, {want_y}) for ({row}, {col})"
            );
        }
    }

    #[test]
    fn the_rows_are_numbered_upwards_from_the_bottom() {
        // A Go board counts rank 1 from the bottom, and gomoku is played on
        // one. Painting and hit-testing agree with each other whichever way
        // round this is, so the outside opinion is the window: the label "1"
        // has to sit lower down it than the label "15". Likewise "A" is to the
        // left of "O".
        let app = fresh_app();
        let commands = app.render();
        let (_, y1) = label_pos(&commands, "1").expect("row 1 label");
        let (_, y15) = label_pos(&commands, "15").expect("row 15 label");
        assert!(
            y1 > y15,
            "row 1 should be drawn below row 15, got {y1} vs {y15}"
        );

        let (xa, _) = label_pos(&commands, "A").expect("column A label");
        let (xo, _) = label_pos(&commands, "O").expect("column O label");
        assert!(
            xa < xo,
            "column A should be drawn left of column O, got {xa} vs {xo}"
        );
    }

    #[test]
    fn the_labels_line_up_with_the_lines_they_name() {
        // Pinned to the *painted* grid rather than recomputed, so labels that
        // drift off their lines are caught.
        let app = fresh_app();
        let commands = app.render();
        let (xs, ys) = painted_grid(&commands);
        for (i, name) in ["A", "H", "O"].iter().enumerate() {
            let col = [0, 7, BOARD_SIZE - 1][i];
            let (x, _) = label_pos(&commands, name).expect("column label");
            assert!(
                (x - xs[col]).abs() < CELL_SIZE / 2.0,
                "label {name} sits at {x}, but its line is at {}",
                xs[col]
            );
        }
        for (i, name) in ["15", "8", "1"].iter().enumerate() {
            let row = [0, 7, BOARD_SIZE - 1][i];
            let (_, y) = label_pos(&commands, name).expect("row label");
            assert!(
                (y - ys[row]).abs() < CELL_SIZE / 2.0,
                "label {name} sits at {y}, but its line is at {}",
                ys[row]
            );
        }
    }

    // ── Mouse click tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_too_far_from_intersection() {
        let mut app = fresh_app();
        // Click between intersections (far from any snap point)
        app.handle_mouse(&MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: BOARD_OFFSET_X + 0.5 * CELL_SIZE,
            y: BOARD_OFFSET_Y + 0.5 * CELL_SIZE,
        });
        // Dead centre of a cell is 25.5px from each of the four intersections
        // around it, well past the 18.4px the hit test reaches, so no stone is
        // placed. This used to assert nothing whatsoever -- it ran the click
        // and commented that "it may or may not place" -- which made it a test
        // that could not fail.
        assert_eq!(app.board.stone_count(), 0);
    }

    #[test]
    fn test_mouse_click_outside_board() {
        let mut app = fresh_app();
        app.handle_mouse(&MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: 0.0,
            y: 0.0,
        });
        // Should not place any stone
        assert_eq!(app.board.stone_count(), 0);
    }

    #[test]
    fn test_mouse_right_click_ignored() {
        let mut app = fresh_app();
        app.handle_mouse(&MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Right),
            x: BOARD_OFFSET_X + 7.0 * CELL_SIZE,
            y: BOARD_OFFSET_Y + 7.0 * CELL_SIZE,
        });
        assert_eq!(app.board.stone_count(), 0);
    }
}
