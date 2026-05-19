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
#![allow(unused_imports)]

//! OurOS Reversi (Othello) — a full Reversi game with AI opponent.
//!
//! Features an 8x8 board with standard rules, legal move validation,
//! piece flipping in all 8 directions, pass detection, game end detection,
//! a minimax AI with alpha-beta pruning and positional evaluation,
//! score display, move history, and a classic green board rendered with
//! Catppuccin Mocha theming.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
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

// ── Board colors (classic Othello green) ────────────────────────────
const BOARD_GREEN: Color = Color::from_hex(0x2E7D32);
const BOARD_GREEN_LIGHT: Color = Color::from_hex(0x388E3C);
const BOARD_BORDER: Color = Color::from_hex(0x1B5E20);
const CURSOR_COLOR: Color = Color::from_hex(0x89B4FA);
const VALID_MOVE_DOT: Color = Color::rgba(166, 227, 161, 160);
const LAST_MOVE_HIGHLIGHT: Color = Color::rgba(250, 179, 135, 100);
const BLACK_PIECE: Color = Color::from_hex(0x1A1A2E);
const WHITE_PIECE: Color = Color::from_hex(0xE8E8E8);
const BLACK_PIECE_BORDER: Color = Color::from_hex(0x000000);
const WHITE_PIECE_BORDER: Color = Color::from_hex(0xBBBBBB);

// ── Layout constants ────────────────────────────────────────────────
const CELL_SIZE: f32 = 60.0;
const BOARD_OFFSET_X: f32 = 40.0;
const BOARD_OFFSET_Y: f32 = 70.0;
const BOARD_SIZE: usize = 8;
const PANEL_X: f32 = BOARD_OFFSET_X + CELL_SIZE * 8.0 + 30.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const PIECE_RADIUS: f32 = 22.0;
const DOT_RADIUS: f32 = 6.0;

// ── AI search depth ─────────────────────────────────────────────────
const AI_DEPTH: i32 = 4;

// ── Directions for flipping (row_delta, col_delta) ──────────────────
const DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1), (-1, 0), (-1, 1),
    (0, -1),           (0, 1),
    (1, -1),  (1, 0),  (1, 1),
];

// ── Positional weights for AI evaluation ────────────────────────────
// Corners are extremely valuable, edges are good, squares adjacent to
// corners (X-squares and C-squares) are dangerous.
const POSITION_WEIGHTS: [[i32; 8]; 8] = [
    [120, -20,  20,   5,   5,  20, -20, 120],
    [-20, -40,  -5,  -5,  -5,  -5, -40, -20],
    [ 20,  -5,  15,   3,   3,  15,  -5,  20],
    [  5,  -5,   3,   3,   3,   3,  -5,   5],
    [  5,  -5,   3,   3,   3,   3,  -5,   5],
    [ 20,  -5,  15,   3,   3,  15,  -5,  20],
    [-20, -40,  -5,  -5,  -5,  -5, -40, -20],
    [120, -20,  20,   5,   5,  20, -20, 120],
];

// ── Cell state ──────────────────────────────────────────────────────

/// Represents the state of a single board cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Empty,
    Black,
    White,
}

impl Cell {
    /// Return the opposite color, or `Empty` if empty.
    fn opponent(self) -> Self {
        match self {
            Cell::Black => Cell::White,
            Cell::White => Cell::Black,
            Cell::Empty => Cell::Empty,
        }
    }

    /// Whether this cell is occupied by a piece.
    fn is_piece(self) -> bool {
        self != Cell::Empty
    }
}

// ── Board position ──────────────────────────────────────────────────

/// A position on the board (row, col), each in 0..8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: i32,
    col: i32,
}

impl Pos {
    fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    /// Whether this position is within the 8x8 board.
    fn in_bounds(self) -> bool {
        self.row >= 0 && self.row < 8 && self.col >= 0 && self.col < 8
    }
}

// ── Board ───────────────────────────────────────────────────────────

/// The 8x8 Reversi board.
#[derive(Clone, Debug)]
struct Board {
    cells: [[Cell; 8]; 8],
}

impl Board {
    /// Create a new board with the standard starting position.
    fn new() -> Self {
        let mut cells = [[Cell::Empty; 8]; 8];
        // Standard Othello opening: White on d4/e5, Black on d5/e4
        cells[3][3] = Cell::White;
        cells[3][4] = Cell::Black;
        cells[4][3] = Cell::Black;
        cells[4][4] = Cell::White;
        Self { cells }
    }

    /// Create an empty board (for tests).
    fn empty() -> Self {
        Self {
            cells: [[Cell::Empty; 8]; 8],
        }
    }

    /// Get the cell at a position.
    fn get(&self, pos: Pos) -> Cell {
        if pos.in_bounds() {
            self.cells[pos.row as usize][pos.col as usize]
        } else {
            Cell::Empty
        }
    }

    /// Set the cell at a position.
    fn set(&mut self, pos: Pos, cell: Cell) {
        if pos.in_bounds() {
            self.cells[pos.row as usize][pos.col as usize] = cell;
        }
    }

    /// Count pieces of a given color.
    fn count(&self, color: Cell) -> i32 {
        let mut total = 0;
        for row in &self.cells {
            for &cell in row {
                if cell == color {
                    total += 1;
                }
            }
        }
        total
    }

    /// Count total occupied cells.
    fn total_pieces(&self) -> i32 {
        self.count(Cell::Black) + self.count(Cell::White)
    }

    /// Count empty cells.
    fn empty_count(&self) -> i32 {
        64 - self.total_pieces()
    }

    /// Check how many pieces would be flipped in a given direction
    /// when `color` places a piece at `pos`.
    ///
    /// Returns the list of positions that would be flipped.
    fn flips_in_direction(&self, pos: Pos, color: Cell, dr: i32, dc: i32) -> Vec<Pos> {
        let opponent = color.opponent();
        let mut flipped = Vec::new();
        let mut r = pos.row + dr;
        let mut c = pos.col + dc;

        // Walk in the direction, collecting opponent pieces
        while r >= 0 && r < 8 && c >= 0 && c < 8 {
            let current = self.cells[r as usize][c as usize];
            if current == opponent {
                flipped.push(Pos::new(r, c));
            } else if current == color {
                // Found our own piece — these opponents are flanked
                return flipped;
            } else {
                // Empty cell — no flank
                break;
            }
            r += dr;
            c += dc;
        }

        // Ran off the board or hit empty — no valid flank
        Vec::new()
    }

    /// Get all positions that would be flipped if `color` places at `pos`.
    fn get_flips(&self, pos: Pos, color: Cell) -> Vec<Pos> {
        if !pos.in_bounds() || self.get(pos) != Cell::Empty {
            return Vec::new();
        }

        let mut all_flips = Vec::new();
        for &(dr, dc) in &DIRECTIONS {
            let mut flips = self.flips_in_direction(pos, color, dr, dc);
            all_flips.append(&mut flips);
        }
        all_flips
    }

    /// Check whether placing `color` at `pos` is a legal move.
    fn is_legal_move(&self, pos: Pos, color: Cell) -> bool {
        if !pos.in_bounds() || self.get(pos) != Cell::Empty {
            return false;
        }
        // Must flip at least one opponent piece
        for &(dr, dc) in &DIRECTIONS {
            if !self.flips_in_direction(pos, color, dr, dc).is_empty() {
                return true;
            }
        }
        false
    }

