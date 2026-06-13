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

//! SlateOS Checkers — American Checkers (English draughts) with AI opponent.
//!
//! Features an 8x8 board with standard setup, mandatory jumps, multi-jump
//! chains, king promotion, a minimax AI with alpha-beta pruning, legal move
//! highlighting, and Catppuccin Mocha theming.

use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
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

// ── Board colors ────────────────────────────────────────────────────
const LIGHT_SQUARE: Color = Color::from_hex(0x9CA0B0);
const DARK_SQUARE: Color = Color::from_hex(0x585B70);
const SELECTED_SQUARE: Color = Color::from_hex(0x89B4FA);
const LEGAL_MOVE_DOT: Color = Color::rgba(166, 227, 161, 140);
const LAST_MOVE_HIGHLIGHT: Color = Color::rgba(250, 179, 135, 80);

// ── Piece colors ────────────────────────────────────────────────────
const RED_PIECE: Color = Color::from_hex(0xF38BA8);
const RED_PIECE_DARK: Color = Color::from_hex(0xD06080);
const BLACK_PIECE: Color = Color::from_hex(0x45475A);
const BLACK_PIECE_DARK: Color = Color::from_hex(0x313244);
const KING_CROWN: Color = Color::from_hex(0xF9E2AF);

// ── Layout constants ────────────────────────────────────────────────
const SQUARE_SIZE: f32 = 64.0;
const BOARD_OFFSET_X: f32 = 40.0;
const BOARD_OFFSET_Y: f32 = 60.0;
const PANEL_X: f32 = BOARD_OFFSET_X + SQUARE_SIZE * 8.0 + 20.0;
const PIECE_RADIUS: f32 = 24.0;
const PIECE_INNER_RADIUS: f32 = 18.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const CROWN_FONT_SIZE: f32 = 20.0;
const DOT_SIZE: f32 = 16.0;

// ── AI search depth ─────────────────────────────────────────────────
const AI_DEPTH: i32 = 3;

// ── Piece values for AI evaluation ─────────────────────────────────
const MAN_VALUE: i32 = 100;
const KING_VALUE: i32 = 300;
const CENTER_BONUS: i32 = 10;
const ADVANCE_BONUS: i32 = 5;
const BACK_ROW_BONUS: i32 = 15;

// ── Types ───────────────────────────────────────────────────────────

/// Which side a piece belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    Red,
    Black,
}

impl Side {
    fn opponent(self) -> Self {
        match self {
            Self::Red => Self::Black,
            Self::Black => Self::Red,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Black => "Black",
        }
    }
}

/// A checkers piece: either a regular man or a king.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Piece {
    side: Side,
    is_king: bool,
}

impl Piece {
    const fn man(side: Side) -> Self {
        Self {
            side,
            is_king: false,
        }
    }

    const fn king(side: Side) -> Self {
        Self {
            side,
            is_king: true,
        }
    }
}

/// Board position (row 0 = bottom row / Red's back rank, row 7 = top / Black's back rank).
/// Only dark squares (where (row + col) is odd) are used in checkers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Pos {
    row: i8,
    col: i8,
}

impl Pos {
    const fn new(row: i8, col: i8) -> Self {
        Self { row, col }
    }

    fn is_valid(self) -> bool {
        self.row >= 0 && self.row < 8 && self.col >= 0 && self.col < 8
    }

    /// Whether this is a playable (dark) square.
    fn is_dark(self) -> bool {
        (self.row + self.col) % 2 == 1
    }

    /// Index into the 32-element dark-square array (0-31).
    fn dark_index(self) -> Option<usize> {
        if !self.is_valid() || !self.is_dark() {
            return None;
        }
        // Row 0: dark squares at cols 1,3,5,7 → indices 0,1,2,3
        // Row 1: dark squares at cols 0,2,4,6 → indices 4,5,6,7
        // etc.
        Some((self.row as usize) * 4 + (self.col as usize) / 2)
    }

    /// Label for display (e.g. "a1").
    fn label(self) -> String {
        let file = (b'a' + self.col as u8) as char;
        let rank = (b'1' + self.row as u8) as char;
        format!("{file}{rank}")
    }
}

/// A single move in checkers: from one square to another.
/// For jumps, `captured` contains the position of the jumped piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CheckersMove {
    from: Pos,
    to: Pos,
    captured: Option<Pos>,
}

impl CheckersMove {
    const fn simple(from: Pos, to: Pos) -> Self {
        Self {
            from,
            to,
            captured: None,
        }
    }

    const fn jump(from: Pos, to: Pos, captured: Pos) -> Self {
        Self {
            from,
            to,
            captured: Some(captured),
        }
    }

    fn is_jump(self) -> bool {
        self.captured.is_some()
    }
}

/// A complete move sequence, which may include multiple jumps (multi-jump chain).
#[derive(Clone, Debug, PartialEq, Eq)]
struct MoveSequence {
    steps: Vec<CheckersMove>,
}

impl MoveSequence {
    fn new(steps: Vec<CheckersMove>) -> Self {
        Self { steps }
    }

    fn single(mv: CheckersMove) -> Self {
        Self { steps: vec![mv] }
    }

    // Renamed from `from_pos` to satisfy `wrong_self_convention`
    // (from_* should not take `&self`).
    fn origin_pos(&self) -> Pos {
        self.steps.first().map_or(Pos::new(0, 0), |s| s.from)
    }

    fn to_pos(&self) -> Pos {
        self.steps.last().map_or(Pos::new(0, 0), |s| s.to)
    }

    fn is_jump(&self) -> bool {
        self.steps.first().is_some_and(|s| s.is_jump())
    }

    fn captured_count(&self) -> usize {
        self.steps.iter().filter(|s| s.is_jump()).count()
    }

    /// Descriptive notation for the move.
    fn notation(&self) -> String {
        if self.steps.is_empty() {
            return String::new();
        }
        let mut result = self.steps[0].from.label();
        let sep = if self.is_jump() { "x" } else { "-" };
        for step in &self.steps {
            result.push_str(sep);
            result.push_str(&step.to.label());
        }
        result
    }
}

/// The game result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameResult {
    Ongoing,
    RedWins,
    BlackWins,
    Draw,
}

// ── Board ───────────────────────────────────────────────────────────

/// The checkers board. 8x8 grid, pieces only on dark squares.
#[derive(Clone, Debug)]
struct Board {
    squares: [[Option<Piece>; 8]; 8],
    side_to_move: Side,
    move_count: u32,
    /// Consecutive moves without a capture (for draw detection).
    no_capture_count: u32,
}

impl Board {
    /// Create a new board with standard starting position.
    fn new() -> Self {
        let mut squares = [[None; 8]; 8];

        // Black pieces on rows 5, 6, 7 (top three rows), dark squares only
        for row in 5..=7i8 {
            for col in 0..8i8 {
                if (row + col) % 2 == 1 {
                    squares[row as usize][col as usize] = Some(Piece::man(Side::Black));
                }
            }
        }

        // Red pieces on rows 0, 1, 2 (bottom three rows), dark squares only
        for row in 0..=2i8 {
            for col in 0..8i8 {
                if (row + col) % 2 == 1 {
                    squares[row as usize][col as usize] = Some(Piece::man(Side::Red));
                }
            }
        }

        Self {
            squares,
            side_to_move: Side::Red,
            move_count: 0,
            no_capture_count: 0,
        }
    }

    /// Create an empty board.
    fn empty() -> Self {
        Self {
            squares: [[None; 8]; 8],
            side_to_move: Side::Red,
            move_count: 0,
            no_capture_count: 0,
        }
    }

    /// Get the piece at a position.
    fn get(&self, pos: Pos) -> Option<Piece> {
        if pos.is_valid() {
            self.squares[pos.row as usize][pos.col as usize]
        } else {
            None
        }
    }

    /// Set a piece at a position.
    fn set(&mut self, pos: Pos, piece: Option<Piece>) {
        if pos.is_valid() {
            self.squares[pos.row as usize][pos.col as usize] = piece;
        }
    }

    /// Count pieces of a given side.
    fn count_pieces(&self, side: Side) -> usize {
        self.squares
            .iter()
            .flatten()
            .filter(|p| p.is_some_and(|p| p.side == side))
            .count()
    }

    /// Count kings of a given side.
    fn count_kings(&self, side: Side) -> usize {
        self.squares
            .iter()
            .flatten()
            .filter(|p| p.is_some_and(|p| p.side == side && p.is_king))
            .count()
    }

