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

//! Slate OS Connect Four — classic two-player connection game with AI opponent.
//!
//! Features a 7-column by 6-row grid, gravity-based piece dropping,
//! alternating Red/Yellow turns, four-in-a-row win detection (horizontal,
//! vertical, and both diagonals), draw detection, minimax AI with alpha-beta
//! pruning at configurable depth, win line highlighting, and a
//! Catppuccin Mocha themed board. Controls: Left/Right to select column,
//! Enter/Space to drop a piece, N for new game, Escape to quit.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};

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

// ── Layout constants ────────────────────────────────────────────────
const COLS: usize = 7;
const ROWS: usize = 6;
const CELL_SIZE: f32 = 72.0;
const CELL_GAP: f32 = 4.0;
const BOARD_PADDING: f32 = 16.0;
const BOARD_OFFSET_X: f32 = 40.0;
const BOARD_OFFSET_Y: f32 = 90.0;
const PIECE_RADIUS: f32 = 28.0;
const TITLE_FONT_SIZE: f32 = 24.0;
const INFO_FONT_SIZE: f32 = 16.0;
const INDICATOR_SIZE: f32 = 20.0;
const CORNER_RADIUS: f32 = 8.0;
const CELL_CORNER_RADIUS: f32 = 4.0;
const WIN_LINE_WIDTH: f32 = 4.0;

// ── AI constants ────────────────────────────────────────────────────
const AI_DEPTH: i32 = 6;

// ── Score constants for minimax evaluation ──────────────────────────
const SCORE_WIN: i32 = 1_000_000;
const SCORE_THREE: i32 = 100;
const SCORE_TWO: i32 = 10;
const SCORE_CENTER: i32 = 6;
const SCORE_OPP_THREE: i32 = -80;

// ── Cell and player types ───────────────────────────────────────────

/// Represents the contents of a single board cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    Red,
    Yellow,
}

impl Cell {
    /// Returns the opponent's cell color, or `Empty` if called on `Empty`.
    fn opponent(self) -> Self {
        match self {
            Self::Red => Self::Yellow,
            Self::Yellow => Self::Red,
            Self::Empty => Self::Empty,
        }
    }

    /// Returns the display color for this cell type.
    fn color(self) -> Color {
        match self {
            Self::Red => RED,
            Self::Yellow => YELLOW,
            Self::Empty => SURFACE0,
        }
    }
}

// ── Game status ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameStatus {
    /// Game is in progress.
    Playing,
    /// A player has won. Contains the winning player.
    Won(Cell),
    /// The board is full with no winner.
    Draw,
}

// ── Win line ────────────────────────────────────────────────────────

/// Coordinates of the four cells forming a winning line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WinLine {
    cells: [(usize, usize); 4],
}

// ── Board ───────────────────────────────────────────────────────────

/// The 7x6 Connect Four board. Column 0 is leftmost, row 0 is the bottom.
#[derive(Debug, Clone)]
struct Board {
    /// Grid indexed as `grid[row][col]`, row 0 = bottom.
    grid: [[Cell; COLS]; ROWS],
    /// Number of pieces in each column (also the index of the next free row).
    heights: [usize; COLS],
    /// Total number of pieces on the board.
    piece_count: usize,
}

impl Board {
    /// Creates a new empty board.
    fn new() -> Self {
        Self {
            grid: [[Cell::Empty; COLS]; ROWS],
            heights: [0; COLS],
            piece_count: 0,
        }
    }

    /// Returns `true` if the given column can accept another piece.
    fn can_drop(&self, col: usize) -> bool {
        col < COLS && self.heights[col] < ROWS
    }

    /// Drops a piece into the given column. Returns the row it landed in,
    /// or `None` if the column is full.
    fn drop_piece(&mut self, col: usize, piece: Cell) -> Option<usize> {
        if !self.can_drop(col) {
            return None;
        }
        let row = self.heights[col];
        self.grid[row][col] = piece;
        self.heights[col] = row + 1;
        self.piece_count += 1;
        Some(row)
    }

    /// Removes the top piece from the given column (for AI undo). Returns
    /// the cell that was removed, or `None` if the column is empty.
    fn undo_drop(&mut self, col: usize) -> Option<Cell> {
        if self.heights[col] == 0 {
            return None;
        }
        self.heights[col] -= 1;
        let row = self.heights[col];
        let cell = self.grid[row][col];
        self.grid[row][col] = Cell::Empty;
        self.piece_count -= 1;
        Some(cell)
    }

    /// Returns the cell at the given (row, col).
    fn get(&self, row: usize, col: usize) -> Cell {
        if row < ROWS && col < COLS {
            self.grid[row][col]
        } else {
            Cell::Empty
        }
    }

    /// Returns `true` if every column is full.
    fn is_full(&self) -> bool {
        self.piece_count >= COLS * ROWS
    }

    /// Checks for a four-in-a-row starting at (row, col) in a given
    /// direction (dr, dc). Returns the winning line if found.
    fn check_line(
        &self,
        row: usize,
        col: usize,
        dr: isize,
        dc: isize,
    ) -> Option<WinLine> {
        let piece = self.grid[row][col];
        if piece == Cell::Empty {
            return None;
        }
        let mut cells = [(row, col); 4];
        // i is used both as a multiplier on (dr, dc) and as the write index
        // into `cells`; iter_mut would force a parallel counter.
        #[allow(clippy::needless_range_loop)]
        for i in 1..4 {
            let nr = row as isize + dr * i as isize;
            let nc = col as isize + dc * i as isize;
            if nr < 0 || nr >= ROWS as isize || nc < 0 || nc >= COLS as isize {
                return None;
            }
            let nr = nr as usize;
            let nc = nc as usize;
            if self.grid[nr][nc] != piece {
                return None;
            }
            cells[i] = (nr, nc);
        }
        Some(WinLine { cells })
    }

    /// Scans the entire board for a four-in-a-row. Returns the winner
    /// and the winning line if found.
    fn find_winner(&self) -> Option<(Cell, WinLine)> {
        for row in 0..ROWS {
            for col in 0..COLS {
                if self.grid[row][col] == Cell::Empty {
                    continue;
                }
                // Right
                if col + 3 < COLS
                    && let Some(line) = self.check_line(row, col, 0, 1) {
                        return Some((self.grid[row][col], line));
                    }
                // Up
                if row + 3 < ROWS
                    && let Some(line) = self.check_line(row, col, 1, 0) {
                        return Some((self.grid[row][col], line));
                    }
                // Up-right diagonal
                if row + 3 < ROWS && col + 3 < COLS
                    && let Some(line) = self.check_line(row, col, 1, 1) {
                        return Some((self.grid[row][col], line));
                    }
                // Up-left diagonal
                if row + 3 < ROWS && col >= 3
                    && let Some(line) = self.check_line(row, col, 1, -1) {
                        return Some((self.grid[row][col], line));
                    }
            }
        }
        None
    }

    /// Returns the current game status by checking for winners and draws.
    fn status(&self) -> GameStatus {
        if let Some((winner, _)) = self.find_winner() {
            GameStatus::Won(winner)
        } else if self.is_full() {
            GameStatus::Draw
        } else {
            GameStatus::Playing
        }
    }