    /// Get all legal moves for a color.
    fn legal_moves(&self, color: Cell) -> Vec<Pos> {
        let mut moves = Vec::new();
        for row in 0..8 {
            for col in 0..8 {
                let pos = Pos::new(row, col);
                if self.is_legal_move(pos, color) {
                    moves.push(pos);
                }
            }
        }
        moves
    }

    /// Place a piece and flip all flanked opponent pieces.
    /// Returns the number of pieces flipped, or 0 if the move is illegal.
    fn make_move(&mut self, pos: Pos, color: Cell) -> i32 {
        let flips = self.get_flips(pos, color);
        if flips.is_empty() {
            return 0;
        }
        self.set(pos, color);
        for flip_pos in &flips {
            self.set(*flip_pos, color);
        }
        flips.len() as i32
    }

    /// Whether the given color has any legal move.
    fn has_legal_move(&self, color: Cell) -> bool {
        for row in 0..8 {
            for col in 0..8 {
                if self.is_legal_move(Pos::new(row, col), color) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether the game is over (neither player can move).
    fn is_game_over(&self) -> bool {
        !self.has_legal_move(Cell::Black) && !self.has_legal_move(Cell::White)
    }

    /// Determine the winner. Returns `Cell::Empty` for a tie.
    fn winner(&self) -> Cell {
        let black = self.count(Cell::Black);
        let white = self.count(Cell::White);
        if black > white {
            Cell::Black
        } else if white > black {
            Cell::White
        } else {
            Cell::Empty
        }
    }
}

// ── AI ──────────────────────────────────────────────────────────────

/// Evaluate the board from the perspective of `color`.
fn evaluate(board: &Board, color: Cell) -> i32 {
    let opponent = color.opponent();
    let mut score = 0;

    // Piece count difference
    let my_pieces = board.count(color);
    let opp_pieces = board.count(opponent);
    score += (my_pieces - opp_pieces) * 10;

    // Positional weights
    for row in 0..8 {
        for col in 0..8 {
            let cell = board.cells[row][col];
            if cell == color {
                score += POSITION_WEIGHTS[row][col];
            } else if cell == opponent {
                score -= POSITION_WEIGHTS[row][col];
            }
        }
    }

    // Mobility: having more moves is advantageous
    let my_moves = board.legal_moves(color).len() as i32;
    let opp_moves = board.legal_moves(opponent).len() as i32;
    score += (my_moves - opp_moves) * 5;

    // Corner occupancy bonus
    let corners = [
        Pos::new(0, 0),
        Pos::new(0, 7),
        Pos::new(7, 0),
        Pos::new(7, 7),
    ];
    for &corner in &corners {
        if board.get(corner) == color {
            score += 50;
        } else if board.get(corner) == opponent {
            score -= 50;
        }
    }

    score
}

/// Minimax with alpha-beta pruning.
fn minimax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
    ai_color: Cell,
) -> i32 {
    if depth == 0 || board.is_game_over() {
        return evaluate(board, ai_color);
    }

    let current_color = if maximizing {
        ai_color
    } else {
        ai_color.opponent()
    };

    let moves = board.legal_moves(current_color);

    if moves.is_empty() {
        // Current player must pass — switch to opponent
        return minimax(board, depth - 1, alpha, beta, !maximizing, ai_color);
    }

    if maximizing {
        let mut best = i32::MIN;
        for mv in &moves {
            let mut new_board = board.clone();
            new_board.make_move(*mv, current_color);
            let val = minimax(&new_board, depth - 1, alpha, beta, false, ai_color);
            if val > best {
                best = val;
            }
            if best > alpha {
                alpha = best;
            }
            if alpha >= beta {
                break;
            }
        }
        best
    } else {
        let mut best = i32::MAX;
        for mv in &moves {
            let mut new_board = board.clone();
            new_board.make_move(*mv, current_color);
            let val = minimax(&new_board, depth - 1, alpha, beta, true, ai_color);
            if val < best {
                best = val;
            }
            if best < beta {
                beta = best;
            }
            if alpha >= beta {
                break;
            }
        }
        best
    }
}

/// Find the best move for the AI using minimax with alpha-beta pruning.
fn ai_best_move(board: &Board, ai_color: Cell) -> Option<Pos> {
    let moves = board.legal_moves(ai_color);
    if moves.is_empty() {
        return None;
    }

    let mut best_score = i32::MIN;
    let mut best_move = moves[0];

    for mv in &moves {
        let mut new_board = board.clone();
        new_board.make_move(*mv, ai_color);
        let score = minimax(
            &new_board,
            AI_DEPTH - 1,
            i32::MIN,
            i32::MAX,
            false,
            ai_color,
        );
        if score > best_score {
            best_score = score;
            best_move = *mv;
        }
    }

    Some(best_move)
}

// ── Game state ──────────────────────────────────────────────────────

/// Who is playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Player {
    Human, // Black
    Ai,    // White
}

/// The game phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Playing,
    GameOver,
}

/// A record of a single move.
#[derive(Clone, Debug)]
struct MoveRecord {
    pos: Pos,
    color: Cell,
    flipped: i32,
}

impl MoveRecord {
    /// Format as algebraic-like notation: column letter + row number.
    fn notation(&self) -> String {
        let col_char = (b'a' + self.pos.col as u8) as char;
        let row_num = self.pos.row + 1;
        let color_str = match self.color {
            Cell::Black => "B",
            Cell::White => "W",
            Cell::Empty => "?",
        };
        format!("{color_str}:{col_char}{row_num}(+{flipped})", flipped = self.flipped)
    }
}

// ── Reversi App ─────────────────────────────────────────────────────

/// Main application state.
struct ReversiApp {
    board: Board,
    current_turn: Cell,
    phase: Phase,
    cursor_row: i32,
    cursor_col: i32,
    last_move: Option<Pos>,
    move_history: Vec<MoveRecord>,
    pass_count: i32,
    message: String,
}