    /// Get the forward directions for a side (row deltas).
    /// Red moves up (positive row), Black moves down (negative row).
    fn forward_dirs(side: Side) -> &'static [i8] {
        match side {
            Side::Red => &[1],
            Side::Black => &[-1],
        }
    }

    /// Get all diagonal directions for a king.
    fn king_dirs() -> &'static [(i8, i8)] {
        &[(1, 1), (1, -1), (-1, 1), (-1, -1)]
    }

    /// Get diagonal directions for a regular piece of the given side.
    fn man_dirs(side: Side) -> Vec<(i8, i8)> {
        let row_dirs = Self::forward_dirs(side);
        let mut dirs = Vec::new();
        for &dr in row_dirs {
            dirs.push((dr, 1));
            dirs.push((dr, -1));
        }
        dirs
    }

    /// Generate all simple (non-jump) moves for a piece at `pos`.
    fn generate_simple_moves_for(&self, pos: Pos) -> Vec<CheckersMove> {
        let piece = match self.get(pos) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let dirs: Vec<(i8, i8)> = if piece.is_king {
            Self::king_dirs().to_vec()
        } else {
            Self::man_dirs(piece.side)
        };

        let mut moves = Vec::new();
        for (dr, dc) in dirs {
            let to = Pos::new(pos.row + dr, pos.col + dc);
            if to.is_valid() && self.get(to).is_none() {
                moves.push(CheckersMove::simple(pos, to));
            }
        }
        moves
    }

    /// Generate all single jump moves for a piece at `pos`.
    fn generate_jumps_for(&self, pos: Pos) -> Vec<CheckersMove> {
        let piece = match self.get(pos) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let dirs: Vec<(i8, i8)> = if piece.is_king {
            Self::king_dirs().to_vec()
        } else {
            // For jumps, regular pieces can jump in all diagonal directions
            // in American checkers. Wait -- that's not standard.
            // Standard American checkers: men can only jump forward.
            Self::man_dirs(piece.side)
        };

        let mut jumps = Vec::new();
        for (dr, dc) in dirs {
            let mid = Pos::new(pos.row + dr, pos.col + dc);
            let to = Pos::new(pos.row + 2 * dr, pos.col + 2 * dc);
            if to.is_valid()
                && let Some(mid_piece) = self.get(mid)
                    && mid_piece.side != piece.side && self.get(to).is_none() {
                        jumps.push(CheckersMove::jump(pos, to, mid));
                    }
        }
        jumps
    }

    /// Generate all jump moves for a piece at `pos`, considering a set of
    /// already-captured positions (for multi-jump chains).
    /// `piece` is provided directly because during a chain the piece has
    /// logically moved but the board hasn't been updated. `origin` is the
    /// original square the piece started the chain from (still occupied on
    /// the board but logically vacant).
    fn generate_jumps_for_chain(
        &self,
        pos: Pos,
        piece: Piece,
        captured: &[Pos],
        origin: Option<Pos>,
    ) -> Vec<CheckersMove> {
        let dirs: Vec<(i8, i8)> = if piece.is_king {
            Self::king_dirs().to_vec()
        } else {
            Self::man_dirs(piece.side)
        };

        let mut jumps = Vec::new();
        for (dr, dc) in dirs {
            let mid = Pos::new(pos.row + dr, pos.col + dc);
            let to = Pos::new(pos.row + 2 * dr, pos.col + 2 * dc);
            if to.is_valid() {
                // Cannot jump a piece that was already captured in this chain
                if captured.contains(&mid) {
                    continue;
                }
                if let Some(mid_piece) = self.get(mid)
                    && mid_piece.side != piece.side {
                        // Landing square must be empty. It reads as occupied
                        // if it's the origin square (piece logically left),
                        // so treat that as empty.
                        let landing_empty = self.get(to).is_none()
                            || (origin == Some(to));
                        if landing_empty {
                            jumps.push(CheckersMove::jump(pos, to, mid));
                        }
                    }
            }
        }
        jumps
    }

    /// Recursively build all multi-jump sequences starting from `pos`.
    /// `origin` is the square the piece started the entire chain from (so
    /// we know that square is logically vacant even though the board hasn't
    /// been updated).
    fn build_jump_sequences(
        &self,
        pos: Pos,
        current_chain: &[CheckersMove],
        captured: &[Pos],
        is_king: bool,
        origin: Pos,
    ) -> Vec<MoveSequence> {
        // Determine the piece: on the first call it's on the board; on
        // recursive calls we reconstruct it from the chain context.
        let piece = if current_chain.is_empty() {
            match self.get(pos) {
                Some(p) => p,
                None => return Vec::new(),
            }
        } else {
            Piece {
                side: self.side_to_move,
                is_king,
            }
        };

        // Check for promotion: if a man has just reached the king row, the turn ends.
        if !current_chain.is_empty() && !is_king {
            let promotion_row = match piece.side {
                Side::Red => 7,
                Side::Black => 0,
            };
            if pos.row == promotion_row {
                return vec![MoveSequence::new(current_chain.to_vec())];
            }
        }

        let next_jumps =
            self.generate_jumps_for_chain(pos, piece, captured, Some(origin));

        if next_jumps.is_empty() {
            if current_chain.is_empty() {
                return Vec::new();
            }
            return vec![MoveSequence::new(current_chain.to_vec())];
        }

        let mut sequences = Vec::new();
        for jmp in &next_jumps {
            let mut new_chain = current_chain.to_vec();
            new_chain.push(*jmp);
            let mut new_captured = captured.to_vec();
            if let Some(cap) = jmp.captured {
                new_captured.push(cap);
            }
            let sub =
                self.build_jump_sequences(jmp.to, &new_chain, &new_captured, is_king, origin);
            sequences.extend(sub);
        }
        sequences
    }

    /// Generate all legal move sequences for the current side.
    /// If any jumps exist, only jump moves are returned (mandatory capture rule).
    fn generate_legal_moves(&self) -> Vec<MoveSequence> {
        let side = self.side_to_move;
        let mut all_jumps = Vec::new();
        let mut all_simple = Vec::new();

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if let Some(piece) = self.get(pos) {
                    if piece.side != side {
                        continue;
                    }
                    // Try jumps
                    let jump_seqs =
                        self.build_jump_sequences(pos, &[], &[], piece.is_king, pos);
                    all_jumps.extend(jump_seqs);

                    // Collect simple moves only if we might need them
                    if all_jumps.is_empty() {
                        let simple = self.generate_simple_moves_for(pos);
                        for mv in simple {
                            all_simple.push(MoveSequence::single(mv));
                        }
                    }
                }
            }
        }

        // Mandatory capture: if jumps exist, simple moves are illegal.
        if !all_jumps.is_empty() {
            all_jumps
        } else {
            all_simple
        }
    }

    /// Generate legal moves specifically from a given position.
    fn generate_legal_moves_from(&self, pos: Pos) -> Vec<MoveSequence> {
        self.generate_legal_moves()
            .into_iter()
            .filter(|seq| seq.origin_pos() == pos)
            .collect()
    }

    /// Check if any jump moves exist for the current side.
    fn has_jumps(&self) -> bool {
        let side = self.side_to_move;
        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if let Some(piece) = self.get(pos)
                    && piece.side == side && !self.generate_jumps_for(pos).is_empty() {
                        return true;
                    }
            }
        }
        false
    }

    /// Apply a move sequence to the board, returning the new board state.
    fn apply_move(&self, seq: &MoveSequence) -> Self {
        let mut board = self.clone();
        board.apply_move_in_place(seq);
        board
    }

    /// Apply a move sequence in place.
    fn apply_move_in_place(&mut self, seq: &MoveSequence) {
        if seq.steps.is_empty() {
            return;
        }

        let origin = seq.origin_pos();
        let piece = match self.get(origin) {
            Some(p) => p,
            None => return,
        };

        let mut had_capture = false;

        // Apply each step
        for step in &seq.steps {
            self.set(step.from, None);
            if let Some(cap) = step.captured {
                self.set(cap, None);
                had_capture = true;
            }
            self.set(step.to, Some(piece));
        }

        // Check for king promotion
        let final_pos = seq.to_pos();
        let promotion_row = match piece.side {
            Side::Red => 7,
            Side::Black => 0,
        };
        if !piece.is_king && final_pos.row == promotion_row {
            self.set(final_pos, Some(Piece::king(piece.side)));
        }

        // Update counters
        self.move_count += 1;
        if had_capture {
            self.no_capture_count = 0;
        } else {
            self.no_capture_count += 1;
        }

        // Switch sides
        self.side_to_move = self.side_to_move.opponent();
    }

    /// Check the game result.
    fn check_result(&self) -> GameResult {
        let red_count = self.count_pieces(Side::Red);
        let black_count = self.count_pieces(Side::Black);

        if red_count == 0 {
            return GameResult::BlackWins;
        }
        if black_count == 0 {
            return GameResult::RedWins;
        }

        // Check if the current side has any legal moves
        let moves = self.generate_legal_moves();
        if moves.is_empty() {
            // Current side cannot move: they lose
            return match self.side_to_move {
                Side::Red => GameResult::BlackWins,
                Side::Black => GameResult::RedWins,
            };
        }

        // Draw by 40-move rule (80 half-moves without capture)
        if self.no_capture_count >= 80 {
            return GameResult::Draw;
        }

        GameResult::Ongoing
    }

    /// Evaluate the board for the AI. Positive = good for Black (AI).
    fn evaluate(&self) -> i32 {
        let mut score = 0i32;

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if let Some(piece) = self.get(pos) {
                    let base = if piece.is_king {
                        KING_VALUE
                    } else {
                        MAN_VALUE
                    };

                    // Position bonuses
                    let mut bonus = 0i32;

                    // Center control bonus (columns 2-5, rows 2-5)
                    if (2..=5).contains(&col) && (2..=5).contains(&row) {
                        bonus += CENTER_BONUS;
                    }

                    if !piece.is_king {
                        // Advancement bonus for men
                        let advance = match piece.side {
                            Side::Red => row as i32,
                            Side::Black => (7 - row) as i32,
                        };
                        bonus += advance * ADVANCE_BONUS;

                        // Back row defense bonus
                        let back_row = match piece.side {
                            Side::Red => 0,
                            Side::Black => 7,
                        };
                        if row == back_row {
                            bonus += BACK_ROW_BONUS;
                        }
                    }

                    let val = base + bonus;
                    match piece.side {
                        Side::Black => score += val,
                        Side::Red => score -= val,
                    }
                }
            }
        }

        score
    }
}