    /// Quick check: does the given player have four in a row?
    /// More efficient than `find_winner` when we only need a boolean.
    fn has_won(&self, player: Cell) -> bool {
        // Horizontal
        for row in 0..ROWS {
            for col in 0..COLS.saturating_sub(3) {
                if self.grid[row][col] == player
                    && self.grid[row][col + 1] == player
                    && self.grid[row][col + 2] == player
                    && self.grid[row][col + 3] == player
                {
                    return true;
                }
            }
        }
        // Vertical
        for col in 0..COLS {
            for row in 0..ROWS.saturating_sub(3) {
                if self.grid[row][col] == player
                    && self.grid[row + 1][col] == player
                    && self.grid[row + 2][col] == player
                    && self.grid[row + 3][col] == player
                {
                    return true;
                }
            }
        }
        // Diagonal up-right
        for row in 0..ROWS.saturating_sub(3) {
            for col in 0..COLS.saturating_sub(3) {
                if self.grid[row][col] == player
                    && self.grid[row + 1][col + 1] == player
                    && self.grid[row + 2][col + 2] == player
                    && self.grid[row + 3][col + 3] == player
                {
                    return true;
                }
            }
        }
        // Diagonal up-left
        for row in 0..ROWS.saturating_sub(3) {
            for col in 3..COLS {
                if self.grid[row][col] == player
                    && self.grid[row + 1][col - 1] == player
                    && self.grid[row + 2][col - 2] == player
                    && self.grid[row + 3][col - 3] == player
                {
                    return true;
                }
            }
        }
        false
    }

    /// Returns a list of columns that can accept pieces, ordered from the
    /// center outward (better move ordering for alpha-beta pruning).
    fn valid_moves(&self) -> Vec<usize> {
        // Center-first ordering: 3, 2, 4, 1, 5, 0, 6
        const ORDER: [usize; COLS] = [3, 2, 4, 1, 5, 0, 6];
        ORDER.iter().copied().filter(|&c| self.can_drop(c)).collect()
    }
}

// ── AI: Minimax with alpha-beta pruning ─────────────────────────────

/// Evaluates a window of 4 cells for scoring.
fn evaluate_window(window: &[Cell; 4], player: Cell) -> i32 {
    let opp = player.opponent();
    let player_count = window.iter().filter(|&&c| c == player).count();
    let opp_count = window.iter().filter(|&&c| c == opp).count();
    let empty_count = window.iter().filter(|&&c| c == Cell::Empty).count();

    if player_count == 4 {
        SCORE_WIN
    } else if player_count == 3 && empty_count == 1 {
        SCORE_THREE
    } else if player_count == 2 && empty_count == 2 {
        SCORE_TWO
    } else if opp_count == 3 && empty_count == 1 {
        SCORE_OPP_THREE
    } else {
        0
    }
}

/// Evaluates the entire board position from the perspective of `player`.
fn evaluate_board(board: &Board, player: Cell) -> i32 {
    let mut score: i32 = 0;

    // Center column preference
    let center_col = COLS / 2;
    for row in 0..ROWS {
        if board.grid[row][center_col] == player {
            score += SCORE_CENTER;
        }
    }

    // Horizontal windows
    for row in 0..ROWS {
        for col in 0..COLS.saturating_sub(3) {
            let window = [
                board.grid[row][col],
                board.grid[row][col + 1],
                board.grid[row][col + 2],
                board.grid[row][col + 3],
            ];
            score += evaluate_window(&window, player);
        }
    }

    // Vertical windows
    for col in 0..COLS {
        for row in 0..ROWS.saturating_sub(3) {
            let window = [
                board.grid[row][col],
                board.grid[row + 1][col],
                board.grid[row + 2][col],
                board.grid[row + 3][col],
            ];
            score += evaluate_window(&window, player);
        }
    }

    // Diagonal up-right
    for row in 0..ROWS.saturating_sub(3) {
        for col in 0..COLS.saturating_sub(3) {
            let window = [
                board.grid[row][col],
                board.grid[row + 1][col + 1],
                board.grid[row + 2][col + 2],
                board.grid[row + 3][col + 3],
            ];
            score += evaluate_window(&window, player);
        }
    }

    // Diagonal up-left
    for row in 0..ROWS.saturating_sub(3) {
        for col in 3..COLS {
            let window = [
                board.grid[row][col],
                board.grid[row + 1][col - 1],
                board.grid[row + 2][col - 2],
                board.grid[row + 3][col - 3],
            ];
            score += evaluate_window(&window, player);
        }
    }

    score
}

/// Returns `true` if the position is terminal (someone won or board is full).
fn is_terminal(board: &Board) -> bool {
    board.has_won(Cell::Red) || board.has_won(Cell::Yellow) || board.is_full()
}