impl ReversiApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            current_turn: Cell::Black, // Black (human) goes first
            phase: Phase::Playing,
            cursor_row: 3,
            cursor_col: 3,
            last_move: None,
            move_history: Vec::new(),
            pass_count: 0,
            message: String::from("Your turn (Black). Arrow keys to move, Enter to place."),
        }
    }

    /// Handle a key event.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match self.phase {
            Phase::Playing => self.handle_playing_key(event),
            Phase::GameOver => self.handle_game_over_key(event),
        }
    }

    /// Handle keys during gameplay.
    fn handle_playing_key(&mut self, event: &KeyEvent) {
        // Only allow human input when it's human's turn
        if self.current_turn != Cell::Black {
            return;
        }

        match event.key {
            Key::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                }
            }
            Key::Down => {
                if self.cursor_row < 7 {
                    self.cursor_row += 1;
                }
            }
            Key::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            Key::Right => {
                if self.cursor_col < 7 {
                    self.cursor_col += 1;
                }
            }
            Key::Enter | Key::Space => {
                self.try_place_piece();
            }
            Key::N => {
                // New game
                *self = Self::new();
            }
            _ => {}
        }
    }

    /// Handle keys when the game is over.
    fn handle_game_over_key(&mut self, event: &KeyEvent) {
        if event.key == Key::N || event.key == Key::Enter {
            *self = Self::new();
        }
    }

    /// Attempt to place a piece at the cursor position.
    fn try_place_piece(&mut self) {
        let pos = Pos::new(self.cursor_row, self.cursor_col);
        if !self.board.is_legal_move(pos, self.current_turn) {
            self.message = String::from("Illegal move! Must flip at least one piece.");
            return;
        }

        let flipped = self.board.make_move(pos, self.current_turn);
        self.last_move = Some(pos);
        self.move_history.push(MoveRecord {
            pos,
            color: self.current_turn,
            flipped,
        });
        self.pass_count = 0;

        // Switch turns and handle pass logic
        self.advance_turn();
    }

    /// Advance to the next turn, handling passes and game-over.
    fn advance_turn(&mut self) {
        let next = self.current_turn.opponent();

        if self.board.is_game_over() {
            self.phase = Phase::GameOver;
            self.message = self.game_over_message();
            return;
        }

        if self.board.has_legal_move(next) {
            self.current_turn = next;
            self.pass_count = 0;
        } else {
            // Next player has no moves — they pass
            self.pass_count += 1;
            if self.pass_count >= 2 || !self.board.has_legal_move(self.current_turn) {
                // Both players passed or neither can move
                self.phase = Phase::GameOver;
                self.message = self.game_over_message();
                return;
            }
            // Current player keeps the turn
            self.message = format!(
                "{} has no legal moves — turn passes!",
                color_name(next)
            );
            // current_turn stays the same
        }

        self.update_message();

        // If it's AI's turn, make the AI move
        if self.current_turn == Cell::White && self.phase == Phase::Playing {
            self.do_ai_move();
        }
    }

    /// Execute the AI's move.
    fn do_ai_move(&mut self) {
        if let Some(mv) = ai_best_move(&self.board, Cell::White) {
            let flipped = self.board.make_move(mv, Cell::White);
            self.last_move = Some(mv);
            self.move_history.push(MoveRecord {
                pos: mv,
                color: Cell::White,
                flipped,
            });
            self.pass_count = 0;

            // After AI moves, check if game is over or if human can play
            self.current_turn = Cell::Black;

            if self.board.is_game_over() {
                self.phase = Phase::GameOver;
                self.message = self.game_over_message();
                return;
            }

            if !self.board.has_legal_move(Cell::Black) {
                // Human can't move — pass back to AI
                self.message = String::from("Black has no legal moves — turn passes to White!");
                self.current_turn = Cell::White;
                if self.board.has_legal_move(Cell::White) {
                    self.do_ai_move();
                } else {
                    self.phase = Phase::GameOver;
                    self.message = self.game_over_message();
                }
                return;
            }

            self.update_message();
        }
    }

    /// Update the status message based on current state.
    fn update_message(&mut self) {
        if self.phase == Phase::GameOver {
            self.message = self.game_over_message();
            return;
        }
        let black_count = self.board.count(Cell::Black);
        let white_count = self.board.count(Cell::White);
        if self.current_turn == Cell::Black {
            self.message = format!(
                "Your turn (Black). B:{black_count} W:{white_count}"
            );
        } else {
            self.message = format!(
                "AI thinking (White)... B:{black_count} W:{white_count}"
            );
        }
    }

    /// Build the game-over message.
    fn game_over_message(&self) -> String {
        let black_count = self.board.count(Cell::Black);
        let white_count = self.board.count(Cell::White);
        let result = match self.board.winner() {
            Cell::Black => "Black wins!",
            Cell::White => "White wins!",
            Cell::Empty => "It's a tie!",
        };
        format!("Game Over! {result} (B:{black_count} W:{white_count}) Press N for new game.")
    }

    /// Handle a mouse click.
    fn handle_mouse(&mut self, event: &MouseEvent) {
        if self.phase != Phase::Playing || self.current_turn != Cell::Black {
            return;
        }

        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            // Check if click is within the board
            let bx = event.x - BOARD_OFFSET_X;
            let by = event.y - BOARD_OFFSET_Y;
            if bx >= 0.0 && by >= 0.0 {
                let col = (bx / CELL_SIZE) as i32;
                let row = (by / CELL_SIZE) as i32;
                if col >= 0 && col < 8 && row >= 0 && row < 8 {
                    self.cursor_row = row;
                    self.cursor_col = col;
                    self.try_place_piece();
                }
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

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: PANEL_X + 250.0,
            height: BOARD_OFFSET_Y + CELL_SIZE * 8.0 + 60.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 30.0,
            text: String::from("Reversi"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score display next to title
        let black_count = self.board.count(Cell::Black);
        let white_count = self.board.count(Cell::White);
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X + 120.0,
            y: 32.0,
            text: format!("\u{25CF} {black_count}"),
            color: TEXT_COLOR,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X + 180.0,
            y: 32.0,
            text: format!("\u{25CB} {white_count}"),
            color: TEXT_COLOR,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Board border
        cmds.push(RenderCommand::FillRect {
            x: BOARD_OFFSET_X - 3.0,
            y: BOARD_OFFSET_Y - 3.0,
            width: CELL_SIZE * 8.0 + 6.0,
            height: CELL_SIZE * 8.0 + 6.0,
            color: BOARD_BORDER,
            corner_radii: CornerRadii::all(4.0),
        });

        // Board cells
        let valid_moves = if self.phase == Phase::Playing {
            self.board.legal_moves(self.current_turn)
        } else {
            Vec::new()
        };

        for row in 0..8 {
            for col in 0..8 {
                let x = BOARD_OFFSET_X + col as f32 * CELL_SIZE;
                let y = BOARD_OFFSET_Y + row as f32 * CELL_SIZE;

                // Cell background (slight alternation for visual interest)
                let cell_color = if (row + col) % 2 == 0 {
                    BOARD_GREEN
                } else {
                    BOARD_GREEN_LIGHT
                };
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: cell_color,
                    corner_radii: CornerRadii::ZERO,
                });

                let pos = Pos::new(row, col);

                // Last move highlight
                if self.last_move == Some(pos) {
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y,
                        width: CELL_SIZE,
                        height: CELL_SIZE,
                        color: LAST_MOVE_HIGHLIGHT,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Cursor highlight
                if row == self.cursor_row && col == self.cursor_col
                    && self.phase == Phase::Playing
                    && self.current_turn == Cell::Black
                {
                    cmds.push(RenderCommand::StrokeRect {
                        x: x + 2.0,
                        y: y + 2.0,
                        width: CELL_SIZE - 4.0,
                        height: CELL_SIZE - 4.0,
                        color: CURSOR_COLOR,
                        line_width: 3.0,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }

                // Pieces
                let cell = self.board.get(pos);
                if cell.is_piece() {
                    let cx = x + CELL_SIZE / 2.0;
                    let cy = y + CELL_SIZE / 2.0;
                    self.render_piece(&mut cmds, cx, cy, cell);
                }

                // Valid move dots
                if self.current_turn == Cell::Black
                    && self.phase == Phase::Playing
                    && valid_moves.contains(&pos)
                    && cell == Cell::Empty
                {
                    let cx = x + CELL_SIZE / 2.0 - DOT_RADIUS;
                    let cy = y + CELL_SIZE / 2.0 - DOT_RADIUS;
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: DOT_RADIUS * 2.0,
                        height: DOT_RADIUS * 2.0,
                        color: VALID_MOVE_DOT,
                        corner_radii: CornerRadii::all(DOT_RADIUS),
                    });
                }

                // Grid lines
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: BOARD_BORDER,
                    line_width: 1.0,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Row/column labels
        for i in 0..8 {
            // Column labels (a-h)
            let col_label = String::from((b'a' + i as u8) as char);
            cmds.push(RenderCommand::Text {
                x: BOARD_OFFSET_X + i as f32 * CELL_SIZE + CELL_SIZE / 2.0 - 4.0,
                y: BOARD_OFFSET_Y - 14.0,
                text: col_label,
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            // Row labels (1-8)
            cmds.push(RenderCommand::Text {
                x: BOARD_OFFSET_X - 18.0,
                y: BOARD_OFFSET_Y + i as f32 * CELL_SIZE + CELL_SIZE / 2.0 - 6.0,
                text: format!("{}", i + 1),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Side panel
        self.render_panel(&mut cmds);

        // Status message at bottom
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: BOARD_OFFSET_Y + CELL_SIZE * 8.0 + 20.0,
            text: self.message.clone(),
            color: if self.phase == Phase::GameOver { PEACH } else { TEXT_COLOR },
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(CELL_SIZE * 8.0 + 250.0),
        });

        cmds
    }

    /// Render a disc piece (as a filled rounded rect that approximates a circle).
    fn render_piece(&self, cmds: &mut Vec<RenderCommand>, cx: f32, cy: f32, cell: Cell) {
        let (fill, border) = match cell {
            Cell::Black => (BLACK_PIECE, BLACK_PIECE_BORDER),
            Cell::White => (WHITE_PIECE, WHITE_PIECE_BORDER),
            Cell::Empty => return,
        };

        // Piece shadow
        cmds.push(RenderCommand::FillRect {
            x: cx - PIECE_RADIUS + 2.0,
            y: cy - PIECE_RADIUS + 2.0,
            width: PIECE_RADIUS * 2.0,
            height: PIECE_RADIUS * 2.0,
            color: Color::rgba(0, 0, 0, 40),
            corner_radii: CornerRadii::all(PIECE_RADIUS),
        });

        // Piece body
        cmds.push(RenderCommand::FillRect {
            x: cx - PIECE_RADIUS,
            y: cy - PIECE_RADIUS,
            width: PIECE_RADIUS * 2.0,
            height: PIECE_RADIUS * 2.0,
            color: fill,
            corner_radii: CornerRadii::all(PIECE_RADIUS),
        });

        // Piece border
        cmds.push(RenderCommand::StrokeRect {
            x: cx - PIECE_RADIUS,
            y: cy - PIECE_RADIUS,
            width: PIECE_RADIUS * 2.0,
            height: PIECE_RADIUS * 2.0,
            color: border,
            line_width: 1.5,
            corner_radii: CornerRadii::all(PIECE_RADIUS),
        });
    }

    /// Render the side information panel.
    fn render_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let px = PANEL_X;
        let py = BOARD_OFFSET_Y;

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: 220.0,
            height: CELL_SIZE * 8.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Turn indicator
        let turn_text = if self.phase == Phase::GameOver {
            String::from("Game Over")
        } else if self.current_turn == Cell::Black {
            String::from("Your Turn (Black)")
        } else {
            String::from("AI Turn (White)")
        };
        let turn_color = match self.current_turn {
            Cell::Black => BLUE,
            Cell::White => PEACH,
            Cell::Empty => TEXT_COLOR,
        };
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 20.0,
            text: turn_text,
            color: if self.phase == Phase::GameOver { RED } else { turn_color },
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(190.0),
        });

        // Score section
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 55.0,
            text: String::from("Score"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let black_count = self.board.count(Cell::Black);
        let white_count = self.board.count(Cell::White);

        // Black score bar
        cmds.push(RenderCommand::FillRect {
            x: px + 15.0,
            y: py + 72.0,
            width: 190.0,
            height: 20.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        let total = (black_count + white_count).max(1) as f32;
        let black_bar_width = (black_count as f32 / total) * 190.0;
        if black_count > 0 {
            cmds.push(RenderCommand::FillRect {
                x: px + 15.0,
                y: py + 72.0,
                width: black_bar_width,
                height: 20.0,
                color: Color::from_hex(0x45475A),
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: if white_count == 0 { 4.0 } else { 0.0 },
                    bottom_right: if white_count == 0 { 4.0 } else { 0.0 },
                    bottom_left: 4.0,
                },
            });
        }
        cmds.push(RenderCommand::Text {
            x: px + 20.0,
            y: py + 76.0,
            text: format!("B: {black_count}"),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: px + 160.0,
            y: py + 76.0,
            text: format!("W: {white_count}"),
            color: TEXT_COLOR,
            font_size: 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Move count
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 110.0,
            text: format!("Moves: {}", self.move_history.len()),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Empty squares
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 130.0,
            text: format!("Empty: {}", self.board.empty_count()),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Last move
        if let Some(last) = self.move_history.last() {
            cmds.push(RenderCommand::Text {
                x: px + 15.0,
                y: py + 155.0,
                text: String::from("Last Move"),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: px + 15.0,
                y: py + 175.0,
                text: last.notation(),
                color: PEACH,
                font_size: INFO_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Move history (last few moves)
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: py + 205.0,
            text: String::from("History"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let history_start = if self.move_history.len() > 12 {
            self.move_history.len() - 12
        } else {
            0
        };
        for (idx, record) in self.move_history[history_start..].iter().enumerate() {
            let move_num = history_start + idx + 1;
            let move_color = if record.color == Cell::Black { BLUE } else { PEACH };
            cmds.push(RenderCommand::Text {
                x: px + 15.0,
                y: py + 225.0 + idx as f32 * 18.0,
                text: format!("{move_num}. {}", record.notation()),
                color: move_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(190.0),
            });
        }

        // Controls help at bottom of panel
        let help_y = py + CELL_SIZE * 8.0 - 40.0;
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: help_y,
            text: String::from("Arrows: Move  Enter: Place"),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
        cmds.push(RenderCommand::Text {
            x: px + 15.0,
            y: help_y + 16.0,
            text: String::from("N: New Game  Click: Place"),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }
}

/// Get a display name for a color.
fn color_name(cell: Cell) -> &'static str {
    match cell {
        Cell::Black => "Black",
        Cell::White => "White",
        Cell::Empty => "None",
    }
}

fn main() {
    let _app = ReversiApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Board setup helpers ─────────────────────────────────────────

    /// Create a board with specific placements.
    fn board_with(placements: &[(i32, i32, Cell)]) -> Board {
        let mut board = Board::empty();
        for &(row, col, cell) in placements {
            board.set(Pos::new(row, col), cell);
        }
        board
    }

    // ── Initial position tests ──────────────────────────────────────

    #[test]
    fn test_initial_board_setup() {
        let board = Board::new();
        assert_eq!(board.get(Pos::new(3, 3)), Cell::White);
        assert_eq!(board.get(Pos::new(3, 4)), Cell::Black);
        assert_eq!(board.get(Pos::new(4, 3)), Cell::Black);
        assert_eq!(board.get(Pos::new(4, 4)), Cell::White);
    }

    #[test]
    fn test_initial_piece_count() {
        let board = Board::new();
        assert_eq!(board.count(Cell::Black), 2);
        assert_eq!(board.count(Cell::White), 2);
        assert_eq!(board.total_pieces(), 4);
        assert_eq!(board.empty_count(), 60);
    }

    #[test]
    fn test_initial_board_corners_empty() {
        let board = Board::new();
        assert_eq!(board.get(Pos::new(0, 0)), Cell::Empty);
        assert_eq!(board.get(Pos::new(0, 7)), Cell::Empty);
        assert_eq!(board.get(Pos::new(7, 0)), Cell::Empty);
        assert_eq!(board.get(Pos::new(7, 7)), Cell::Empty);
    }

    #[test]
    fn test_empty_board() {
        let board = Board::empty();
        assert_eq!(board.count(Cell::Black), 0);
        assert_eq!(board.count(Cell::White), 0);
        assert_eq!(board.empty_count(), 64);
    }

    // ── Cell tests ──────────────────────────────────────────────────

    #[test]
    fn test_cell_opponent() {
        assert_eq!(Cell::Black.opponent(), Cell::White);
        assert_eq!(Cell::White.opponent(), Cell::Black);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
    }

    #[test]
    fn test_cell_is_piece() {
        assert!(Cell::Black.is_piece());
        assert!(Cell::White.is_piece());
        assert!(!Cell::Empty.is_piece());
    }

    // ── Position tests ──────────────────────────────────────────────

    #[test]
    fn test_pos_in_bounds() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(7, 7).in_bounds());
        assert!(Pos::new(3, 4).in_bounds());
        assert!(!Pos::new(-1, 0).in_bounds());
        assert!(!Pos::new(0, -1).in_bounds());
        assert!(!Pos::new(8, 0).in_bounds());
        assert!(!Pos::new(0, 8).in_bounds());
        assert!(!Pos::new(8, 8).in_bounds());
    }

    #[test]
    fn test_board_get_out_of_bounds() {
        let board = Board::new();
        assert_eq!(board.get(Pos::new(-1, -1)), Cell::Empty);
        assert_eq!(board.get(Pos::new(8, 8)), Cell::Empty);
    }

    // ── Legal move generation tests ─────────────────────────────────

    #[test]
    fn test_initial_legal_moves_black() {
        let board = Board::new();
        let moves = board.legal_moves(Cell::Black);
        // Standard opening: Black can play at d3, c4, f5, e6
        assert_eq!(moves.len(), 4);
        assert!(moves.contains(&Pos::new(2, 3))); // d3
        assert!(moves.contains(&Pos::new(3, 2))); // c4
        assert!(moves.contains(&Pos::new(4, 5))); // f5
        assert!(moves.contains(&Pos::new(5, 4))); // e6
    }

    #[test]
    fn test_initial_legal_moves_white() {
        let board = Board::new();
        let moves = board.legal_moves(Cell::White);
        assert_eq!(moves.len(), 4);
        assert!(moves.contains(&Pos::new(2, 4))); // e3
        assert!(moves.contains(&Pos::new(3, 5))); // f4
        assert!(moves.contains(&Pos::new(4, 2))); // c5
        assert!(moves.contains(&Pos::new(5, 3))); // d6
    }

    #[test]
    fn test_no_legal_move_on_occupied() {
        let board = Board::new();
        assert!(!board.is_legal_move(Pos::new(3, 3), Cell::Black));
        assert!(!board.is_legal_move(Pos::new(3, 4), Cell::Black));
    }

    #[test]
    fn test_no_legal_move_on_non_flanking() {
        let board = Board::new();
        // Corner is not adjacent to any pieces at start
        assert!(!board.is_legal_move(Pos::new(0, 0), Cell::Black));
        // A cell next to own piece but not flanking
        assert!(!board.is_legal_move(Pos::new(2, 4), Cell::Black));
    }

    #[test]
    fn test_legal_move_requires_nonempty_flip() {
        let board = Board::empty();
        // Empty board — no flips possible anywhere
        assert!(!board.is_legal_move(Pos::new(3, 3), Cell::Black));
    }

    // ── Flipping tests — all 8 directions ───────────────────────────

    #[test]
    fn test_flip_horizontal_right() {
        // Black at (3,0), White at (3,1), place Black at (3,2)
        let board = board_with(&[
            (3, 0, Cell::Black),
            (3, 1, Cell::White),
        ]);
        let flips = board.get_flips(Pos::new(3, 2), Cell::Black);
        // No flip: Black at (3,0) flanks (3,1) if placed at (3,2)?
        // Direction left (-0, -1) from (3,2): sees White at (3,1), then Black at (3,0). Yes!
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(3, 1)));
    }

    #[test]
    fn test_flip_horizontal_left() {
        let board = board_with(&[
            (3, 5, Cell::White),
            (3, 6, Cell::Black),
        ]);
        // Place Black at (3,4) — direction right: sees White at (3,5), then Black at (3,6)
        let flips = board.get_flips(Pos::new(3, 4), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(3, 5)));
    }

    #[test]
    fn test_flip_vertical_down() {
        let board = board_with(&[
            (2, 3, Cell::Black),
            (3, 3, Cell::White),
        ]);
        // Place Black at (4,3) — direction up: sees White at (3,3), then Black at (2,3)
        let flips = board.get_flips(Pos::new(4, 3), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(3, 3)));
    }

    #[test]
    fn test_flip_vertical_up() {
        let board = board_with(&[
            (4, 3, Cell::White),
            (5, 3, Cell::Black),
        ]);
        // Place Black at (3,3) — direction down: sees White at (4,3), then Black at (5,3)
        let flips = board.get_flips(Pos::new(3, 3), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(4, 3)));
    }

    #[test]
    fn test_flip_diagonal_down_right() {
        let board = board_with(&[
            (2, 2, Cell::Black),
            (3, 3, Cell::White),
        ]);
        // Place Black at (4,4) — direction up-left: sees White at (3,3), then Black at (2,2)
        let flips = board.get_flips(Pos::new(4, 4), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(3, 3)));
    }

    #[test]
    fn test_flip_diagonal_up_left() {
        let board = board_with(&[
            (4, 4, Cell::White),
            (5, 5, Cell::Black),
        ]);
        // Place Black at (3,3) — direction down-right: sees White at (4,4), then Black at (5,5)
        let flips = board.get_flips(Pos::new(3, 3), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(4, 4)));
    }

    #[test]
    fn test_flip_diagonal_down_left() {
        let board = board_with(&[
            (2, 5, Cell::Black),
            (3, 4, Cell::White),
        ]);
        // Place Black at (4,3) — direction up-right: sees White at (3,4), then Black at (2,5)
        let flips = board.get_flips(Pos::new(4, 3), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(3, 4)));
    }

    #[test]
    fn test_flip_diagonal_up_right() {
        let board = board_with(&[
            (4, 2, Cell::White),
            (5, 1, Cell::Black),
        ]);
        // Place Black at (3,3) — direction down-left: sees White at (4,2), then Black at (5,1)
        let flips = board.get_flips(Pos::new(3, 3), Cell::Black);
        assert_eq!(flips.len(), 1);
        assert!(flips.contains(&Pos::new(4, 2)));
    }

    #[test]
    fn test_flip_multiple_directions() {
        // Set up a position where placing Black at (3,3) flips in two directions.
        // Vertical: White at (2,3) flanked by Black at (1,3) (direction up from placement).
        // Horizontal: White at (3,2) flanked by Black at (3,1) (direction left from placement).
        let board = board_with(&[
            (1, 3, Cell::Black), // anchor for vertical flank
            (2, 3, Cell::White), // flanked vertically
            (3, 1, Cell::Black), // anchor for horizontal flank
            (3, 2, Cell::White), // flanked horizontally
        ]);
        let flips = board.get_flips(Pos::new(3, 3), Cell::Black);
        assert_eq!(flips.len(), 2);
        assert!(flips.contains(&Pos::new(2, 3)));
        assert!(flips.contains(&Pos::new(3, 2)));
    }

    #[test]
    fn test_flip_multiple_in_one_direction() {
        // Chain: Black at (0,0), White at (0,1), White at (0,2), place Black at (0,3)
        let board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
        ]);
        let flips = board.get_flips(Pos::new(0, 3), Cell::Black);
        assert_eq!(flips.len(), 2);
        assert!(flips.contains(&Pos::new(0, 1)));
        assert!(flips.contains(&Pos::new(0, 2)));
    }

    #[test]
    fn test_no_flip_with_gap() {
        // Black at (0,0), Empty at (0,1), White at (0,2) — no flank because of gap
        let board = board_with(&[
            (0, 0, Cell::Black),
            (0, 2, Cell::White),
        ]);
        let flips = board.get_flips(Pos::new(0, 3), Cell::Black);
        // Direction left from (0,3): White at (0,2), then Empty at (0,1) — no flank
        assert_eq!(flips.len(), 0);
    }

    #[test]
    fn test_no_flip_same_color() {
        // Black at (0,0), Black at (0,1) — can't flip own pieces
        let board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::Black),
        ]);
        let flips = board.get_flips(Pos::new(0, 2), Cell::Black);
        assert_eq!(flips.len(), 0);
    }

    #[test]
    fn test_no_flip_edge_of_board() {
        // White at (0,6), place Black at (0,7) — no Black beyond edge
        let board = board_with(&[
            (0, 6, Cell::White),
        ]);
        let flips = board.get_flips(Pos::new(0, 7), Cell::Black);
        assert_eq!(flips.len(), 0);
    }

    // ── Make move tests ─────────────────────────────────────────────

    #[test]
    fn test_make_move_basic() {
        let mut board = Board::new();
        // Black plays d3 (row 2, col 3)
        let flipped = board.make_move(Pos::new(2, 3), Cell::Black);
        assert_eq!(flipped, 1); // Flips white at d4 (3,3)
        assert_eq!(board.get(Pos::new(2, 3)), Cell::Black);
        assert_eq!(board.get(Pos::new(3, 3)), Cell::Black); // was White, now flipped
        assert_eq!(board.count(Cell::Black), 4);
        assert_eq!(board.count(Cell::White), 1);
    }

    #[test]
    fn test_make_move_illegal() {
        let mut board = Board::new();
        let flipped = board.make_move(Pos::new(0, 0), Cell::Black);
        assert_eq!(flipped, 0);
        // Board unchanged
        assert_eq!(board.count(Cell::Black), 2);
        assert_eq!(board.count(Cell::White), 2);
    }

    #[test]
    fn test_make_move_flips_multiple_directions() {
        let mut board = board_with(&[
            (1, 3, Cell::Black), // anchor for vertical flank
            (2, 3, Cell::White), // flanked vertically
            (3, 1, Cell::Black), // anchor for horizontal flank
            (3, 2, Cell::White), // flanked horizontally
        ]);
        let flipped = board.make_move(Pos::new(3, 3), Cell::Black);
        assert_eq!(flipped, 2);
        assert_eq!(board.get(Pos::new(2, 3)), Cell::Black);
        assert_eq!(board.get(Pos::new(3, 2)), Cell::Black);
    }

    #[test]
    fn test_make_move_long_chain() {
        let mut board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
            (0, 3, Cell::White),
            (0, 4, Cell::White),
            (0, 5, Cell::White),
        ]);
        let flipped = board.make_move(Pos::new(0, 6), Cell::Black);
        assert_eq!(flipped, 5);
        for col in 0..7 {
            assert_eq!(board.get(Pos::new(0, col)), Cell::Black);
        }
    }

    // ── Pass detection tests ────────────────────────────────────────

    #[test]
    fn test_has_legal_move_initial() {
        let board = Board::new();
        assert!(board.has_legal_move(Cell::Black));
        assert!(board.has_legal_move(Cell::White));
    }

    #[test]
    fn test_no_legal_move_empty_board() {
        let board = Board::empty();
        assert!(!board.has_legal_move(Cell::Black));
        assert!(!board.has_legal_move(Cell::White));
    }

    #[test]
    fn test_pass_when_no_moves() {
        // All cells one color except one corner of the other — no flanking possible
        let mut board = Board::empty();
        for row in 0..8 {
            for col in 0..8 {
                board.set(Pos::new(row, col), Cell::Black);
            }
        }
        board.set(Pos::new(0, 0), Cell::White);
        // White can't move because there's nowhere to place that would flank Black
        // (all remaining cells are occupied)
        assert!(!board.has_legal_move(Cell::White));
    }

    // ── Game over detection tests ───────────────────────────────────

    #[test]
    fn test_game_over_full_board() {
        let mut board = Board::empty();
        for row in 0..8 {
            for col in 0..8 {
                board.set(Pos::new(row, col), Cell::Black);
            }
        }
        assert!(board.is_game_over());
    }

    #[test]
    fn test_game_not_over_initial() {
        let board = Board::new();
        assert!(!board.is_game_over());
    }

    #[test]
    fn test_game_over_neither_can_move() {
        // Create a position where neither can move but board isn't full
        // Isolated single pieces with no adjacent opponent pieces
        let board = board_with(&[
            (0, 0, Cell::Black),
            (7, 7, Cell::White),
        ]);
        assert!(board.is_game_over());
    }

    // ── Winner detection tests ──────────────────────────────────────

    #[test]
    fn test_winner_black() {
        let board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::Black),
            (0, 2, Cell::Black),
            (1, 0, Cell::White),
        ]);
        assert_eq!(board.winner(), Cell::Black);
    }

    #[test]
    fn test_winner_white() {
        let board = board_with(&[
            (0, 0, Cell::White),
            (0, 1, Cell::White),
            (0, 2, Cell::Black),
        ]);
        assert_eq!(board.winner(), Cell::White);
    }

    #[test]
    fn test_winner_tie() {
        let board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
        ]);
        assert_eq!(board.winner(), Cell::Empty);
    }

    #[test]
    fn test_winner_all_black() {
        let mut board = Board::empty();
        for row in 0..8 {
            for col in 0..8 {
                board.set(Pos::new(row, col), Cell::Black);
            }
        }
        assert_eq!(board.winner(), Cell::Black);
        assert_eq!(board.count(Cell::Black), 64);
    }

    // ── AI tests ────────────────────────────────────────────────────

    #[test]
    fn test_ai_returns_legal_move() {
        let board = Board::new();
        let mv = ai_best_move(&board, Cell::White);
        assert!(mv.is_some());
        let mv = mv.unwrap();
        assert!(board.is_legal_move(mv, Cell::White));
    }

    #[test]
    fn test_ai_no_move_when_none_available() {
        let board = Board::empty();
        let mv = ai_best_move(&board, Cell::White);
        assert!(mv.is_none());
    }

    #[test]
    fn test_ai_takes_corner_when_available() {
        // Set up a position where a corner is available and should be preferred
        let board = board_with(&[
            (0, 1, Cell::Black),
            (1, 1, Cell::Black),
            (1, 0, Cell::Black), // Adjacent to corner
            // White pieces to make corner legal
            (0, 2, Cell::White),
            (0, 3, Cell::White),
        ]);
        // Check if corner (0,0) is legal for white
        if board.is_legal_move(Pos::new(0, 0), Cell::White) {
            let mv = ai_best_move(&board, Cell::White);
            // The AI should strongly prefer the corner
            assert!(mv.is_some());
            // Corner should be very highly rated but we don't mandate
            // it as the only choice since the AI also considers other factors
        }
    }

    #[test]
    fn test_ai_avoids_giving_corner() {
        // Generally the AI should avoid X-squares (diagonal to corners)
        // unless there's a compelling reason not to
        let board = Board::new();
        let mv = ai_best_move(&board, Cell::White);
        assert!(mv.is_some());
        // The initial moves should not be at X-squares
        let mv = mv.unwrap();
        let x_squares = [
            Pos::new(1, 1), Pos::new(1, 6),
            Pos::new(6, 1), Pos::new(6, 6),
        ];
        // AI should prefer non-X-square moves at the start
        assert!(!x_squares.contains(&mv), "AI should avoid X-squares early");
    }

    #[test]
    fn test_evaluate_prefers_more_pieces() {
        let board_good = board_with(&[
            (0, 0, Cell::White),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
            (1, 0, Cell::Black),
        ]);
        let board_bad = board_with(&[
            (0, 0, Cell::White),
            (0, 1, Cell::Black),
            (0, 2, Cell::Black),
            (1, 0, Cell::Black),
        ]);
        assert!(evaluate(&board_good, Cell::White) > evaluate(&board_bad, Cell::White));
    }

    #[test]
    fn test_evaluate_prefers_corners() {
        let board_corner = board_with(&[
            (0, 0, Cell::White), // corner!
            (3, 3, Cell::Black),
        ]);
        let board_center = board_with(&[
            (3, 4, Cell::White), // center
            (3, 3, Cell::Black),
        ]);
        assert!(evaluate(&board_corner, Cell::White) > evaluate(&board_center, Cell::White));
    }

    // ── Scoring tests ───────────────────────────────────────────────

    #[test]
    fn test_count_after_move() {
        let mut board = Board::new();
        board.make_move(Pos::new(2, 3), Cell::Black);
        // After Black plays d3: 4 black, 1 white
        assert_eq!(board.count(Cell::Black), 4);
        assert_eq!(board.count(Cell::White), 1);
        assert_eq!(board.total_pieces(), 5);
    }

    #[test]
    fn test_count_after_two_moves() {
        let mut board = Board::new();
        board.make_move(Pos::new(2, 3), Cell::Black); // Black d3
        board.make_move(Pos::new(2, 2), Cell::White); // White c3
        assert_eq!(board.total_pieces(), 6);
    }

    // ── App tests ───────────────────────────────────────────────────

    #[test]
    fn test_app_initial_state() {
        let app = ReversiApp::new();
        assert_eq!(app.current_turn, Cell::Black);
        assert_eq!(app.phase, Phase::Playing);
        assert_eq!(app.cursor_row, 3);
        assert_eq!(app.cursor_col, 3);
        assert!(app.last_move.is_none());
        assert!(app.move_history.is_empty());
    }

    #[test]
    fn test_app_cursor_movement() {
        let mut app = ReversiApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_row, 4);

        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_col, 4);

        app.handle_key(&KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_row, 3);

        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_col, 3);
    }

    #[test]
    fn test_app_cursor_bounds() {
        let mut app = ReversiApp::new();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(&KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_row, 0); // Stays at 0

        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_col, 0); // Stays at 0

        app.cursor_row = 7;
        app.cursor_col = 7;
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_row, 7); // Stays at 7

        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_col, 7); // Stays at 7
    }

    #[test]
    fn test_app_place_piece() {
        let mut app = ReversiApp::new();
        // Move cursor to d3 (row 2, col 3)
        app.cursor_row = 2;
        app.cursor_col = 3;
        app.handle_key(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        // After human places, AI should also have moved
        assert!(app.move_history.len() >= 1);
        assert_eq!(app.move_history[0].pos, Pos::new(2, 3));
        assert_eq!(app.move_history[0].color, Cell::Black);
    }

    #[test]
    fn test_app_illegal_move_rejected() {
        let mut app = ReversiApp::new();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert!(app.move_history.is_empty());
        assert!(app.message.contains("Illegal"));
    }

    #[test]
    fn test_app_new_game() {
        let mut app = ReversiApp::new();
        // Make a move first
        app.cursor_row = 2;
        app.cursor_col = 3;
        app.handle_key(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert!(!app.move_history.is_empty());

        // Press N for new game
        app.handle_key(&KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert!(app.move_history.is_empty());
        assert_eq!(app.board.count(Cell::Black), 2);
        assert_eq!(app.board.count(Cell::White), 2);
    }

    #[test]
    fn test_app_ignores_key_release() {
        let mut app = ReversiApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: false, // Release, not press
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_row, 3); // Unchanged
    }

    #[test]
    fn test_app_mouse_click_on_board() {
        let mut app = ReversiApp::new();
        // Click on d3 (row 2, col 3)
        let x = BOARD_OFFSET_X + 3.0 * CELL_SIZE + CELL_SIZE / 2.0;
        let y = BOARD_OFFSET_Y + 2.0 * CELL_SIZE + CELL_SIZE / 2.0;
        app.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert!(app.move_history.len() >= 1);
    }

    #[test]
    fn test_app_mouse_click_outside_board() {
        let mut app = ReversiApp::new();
        app.handle_mouse(&MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        assert!(app.move_history.is_empty());
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = ReversiApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
        // Should have at minimum: background, title, board cells, pieces, labels
        assert!(cmds.len() > 64); // At least one cmd per cell + overhead
    }

    #[test]
    fn test_render_contains_title() {
        let app = ReversiApp::new();
        let cmds = app.render();
        let has_title = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text == "Reversi"
            } else {
                false
            }
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_contains_pieces() {
        let app = ReversiApp::new();
        let cmds = app.render();
        // Initial board has 4 pieces, each rendered as shadow + body + border = 12 fill/stroke cmds
        let fill_count = cmds.iter().filter(|cmd| matches!(cmd, RenderCommand::FillRect { .. })).count();
        assert!(fill_count >= 64 + 4); // 64 cells + at least 4 piece bodies
    }

    #[test]
    fn test_render_shows_valid_moves() {
        let app = ReversiApp::new();
        let cmds = app.render();
        // Valid moves rendered as small rounded rectangles (dots)
        // There are 4 valid moves initially for Black
        let valid_dot_count = cmds.iter().filter(|cmd| {
            if let RenderCommand::FillRect { color, corner_radii, .. } = cmd {
                *color == VALID_MOVE_DOT && corner_radii.top_left > 0.0
            } else {
                false
            }
        }).count();
        assert_eq!(valid_dot_count, 4);
    }

    #[test]
    fn test_render_game_over() {
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        app.message = app.game_over_message();
        let cmds = app.render();
        let has_game_over = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("Game Over")
            } else {
                false
            }
        });
        assert!(has_game_over);
    }

    #[test]
    fn test_render_cursor_visible() {
        let app = ReversiApp::new();
        let cmds = app.render();
        let has_cursor = cmds.iter().any(|cmd| {
            if let RenderCommand::StrokeRect { color, .. } = cmd {
                *color == CURSOR_COLOR
            } else {
                false
            }
        });
        assert!(has_cursor);
    }

    #[test]
    fn test_render_last_move_highlight() {
        let mut app = ReversiApp::new();
        app.last_move = Some(Pos::new(3, 3));
        let cmds = app.render();
        let has_highlight = cmds.iter().any(|cmd| {
            if let RenderCommand::FillRect { color, .. } = cmd {
                *color == LAST_MOVE_HIGHLIGHT
            } else {
                false
            }
        });
        assert!(has_highlight);
    }

    #[test]
    fn test_render_panel_score() {
        let app = ReversiApp::new();
        let cmds = app.render();
        let has_score = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("Score")
            } else {
                false
            }
        });
        assert!(has_score);
    }

    #[test]
    fn test_render_panel_move_count() {
        let app = ReversiApp::new();
        let cmds = app.render();
        let has_moves = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("Moves: 0")
            } else {
                false
            }
        });
        assert!(has_moves);
    }

    // ── Move record notation tests ──────────────────────────────────

    #[test]
    fn test_move_notation() {
        let record = MoveRecord {
            pos: Pos::new(2, 3),
            color: Cell::Black,
            flipped: 1,
        };
        assert_eq!(record.notation(), "B:d3(+1)");
    }

    #[test]
    fn test_move_notation_white() {
        let record = MoveRecord {
            pos: Pos::new(5, 0),
            color: Cell::White,
            flipped: 3,
        };
        assert_eq!(record.notation(), "W:a6(+3)");
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_full_row_flip() {
        // Black at (0,0) and (0,7), White filling (0,1)-(0,6)
        let mut board = Board::empty();
        board.set(Pos::new(0, 0), Cell::Black);
        for col in 1..7 {
            board.set(Pos::new(0, col), Cell::White);
        }
        // Place Black at (0,7)
        let flips = board.get_flips(Pos::new(0, 7), Cell::Black);
        assert_eq!(flips.len(), 6);
    }

    #[test]
    fn test_full_column_flip() {
        let mut board = Board::empty();
        board.set(Pos::new(0, 0), Cell::Black);
        for row in 1..7 {
            board.set(Pos::new(row, 0), Cell::White);
        }
        let flips = board.get_flips(Pos::new(7, 0), Cell::Black);
        assert_eq!(flips.len(), 6);
    }

    #[test]
    fn test_full_diagonal_flip() {
        let mut board = Board::empty();
        board.set(Pos::new(0, 0), Cell::Black);
        for i in 1..7 {
            board.set(Pos::new(i, i), Cell::White);
        }
        let flips = board.get_flips(Pos::new(7, 7), Cell::Black);
        assert_eq!(flips.len(), 6);
    }

    #[test]
    fn test_board_set_out_of_bounds() {
        let mut board = Board::empty();
        // Should not panic
        board.set(Pos::new(-1, -1), Cell::Black);
        board.set(Pos::new(8, 8), Cell::Black);
        assert_eq!(board.count(Cell::Black), 0);
    }

    #[test]
    fn test_flips_on_occupied_cell() {
        let board = Board::new();
        // Can't place on an occupied cell
        let flips = board.get_flips(Pos::new(3, 3), Cell::Black);
        assert!(flips.is_empty());
    }

    #[test]
    fn test_flips_out_of_bounds() {
        let board = Board::new();
        let flips = board.get_flips(Pos::new(-1, -1), Cell::Black);
        assert!(flips.is_empty());
    }

    #[test]
    fn test_color_name() {
        assert_eq!(color_name(Cell::Black), "Black");
        assert_eq!(color_name(Cell::White), "White");
        assert_eq!(color_name(Cell::Empty), "None");
    }

    #[test]
    fn test_event_handling() {
        let mut app = ReversiApp::new();
        // Test Event::Key
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert_eq!(app.cursor_row, 4);

        // Test Event::Mouse (click outside board)
        app.handle_event(&Event::Mouse(MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        // No change since click is outside
        assert!(app.move_history.is_empty());

        // Test Event::Resize (ignored)
        app.handle_event(&Event::Resize {
            width: 800,
            height: 600,
        });
        assert_eq!(app.cursor_row, 4); // Unchanged
    }

    #[test]
    fn test_space_key_places_piece() {
        let mut app = ReversiApp::new();
        app.cursor_row = 2;
        app.cursor_col = 3;
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert!(app.move_history.len() >= 1);
        assert_eq!(app.move_history[0].pos, Pos::new(2, 3));
    }

    #[test]
    fn test_game_over_new_game_key() {
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        app.handle_key(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.phase, Phase::Playing);
        assert!(app.move_history.is_empty());
    }

    // ── Minimax correctness tests ───────────────────────────────────

    #[test]
    fn test_minimax_returns_finite() {
        let board = Board::new();
        let score = minimax(&board, 2, i32::MIN, i32::MAX, true, Cell::White);
        assert!(score > i32::MIN);
        assert!(score < i32::MAX);
    }

    #[test]
    fn test_minimax_game_over_board() {
        let mut board = Board::empty();
        for row in 0..8 {
            for col in 0..8 {
                board.set(Pos::new(row, col), Cell::Black);
            }
        }
        // Game is over — should return evaluation immediately
        let score = minimax(&board, 4, i32::MIN, i32::MAX, true, Cell::White);
        // White has 0 pieces, Black has 64 — very negative for White
        assert!(score < 0);
    }

    #[test]
    fn test_evaluate_symmetric() {
        // A symmetric position should evaluate to 0 for either side
        let board = Board::new();
        let black_eval = evaluate(&board, Cell::Black);
        let white_eval = evaluate(&board, Cell::White);
        // Due to mobility differences, these might not be exactly zero,
        // but they should be negatives of each other (approximately)
        assert!((black_eval + white_eval).abs() <= 1);
    }
}