// ── AI ──────────────────────────────────────────────────────────────

/// Minimax with alpha-beta pruning. Returns (score, best_move_index).
/// Score is from Black's perspective (positive = good for Black).
fn minimax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
) -> (i32, Option<usize>) {
    let result = board.check_result();
    match result {
        GameResult::BlackWins => return (100_000 + depth, None),
        GameResult::RedWins => return (-100_000 - depth, None),
        GameResult::Draw => return (0, None),
        GameResult::Ongoing => {}
    }

    if depth <= 0 {
        return (board.evaluate(), None);
    }

    let moves = board.generate_legal_moves();
    if moves.is_empty() {
        return (board.evaluate(), None);
    }

    let mut best_idx: Option<usize> = Some(0);

    if maximizing {
        let mut max_eval = i32::MIN;
        for (i, mv) in moves.iter().enumerate() {
            let new_board = board.apply_move(mv);
            let (eval, _) = minimax(&new_board, depth - 1, alpha, beta, false);
            if eval > max_eval {
                max_eval = eval;
                best_idx = Some(i);
            }
            alpha = alpha.max(eval);
            if beta <= alpha {
                break;
            }
        }
        (max_eval, best_idx)
    } else {
        let mut min_eval = i32::MAX;
        for (i, mv) in moves.iter().enumerate() {
            let new_board = board.apply_move(mv);
            let (eval, _) = minimax(&new_board, depth - 1, alpha, beta, true);
            if eval < min_eval {
                min_eval = eval;
                best_idx = Some(i);
            }
            beta = beta.min(eval);
            if beta <= alpha {
                break;
            }
        }
        (min_eval, best_idx)
    }
}

/// Pick the best move for the AI (Black).
fn ai_pick_move(board: &Board) -> Option<MoveSequence> {
    let moves = board.generate_legal_moves();
    if moves.is_empty() {
        return None;
    }

    let maximizing = board.side_to_move == Side::Black;
    let (_, best_idx) = minimax(board, AI_DEPTH, i32::MIN, i32::MAX, maximizing);

    best_idx.and_then(|i| moves.into_iter().nth(i))
}

// ── App ─────────────────────────────────────────────────────────────

/// The Checkers application state.
struct CheckersApp {
    board: Board,
    cursor: Pos,
    selected: Option<Pos>,
    legal_moves_for_selected: Vec<MoveSequence>,
    game_result: GameResult,
    status_message: String,
    move_history: Vec<String>,
    last_move_from: Option<Pos>,
    last_move_to: Option<Pos>,
    red_captured: u32,
    black_captured: u32,
}