/// Minimax with alpha-beta pruning. Returns (score, best_column).
/// `maximizing` is `true` when it is the AI player's turn.
fn minimax(
    board: &mut Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
    ai_player: Cell,
) -> (i32, Option<usize>) {
    if depth == 0 || is_terminal(board) {
        if board.has_won(ai_player) {
            return (SCORE_WIN + depth, None);
        }
        if board.has_won(ai_player.opponent()) {
            return (-SCORE_WIN - depth, None);
        }
        if board.is_full() {
            return (0, None);
        }
        return (evaluate_board(board, ai_player), None);
    }

    let moves = board.valid_moves();
    if moves.is_empty() {
        return (0, None);
    }

    if maximizing {
        let mut best_score = i32::MIN;
        let mut best_col = moves[0];
        for &col in &moves {
            let current = ai_player;
            board.drop_piece(col, current);
            let (score, _) = minimax(board, depth - 1, alpha, beta, false, ai_player);
            board.undo_drop(col);
            if score > best_score {
                best_score = score;
                best_col = col;
            }
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        (best_score, Some(best_col))
    } else {
        let mut best_score = i32::MAX;
        let mut best_col = moves[0];
        for &col in &moves {
            let current = ai_player.opponent();
            board.drop_piece(col, current);
            let (score, _) = minimax(board, depth - 1, alpha, beta, true, ai_player);
            board.undo_drop(col);
            if score < best_score {
                best_score = score;
                best_col = col;
            }
            beta = beta.min(score);
            if alpha >= beta {
                break;
            }
        }
        (best_score, Some(best_col))
    }
}

/// Finds the best move for the AI player using minimax.
fn ai_best_move(board: &mut Board, ai_player: Cell, depth: i32) -> Option<usize> {
    // Check for immediate winning move first
    for &col in &board.valid_moves() {
        board.drop_piece(col, ai_player);
        let wins = board.has_won(ai_player);
        board.undo_drop(col);
        if wins {
            return Some(col);
        }
    }

    // Check for immediate block needed
    let opp = ai_player.opponent();
    for &col in &board.valid_moves() {
        board.drop_piece(col, opp);
        let wins = board.has_won(opp);
        board.undo_drop(col);
        if wins {
            return Some(col);
        }
    }

    // Run minimax
    let (_, best_col) = minimax(board, depth, i32::MIN, i32::MAX, true, ai_player);
    best_col
}

// ── Main application ────────────────────────────────────────────────

/// The Connect Four application state.
struct Connect4App {
    board: Board,
    /// Which column the cursor is currently over.
    cursor_col: usize,
    /// Whose turn it is.
    current_player: Cell,
    /// The current game status.
    status: GameStatus,
    /// The winning line, if any.
    win_line: Option<WinLine>,
    /// Human plays Red, AI plays Yellow.
    human_player: Cell,
    ai_player: Cell,
    /// AI search depth.
    ai_depth: i32,
    /// Number of games won by human.
    human_wins: u32,
    /// Number of games won by AI.
    ai_wins: u32,
    /// Number of draws.
    draws: u32,
    /// History of moves: list of (column, player) pairs.
    move_history: Vec<(usize, Cell)>,
}

impl Connect4App {
    /// Creates a new Connect Four game.
    fn new() -> Self {
        Self {
            board: Board::new(),
            cursor_col: COLS / 2,
            current_player: Cell::Red,
            status: GameStatus::Playing,
            win_line: None,
            human_player: Cell::Red,
            ai_player: Cell::Yellow,
            ai_depth: AI_DEPTH,
            human_wins: 0,
            ai_wins: 0,
            draws: 0,
            move_history: Vec::new(),
        }
    }

    /// Resets the board for a new game.
    fn new_game(&mut self) {
        self.board = Board::new();
        self.cursor_col = COLS / 2;
        self.current_player = Cell::Red;
        self.status = GameStatus::Playing;
        self.win_line = None;
        self.move_history.clear();
    }

    /// Moves the cursor left.
    fn move_cursor_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    /// Moves the cursor right.
    fn move_cursor_right(&mut self) {
        if self.cursor_col + 1 < COLS {
            self.cursor_col += 1;
        }
    }

    /// Attempts to drop a piece in the current cursor column.
    /// Returns `true` if the drop succeeded.
    fn drop_current(&mut self) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        if self.current_player != self.human_player {
            return false;
        }
        self.drop_at(self.cursor_col)
    }

    /// Drops a piece at the given column for the current player.
    /// Checks for win/draw after the drop. Returns `true` on success.
    fn drop_at(&mut self, col: usize) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        if !self.board.can_drop(col) {
            return false;
        }
        let player = self.current_player;
        self.board.drop_piece(col, player);
        self.move_history.push((col, player));

        // Check for win
        if let Some((_, line)) = self.board.find_winner() {
            self.status = GameStatus::Won(player);
            self.win_line = Some(line);
            match player {
                Cell::Red => {
                    if player == self.human_player {
                        self.human_wins += 1;
                    } else {
                        self.ai_wins += 1;
                    }
                }
                Cell::Yellow => {
                    if player == self.human_player {
                        self.human_wins += 1;
                    } else {
                        self.ai_wins += 1;
                    }
                }
                Cell::Empty => {}
            }
            return true;
        }

        // Check for draw
        if self.board.is_full() {
            self.status = GameStatus::Draw;
            self.draws += 1;
            return true;
        }

        // Switch turns
        self.current_player = self.current_player.opponent();
        true
    }

    /// Runs the AI turn. Returns the column the AI chose, or `None` if
    /// it is not the AI's turn or the game is over.
    fn ai_turn(&mut self) -> Option<usize> {
        if self.status != GameStatus::Playing {
            return None;
        }
        if self.current_player != self.ai_player {
            return None;
        }
        let col = ai_best_move(&mut self.board, self.ai_player, self.ai_depth)?;
        // Temporarily set current_player for drop_at
        self.drop_at(col);
        Some(col)
    }

    /// Handles a key event. Returns `true` if the event was consumed.
    fn handle_key(&mut self, key: Key) -> bool {
        match key {
            Key::Left => {
                self.move_cursor_left();
                true
            }
            Key::Right => {
                self.move_cursor_right();
                true
            }
            Key::Enter | Key::Space => {
                if self.drop_current() {
                    // After human move, run AI if game is still going
                    if self.status == GameStatus::Playing
                        && self.current_player == self.ai_player
                    {
                        self.ai_turn();
                    }
                }
                true
            }
            Key::N => {
                self.new_game();
                true
            }
            _ => false,
        }
    }

    /// Returns the pixel center of a board cell (col, row) for rendering.
    /// Row 0 is the bottom row, but we render it at the bottom of the screen.
    fn cell_center(col: usize, row: usize) -> (f32, f32) {
        let x = BOARD_OFFSET_X
            + BOARD_PADDING
            + col as f32 * (CELL_SIZE + CELL_GAP)
            + CELL_SIZE / 2.0;
        // Row 0 is bottom, so we flip: row 0 maps to the last visual row
        let visual_row = ROWS - 1 - row;
        let y = BOARD_OFFSET_Y
            + BOARD_PADDING
            + visual_row as f32 * (CELL_SIZE + CELL_GAP)
            + CELL_SIZE / 2.0;
        (x, y)
    }

    /// Generates all render commands for the current state.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let board_width =
            BOARD_PADDING * 2.0 + COLS as f32 * CELL_SIZE + (COLS - 1) as f32 * CELL_GAP;
        let board_height =
            BOARD_PADDING * 2.0 + ROWS as f32 * CELL_SIZE + (ROWS - 1) as f32 * CELL_GAP;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: board_width + BOARD_OFFSET_X * 2.0,
            height: board_height + BOARD_OFFSET_Y + 100.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 20.0,
            text: String::from("Connect Four"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score line
        let score_text = format!(
            "You: {}  AI: {}  Draws: {}",
            self.human_wins, self.ai_wins, self.draws
        );
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 50.0,
            text: score_text,
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Status text
        let status_text = match self.status {
            GameStatus::Playing => {
                if self.current_player == self.human_player {
                    String::from("Your turn (Red)")
                } else {
                    String::from("AI thinking...")
                }
            }
            GameStatus::Won(winner) => {
                if winner == self.human_player {
                    String::from("You win!")
                } else {
                    String::from("AI wins!")
                }
            }
            GameStatus::Draw => String::from("Draw!"),
        };
        let status_color = match self.status {
            GameStatus::Playing => BLUE,
            GameStatus::Won(w) if w == self.human_player => GREEN,
            GameStatus::Won(_) => RED,
            GameStatus::Draw => PEACH,
        };
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X + board_width + 20.0,
            y: 20.0,
            text: status_text,
            color: status_color,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Column cursor indicator (arrow above the selected column)
        if self.status == GameStatus::Playing && self.current_player == self.human_player {
            let (cx, _) = Self::cell_center(self.cursor_col, ROWS - 1);
            let indicator_y = BOARD_OFFSET_Y - 20.0;
            cmds.push(RenderCommand::FillRect {
                x: cx - INDICATOR_SIZE / 2.0,
                y: indicator_y,
                width: INDICATOR_SIZE,
                height: INDICATOR_SIZE,
                color: self.current_player.color(),
                corner_radii: CornerRadii::all(INDICATOR_SIZE / 2.0),
            });
        }

        // Board background
        cmds.push(RenderCommand::FillRect {
            x: BOARD_OFFSET_X,
            y: BOARD_OFFSET_Y,
            width: board_width,
            height: board_height,
            color: BLUE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Cells
        for row in 0..ROWS {
            for col in 0..COLS {
                let (cx, cy) = Self::cell_center(col, row);
                let cell = self.board.get(row, col);
                let cell_color = cell.color();

                // Circle approximated by rounded rect
                cmds.push(RenderCommand::FillRect {
                    x: cx - PIECE_RADIUS,
                    y: cy - PIECE_RADIUS,
                    width: PIECE_RADIUS * 2.0,
                    height: PIECE_RADIUS * 2.0,
                    color: cell_color,
                    corner_radii: CornerRadii::all(PIECE_RADIUS),
                });
            }
        }

        // Win line highlight
        if let Some(ref line) = self.win_line {
            for &(row, col) in &line.cells {
                let (cx, cy) = Self::cell_center(col, row);
                cmds.push(RenderCommand::StrokeRect {
                    x: cx - PIECE_RADIUS - 2.0,
                    y: cy - PIECE_RADIUS - 2.0,
                    width: PIECE_RADIUS * 2.0 + 4.0,
                    height: PIECE_RADIUS * 2.0 + 4.0,
                    color: GREEN,
                    line_width: WIN_LINE_WIDTH,
                    corner_radii: CornerRadii::all(PIECE_RADIUS + 2.0),
                });
            }
            // Draw lines connecting winning cells
            let (r0, c0) = line.cells[0];
            let (r3, c3) = line.cells[3];
            let (x1, y1) = Self::cell_center(c0, r0);
            let (x2, y2) = Self::cell_center(c3, r3);
            cmds.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: GREEN,
                width: WIN_LINE_WIDTH,
            });
        }

        // Column numbers
        for col in 0..COLS {
            let (cx, _) = Self::cell_center(col, 0);
            let col_y = BOARD_OFFSET_Y + board_height + 8.0;
            cmds.push(RenderCommand::Text {
                x: cx - 4.0,
                y: col_y,
                text: format!("{}", col + 1),
                color: OVERLAY0,
                font_size: INFO_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help text at the bottom
        let help_y = BOARD_OFFSET_Y + board_height + 30.0;
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: help_y,
            text: String::from("Left/Right: select  Enter/Space: drop  N: new game"),
            color: OVERLAY0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Move history panel (right side)
        let panel_x = BOARD_OFFSET_X + board_width + 20.0;
        let panel_y = 50.0;
        cmds.push(RenderCommand::Text {
            x: panel_x,
            y: panel_y,
            text: String::from("Move History"),
            color: TEXT_COLOR,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let max_display = 20;
        let start = if self.move_history.len() > max_display {
            self.move_history.len() - max_display
        } else {
            0
        };
        for (i, &(col, player)) in self.move_history[start..].iter().enumerate() {
            let move_num = start + i + 1;
            let player_name = if player == Cell::Red { "R" } else { "Y" };
            let line = format!("{move_num}. {player_name} -> col {}", col + 1);
            let color = if player == Cell::Red { RED } else { YELLOW };
            cmds.push(RenderCommand::Text {
                x: panel_x,
                y: panel_y + 22.0 + i as f32 * 18.0,
                text: line,
                color,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

fn main() {
    let _app = Connect4App::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Board basic tests ───────────────────────────────────────────

    #[test]
    fn test_new_board_is_empty() {
        let board = Board::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                assert_eq!(board.get(row, col), Cell::Empty);
            }
        }
    }

    #[test]
    fn test_new_board_heights_zero() {
        let board = Board::new();
        for col in 0..COLS {
            assert_eq!(board.heights[col], 0);
        }
    }

    #[test]
    fn test_new_board_piece_count_zero() {
        let board = Board::new();
        assert_eq!(board.piece_count, 0);
    }

    #[test]
    fn test_can_drop_empty_board() {
        let board = Board::new();
        for col in 0..COLS {
            assert!(board.can_drop(col));
        }
    }

    #[test]
    fn test_can_drop_invalid_column() {
        let board = Board::new();
        assert!(!board.can_drop(COLS));
        assert!(!board.can_drop(COLS + 1));
    }

    #[test]
    fn test_drop_piece_returns_row() {
        let mut board = Board::new();
        assert_eq!(board.drop_piece(3, Cell::Red), Some(0));
        assert_eq!(board.drop_piece(3, Cell::Yellow), Some(1));
        assert_eq!(board.drop_piece(3, Cell::Red), Some(2));
    }

    #[test]
    fn test_drop_piece_updates_grid() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        assert_eq!(board.get(0, 0), Cell::Red);
        assert_eq!(board.get(1, 0), Cell::Empty);
    }

    #[test]
    fn test_drop_piece_updates_height() {
        let mut board = Board::new();
        board.drop_piece(2, Cell::Red);
        assert_eq!(board.heights[2], 1);
        board.drop_piece(2, Cell::Yellow);
        assert_eq!(board.heights[2], 2);
    }

    #[test]
    fn test_drop_piece_updates_count() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        assert_eq!(board.piece_count, 1);
        board.drop_piece(1, Cell::Yellow);
        assert_eq!(board.piece_count, 2);
    }

    #[test]
    fn test_drop_full_column() {
        let mut board = Board::new();
        for i in 0..ROWS {
            let piece = if i % 2 == 0 { Cell::Red } else { Cell::Yellow };
            assert!(board.drop_piece(0, piece).is_some());
        }
        assert!(!board.can_drop(0));
        assert_eq!(board.drop_piece(0, Cell::Red), None);
    }

    #[test]
    fn test_undo_drop_basic() {
        let mut board = Board::new();
        board.drop_piece(3, Cell::Red);
        assert_eq!(board.get(0, 3), Cell::Red);
        let removed = board.undo_drop(3);
        assert_eq!(removed, Some(Cell::Red));
        assert_eq!(board.get(0, 3), Cell::Empty);
        assert_eq!(board.heights[3], 0);
        assert_eq!(board.piece_count, 0);
    }

    #[test]
    fn test_undo_drop_empty_column() {
        let mut board = Board::new();
        assert_eq!(board.undo_drop(0), None);
    }

    #[test]
    fn test_undo_drop_multiple() {
        let mut board = Board::new();
        board.drop_piece(2, Cell::Red);
        board.drop_piece(2, Cell::Yellow);
        board.drop_piece(2, Cell::Red);

        assert_eq!(board.undo_drop(2), Some(Cell::Red));
        assert_eq!(board.heights[2], 2);
        assert_eq!(board.undo_drop(2), Some(Cell::Yellow));
        assert_eq!(board.heights[2], 1);
        assert_eq!(board.undo_drop(2), Some(Cell::Red));
        assert_eq!(board.heights[2], 0);
    }

    #[test]
    fn test_get_out_of_bounds() {
        let board = Board::new();
        assert_eq!(board.get(ROWS, 0), Cell::Empty);
        assert_eq!(board.get(0, COLS), Cell::Empty);
        assert_eq!(board.get(100, 100), Cell::Empty);
    }

    #[test]
    fn test_is_full_empty_board() {
        let board = Board::new();
        assert!(!board.is_full());
    }

    #[test]
    fn test_is_full_full_board() {
        let mut board = Board::new();
        for col in 0..COLS {
            for row_idx in 0..ROWS {
                let piece = if (col + row_idx) % 2 == 0 {
                    Cell::Red
                } else {
                    Cell::Yellow
                };
                board.drop_piece(col, piece);
            }
        }
        assert!(board.is_full());
    }

    // ── Win detection tests ─────────────────────────────────────────

    #[test]
    fn test_horizontal_win_bottom_row() {
        let mut board = Board::new();
        for col in 0..4 {
            board.drop_piece(col, Cell::Red);
        }
        assert!(board.has_won(Cell::Red));
        assert!(!board.has_won(Cell::Yellow));
    }

    #[test]
    fn test_horizontal_win_middle() {
        let mut board = Board::new();
        // Fill bottom row first so we can place on second row
        for col in 2..6 {
            board.drop_piece(col, Cell::Yellow);
        }
        for col in 2..6 {
            board.drop_piece(col, Cell::Red);
        }
        assert!(board.has_won(Cell::Red));
    }

    #[test]
    fn test_horizontal_win_right_edge() {
        let mut board = Board::new();
        for col in 3..7 {
            board.drop_piece(col, Cell::Yellow);
        }
        assert!(board.has_won(Cell::Yellow));
    }

    #[test]
    fn test_vertical_win() {
        let mut board = Board::new();
        for _ in 0..4 {
            board.drop_piece(0, Cell::Red);
        }
        assert!(board.has_won(Cell::Red));
    }

    #[test]
    fn test_vertical_win_not_bottom() {
        let mut board = Board::new();
        // Put 2 yellow at bottom, then 4 red on top
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Yellow);
        for _ in 0..4 {
            board.drop_piece(3, Cell::Red);
        }
        assert!(board.has_won(Cell::Red));
    }

    #[test]
    fn test_diagonal_up_right_win() {
        let mut board = Board::new();
        // Build a diagonal: (0,0), (1,1), (2,2), (3,3)
        // Col 0: R
        board.drop_piece(0, Cell::Red);
        // Col 1: Y, R
        board.drop_piece(1, Cell::Yellow);
        board.drop_piece(1, Cell::Red);
        // Col 2: Y, Y, R
        board.drop_piece(2, Cell::Yellow);
        board.drop_piece(2, Cell::Yellow);
        board.drop_piece(2, Cell::Red);
        // Col 3: Y, Y, Y, R
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Red);

        assert!(board.has_won(Cell::Red));
    }

    #[test]
    fn test_diagonal_up_left_win() {
        let mut board = Board::new();
        // Build a diagonal: (0,6), (1,5), (2,4), (3,3)
        // Col 6: R
        board.drop_piece(6, Cell::Red);
        // Col 5: Y, R
        board.drop_piece(5, Cell::Yellow);
        board.drop_piece(5, Cell::Red);
        // Col 4: Y, Y, R
        board.drop_piece(4, Cell::Yellow);
        board.drop_piece(4, Cell::Yellow);
        board.drop_piece(4, Cell::Red);
        // Col 3: Y, Y, Y, R
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(3, Cell::Red);

        assert!(board.has_won(Cell::Red));
    }

    #[test]
    fn test_no_win_three_in_a_row() {
        let mut board = Board::new();
        for col in 0..3 {
            board.drop_piece(col, Cell::Red);
        }
        assert!(!board.has_won(Cell::Red));
    }

    #[test]
    fn test_no_win_interrupted_line() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        board.drop_piece(1, Cell::Red);
        board.drop_piece(2, Cell::Yellow);
        board.drop_piece(3, Cell::Red);
        assert!(!board.has_won(Cell::Red));
    }

    #[test]
    fn test_find_winner_returns_correct_line() {
        let mut board = Board::new();
        for col in 0..4 {
            board.drop_piece(col, Cell::Red);
        }
        let result = board.find_winner();
        assert!(result.is_some());
        let (winner, line) = result.unwrap();
        assert_eq!(winner, Cell::Red);
        // The line should contain the four bottom-left cells
        for &(row, col) in &line.cells {
            assert_eq!(row, 0);
            assert!(col < 4);
        }
    }

    #[test]
    fn test_find_winner_none_on_empty() {
        let board = Board::new();
        assert!(board.find_winner().is_none());
    }

    // ── Board status tests ──────────────────────────────────────────

    #[test]
    fn test_status_playing_empty() {
        let board = Board::new();
        assert_eq!(board.status(), GameStatus::Playing);
    }

    #[test]
    fn test_status_won() {
        let mut board = Board::new();
        for col in 0..4 {
            board.drop_piece(col, Cell::Red);
        }
        assert_eq!(board.status(), GameStatus::Won(Cell::Red));
    }

    #[test]
    fn test_status_draw() {
        let mut board = Board::new();
        // Fill the board without any four-in-a-row.
        // Pattern that avoids 4-in-a-row:
        // Columns filled alternating in groups of 3 to prevent horizontal wins
        let pattern = [
            // col 0       col 1       col 2       col 3       col 4       col 5       col 6
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
        ];
        board.grid = pattern;
        board.heights = [ROWS; COLS];
        board.piece_count = ROWS * COLS;

        // Verify no winner exists in this pattern
        assert!(board.find_winner().is_none());
        assert_eq!(board.status(), GameStatus::Draw);
    }

    // ── Cell tests ──────────────────────────────────────────────────

    #[test]
    fn test_cell_opponent() {
        assert_eq!(Cell::Red.opponent(), Cell::Yellow);
        assert_eq!(Cell::Yellow.opponent(), Cell::Red);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
    }

    #[test]
    fn test_cell_color() {
        assert_eq!(Cell::Red.color(), RED);
        assert_eq!(Cell::Yellow.color(), YELLOW);
        assert_eq!(Cell::Empty.color(), SURFACE0);
    }

    // ── Valid moves tests ───────────────────────────────────────────

    #[test]
    fn test_valid_moves_empty_board() {
        let board = Board::new();
        let moves = board.valid_moves();
        assert_eq!(moves.len(), COLS);
        // Should be center-first ordered
        assert_eq!(moves[0], 3);
    }

    #[test]
    fn test_valid_moves_full_column_excluded() {
        let mut board = Board::new();
        for _ in 0..ROWS {
            board.drop_piece(3, Cell::Red);
        }
        let moves = board.valid_moves();
        assert_eq!(moves.len(), COLS - 1);
        assert!(!moves.contains(&3));
    }

    #[test]
    fn test_valid_moves_empty_on_full_board() {
        let mut board = Board::new();
        for col in 0..COLS {
            for _ in 0..ROWS {
                board.drop_piece(col, Cell::Red);
            }
        }
        assert!(board.valid_moves().is_empty());
    }

    // ── AI evaluation tests ─────────────────────────────────────────

    #[test]
    fn test_evaluate_window_four_player() {
        let window = [Cell::Red, Cell::Red, Cell::Red, Cell::Red];
        assert_eq!(evaluate_window(&window, Cell::Red), SCORE_WIN);
    }

    #[test]
    fn test_evaluate_window_three_player_one_empty() {
        let window = [Cell::Red, Cell::Red, Cell::Red, Cell::Empty];
        assert_eq!(evaluate_window(&window, Cell::Red), SCORE_THREE);
    }

    #[test]
    fn test_evaluate_window_two_player_two_empty() {
        let window = [Cell::Red, Cell::Empty, Cell::Red, Cell::Empty];
        assert_eq!(evaluate_window(&window, Cell::Red), SCORE_TWO);
    }

    #[test]
    fn test_evaluate_window_opp_three_block() {
        let window = [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Empty];
        assert_eq!(evaluate_window(&window, Cell::Red), SCORE_OPP_THREE);
    }

    #[test]
    fn test_evaluate_window_mixed_no_score() {
        let window = [Cell::Red, Cell::Yellow, Cell::Red, Cell::Empty];
        assert_eq!(evaluate_window(&window, Cell::Red), 0);
    }

    #[test]
    fn test_evaluate_window_all_empty() {
        let window = [Cell::Empty, Cell::Empty, Cell::Empty, Cell::Empty];
        assert_eq!(evaluate_window(&window, Cell::Red), 0);
    }

    #[test]
    fn test_evaluate_board_center_preference() {
        let mut board1 = Board::new();
        board1.drop_piece(3, Cell::Red); // center
        let mut board2 = Board::new();
        board2.drop_piece(0, Cell::Red); // edge
        assert!(evaluate_board(&board1, Cell::Red) > evaluate_board(&board2, Cell::Red));
    }

    // ── AI behavior tests ───────────────────────────────────────────

    #[test]
    fn test_ai_blocks_horizontal_win() {
        let mut board = Board::new();
        // Human has three in a row, AI must block
        board.drop_piece(0, Cell::Red);
        board.drop_piece(1, Cell::Red);
        board.drop_piece(2, Cell::Red);
        // AI should play column 3 to block
        let col = ai_best_move(&mut board, Cell::Yellow, 4);
        assert_eq!(col, Some(3));
    }

    #[test]
    fn test_ai_blocks_vertical_win() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        board.drop_piece(0, Cell::Red);
        board.drop_piece(0, Cell::Red);
        // AI should play column 0 to block vertical
        let col = ai_best_move(&mut board, Cell::Yellow, 4);
        assert_eq!(col, Some(0));
    }

    #[test]
    fn test_ai_takes_winning_move() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Yellow);
        board.drop_piece(1, Cell::Yellow);
        board.drop_piece(2, Cell::Yellow);
        // AI should play column 3 to win
        let col = ai_best_move(&mut board, Cell::Yellow, 4);
        assert_eq!(col, Some(3));
    }

    #[test]
    fn test_ai_prefers_win_over_block() {
        let mut board = Board::new();
        // AI has 3 in a row on bottom
        board.drop_piece(0, Cell::Yellow);
        board.drop_piece(1, Cell::Yellow);
        board.drop_piece(2, Cell::Yellow);
        // Human also has 3 in a row on second row
        board.drop_piece(4, Cell::Red);
        board.drop_piece(5, Cell::Red);
        board.drop_piece(6, Cell::Red);
        // But col 3 finishes AI's win, AI should take the win
        let col = ai_best_move(&mut board, Cell::Yellow, 4);
        assert_eq!(col, Some(3));
    }

    #[test]
    fn test_ai_returns_some_on_non_full_board() {
        let mut board = Board::new();
        board.drop_piece(3, Cell::Red);
        let col = ai_best_move(&mut board, Cell::Yellow, 2);
        assert!(col.is_some());
    }

    #[test]
    fn test_is_terminal_win() {
        let mut board = Board::new();
        for col in 0..4 {
            board.drop_piece(col, Cell::Red);
        }
        assert!(is_terminal(&board));
    }

    #[test]
    fn test_is_terminal_draw() {
        let mut board = Board::new();
        let pattern = [
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
        ];
        board.grid = pattern;
        board.heights = [ROWS; COLS];
        board.piece_count = ROWS * COLS;
        assert!(is_terminal(&board));
    }

    #[test]
    fn test_is_terminal_playing() {
        let board = Board::new();
        assert!(!is_terminal(&board));
    }

    #[test]
    fn test_minimax_immediate_win() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        board.drop_piece(1, Cell::Red);
        board.drop_piece(2, Cell::Red);
        let (score, col) = minimax(&mut board, 1, i32::MIN, i32::MAX, true, Cell::Red);
        assert!(score > 0);
        assert_eq!(col, Some(3));
    }

    // ── App tests ───────────────────────────────────────────────────

    #[test]
    fn test_app_new() {
        let app = Connect4App::new();
        assert_eq!(app.current_player, Cell::Red);
        assert_eq!(app.status, GameStatus::Playing);
        assert!(app.win_line.is_none());
        assert_eq!(app.cursor_col, COLS / 2);
        assert_eq!(app.human_player, Cell::Red);
        assert_eq!(app.ai_player, Cell::Yellow);
    }

    #[test]
    fn test_app_new_game_resets() {
        let mut app = Connect4App::new();
        app.drop_at(0);
        app.drop_at(1);
        app.human_wins = 5;
        app.new_game();
        assert_eq!(app.board.piece_count, 0);
        assert_eq!(app.current_player, Cell::Red);
        assert_eq!(app.status, GameStatus::Playing);
        assert!(app.win_line.is_none());
        assert!(app.move_history.is_empty());
        // Score should persist across games
        assert_eq!(app.human_wins, 5);
    }

    #[test]
    fn test_app_move_cursor_left() {
        let mut app = Connect4App::new();
        app.cursor_col = 3;
        app.move_cursor_left();
        assert_eq!(app.cursor_col, 2);
    }

    #[test]
    fn test_app_move_cursor_left_at_zero() {
        let mut app = Connect4App::new();
        app.cursor_col = 0;
        app.move_cursor_left();
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_app_move_cursor_right() {
        let mut app = Connect4App::new();
        app.cursor_col = 3;
        app.move_cursor_right();
        assert_eq!(app.cursor_col, 4);
    }

    #[test]
    fn test_app_move_cursor_right_at_max() {
        let mut app = Connect4App::new();
        app.cursor_col = COLS - 1;
        app.move_cursor_right();
        assert_eq!(app.cursor_col, COLS - 1);
    }

    #[test]
    fn test_app_drop_current_places_piece() {
        let mut app = Connect4App::new();
        app.cursor_col = 2;
        assert!(app.drop_current());
        assert_eq!(app.board.get(0, 2), Cell::Red);
    }

    #[test]
    fn test_app_drop_current_switches_turn() {
        let mut app = Connect4App::new();
        app.cursor_col = 2;
        app.drop_current();
        // After human drops, AI should play too (since ai_turn is called by handle_key).
        // But drop_current alone just switches turns
        assert_eq!(app.current_player, Cell::Yellow);
    }

    #[test]
    fn test_app_drop_at_full_column_fails() {
        let mut app = Connect4App::new();
        for _ in 0..ROWS {
            app.board.drop_piece(0, Cell::Red);
        }
        app.board.heights[0] = ROWS;
        app.cursor_col = 0;
        assert!(!app.drop_at(0));
    }

    #[test]
    fn test_app_drop_when_not_playing_fails() {
        let mut app = Connect4App::new();
        app.status = GameStatus::Draw;
        assert!(!app.drop_current());
    }

    #[test]
    fn test_app_drop_records_move_history() {
        let mut app = Connect4App::new();
        app.drop_at(3);
        assert_eq!(app.move_history.len(), 1);
        assert_eq!(app.move_history[0], (3, Cell::Red));
    }

    #[test]
    fn test_app_drop_detects_win() {
        let mut app = Connect4App::new();
        // Set up a winning position for Red
        app.board.drop_piece(0, Cell::Red);
        app.board.heights[0] = 1;
        app.board.piece_count = 1;
        app.board.drop_piece(1, Cell::Red);
        app.board.heights[1] = 1;
        app.board.piece_count = 2;
        app.board.drop_piece(2, Cell::Red);
        app.board.heights[2] = 1;
        app.board.piece_count = 3;
        app.move_history.push((0, Cell::Red));
        app.move_history.push((1, Cell::Red));
        app.move_history.push((2, Cell::Red));

        // Drop the fourth piece
        app.drop_at(3);
        assert_eq!(app.status, GameStatus::Won(Cell::Red));
        assert!(app.win_line.is_some());
        assert_eq!(app.human_wins, 1);
    }

    #[test]
    fn test_app_ai_turn_only_when_ai_player() {
        let mut app = Connect4App::new();
        // Current player is Red (human), so AI should not play
        assert!(app.ai_turn().is_none());
    }

    #[test]
    fn test_app_ai_turn_plays() {
        let mut app = Connect4App::new();
        app.current_player = Cell::Yellow; // AI's turn
        let col = app.ai_turn();
        assert!(col.is_some());
    }

    #[test]
    fn test_app_ai_turn_not_when_game_over() {
        let mut app = Connect4App::new();
        app.status = GameStatus::Won(Cell::Red);
        app.current_player = Cell::Yellow;
        assert!(app.ai_turn().is_none());
    }

    #[test]
    fn test_app_handle_key_left() {
        let mut app = Connect4App::new();
        app.cursor_col = 4;
        assert!(app.handle_key(Key::Left));
        assert_eq!(app.cursor_col, 3);
    }

    #[test]
    fn test_app_handle_key_right() {
        let mut app = Connect4App::new();
        app.cursor_col = 2;
        assert!(app.handle_key(Key::Right));
        assert_eq!(app.cursor_col, 3);
    }

    #[test]
    fn test_app_handle_key_enter_drops() {
        let mut app = Connect4App::new();
        app.cursor_col = 0;
        app.handle_key(Key::Enter);
        assert_eq!(app.board.get(0, 0), Cell::Red);
    }

    #[test]
    fn test_app_handle_key_space_drops() {
        let mut app = Connect4App::new();
        app.cursor_col = 1;
        app.handle_key(Key::Space);
        assert_eq!(app.board.get(0, 1), Cell::Red);
    }

    #[test]
    fn test_app_handle_key_n_new_game() {
        let mut app = Connect4App::new();
        app.drop_at(0);
        app.handle_key(Key::N);
        assert_eq!(app.board.piece_count, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_app_handle_key_unknown() {
        let mut app = Connect4App::new();
        assert!(!app.handle_key(Key::A));
    }

    #[test]
    fn test_app_human_drop_triggers_ai() {
        let mut app = Connect4App::new();
        app.cursor_col = 3;
        app.handle_key(Key::Enter);
        // After human drops at col 3, AI should also have played
        // Board should have at least 2 pieces (human + AI)
        assert!(app.board.piece_count >= 2);
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = Connect4App::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_has_title() {
        let app = Connect4App::new();
        let cmds = app.render();
        let has_title = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Connect Four")
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_has_board_background() {
        let app = Connect4App::new();
        let cmds = app.render();
        let has_blue_board = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::FillRect { color, .. } if *color == BLUE)
        });
        assert!(has_blue_board);
    }

    #[test]
    fn test_render_has_cursor_when_playing() {
        let app = Connect4App::new();
        let cmds = app.render();
        // Should have a red indicator (cursor)
        let has_indicator = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::FillRect { color, .. } if *color == RED)
        });
        assert!(has_indicator);
    }

    #[test]
    fn test_render_no_cursor_when_game_over() {
        let mut app = Connect4App::new();
        app.status = GameStatus::Won(Cell::Red);
        let cmds = app.render();
        // The cursor indicator should not appear
        // Count RED fill rects - should only be from pieces, not cursor
        // (There may still be red pieces, so this is a weak test,
        // but the indicator specifically has INDICATOR_SIZE dimensions)
        let indicator_count = cmds.iter().filter(|cmd| {
            matches!(cmd, RenderCommand::FillRect { width, color, .. }
                if *color == RED && (*width - INDICATOR_SIZE).abs() < 0.01)
        }).count();
        assert_eq!(indicator_count, 0);
    }

    #[test]
    fn test_render_win_highlight() {
        let mut app = Connect4App::new();
        for col in 0..4 {
            app.board.drop_piece(col, Cell::Red);
        }
        app.status = GameStatus::Won(Cell::Red);
        app.win_line = Some(WinLine {
            cells: [(0, 0), (0, 1), (0, 2), (0, 3)],
        });
        let cmds = app.render();
        let stroke_count = cmds.iter().filter(|cmd| {
            matches!(cmd, RenderCommand::StrokeRect { color, .. } if *color == GREEN)
        }).count();
        assert_eq!(stroke_count, 4);
    }

    #[test]
    fn test_render_move_history() {
        let mut app = Connect4App::new();
        app.move_history.push((3, Cell::Red));
        app.move_history.push((2, Cell::Yellow));
        let cmds = app.render();
        let history_entries = cmds.iter().filter(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("->"))
        }).count();
        assert_eq!(history_entries, 2);
    }

    #[test]
    fn test_cell_center_bottom_left() {
        let (x, y) = Connect4App::cell_center(0, 0);
        // Row 0 is bottom, so visual row = ROWS - 1
        let expected_y = BOARD_OFFSET_Y
            + BOARD_PADDING
            + (ROWS - 1) as f32 * (CELL_SIZE + CELL_GAP)
            + CELL_SIZE / 2.0;
        let expected_x = BOARD_OFFSET_X + BOARD_PADDING + CELL_SIZE / 2.0;
        assert!((x - expected_x).abs() < 0.01);
        assert!((y - expected_y).abs() < 0.01);
    }

    #[test]
    fn test_cell_center_top_right() {
        let (x, y) = Connect4App::cell_center(COLS - 1, ROWS - 1);
        let expected_x = BOARD_OFFSET_X
            + BOARD_PADDING
            + (COLS - 1) as f32 * (CELL_SIZE + CELL_GAP)
            + CELL_SIZE / 2.0;
        let expected_y = BOARD_OFFSET_Y + BOARD_PADDING + CELL_SIZE / 2.0;
        assert!((x - expected_x).abs() < 0.01);
        assert!((y - expected_y).abs() < 0.01);
    }

    // ── Diagonal win variants ───────────────────────────────────────

    #[test]
    fn test_diagonal_win_detected_by_find_winner() {
        let mut board = Board::new();
        // Diagonal (0,0) (1,1) (2,2) (3,3)
        board.drop_piece(0, Cell::Yellow);
        board.drop_piece(1, Cell::Red);
        board.drop_piece(1, Cell::Yellow);
        board.drop_piece(2, Cell::Red);
        board.drop_piece(2, Cell::Red);
        board.drop_piece(2, Cell::Yellow);
        board.drop_piece(3, Cell::Red);
        board.drop_piece(3, Cell::Red);
        board.drop_piece(3, Cell::Red);
        board.drop_piece(3, Cell::Yellow);
        let result = board.find_winner();
        assert!(result.is_some());
        let (winner, _) = result.unwrap();
        assert_eq!(winner, Cell::Yellow);
    }

    #[test]
    fn test_vertical_win_detected_by_find_winner() {
        let mut board = Board::new();
        for _ in 0..4 {
            board.drop_piece(5, Cell::Yellow);
        }
        let result = board.find_winner();
        assert!(result.is_some());
        let (winner, line) = result.unwrap();
        assert_eq!(winner, Cell::Yellow);
        for &(_, col) in &line.cells {
            assert_eq!(col, 5);
        }
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_drop_and_undo_preserves_board() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        board.drop_piece(1, Cell::Yellow);
        let snapshot_grid = board.grid;
        let snapshot_heights = board.heights;
        let snapshot_count = board.piece_count;

        board.drop_piece(3, Cell::Red);
        board.undo_drop(3);

        assert_eq!(board.grid, snapshot_grid);
        assert_eq!(board.heights, snapshot_heights);
        assert_eq!(board.piece_count, snapshot_count);
    }

    #[test]
    fn test_multiple_drops_same_column() {
        let mut board = Board::new();
        let pieces = [Cell::Red, Cell::Yellow, Cell::Red, Cell::Yellow, Cell::Red, Cell::Yellow];
        for (i, &piece) in pieces.iter().enumerate() {
            let row = board.drop_piece(4, piece);
            assert_eq!(row, Some(i));
        }
        assert!(!board.can_drop(4));
    }

    #[test]
    fn test_check_line_at_boundary() {
        let mut board = Board::new();
        // Place 4 in a row at the right edge
        for col in 3..7 {
            board.drop_piece(col, Cell::Red);
        }
        let line = board.check_line(0, 3, 0, 1);
        assert!(line.is_some());
    }

    #[test]
    fn test_check_line_out_of_bounds() {
        let mut board = Board::new();
        board.drop_piece(6, Cell::Red);
        // Trying to check a line going right from col 6 should fail
        let line = board.check_line(0, 6, 0, 1);
        assert!(line.is_none());
    }

    #[test]
    fn test_check_line_on_empty_cell() {
        let board = Board::new();
        let line = board.check_line(0, 0, 0, 1);
        assert!(line.is_none());
    }

    #[test]
    fn test_app_win_increments_score_human() {
        let mut app = Connect4App::new();
        assert_eq!(app.human_wins, 0);
        // Set up human win
        app.board.drop_piece(0, Cell::Red);
        app.board.heights[0] = 1;
        app.board.piece_count = 1;
        app.board.drop_piece(1, Cell::Red);
        app.board.heights[1] = 1;
        app.board.piece_count = 2;
        app.board.drop_piece(2, Cell::Red);
        app.board.heights[2] = 1;
        app.board.piece_count = 3;
        app.drop_at(3);
        assert_eq!(app.human_wins, 1);
        assert_eq!(app.ai_wins, 0);
    }

    #[test]
    fn test_app_win_increments_score_ai() {
        let mut app = Connect4App::new();
        assert_eq!(app.ai_wins, 0);
        app.current_player = Cell::Yellow;
        app.board.drop_piece(0, Cell::Yellow);
        app.board.heights[0] = 1;
        app.board.piece_count = 1;
        app.board.drop_piece(1, Cell::Yellow);
        app.board.heights[1] = 1;
        app.board.piece_count = 2;
        app.board.drop_piece(2, Cell::Yellow);
        app.board.heights[2] = 1;
        app.board.piece_count = 3;
        app.drop_at(3);
        assert_eq!(app.ai_wins, 1);
        assert_eq!(app.human_wins, 0);
    }

    #[test]
    fn test_app_draw_increments_draws() {
        let mut app = Connect4App::new();
        let pattern = [
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red],
            [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow],
            [Cell::Red,    Cell::Red,    Cell::Red,    Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Yellow],
        ];
        // Fill all but one cell
        app.board.grid = pattern;
        // Leave top of last column open
        app.board.grid[ROWS - 1][COLS - 1] = Cell::Empty;
        for col in 0..COLS - 1 {
            app.board.heights[col] = ROWS;
        }
        app.board.heights[COLS - 1] = ROWS - 1;
        app.board.piece_count = ROWS * COLS - 1;
        // Make sure there is no winner yet
        assert!(app.board.find_winner().is_none());
        // Drop the last piece to fill the board
        app.cursor_col = COLS - 1;
        app.drop_at(COLS - 1);
        assert_eq!(app.status, GameStatus::Draw);
        assert_eq!(app.draws, 1);
    }

    #[test]
    fn test_ai_blocks_diagonal_threat() {
        let mut board = Board::new();
        // Set up a diagonal threat for Red: (0,0), (1,1), (2,2)
        // Need to fill support pieces
        board.grid[0][0] = Cell::Red;
        board.heights[0] = 1;
        board.piece_count = 1;

        board.grid[0][1] = Cell::Yellow;
        board.grid[1][1] = Cell::Red;
        board.heights[1] = 2;
        board.piece_count = 3;

        board.grid[0][2] = Cell::Yellow;
        board.grid[1][2] = Cell::Yellow;
        board.grid[2][2] = Cell::Red;
        board.heights[2] = 3;
        board.piece_count = 6;

        // Red needs (3,3) to win. Col 3 height is 0, so to block,
        // AI needs to fill col 3 up to row 3. But that means AI
        // should first play col 3 to start blocking.
        // Actually the direct block would require col 3 at row 3,
        // but we need supports. Let's simplify: AI just needs to
        // notice the threat.
        // Since filling col 3 to row 3 requires 3 pieces first,
        // let's fill them
        board.grid[0][3] = Cell::Yellow;
        board.grid[1][3] = Cell::Yellow;
        board.grid[2][3] = Cell::Yellow;
        board.heights[3] = 3;
        board.piece_count = 9;

        // Now Red threatens (3,3) for the diagonal win
        // AI (Yellow) should block by playing col 3 (which puts piece at row 3)
        let col = ai_best_move(&mut board, Cell::Yellow, 4);
        assert_eq!(col, Some(3));
    }

    #[test]
    fn test_evaluate_board_symmetry() {
        // An empty board should evaluate the same for both players
        let board = Board::new();
        let red_score = evaluate_board(&board, Cell::Red);
        let yellow_score = evaluate_board(&board, Cell::Yellow);
        assert_eq!(red_score, yellow_score);
    }

    #[test]
    fn test_app_drop_not_human_turn() {
        let mut app = Connect4App::new();
        app.current_player = Cell::Yellow; // AI's turn
        assert!(!app.drop_current()); // Human can't drop
    }

    #[test]
    fn test_board_clone_independence() {
        let mut board = Board::new();
        board.drop_piece(0, Cell::Red);
        let clone = board.clone();
        board.drop_piece(1, Cell::Yellow);
        // Clone should not be affected
        assert_eq!(clone.get(0, 1), Cell::Empty);
        assert_eq!(clone.piece_count, 1);
    }

    #[test]
    fn test_win_line_struct() {
        let line = WinLine {
            cells: [(0, 0), (0, 1), (0, 2), (0, 3)],
        };
        assert_eq!(line.cells[0], (0, 0));
        assert_eq!(line.cells[3], (0, 3));
    }

    #[test]
    fn test_game_status_variants() {
        assert_eq!(GameStatus::Playing, GameStatus::Playing);
        assert_eq!(GameStatus::Draw, GameStatus::Draw);
        assert_eq!(GameStatus::Won(Cell::Red), GameStatus::Won(Cell::Red));
        assert_ne!(GameStatus::Won(Cell::Red), GameStatus::Won(Cell::Yellow));
        assert_ne!(GameStatus::Playing, GameStatus::Draw);
    }

    #[test]
    fn test_render_column_numbers() {
        let app = Connect4App::new();
        let cmds = app.render();
        // Should have column number labels 1..7
        for num in 1..=7 {
            let num_str = format!("{num}");
            let has_col = cmds.iter().any(|cmd| {
                matches!(cmd, RenderCommand::Text { text, .. } if text == &num_str)
            });
            assert!(has_col, "Missing column number {num}");
        }
    }

    #[test]
    fn test_render_help_text() {
        let app = Connect4App::new();
        let cmds = app.render();
        let has_help = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Left/Right"))
        });
        assert!(has_help);
    }
}