impl CheckersApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            cursor: Pos::new(0, 1), // Start on a dark square
            selected: None,
            legal_moves_for_selected: Vec::new(),
            game_result: GameResult::Ongoing,
            status_message: "Red to move".to_string(),
            move_history: Vec::new(),
            last_move_from: None,
            last_move_to: None,
            red_captured: 0,
            black_captured: 0,
        }
    }

    /// Start a new game.
    fn new_game(&mut self) {
        self.board = Board::new();
        self.cursor = Pos::new(0, 1);
        self.selected = None;
        self.legal_moves_for_selected.clear();
        self.game_result = GameResult::Ongoing;
        self.status_message = "Red to move".to_string();
        self.move_history.clear();
        self.last_move_from = None;
        self.last_move_to = None;
        self.red_captured = 0;
        self.black_captured = 0;
    }

    /// Handle clicking on a board square.
    fn click_square(&mut self, pos: Pos) {
        if self.game_result != GameResult::Ongoing {
            return;
        }

        // Only player (Red) can interact during their turn
        if self.board.side_to_move != Side::Red {
            return;
        }

        // Only dark squares are playable
        if !pos.is_dark() {
            return;
        }

        if self.selected.is_some() {
            // A piece is already selected: try to make a move to `pos`
            let matching_move = self
                .legal_moves_for_selected
                .iter()
                .find(|seq| seq.to_pos() == pos)
                .cloned();

            if let Some(mv) = matching_move {
                self.execute_move(&mv);
                return;
            }

            // Clicked on a different own piece? Select it instead.
            if let Some(piece) = self.board.get(pos)
                && piece.side == Side::Red {
                    self.select_piece(pos);
                    return;
                }

            // Clicked elsewhere: deselect
            self.selected = None;
            self.legal_moves_for_selected.clear();
        } else {
            // No piece selected: try to select one
            if let Some(piece) = self.board.get(pos)
                && piece.side == Side::Red {
                    self.select_piece(pos);
                }
        }
    }

    /// Select a piece at `pos` and compute its legal moves.
    fn select_piece(&mut self, pos: Pos) {
        // Check if this piece has any legal moves
        let moves = self.board.generate_legal_moves_from(pos);
        if moves.is_empty() {
            // This piece can't move (e.g., mandatory capture on another piece)
            return;
        }
        self.selected = Some(pos);
        self.legal_moves_for_selected = moves;
    }

    /// Execute a move and handle AI response.
    fn execute_move(&mut self, mv: &MoveSequence) {
        let notation = mv.notation();
        let captured = mv.captured_count();

        self.last_move_from = Some(mv.origin_pos());
        self.last_move_to = Some(mv.to_pos());

        self.board.apply_move_in_place(mv);
        self.black_captured += captured as u32;

        self.move_history.push(notation);

        self.selected = None;
        self.legal_moves_for_selected.clear();

        // Check game state
        self.game_result = self.board.check_result();
        if self.game_result != GameResult::Ongoing {
            self.update_status_for_result();
            return;
        }

        self.status_message = "Black thinking...".to_string();

        // AI's turn
        self.do_ai_move();
    }

    /// Execute the AI's move.
    fn do_ai_move(&mut self) {
        if self.board.side_to_move != Side::Black {
            return;
        }

        if let Some(ai_mv) = ai_pick_move(&self.board) {
            let notation = ai_mv.notation();
            let captured = ai_mv.captured_count();

            self.last_move_from = Some(ai_mv.origin_pos());
            self.last_move_to = Some(ai_mv.to_pos());

            self.board.apply_move_in_place(&ai_mv);
            self.red_captured += captured as u32;

            self.move_history.push(notation);

            self.game_result = self.board.check_result();
            if self.game_result != GameResult::Ongoing {
                self.update_status_for_result();
            } else {
                self.status_message = "Red to move".to_string();
            }
        } else {
            // AI has no moves
            self.game_result = self.board.check_result();
            self.update_status_for_result();
        }
    }

    /// Update status message for game-over state.
    fn update_status_for_result(&mut self) {
        self.status_message = match self.game_result {
            GameResult::RedWins => "Red wins!".to_string(),
            GameResult::BlackWins => "Black wins!".to_string(),
            GameResult::Draw => "Draw!".to_string(),
            GameResult::Ongoing => format!("{} to move", self.board.side_to_move.name()),
        };
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.key {
            Key::N if event.modifiers.ctrl => {
                self.new_game();
            }
            Key::Left => {
                self.cursor.col = (self.cursor.col - 1).max(0);
            }
            Key::Right => {
                self.cursor.col = (self.cursor.col + 1).min(7);
            }
            Key::Up => {
                self.cursor.row = (self.cursor.row + 1).min(7);
            }
            Key::Down => {
                self.cursor.row = (self.cursor.row - 1).max(0);
            }
            Key::Enter | Key::Space => {
                self.click_square(self.cursor);
            }
            Key::Escape => {
                self.selected = None;
                self.legal_moves_for_selected.clear();
            }
            _ => {}
        }
    }

    /// Handle mouse events.
    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            let board_x = event.x - BOARD_OFFSET_X;
            let board_y = event.y - BOARD_OFFSET_Y;

            if board_x >= 0.0
                && board_y >= 0.0
                && board_x < SQUARE_SIZE * 8.0
                && board_y < SQUARE_SIZE * 8.0
            {
                let col = (board_x / SQUARE_SIZE) as i8;
                // Screen y=0 is top, row 7 is at top
                let row = 7 - (board_y / SQUARE_SIZE) as i8;
                let pos = Pos::new(row, col);
                self.click_square(pos);
            }
        }
    }

    /// Handle any event.
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    /// Render the entire UI.
    fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::with_capacity(256);

        // Background
        commands.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: PANEL_X + 250.0,
            height: BOARD_OFFSET_Y + SQUARE_SIZE * 8.0 + 80.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_board(&mut commands);
        self.render_panel(&mut commands);

        commands
    }

    /// Render the checkers board.
    fn render_board(&self, commands: &mut Vec<RenderCommand>) {
        // Board border
        commands.push(RenderCommand::StrokeRect {
            x: BOARD_OFFSET_X - 2.0,
            y: BOARD_OFFSET_Y - 2.0,
            width: SQUARE_SIZE * 8.0 + 4.0,
            height: SQUARE_SIZE * 8.0 + 4.0,
            color: SURFACE1,
            line_width: 2.0,
            corner_radii: CornerRadii::ZERO,
        });

        // Collect legal move destinations for highlighting
        let legal_dests: Vec<Pos> = self
            .legal_moves_for_selected
            .iter()
            .map(|seq| seq.to_pos())
            .collect();

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                // Screen coordinates: row 7 is at top (screen_row 0)
                let screen_row = 7 - row;
                let sx = BOARD_OFFSET_X + col as f32 * SQUARE_SIZE;
                let sy = BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE;

                // Square color
                let is_dark = pos.is_dark();
                let mut square_color = if is_dark { DARK_SQUARE } else { LIGHT_SQUARE };

                // Highlight last move
                if self.last_move_from == Some(pos) || self.last_move_to == Some(pos) {
                    square_color = LAST_MOVE_HIGHLIGHT;
                }

                commands.push(RenderCommand::FillRect {
                    x: sx,
                    y: sy,
                    width: SQUARE_SIZE,
                    height: SQUARE_SIZE,
                    color: square_color,
                    corner_radii: CornerRadii::ZERO,
                });

                // Selected square highlight
                if self.selected == Some(pos) {
                    commands.push(RenderCommand::StrokeRect {
                        x: sx + 1.0,
                        y: sy + 1.0,
                        width: SQUARE_SIZE - 2.0,
                        height: SQUARE_SIZE - 2.0,
                        color: SELECTED_SQUARE,
                        line_width: 3.0,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Cursor
                if self.cursor == pos {
                    commands.push(RenderCommand::StrokeRect {
                        x: sx + 2.0,
                        y: sy + 2.0,
                        width: SQUARE_SIZE - 4.0,
                        height: SQUARE_SIZE - 4.0,
                        color: YELLOW,
                        line_width: 2.0,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Draw piece
                if let Some(piece) = self.board.get(pos) {
                    self.render_piece(commands, piece, sx, sy);
                }

                // Legal move indicator
                if legal_dests.contains(&pos) {
                    let cx = sx + SQUARE_SIZE / 2.0 - DOT_SIZE / 2.0;
                    let cy = sy + SQUARE_SIZE / 2.0 - DOT_SIZE / 2.0;
                    commands.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: DOT_SIZE,
                        height: DOT_SIZE,
                        color: LEGAL_MOVE_DOT,
                        corner_radii: CornerRadii::all(DOT_SIZE / 2.0),
                    });
                }
            }
        }

        // Row labels (1-8)
        for row in 0..8i8 {
            let screen_row = 7 - row;
            let label = format!("{}", row + 1);
            commands.push(RenderCommand::Text {
                x: BOARD_OFFSET_X - 18.0,
                y: BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0 - 7.0,
                text: label,
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Column labels (a-h)
        for col in 0..8i8 {
            let label = format!("{}", (b'a' + col as u8) as char);
            commands.push(RenderCommand::Text {
                x: BOARD_OFFSET_X + col as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0 - 4.0,
                y: BOARD_OFFSET_Y + SQUARE_SIZE * 8.0 + 6.0,
                text: label,
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render a single checker piece as concentric circles.
    fn render_piece(&self, commands: &mut Vec<RenderCommand>, piece: Piece, sx: f32, sy: f32) {
        let (outer_color, inner_color) = match piece.side {
            Side::Red => (RED_PIECE, RED_PIECE_DARK),
            Side::Black => (BLACK_PIECE, BLACK_PIECE_DARK),
        };

        let cx = sx + SQUARE_SIZE / 2.0;
        let cy = sy + SQUARE_SIZE / 2.0;

        // Outer circle
        commands.push(RenderCommand::FillRect {
            x: cx - PIECE_RADIUS,
            y: cy - PIECE_RADIUS,
            width: PIECE_RADIUS * 2.0,
            height: PIECE_RADIUS * 2.0,
            color: outer_color,
            corner_radii: CornerRadii::all(PIECE_RADIUS),
        });

        // Inner circle
        commands.push(RenderCommand::FillRect {
            x: cx - PIECE_INNER_RADIUS,
            y: cy - PIECE_INNER_RADIUS,
            width: PIECE_INNER_RADIUS * 2.0,
            height: PIECE_INNER_RADIUS * 2.0,
            color: inner_color,
            corner_radii: CornerRadii::all(PIECE_INNER_RADIUS),
        });

        // Crown for kings
        if piece.is_king {
            commands.push(RenderCommand::Text {
                x: cx - 8.0,
                y: cy - 10.0,
                text: "\u{265A}".to_string(), // crown symbol
                color: KING_CROWN,
                font_size: CROWN_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    /// Render the info panel on the right side.
    fn render_panel(&self, commands: &mut Vec<RenderCommand>) {
        // Panel background
        commands.push(RenderCommand::FillRect {
            x: PANEL_X,
            y: BOARD_OFFSET_Y,
            width: 230.0,
            height: SQUARE_SIZE * 8.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 15.0,
            text: "Checkers".to_string(),
            color: TEXT_COLOR,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status message
        let status_color = match self.game_result {
            GameResult::RedWins => GREEN,
            GameResult::BlackWins => RED,
            GameResult::Draw => YELLOW,
            GameResult::Ongoing => TEXT_COLOR,
        };
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 50.0,
            text: self.status_message.clone(),
            color: status_color,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Piece counts
        let red_count = self.board.count_pieces(Side::Red);
        let black_count = self.board.count_pieces(Side::Black);
        let red_kings = self.board.count_kings(Side::Red);
        let black_kings = self.board.count_kings(Side::Black);

        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 85.0,
            text: format!("Red:   {} pieces ({} kings)", red_count, red_kings),
            color: RED_PIECE,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 110.0,
            text: format!("Black: {} pieces ({} kings)", black_count, black_kings),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Captured counts
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 145.0,
            text: format!(
                "Captured: Red {} | Black {}",
                self.red_captured, self.black_captured
            ),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Move counter
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 170.0,
            text: format!("Move: {}", self.board.move_count),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Separator
        commands.push(RenderCommand::Line {
            x1: PANEL_X + 10.0,
            y1: BOARD_OFFSET_Y + 195.0,
            x2: PANEL_X + 220.0,
            y2: BOARD_OFFSET_Y + 195.0,
            color: SURFACE0,
            width: 1.0,
        });

        // Move history header
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + 210.0,
            text: "Move History".to_string(),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Recent moves (show last ~18 moves)
        let max_display = 18;
        let start = if self.move_history.len() > max_display {
            self.move_history.len() - max_display
        } else {
            0
        };
        let hist_font_size = 13.0_f32;
        for (i, mv_str) in self.move_history[start..].iter().enumerate() {
            let actual_idx = start + i;
            let label = if actual_idx % 2 == 0 {
                format!("{}. {}", actual_idx / 2 + 1, mv_str)
            } else {
                format!("   {}", mv_str)
            };
            let color = if actual_idx % 2 == 0 {
                RED_PIECE
            } else {
                SUBTEXT0
            };
            commands.push(RenderCommand::Text {
                x: PANEL_X + 15.0,
                y: BOARD_OFFSET_Y + 235.0 + i as f32 * 18.0,
                text: label,
                color,
                font_size: hist_font_size,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }

        // Controls hint at bottom
        commands.push(RenderCommand::Text {
            x: PANEL_X + 15.0,
            y: BOARD_OFFSET_Y + SQUARE_SIZE * 8.0 - 25.0,
            text: "Arrows/Click | Enter/Space | Ctrl+N".to_string(),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });
    }
}

fn main() {
    let _app = CheckersApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper ──────────────────────────────────────────────────────

    /// Place a piece on the board.
    fn place(board: &mut Board, row: i8, col: i8, side: Side, is_king: bool) {
        let piece = if is_king {
            Piece::king(side)
        } else {
            Piece::man(side)
        };
        board.set(Pos::new(row, col), Some(piece));
    }

    // ── Pos tests ───────────────────────────────────────────────────

    #[test]
    fn test_pos_validity() {
        assert!(Pos::new(0, 0).is_valid());
        assert!(Pos::new(7, 7).is_valid());
        assert!(!Pos::new(-1, 0).is_valid());
        assert!(!Pos::new(0, 8).is_valid());
        assert!(!Pos::new(8, 0).is_valid());
    }

    #[test]
    fn test_pos_dark_squares() {
        // (0,1) is dark because 0+1=1 is odd
        assert!(Pos::new(0, 1).is_dark());
        // (0,0) is light because 0+0=0 is even
        assert!(!Pos::new(0, 0).is_dark());
        assert!(Pos::new(1, 0).is_dark());
        assert!(!Pos::new(1, 1).is_dark());
        assert!(Pos::new(3, 4).is_dark());
    }

    #[test]
    fn test_pos_dark_index() {
        assert_eq!(Pos::new(0, 1).dark_index(), Some(0));
        assert_eq!(Pos::new(0, 3).dark_index(), Some(1));
        assert_eq!(Pos::new(1, 0).dark_index(), Some(4));
        // Light square has no dark index
        assert_eq!(Pos::new(0, 0).dark_index(), None);
        // Invalid pos
        assert_eq!(Pos::new(-1, 0).dark_index(), None);
    }

    #[test]
    fn test_pos_label() {
        assert_eq!(Pos::new(0, 0).label(), "a1");
        assert_eq!(Pos::new(7, 7).label(), "h8");
        assert_eq!(Pos::new(3, 4).label(), "e4");
    }

    // ── Side tests ──────────────────────────────────────────────────

    #[test]
    fn test_side_opponent() {
        assert_eq!(Side::Red.opponent(), Side::Black);
        assert_eq!(Side::Black.opponent(), Side::Red);
    }

    #[test]
    fn test_side_name() {
        assert_eq!(Side::Red.name(), "Red");
        assert_eq!(Side::Black.name(), "Black");
    }

    // ── Piece tests ─────────────────────────────────────────────────

    #[test]
    fn test_piece_man() {
        let p = Piece::man(Side::Red);
        assert_eq!(p.side, Side::Red);
        assert!(!p.is_king);
    }

    #[test]
    fn test_piece_king() {
        let p = Piece::king(Side::Black);
        assert_eq!(p.side, Side::Black);
        assert!(p.is_king);
    }

    // ── Board setup tests ───────────────────────────────────────────

    #[test]
    fn test_initial_board_piece_counts() {
        let board = Board::new();
        assert_eq!(board.count_pieces(Side::Red), 12);
        assert_eq!(board.count_pieces(Side::Black), 12);
    }

    #[test]
    fn test_initial_board_no_kings() {
        let board = Board::new();
        assert_eq!(board.count_kings(Side::Red), 0);
        assert_eq!(board.count_kings(Side::Black), 0);
    }

    #[test]
    fn test_initial_board_red_placement() {
        let board = Board::new();
        // Red pieces on rows 0-2, dark squares only
        for row in 0..=2i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if pos.is_dark() {
                    let piece = board.get(pos);
                    assert!(piece.is_some(), "Red piece missing at {:?}", pos);
                    assert_eq!(piece.unwrap().side, Side::Red);
                    assert!(!piece.unwrap().is_king);
                } else {
                    assert!(board.get(pos).is_none(), "Piece on light square {:?}", pos);
                }
            }
        }
    }

    #[test]
    fn test_initial_board_black_placement() {
        let board = Board::new();
        // Black pieces on rows 5-7, dark squares only
        for row in 5..=7i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if pos.is_dark() {
                    let piece = board.get(pos);
                    assert!(piece.is_some(), "Black piece missing at {:?}", pos);
                    assert_eq!(piece.unwrap().side, Side::Black);
                } else {
                    assert!(board.get(pos).is_none());
                }
            }
        }
    }

    #[test]
    fn test_initial_board_middle_rows_empty() {
        let board = Board::new();
        for row in 3..=4i8 {
            for col in 0..8i8 {
                assert!(board.get(Pos::new(row, col)).is_none());
            }
        }
    }

    #[test]
    fn test_empty_board() {
        let board = Board::empty();
        for row in 0..8i8 {
            for col in 0..8i8 {
                assert!(board.get(Pos::new(row, col)).is_none());
            }
        }
        assert_eq!(board.side_to_move, Side::Red);
        assert_eq!(board.move_count, 0);
    }

    #[test]
    fn test_initial_board_red_to_move() {
        let board = Board::new();
        assert_eq!(board.side_to_move, Side::Red);
    }

    // ── Simple move generation ──────────────────────────────────────

    #[test]
    fn test_red_man_moves_forward() {
        let mut board = Board::empty();
        place(&mut board, 3, 2, Side::Red, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 2));
        // Red man moves diagonally forward (up): (4,1) and (4,3)
        assert_eq!(moves.len(), 2);
        assert!(moves.iter().any(|m| m.to == Pos::new(4, 1)));
        assert!(moves.iter().any(|m| m.to == Pos::new(4, 3)));
    }

    #[test]
    fn test_black_man_moves_forward() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 4, 3, Side::Black, false);
        let moves = board.generate_simple_moves_for(Pos::new(4, 3));
        // Black man moves diagonally forward (down): (3,2) and (3,4)
        assert_eq!(moves.len(), 2);
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 2)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 4)));
    }

    #[test]
    fn test_man_blocked_by_own_piece() {
        let mut board = Board::empty();
        place(&mut board, 3, 2, Side::Red, false);
        place(&mut board, 4, 3, Side::Red, false); // blocks forward-right
        let moves = board.generate_simple_moves_for(Pos::new(3, 2));
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, Pos::new(4, 1));
    }

    #[test]
    fn test_man_blocked_by_opponent() {
        let mut board = Board::empty();
        place(&mut board, 3, 2, Side::Red, false);
        place(&mut board, 4, 1, Side::Black, false);
        place(&mut board, 4, 3, Side::Black, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 2));
        // Both diagonal squares occupied
        assert_eq!(moves.len(), 0);
    }

    #[test]
    fn test_man_at_edge() {
        let mut board = Board::empty();
        place(&mut board, 3, 0, Side::Red, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 0));
        // Only one diagonal (right) is on the board
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, Pos::new(4, 1));
    }

    #[test]
    fn test_king_moves_all_directions() {
        let mut board = Board::empty();
        place(&mut board, 4, 3, Side::Red, true);
        let moves = board.generate_simple_moves_for(Pos::new(4, 3));
        // King can move in 4 diagonal directions
        assert_eq!(moves.len(), 4);
        assert!(moves.iter().any(|m| m.to == Pos::new(5, 4)));
        assert!(moves.iter().any(|m| m.to == Pos::new(5, 2)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 4)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 2)));
    }

    #[test]
    fn test_king_at_corner() {
        let mut board = Board::empty();
        place(&mut board, 0, 1, Side::Red, true);
        let moves = board.generate_simple_moves_for(Pos::new(0, 1));
        // Corner king has limited moves: (1,0) and (1,2)
        assert_eq!(moves.len(), 2);
    }

    // ── Jump generation ─────────────────────────────────────────────

    #[test]
    fn test_red_man_single_jump() {
        let mut board = Board::empty();
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        let jumps = board.generate_jumps_for(Pos::new(2, 1));
        assert_eq!(jumps.len(), 1);
        assert_eq!(jumps[0].to, Pos::new(4, 3));
        assert_eq!(jumps[0].captured, Some(Pos::new(3, 2)));
    }

    #[test]
    fn test_no_jump_over_own_piece() {
        let mut board = Board::empty();
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Red, false); // own piece
        let jumps = board.generate_jumps_for(Pos::new(2, 1));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_no_jump_when_landing_occupied() {
        let mut board = Board::empty();
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        place(&mut board, 4, 3, Side::Red, false); // landing blocked
        let jumps = board.generate_jumps_for(Pos::new(2, 1));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_no_jump_off_board() {
        let mut board = Board::empty();
        place(&mut board, 6, 6, Side::Red, false);
        place(&mut board, 7, 7, Side::Black, false);
        // Jump would land at (8,8) which is off the board
        let jumps = board.generate_jumps_for(Pos::new(6, 6));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_king_jumps_backward() {
        let mut board = Board::empty();
        place(&mut board, 4, 3, Side::Red, true); // king
        place(&mut board, 3, 2, Side::Black, false);
        let jumps = board.generate_jumps_for(Pos::new(4, 3));
        // King can jump backward
        assert!(jumps.iter().any(|j| j.to == Pos::new(2, 1)));
    }

    #[test]
    fn test_king_jumps_all_directions() {
        let mut board = Board::empty();
        place(&mut board, 4, 3, Side::Red, true); // king
        place(&mut board, 5, 4, Side::Black, false);
        place(&mut board, 3, 2, Side::Black, false);
        let jumps = board.generate_jumps_for(Pos::new(4, 3));
        assert_eq!(jumps.len(), 2);
    }

    // ── Multi-jump chains ───────────────────────────────────────────

    #[test]
    fn test_double_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 1, Side::Red, false);
        place(&mut board, 1, 2, Side::Black, false);
        place(&mut board, 3, 4, Side::Black, false);
        // Red at (0,1) jumps (1,2) to (2,3), then jumps (3,4) to (4,5)
        let moves = board.generate_legal_moves();
        let jump_moves: Vec<_> = moves.iter().filter(|m| m.is_jump()).collect();
        assert!(!jump_moves.is_empty());
        // Should find a 2-step chain
        let has_double = jump_moves.iter().any(|m| m.steps.len() == 2);
        assert!(has_double, "Should find a double-jump sequence");
    }

    #[test]
    fn test_triple_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 1, Side::Red, false);
        place(&mut board, 1, 2, Side::Black, false);
        place(&mut board, 3, 4, Side::Black, false);
        place(&mut board, 5, 6, Side::Black, false);
        let moves = board.generate_legal_moves();
        let has_triple = moves.iter().any(|m| m.steps.len() == 3);
        assert!(has_triple, "Should find a triple-jump sequence");
    }

    #[test]
    fn test_multi_jump_captures_all() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 1, Side::Red, false);
        place(&mut board, 1, 2, Side::Black, false);
        place(&mut board, 3, 4, Side::Black, false);
        let moves = board.generate_legal_moves();
        let double = moves.iter().find(|m| m.steps.len() == 2).unwrap();
        assert_eq!(double.captured_count(), 2);
    }

    // ── Mandatory capture ───────────────────────────────────────────

    #[test]
    fn test_mandatory_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 2, 5, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        // Piece at (2,1) can jump, piece at (2,5) can only move forward.
        // Mandatory capture means only jumps are returned.
        let moves = board.generate_legal_moves();
        assert!(
            moves.iter().all(|m| m.is_jump()),
            "All moves must be jumps when jumps are available"
        );
    }

    #[test]
    fn test_no_mandatory_capture_when_no_jumps() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        // No opponent pieces to jump
        let moves = board.generate_legal_moves();
        assert!(
            moves.iter().all(|m| !m.is_jump()),
            "All moves should be simple when no jumps available"
        );
    }

    // ── King promotion ──────────────────────────────────────────────

    #[test]
    fn test_red_promotion() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 6, 0, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(6, 0), Pos::new(7, 1)));
        board.apply_move_in_place(&mv);
        let promoted = board.get(Pos::new(7, 1));
        assert!(promoted.is_some());
        assert!(promoted.unwrap().is_king, "Red should be promoted at row 7");
    }

    #[test]
    fn test_black_promotion() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 1, 0, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(1, 0), Pos::new(0, 1)));
        board.apply_move_in_place(&mv);
        let promoted = board.get(Pos::new(0, 1));
        assert!(promoted.is_some());
        assert!(
            promoted.unwrap().is_king,
            "Black should be promoted at row 0"
        );
    }

    #[test]
    fn test_no_promotion_in_middle() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 3, 2, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(3, 2), Pos::new(4, 3)));
        board.apply_move_in_place(&mv);
        let piece = board.get(Pos::new(4, 3));
        assert!(piece.is_some());
        assert!(!piece.unwrap().is_king, "Should not promote in the middle");
    }

    #[test]
    fn test_promotion_via_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 5, 2, Side::Red, false);
        place(&mut board, 6, 3, Side::Black, false);
        // Red jumps from (5,2) over (6,3) to (7,4) -> promotion
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(5, 2),
            Pos::new(7, 4),
            Pos::new(6, 3),
        ));
        board.apply_move_in_place(&mv);
        let piece = board.get(Pos::new(7, 4));
        assert!(piece.is_some());
        assert!(piece.unwrap().is_king, "Should promote after jump to row 7");
        // Captured piece should be removed
        assert!(board.get(Pos::new(6, 3)).is_none());
    }

    #[test]
    fn test_promotion_stops_multi_jump() {
        // When a man reaches the promotion row mid-chain, the turn ends in American checkers.
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 5, 0, Side::Red, false);
        place(&mut board, 6, 1, Side::Black, false);
        // Place another black piece that could be jumped after promotion
        place(&mut board, 6, 3, Side::Black, false);
        let moves = board.generate_legal_moves();
        // The jump (5,0) -> (7,2) should stop at promotion, not continue
        let jump_from = moves
            .iter()
            .filter(|m| m.origin_pos() == Pos::new(5, 0) && m.is_jump())
            .collect::<Vec<_>>();
        assert!(!jump_from.is_empty());
        // All chains from this piece should end at row 7
        for jm in &jump_from {
            assert_eq!(jm.to_pos().row, 7, "Chain should stop at promotion row");
            assert_eq!(jm.steps.len(), 1, "Should be a single jump (stops at promotion)");
        }
    }

    // ── Move application ────────────────────────────────────────────

    #[test]
    fn test_simple_move_applies() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2)));
        board.apply_move_in_place(&mv);
        assert!(board.get(Pos::new(2, 1)).is_none());
        assert!(board.get(Pos::new(3, 2)).is_some());
        assert_eq!(board.side_to_move, Side::Black);
    }

    #[test]
    fn test_jump_removes_captured() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 1),
            Pos::new(4, 3),
            Pos::new(3, 2),
        ));
        board.apply_move_in_place(&mv);
        assert!(board.get(Pos::new(2, 1)).is_none());
        assert!(board.get(Pos::new(3, 2)).is_none(), "Captured piece removed");
        assert!(board.get(Pos::new(4, 3)).is_some());
    }

    #[test]
    fn test_side_switches_after_move() {
        let mut board = Board::new();
        assert_eq!(board.side_to_move, Side::Red);
        let moves = board.generate_legal_moves();
        board.apply_move_in_place(&moves[0]);
        assert_eq!(board.side_to_move, Side::Black);
    }

    #[test]
    fn test_move_count_increments() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        assert_eq!(board.move_count, 0);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2)));
        board.apply_move_in_place(&mv);
        assert_eq!(board.move_count, 1);
    }

    #[test]
    fn test_no_capture_count_increments() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2)));
        board.apply_move_in_place(&mv);
        assert_eq!(board.no_capture_count, 1);
    }

    #[test]
    fn test_capture_resets_no_capture_count() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        board.no_capture_count = 10;
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 1),
            Pos::new(4, 3),
            Pos::new(3, 2),
        ));
        board.apply_move_in_place(&mv);
        assert_eq!(board.no_capture_count, 0);
    }

    // ── Game result detection ───────────────────────────────────────

    #[test]
    fn test_red_wins_no_black_pieces() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 0, 1, Side::Red, false);
        // No black pieces -> Red wins
        assert_eq!(board.check_result(), GameResult::RedWins);
    }

    #[test]
    fn test_correct_win_detection() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 1, Side::Red, false);
        // No black pieces -> Red wins
        assert_eq!(board.check_result(), GameResult::RedWins);
    }

    #[test]
    fn test_black_wins_no_red_pieces() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 7, 0, Side::Black, false);
        // No red pieces -> Black wins
        assert_eq!(board.check_result(), GameResult::BlackWins);
    }

    #[test]
    fn test_draw_by_no_capture_rule() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        board.no_capture_count = 80;
        place(&mut board, 0, 1, Side::Red, true);
        place(&mut board, 7, 0, Side::Black, true);
        assert_eq!(board.check_result(), GameResult::Draw);
    }

    #[test]
    fn test_no_moves_loses() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        // Red piece in corner, blocked
        place(&mut board, 7, 0, Side::Red, false);
        // Red can't move forward (already at top row for a man),
        // so Red has no moves -> Black wins
        place(&mut board, 0, 1, Side::Black, false);
        let result = board.check_result();
        assert_eq!(result, GameResult::BlackWins);
    }

    #[test]
    fn test_ongoing_game() {
        let board = Board::new();
        assert_eq!(board.check_result(), GameResult::Ongoing);
    }

    // ── AI tests ────────────────────────────────────────────────────

    #[test]
    fn test_ai_picks_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 4, 3, Side::Black, false);
        place(&mut board, 3, 4, Side::Red, false);
        // Give red a piece so game is not over
        place(&mut board, 0, 1, Side::Red, false);
        let ai_mv = ai_pick_move(&board);
        assert!(ai_mv.is_some());
        assert!(ai_mv.unwrap().is_jump(), "AI should prefer capture");
    }

    #[test]
    fn test_ai_returns_none_when_no_moves() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        // No black pieces
        place(&mut board, 0, 1, Side::Red, false);
        let ai_mv = ai_pick_move(&board);
        assert!(ai_mv.is_none());
    }

    #[test]
    fn test_ai_returns_move_when_available() {
        let board = Board::new();
        // Change to black's turn for AI
        let mut board = board;
        board.side_to_move = Side::Black;
        let ai_mv = ai_pick_move(&board);
        assert!(ai_mv.is_some());
    }

    #[test]
    fn test_minimax_terminal_black_wins() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 7, 0, Side::Black, false);
        // No red pieces -> Black wins
        let (score, _) = minimax(&board, 2, i32::MIN, i32::MAX, true);
        assert!(score > 0, "Black win should have positive score");
    }

    #[test]
    fn test_minimax_terminal_red_wins() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 0, 1, Side::Red, false);
        // No black pieces -> Red wins
        let (score, _) = minimax(&board, 2, i32::MIN, i32::MAX, true);
        assert!(score < 0, "Red win should have negative score");
    }

    // ── Evaluate tests ──────────────────────────────────────────────

    #[test]
    fn test_evaluate_balanced() {
        let board = Board::new();
        let score = board.evaluate();
        // Initial position should be roughly balanced (0)
        assert!(
            score.abs() < 50,
            "Initial position should be roughly balanced, got {}",
            score
        );
    }

    #[test]
    fn test_evaluate_black_advantage() {
        let mut board = Board::empty();
        place(&mut board, 4, 3, Side::Black, false);
        place(&mut board, 4, 5, Side::Black, false);
        place(&mut board, 0, 1, Side::Red, false);
        let score = board.evaluate();
        assert!(score > 0, "Black should have positive score with more pieces");
    }

    #[test]
    fn test_evaluate_king_worth_more() {
        let mut board = Board::empty();
        place(&mut board, 4, 3, Side::Black, true); // king
        place(&mut board, 0, 1, Side::Red, false);  // man
        let score = board.evaluate();
        assert!(
            score > MAN_VALUE,
            "King should be worth significantly more than man"
        );
    }

    // ── MoveSequence tests ──────────────────────────────────────────

    #[test]
    fn test_move_sequence_notation_simple() {
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2)));
        let notation = mv.notation();
        assert_eq!(notation, "b3-c4");
    }

    #[test]
    fn test_move_sequence_notation_jump() {
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 1),
            Pos::new(4, 3),
            Pos::new(3, 2),
        ));
        let notation = mv.notation();
        assert_eq!(notation, "b3xd5");
    }

    #[test]
    fn test_move_sequence_notation_double_jump() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 1), Pos::new(2, 3), Pos::new(1, 2)),
            CheckersMove::jump(Pos::new(2, 3), Pos::new(4, 5), Pos::new(3, 4)),
        ]);
        let notation = mv.notation();
        assert_eq!(notation, "b1xd3xf5");
    }

    #[test]
    fn test_move_sequence_from_to() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 1), Pos::new(2, 3), Pos::new(1, 2)),
            CheckersMove::jump(Pos::new(2, 3), Pos::new(4, 5), Pos::new(3, 4)),
        ]);
        assert_eq!(mv.origin_pos(), Pos::new(0, 1));
        assert_eq!(mv.to_pos(), Pos::new(4, 5));
    }

    #[test]
    fn test_move_sequence_captured_count() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 1), Pos::new(2, 3), Pos::new(1, 2)),
            CheckersMove::jump(Pos::new(2, 3), Pos::new(4, 5), Pos::new(3, 4)),
        ]);
        assert_eq!(mv.captured_count(), 2);
    }

    // ── CheckersApp tests ───────────────────────────────────────────

    #[test]
    fn test_app_new() {
        let app = CheckersApp::new();
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert_eq!(app.board.side_to_move, Side::Red);
        assert!(app.selected.is_none());
        assert!(app.move_history.is_empty());
    }

    #[test]
    fn test_app_new_game_resets() {
        let mut app = CheckersApp::new();
        app.game_result = GameResult::RedWins;
        app.move_history.push("test".to_string());
        app.red_captured = 5;
        app.new_game();
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert!(app.move_history.is_empty());
        assert_eq!(app.red_captured, 0);
    }

    #[test]
    fn test_click_light_square_does_nothing() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(0, 0)); // light square
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_click_empty_dark_square_does_nothing() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(3, 2)); // empty dark square
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_click_own_piece_selects() {
        let mut app = CheckersApp::new();
        // Red piece at (2,1)
        app.click_square(Pos::new(2, 1));
        assert_eq!(app.selected, Some(Pos::new(2, 1)));
        assert!(!app.legal_moves_for_selected.is_empty());
    }

    #[test]
    fn test_click_opponent_piece_no_select() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(5, 0)); // Black piece
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_escape_deselects() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 1)); // select
        assert!(app.selected.is_some());
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&event);
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_game_over_prevents_moves() {
        let mut app = CheckersApp::new();
        app.game_result = GameResult::RedWins;
        app.click_square(Pos::new(2, 1));
        assert!(app.selected.is_none(), "Should not select when game is over");
    }

    #[test]
    fn test_cursor_movement() {
        let mut app = CheckersApp::new();
        let right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&right);
        assert_eq!(app.cursor.col, 2);

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&up);
        assert_eq!(app.cursor.row, 1);

        let left = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&left);
        assert_eq!(app.cursor.col, 1);

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&down);
        assert_eq!(app.cursor.row, 0);
    }

    #[test]
    fn test_cursor_bounds() {
        let mut app = CheckersApp::new();
        app.cursor = Pos::new(0, 0);
        let left = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&left);
        assert_eq!(app.cursor.col, 0, "Cursor should not go below 0");

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&down);
        assert_eq!(app.cursor.row, 0, "Cursor should not go below 0");
    }

    #[test]
    fn test_cursor_upper_bounds() {
        let mut app = CheckersApp::new();
        app.cursor = Pos::new(7, 7);
        let right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&right);
        assert_eq!(app.cursor.col, 7, "Cursor should not exceed 7");

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&up);
        assert_eq!(app.cursor.row, 7);
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = CheckersApp::new();
        let commands = app.render();
        assert!(!commands.is_empty(), "Render should produce commands");
        // Should have at least: background + 64 squares + board border + pieces
        assert!(commands.len() > 70, "Should produce many render commands");
    }

    #[test]
    fn test_render_selected_square_highlight() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 1)); // select a red piece
        let commands = app.render();
        let has_selection = commands.iter().any(|c| {
            matches!(c, RenderCommand::StrokeRect { color, .. } if *color == SELECTED_SQUARE)
        });
        assert!(has_selection, "Should render selected square highlight");
    }

    #[test]
    fn test_render_legal_move_indicators() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 1)); // select a piece with moves
        let commands = app.render();
        let dot_count = commands
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::FillRect { color, .. } if *color == LEGAL_MOVE_DOT)
            })
            .count();
        assert!(
            dot_count >= 1,
            "Should show legal move dots, got {dot_count}"
        );
    }

    #[test]
    fn test_render_cursor_highlight() {
        let app = CheckersApp::new();
        let commands = app.render();
        let has_cursor = commands.iter().any(|c| {
            matches!(c, RenderCommand::StrokeRect { color, .. } if *color == YELLOW)
        });
        assert!(has_cursor, "Should render cursor highlight");
    }

    #[test]
    fn test_render_pieces_on_board() {
        let app = CheckersApp::new();
        let commands = app.render();
        // Count piece circles (each piece = 2 FillRects with rounded corners)
        let circle_count = commands
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::FillRect { corner_radii, color, .. }
                    if *corner_radii != CornerRadii::ZERO
                    && (*color == RED_PIECE || *color == BLACK_PIECE
                        || *color == RED_PIECE_DARK || *color == BLACK_PIECE_DARK))
            })
            .count();
        // 24 pieces, 2 circles each = 48
        assert_eq!(circle_count, 48, "Should render all 24 pieces as circles");
    }

    // ── Mouse event tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_on_piece() {
        let mut app = CheckersApp::new();
        // Click on a red piece at (2,1). Screen: col=1, screen_row=5
        let x = BOARD_OFFSET_X + 1.0 * SQUARE_SIZE + SQUARE_SIZE / 2.0;
        let y = BOARD_OFFSET_Y + 5.0 * SQUARE_SIZE + SQUARE_SIZE / 2.0; // row 2 = screen row 5
        let event = MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse(&event);
        assert_eq!(app.selected, Some(Pos::new(2, 1)));
    }

    #[test]
    fn test_mouse_click_outside_board() {
        let mut app = CheckersApp::new();
        let event = MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse(&event);
        assert!(app.selected.is_none());
    }

    // ── Event dispatch tests ────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = CheckersApp::new();
        let event = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.cursor.col, 2);
    }

    #[test]
    fn test_handle_event_resize_ignored() {
        let mut app = CheckersApp::new();
        let event = Event::Resize {
            width: 800,
            height: 600,
        };
        app.handle_event(&event);
        assert_eq!(app.game_result, GameResult::Ongoing);
    }

    #[test]
    fn test_handle_key_not_pressed_ignored() {
        let mut app = CheckersApp::new();
        let event = KeyEvent {
            key: Key::Right,
            pressed: false, // key released, not pressed
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&event);
        // Cursor should not have moved
        assert_eq!(app.cursor.col, 1);
    }

    // ── Full game flow tests ────────────────────────────────────────

    #[test]
    fn test_make_move_and_ai_responds() {
        let mut app = CheckersApp::new();
        // Select a red piece and make a valid move
        app.click_square(Pos::new(2, 1)); // select
        assert!(app.selected.is_some());

        // Find a legal move destination
        let dest = app.legal_moves_for_selected[0].to_pos();
        app.click_square(dest); // move

        // After red moves, AI should have responded
        assert_eq!(app.board.side_to_move, Side::Red);
        assert!(!app.move_history.is_empty());
        // Should have 2 moves in history (Red + Black)
        assert_eq!(app.move_history.len(), 2);
    }

    #[test]
    fn test_select_different_piece() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 1)); // select first piece
        assert_eq!(app.selected, Some(Pos::new(2, 1)));
        app.click_square(Pos::new(2, 3)); // select different piece
        assert_eq!(app.selected, Some(Pos::new(2, 3)));
    }

    #[test]
    fn test_click_invalid_deselects() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 1)); // select
        assert!(app.selected.is_some());
        app.click_square(Pos::new(4, 3)); // empty square, not a legal move
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_ctrl_n_new_game() {
        let mut app = CheckersApp::new();
        app.game_result = GameResult::RedWins;
        app.red_captured = 3;
        let event = KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        app.handle_key(&event);
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert_eq!(app.red_captured, 0);
    }

    // ── Has jumps tests ─────────────────────────────────────────────

    #[test]
    fn test_has_jumps_initial() {
        let board = Board::new();
        assert!(!board.has_jumps(), "No jumps at start");
    }

    #[test]
    fn test_has_jumps_when_available() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        place(&mut board, 3, 2, Side::Black, false);
        assert!(board.has_jumps());
    }

    // ── CheckersMove tests ──────────────────────────────────────────

    #[test]
    fn test_checkers_move_simple() {
        let mv = CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2));
        assert!(!mv.is_jump());
        assert!(mv.captured.is_none());
    }

    #[test]
    fn test_checkers_move_jump() {
        let mv = CheckersMove::jump(Pos::new(2, 1), Pos::new(4, 3), Pos::new(3, 2));
        assert!(mv.is_jump());
        assert_eq!(mv.captured, Some(Pos::new(3, 2)));
    }

    // ── Board::apply_move (non-destructive) test ────────────────────

    #[test]
    fn test_apply_move_returns_new_board() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 1), Pos::new(3, 2)));
        let new_board = board.apply_move(&mv);
        // Original board unchanged
        assert!(board.get(Pos::new(2, 1)).is_some());
        // New board has the move applied
        assert!(new_board.get(Pos::new(2, 1)).is_none());
        assert!(new_board.get(Pos::new(3, 2)).is_some());
    }

    // ── Legal moves from specific position ──────────────────────────

    #[test]
    fn test_generate_legal_moves_from() {
        let board = Board::new();
        let moves = board.generate_legal_moves_from(Pos::new(2, 1));
        // Piece at (2,1) can move to (3,0) and (3,2)
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn test_generate_legal_moves_from_empty() {
        let board = Board::new();
        let moves = board.generate_legal_moves_from(Pos::new(4, 3));
        assert!(moves.is_empty(), "No piece = no moves");
    }

    // ── Initial legal moves ─────────────────────────────────────────

    #[test]
    fn test_initial_red_legal_moves_count() {
        let board = Board::new();
        let moves = board.generate_legal_moves();
        // Red has pieces on row 2 that can move. Row 2 dark squares: (2,1),(2,3),(2,5),(2,7)
        // Each can move 1-2 diag forward -> 7 moves total
        assert_eq!(moves.len(), 7, "Red should have 7 opening moves");
    }

    // ── Board directions ────────────────────────────────────────────

    #[test]
    fn test_forward_dirs() {
        assert_eq!(Board::forward_dirs(Side::Red), &[1]);
        assert_eq!(Board::forward_dirs(Side::Black), &[-1]);
    }

    #[test]
    fn test_king_dirs() {
        let dirs = Board::king_dirs();
        assert_eq!(dirs.len(), 4);
    }

    #[test]
    fn test_man_dirs() {
        let red_dirs = Board::man_dirs(Side::Red);
        assert_eq!(red_dirs.len(), 2);
        assert!(red_dirs.contains(&(1, 1)));
        assert!(red_dirs.contains(&(1, -1)));

        let black_dirs = Board::man_dirs(Side::Black);
        assert_eq!(black_dirs.len(), 2);
        assert!(black_dirs.contains(&(-1, 1)));
        assert!(black_dirs.contains(&(-1, -1)));
    }

    // ── Generate legal moves for specific side ──────────────────────

    #[test]
    fn test_cannot_select_piece_with_no_legal_moves() {
        // When mandatory capture is in effect, a piece without a jump can't be selected.
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 1, Side::Red, false); // no jump available
        place(&mut board, 2, 5, Side::Red, false);
        place(&mut board, 3, 6, Side::Black, false); // jump available for (2,5)
        // (2,5) can jump to (4,7). Mandatory capture.
        let mut app = CheckersApp {
            board,
            cursor: Pos::new(0, 1),
            selected: None,
            legal_moves_for_selected: Vec::new(),
            game_result: GameResult::Ongoing,
            status_message: String::new(),
            move_history: Vec::new(),
            last_move_from: None,
            last_move_to: None,
            red_captured: 0,
            black_captured: 0,
        };
        // Try to select (2,1) which has no jump
        app.click_square(Pos::new(2, 1));
        assert!(
            app.selected.is_none(),
            "Should not select a piece that cannot participate in mandatory capture"
        );
        // Select (2,5) which has a jump
        app.click_square(Pos::new(2, 5));
        assert_eq!(app.selected, Some(Pos::new(2, 5)));
    }

    // ── King in multi-jump ──────────────────────────────────────────

    #[test]
    fn test_king_multi_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 4, 3, Side::Red, true); // king
        place(&mut board, 3, 2, Side::Black, false);
        place(&mut board, 3, 4, Side::Black, false);
        // King at (4,3) can jump (3,2) to (2,1), then potentially (3,4)... wait,
        // from (2,1) to jump (3,4) would require being at adjacent diagonal which doesn't work.
        // Let's set up a proper king multi-jump:
        // King at (4,3), black at (5,4) and (3,4). King jumps (5,4) to (6,5), then backward
        // to jump... that won't chain either. Let's use:
        // King at (4,3), black at (3,2) and (1,2).
        // Jump (3,2) -> (2,1), jump (1,2) -> (0,3)
        let mut board2 = Board::empty();
        board2.side_to_move = Side::Red;
        place(&mut board2, 4, 3, Side::Red, true);
        place(&mut board2, 3, 2, Side::Black, false);
        place(&mut board2, 1, 2, Side::Black, false);
        let moves = board2.generate_legal_moves();
        let multi = moves.iter().find(|m| m.steps.len() == 2);
        assert!(multi.is_some(), "King should be able to do multi-jump");
    }

    // ── Apply move multi-jump ───────────────────────────────────────

    #[test]
    fn test_apply_multi_jump_removes_all_captured() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 1, Side::Red, false);
        place(&mut board, 1, 2, Side::Black, false);
        place(&mut board, 3, 4, Side::Black, false);
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 1), Pos::new(2, 3), Pos::new(1, 2)),
            CheckersMove::jump(Pos::new(2, 3), Pos::new(4, 5), Pos::new(3, 4)),
        ]);
        board.apply_move_in_place(&mv);
        assert!(board.get(Pos::new(1, 2)).is_none(), "First captured removed");
        assert!(board.get(Pos::new(3, 4)).is_none(), "Second captured removed");
        assert!(board.get(Pos::new(4, 5)).is_some(), "Piece at destination");
        assert!(board.get(Pos::new(0, 1)).is_none(), "Origin cleared");
    }

    // ── Game result: stalemate (no moves) ───────────────────────────

    #[test]
    fn test_stalemate_red_blocked() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        // Red man at top row can't move forward
        place(&mut board, 7, 6, Side::Red, false);
        // Black piece to keep game going
        place(&mut board, 0, 1, Side::Black, false);
        let result = board.check_result();
        assert_eq!(result, GameResult::BlackWins, "Blocked player loses");
    }
}
