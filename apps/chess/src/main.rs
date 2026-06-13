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

//! SlateOS Chess — a full chess game with AI opponent.
//!
//! Features a complete chess engine with legal move generation (including
//! castling, en passant, pawn promotion), check/checkmate/stalemate detection,
//! a minimax AI with alpha-beta pruning, move history in algebraic notation,
//! captured pieces display, and a Catppuccin Mocha themed board.

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

// ── Board colors ────────────────────────────────────────────────────
const LIGHT_SQUARE: Color = Color::from_hex(0x9CA0B0);
const DARK_SQUARE: Color = Color::from_hex(0x585B70);
const SELECTED_SQUARE: Color = Color::from_hex(0x89B4FA);
const LEGAL_MOVE_DOT: Color = Color::rgba(166, 227, 161, 140);
const LAST_MOVE_HIGHLIGHT: Color = Color::rgba(250, 179, 135, 80);
const CHECK_HIGHLIGHT: Color = Color::rgba(243, 139, 168, 120);

// ── Layout constants ────────────────────────────────────────────────
const SQUARE_SIZE: f32 = 64.0;
const BOARD_OFFSET_X: f32 = 40.0;
const BOARD_OFFSET_Y: f32 = 60.0;
const PANEL_X: f32 = BOARD_OFFSET_X + SQUARE_SIZE * 8.0 + 20.0;
const PIECE_FONT_SIZE: f32 = 38.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const MOVE_FONT_SIZE: f32 = 13.0;
const DOT_RADIUS: f32 = 8.0;

// ── Piece values for AI evaluation ─────────────────────────────────
const PAWN_VALUE: i32 = 100;
const KNIGHT_VALUE: i32 = 320;
const BISHOP_VALUE: i32 = 330;
const ROOK_VALUE: i32 = 500;
const QUEEN_VALUE: i32 = 900;
const KING_VALUE: i32 = 20_000;

// ── AI search depth ─────────────────────────────────────────────────
const AI_DEPTH: i32 = 3;

// ── Piece-square tables (for White; mirrored for Black) ─────────────
// Values from a simplified evaluation: bonus for good positions.
// Indexed as [rank * 8 + file] where rank 0 = rank 1 (white's back rank).
const PAWN_TABLE: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 50, 50, 50, 50, 50, 50, 50, 50, 10, 10, 20, 30, 30, 20, 10, 10, 5, 5,
    10, 25, 25, 10, 5, 5, 0, 0, 0, 20, 20, 0, 0, 0, 5, -5, -10, 0, 0, -10, -5, 5, 5, 10, 10, -20,
    -20, 10, 10, 5, 0, 0, 0, 0, 0, 0, 0, 0,
];

const KNIGHT_TABLE: [i32; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, -40, -20, 0, 0, 0, 0, -20, -40, -30, 0, 10, 15, 15, 10,
    0, -30, -30, 5, 15, 20, 20, 15, 5, -30, -30, 0, 15, 20, 20, 15, 0, -30, -30, 5, 10, 15, 15, 10,
    5, -30, -40, -20, 0, 5, 5, 0, -20, -40, -50, -40, -30, -30, -30, -30, -40, -50,
];

const BISHOP_TABLE: [i32; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 10, 10, 10, 10, 0,
    -10, -10, 5, 5, 10, 10, 5, 5, -10, -10, 0, 5, 10, 10, 5, 0, -10, -10, 10, 10, 10, 10, 10, 10,
    -10, -10, 5, 0, 0, 0, 0, 5, -10, -20, -10, -10, -10, -10, -10, -10, -20,
];

const ROOK_TABLE: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 5, 10, 10, 10, 10, 10, 10, 5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0,
    0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, 0, 0,
    0, 5, 5, 0, 0, 0,
];

const QUEEN_TABLE: [i32; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 5, 5, 5, 5, 0, -10,
    -5, 0, 5, 5, 5, 5, 0, -5, 0, 0, 5, 5, 5, 5, 0, -5, -10, 5, 5, 5, 5, 5, 0, -10, -10, 0, 5, 0, 0,
    0, 0, -10, -20, -10, -10, -5, -5, -10, -10, -20,
];

const KING_MIDDLEGAME_TABLE: [i32; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40,
    -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30, -20, -30, -30, -40, -40, -30,
    -30, -20, -10, -20, -20, -20, -20, -20, -20, -10, 20, 20, 0, 0, 0, 0, 20, 20, 20, 30, 10, 0, 0,
    10, 30, 20,
];

// ── Chess types ─────────────────────────────────────────────────────

/// Piece color (side).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Side {
    White,
    Black,
}

impl Side {
    fn opponent(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

/// Piece type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PieceKind {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

impl PieceKind {
    fn value(self) -> i32 {
        match self {
            Self::Pawn => PAWN_VALUE,
            Self::Knight => KNIGHT_VALUE,
            Self::Bishop => BISHOP_VALUE,
            Self::Rook => ROOK_VALUE,
            Self::Queen => QUEEN_VALUE,
            Self::King => KING_VALUE,
        }
    }

    fn unicode_white(self) -> &'static str {
        match self {
            Self::King => "\u{2654}",
            Self::Queen => "\u{2655}",
            Self::Rook => "\u{2656}",
            Self::Bishop => "\u{2657}",
            Self::Knight => "\u{2658}",
            Self::Pawn => "\u{2659}",
        }
    }

    fn unicode_black(self) -> &'static str {
        match self {
            Self::King => "\u{265A}",
            Self::Queen => "\u{265B}",
            Self::Rook => "\u{265C}",
            Self::Bishop => "\u{265D}",
            Self::Knight => "\u{265E}",
            Self::Pawn => "\u{265F}",
        }
    }

    fn letter(self) -> &'static str {
        match self {
            Self::King => "K",
            Self::Queen => "Q",
            Self::Rook => "R",
            Self::Bishop => "B",
            Self::Knight => "N",
            Self::Pawn => "",
        }
    }
}

/// A chess piece (side + kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Piece {
    side: Side,
    kind: PieceKind,
}

impl Piece {
    const fn new(side: Side, kind: PieceKind) -> Self {
        Self { side, kind }
    }

    fn unicode(self) -> &'static str {
        match self.side {
            Side::White => self.kind.unicode_white(),
            Side::Black => self.kind.unicode_black(),
        }
    }
}

/// Board position (row 0 = rank 1 = white's back rank; col 0 = file a).
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

    /// Convert to algebraic notation (e.g. "e4").
    fn to_algebraic(self) -> String {
        let file = (b'a' + self.col as u8) as char;
        let rank = (b'1' + self.row as u8) as char;
        format!("{file}{rank}")
    }

    /// Index into a 64-element array (row * 8 + col).
    fn index(self) -> usize {
        (self.row as usize) * 8 + self.col as usize
    }

    /// Mirror index for black piece-square tables (flip rank).
    fn mirror_index(self) -> usize {
        ((7 - self.row) as usize) * 8 + self.col as usize
    }
}

/// A chess move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Move {
    from: Pos,
    to: Pos,
    promotion: Option<PieceKind>,
    is_castling: bool,
    is_en_passant: bool,
}

impl Move {
    const fn normal(from: Pos, to: Pos) -> Self {
        Self {
            from,
            to,
            promotion: None,
            is_castling: false,
            is_en_passant: false,
        }
    }

    const fn promotion(from: Pos, to: Pos, piece: PieceKind) -> Self {
        Self {
            from,
            to,
            promotion: Some(piece),
            is_castling: false,
            is_en_passant: false,
        }
    }

    const fn castling(from: Pos, to: Pos) -> Self {
        Self {
            from,
            to,
            promotion: None,
            is_castling: true,
            is_en_passant: false,
        }
    }

    const fn en_passant(from: Pos, to: Pos) -> Self {
        Self {
            from,
            to,
            promotion: None,
            is_castling: false,
            is_en_passant: true,
        }
    }
}

/// Castling rights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CastlingRights {
    white_kingside: bool,
    white_queenside: bool,
    black_kingside: bool,
    black_queenside: bool,
}

impl CastlingRights {
    const fn all() -> Self {
        Self {
            white_kingside: true,
            white_queenside: true,
            black_kingside: true,
            black_queenside: true,
        }
    }
}

/// Game result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameResult {
    Ongoing,
    WhiteWins,
    BlackWins,
    Stalemate,
    Draw,
}

// ── Move record for history ─────────────────────────────────────────

#[derive(Clone, Debug)]
struct MoveRecord {
    mv: Move,
    notation: String,
    captured: Option<Piece>,
}

// ── Board ───────────────────────────────────────────────────────────

/// The chess board state.
#[derive(Clone, Debug)]
struct Board {
    /// 8x8 grid. squares[row][col]. Row 0 = rank 1 (white's back rank).
    squares: [[Option<Piece>; 8]; 8],
    side_to_move: Side,
    castling: CastlingRights,
    /// En passant target square (the square a pawn can capture via en passant).
    en_passant: Option<Pos>,
    halfmove_clock: u32,
    fullmove_number: u32,
}

impl Board {
    /// Create a new board with standard starting position.
    fn new() -> Self {
        let mut squares = [[None; 8]; 8];

        // White pieces (rank 1 = row 0)
        squares[0][0] = Some(Piece::new(Side::White, PieceKind::Rook));
        squares[0][1] = Some(Piece::new(Side::White, PieceKind::Knight));
        squares[0][2] = Some(Piece::new(Side::White, PieceKind::Bishop));
        squares[0][3] = Some(Piece::new(Side::White, PieceKind::Queen));
        squares[0][4] = Some(Piece::new(Side::White, PieceKind::King));
        squares[0][5] = Some(Piece::new(Side::White, PieceKind::Bishop));
        squares[0][6] = Some(Piece::new(Side::White, PieceKind::Knight));
        squares[0][7] = Some(Piece::new(Side::White, PieceKind::Rook));
        for sq in &mut squares[1] {
            *sq = Some(Piece::new(Side::White, PieceKind::Pawn));
        }

        // Black pieces (rank 8 = row 7)
        squares[7][0] = Some(Piece::new(Side::Black, PieceKind::Rook));
        squares[7][1] = Some(Piece::new(Side::Black, PieceKind::Knight));
        squares[7][2] = Some(Piece::new(Side::Black, PieceKind::Bishop));
        squares[7][3] = Some(Piece::new(Side::Black, PieceKind::Queen));
        squares[7][4] = Some(Piece::new(Side::Black, PieceKind::King));
        squares[7][5] = Some(Piece::new(Side::Black, PieceKind::Bishop));
        squares[7][6] = Some(Piece::new(Side::Black, PieceKind::Knight));
        squares[7][7] = Some(Piece::new(Side::Black, PieceKind::Rook));
        for sq in &mut squares[6] {
            *sq = Some(Piece::new(Side::Black, PieceKind::Pawn));
        }

        Self {
            squares,
            side_to_move: Side::White,
            castling: CastlingRights::all(),
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    /// Create an empty board (for testing).
    fn empty() -> Self {
        Self {
            squares: [[None; 8]; 8],
            side_to_move: Side::White,
            castling: CastlingRights {
                white_kingside: false,
                white_queenside: false,
                black_kingside: false,
                black_queenside: false,
            },
            en_passant: None,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    fn get(&self, pos: Pos) -> Option<Piece> {
        if pos.is_valid() {
            self.squares[pos.row as usize][pos.col as usize]
        } else {
            None
        }
    }

    fn set(&mut self, pos: Pos, piece: Option<Piece>) {
        if pos.is_valid() {
            self.squares[pos.row as usize][pos.col as usize] = piece;
        }
    }

    /// Find the king position for the given side.
    fn find_king(&self, side: Side) -> Option<Pos> {
        for row in 0..8 {
            for col in 0..8 {
                if let Some(p) = self.squares[row][col]
                    && p.side == side && p.kind == PieceKind::King {
                        return Some(Pos::new(row as i8, col as i8));
                    }
            }
        }
        None
    }

    /// Check if a square is attacked by any piece of the given side.
    fn is_attacked_by(&self, pos: Pos, attacker: Side) -> bool {
        // Check knight attacks
        for &(dr, dc) in &[
            (-2, -1),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ] {
            let p = Pos::new(pos.row + dr, pos.col + dc);
            if let Some(piece) = self.get(p)
                && piece.side == attacker && piece.kind == PieceKind::Knight {
                    return true;
                }
        }

        // Check king attacks (adjacent squares)
        for dr in -1..=1 {
            for dc in -1..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let p = Pos::new(pos.row + dr, pos.col + dc);
                if let Some(piece) = self.get(p)
                    && piece.side == attacker && piece.kind == PieceKind::King {
                        return true;
                    }
            }
        }

        // Check pawn attacks
        let pawn_dir: i8 = if attacker == Side::White { -1 } else { 1 };
        for &dc in &[-1i8, 1] {
            let p = Pos::new(pos.row + pawn_dir, pos.col + dc);
            if let Some(piece) = self.get(p)
                && piece.side == attacker && piece.kind == PieceKind::Pawn {
                    return true;
                }
        }

        // Check sliding piece attacks (rook/queen on ranks/files)
        for &(dr, dc) in &[(0, 1), (0, -1), (1, 0), (-1, 0)] {
            let mut r = pos.row + dr;
            let mut c = pos.col + dc;
            while (0..8).contains(&r) && (0..8).contains(&c) {
                let p = Pos::new(r, c);
                if let Some(piece) = self.get(p) {
                    if piece.side == attacker
                        && (piece.kind == PieceKind::Rook || piece.kind == PieceKind::Queen)
                    {
                        return true;
                    }
                    break; // blocked
                }
                r += dr;
                c += dc;
            }
        }

        // Check sliding piece attacks (bishop/queen on diagonals)
        for &(dr, dc) in &[(1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let mut r = pos.row + dr;
            let mut c = pos.col + dc;
            while (0..8).contains(&r) && (0..8).contains(&c) {
                let p = Pos::new(r, c);
                if let Some(piece) = self.get(p) {
                    if piece.side == attacker
                        && (piece.kind == PieceKind::Bishop || piece.kind == PieceKind::Queen)
                    {
                        return true;
                    }
                    break; // blocked
                }
                r += dr;
                c += dc;
            }
        }

        false
    }

    /// Check if the current side's king is in check.
    fn is_in_check(&self, side: Side) -> bool {
        if let Some(king_pos) = self.find_king(side) {
            self.is_attacked_by(king_pos, side.opponent())
        } else {
            false
        }
    }

    /// Generate all pseudo-legal moves for the given side (may leave king in check).
    fn generate_pseudo_legal_moves(&self, side: Side) -> Vec<Move> {
        let mut moves = Vec::with_capacity(64);

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                if let Some(piece) = self.get(pos) {
                    if piece.side != side {
                        continue;
                    }
                    match piece.kind {
                        PieceKind::Pawn => self.generate_pawn_moves(pos, side, &mut moves),
                        PieceKind::Knight => self.generate_knight_moves(pos, side, &mut moves),
                        PieceKind::Bishop => {
                            self.generate_sliding_moves(pos, side, &BISHOP_DIRS, &mut moves);
                        }
                        PieceKind::Rook => {
                            self.generate_sliding_moves(pos, side, &ROOK_DIRS, &mut moves);
                        }
                        PieceKind::Queen => {
                            self.generate_sliding_moves(pos, side, &QUEEN_DIRS, &mut moves);
                        }
                        PieceKind::King => self.generate_king_moves(pos, side, &mut moves),
                    }
                }
            }
        }

        moves
    }

    /// Generate all legal moves for the current side.
    fn generate_legal_moves(&self) -> Vec<Move> {
        let pseudo = self.generate_pseudo_legal_moves(self.side_to_move);
        let mut legal = Vec::with_capacity(pseudo.len());

        for mv in pseudo {
            let mut test_board = self.clone();
            test_board.make_move_unchecked(mv);
            // After making the move, the side that just moved should not be in check
            if !test_board.is_in_check(self.side_to_move) {
                legal.push(mv);
            }
        }

        legal
    }

    /// Generate pawn moves from a position.
    fn generate_pawn_moves(&self, pos: Pos, side: Side, moves: &mut Vec<Move>) {
        let dir: i8 = if side == Side::White { 1 } else { -1 };
        let start_rank = if side == Side::White { 1 } else { 6 };
        let promo_rank = if side == Side::White { 7 } else { 0 };

        // Single push
        let one_ahead = Pos::new(pos.row + dir, pos.col);
        if one_ahead.is_valid() && self.get(one_ahead).is_none() {
            if one_ahead.row == promo_rank {
                for &kind in &[
                    PieceKind::Queen,
                    PieceKind::Rook,
                    PieceKind::Bishop,
                    PieceKind::Knight,
                ] {
                    moves.push(Move::promotion(pos, one_ahead, kind));
                }
            } else {
                moves.push(Move::normal(pos, one_ahead));
            }

            // Double push from starting rank
            if pos.row == start_rank {
                let two_ahead = Pos::new(pos.row + 2 * dir, pos.col);
                if two_ahead.is_valid() && self.get(two_ahead).is_none() {
                    moves.push(Move::normal(pos, two_ahead));
                }
            }
        }

        // Captures (including en passant)
        for &dc in &[-1i8, 1] {
            let cap_pos = Pos::new(pos.row + dir, pos.col + dc);
            if !cap_pos.is_valid() {
                continue;
            }

            if let Some(target) = self.get(cap_pos) {
                if target.side != side {
                    if cap_pos.row == promo_rank {
                        for &kind in &[
                            PieceKind::Queen,
                            PieceKind::Rook,
                            PieceKind::Bishop,
                            PieceKind::Knight,
                        ] {
                            moves.push(Move::promotion(pos, cap_pos, kind));
                        }
                    } else {
                        moves.push(Move::normal(pos, cap_pos));
                    }
                }
            } else if self.en_passant == Some(cap_pos) {
                moves.push(Move::en_passant(pos, cap_pos));
            }
        }
    }

    fn generate_knight_moves(&self, pos: Pos, side: Side, moves: &mut Vec<Move>) {
        for &(dr, dc) in &[
            (-2, -1),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ] {
            let to = Pos::new(pos.row + dr, pos.col + dc);
            if !to.is_valid() {
                continue;
            }
            match self.get(to) {
                None => moves.push(Move::normal(pos, to)),
                Some(p) if p.side != side => moves.push(Move::normal(pos, to)),
                _ => {}
            }
        }
    }

    fn generate_sliding_moves(
        &self,
        pos: Pos,
        side: Side,
        dirs: &[(i8, i8)],
        moves: &mut Vec<Move>,
    ) {
        for &(dr, dc) in dirs {
            let mut r = pos.row + dr;
            let mut c = pos.col + dc;
            while (0..8).contains(&r) && (0..8).contains(&c) {
                let to = Pos::new(r, c);
                match self.get(to) {
                    None => {
                        moves.push(Move::normal(pos, to));
                    }
                    Some(p) if p.side != side => {
                        moves.push(Move::normal(pos, to));
                        break;
                    }
                    _ => break,
                }
                r += dr;
                c += dc;
            }
        }
    }

    fn generate_king_moves(&self, pos: Pos, side: Side, moves: &mut Vec<Move>) {
        // Normal king moves
        for dr in -1..=1i8 {
            for dc in -1..=1i8 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let to = Pos::new(pos.row + dr, pos.col + dc);
                if !to.is_valid() {
                    continue;
                }
                match self.get(to) {
                    None => moves.push(Move::normal(pos, to)),
                    Some(p) if p.side != side => moves.push(Move::normal(pos, to)),
                    _ => {}
                }
            }
        }

        // Castling
        let rank = if side == Side::White { 0 } else { 7 };
        let opponent = side.opponent();

        // King must be on its starting square
        if pos.row != rank || pos.col != 4 {
            return;
        }

        // Cannot castle out of check
        if self.is_attacked_by(pos, opponent) {
            return;
        }

        // Kingside castling
        let can_ks = if side == Side::White {
            self.castling.white_kingside
        } else {
            self.castling.black_kingside
        };
        if can_ks {
            let f_sq = Pos::new(rank, 5);
            let g_sq = Pos::new(rank, 6);
            let rook_sq = Pos::new(rank, 7);
            if self.get(f_sq).is_none()
                && self.get(g_sq).is_none()
                && self.get(rook_sq) == Some(Piece::new(side, PieceKind::Rook))
                && !self.is_attacked_by(f_sq, opponent)
                && !self.is_attacked_by(g_sq, opponent)
            {
                moves.push(Move::castling(pos, g_sq));
            }
        }

        // Queenside castling
        let can_qs = if side == Side::White {
            self.castling.white_queenside
        } else {
            self.castling.black_queenside
        };
        if can_qs {
            let d_sq = Pos::new(rank, 3);
            let c_sq = Pos::new(rank, 2);
            let b_sq = Pos::new(rank, 1);
            let rook_sq = Pos::new(rank, 0);
            if self.get(d_sq).is_none()
                && self.get(c_sq).is_none()
                && self.get(b_sq).is_none()
                && self.get(rook_sq) == Some(Piece::new(side, PieceKind::Rook))
                && !self.is_attacked_by(d_sq, opponent)
                && !self.is_attacked_by(c_sq, opponent)
            {
                moves.push(Move::castling(pos, c_sq));
            }
        }
    }

    /// Make a move without checking legality (used for pseudo-legal move testing).
    fn make_move_unchecked(&mut self, mv: Move) {
        let piece = match self.get(mv.from) {
            Some(p) => p,
            None => return,
        };

        // Handle en passant capture
        if mv.is_en_passant {
            let captured_row = mv.from.row; // The captured pawn is on the same rank as the capturing pawn
            self.set(Pos::new(captured_row, mv.to.col), None);
        }

        // Move the piece
        self.set(mv.from, None);

        // Handle promotion
        if let Some(promo_kind) = mv.promotion {
            self.set(mv.to, Some(Piece::new(piece.side, promo_kind)));
        } else {
            self.set(mv.to, Some(piece));
        }

        // Handle castling — move the rook
        if mv.is_castling {
            let rank = mv.from.row;
            if mv.to.col == 6 {
                // Kingside
                let rook = self.get(Pos::new(rank, 7));
                self.set(Pos::new(rank, 7), None);
                self.set(Pos::new(rank, 5), rook);
            } else if mv.to.col == 2 {
                // Queenside
                let rook = self.get(Pos::new(rank, 0));
                self.set(Pos::new(rank, 0), None);
                self.set(Pos::new(rank, 3), rook);
            }
        }

        // Update en passant target
        self.en_passant = None;
        if piece.kind == PieceKind::Pawn {
            let diff = mv.to.row - mv.from.row;
            if diff == 2 || diff == -2 {
                self.en_passant = Some(Pos::new((mv.from.row + mv.to.row) / 2, mv.from.col));
            }
        }

        // Update castling rights
        // If king moves, lose both castling rights for that side
        if piece.kind == PieceKind::King {
            match piece.side {
                Side::White => {
                    self.castling.white_kingside = false;
                    self.castling.white_queenside = false;
                }
                Side::Black => {
                    self.castling.black_kingside = false;
                    self.castling.black_queenside = false;
                }
            }
        }
        // If rook moves from its starting square, lose that castling right
        if piece.kind == PieceKind::Rook {
            match (piece.side, mv.from.row, mv.from.col) {
                (Side::White, 0, 0) => self.castling.white_queenside = false,
                (Side::White, 0, 7) => self.castling.white_kingside = false,
                (Side::Black, 7, 0) => self.castling.black_queenside = false,
                (Side::Black, 7, 7) => self.castling.black_kingside = false,
                _ => {}
            }
        }
        // If a rook is captured on its starting square, lose that right too
        match (mv.to.row, mv.to.col) {
            (0, 0) => self.castling.white_queenside = false,
            (0, 7) => self.castling.white_kingside = false,
            (7, 0) => self.castling.black_queenside = false,
            (7, 7) => self.castling.black_kingside = false,
            _ => {}
        }

        // Update halfmove clock
        if piece.kind == PieceKind::Pawn || mv.is_en_passant {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock += 1;
        }

        // Update fullmove number
        if self.side_to_move == Side::Black {
            self.fullmove_number += 1;
        }

        self.side_to_move = self.side_to_move.opponent();
    }
}

const ROOK_DIRS: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const QUEEN_DIRS: [(i8, i8); 8] = [
    (0, 1),
    (0, -1),
    (1, 0),
    (-1, 0),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

// ── AI Evaluation ───────────────────────────────────────────────────

/// Evaluate the board from White's perspective.
/// Positive = White is better, negative = Black is better.
fn evaluate(board: &Board) -> i32 {
    let mut score = 0i32;

    for row in 0..8i8 {
        for col in 0..8i8 {
            let pos = Pos::new(row, col);
            if let Some(piece) = board.get(pos) {
                let material = piece.kind.value();
                let positional = piece_square_value(piece, pos);
                let total = material + positional;
                match piece.side {
                    Side::White => score += total,
                    Side::Black => score -= total,
                }
            }
        }
    }

    score
}

/// Get piece-square table bonus for a piece at a given position.
fn piece_square_value(piece: Piece, pos: Pos) -> i32 {
    let idx = match piece.side {
        Side::White => pos.index(),
        Side::Black => pos.mirror_index(),
    };
    // Bounds check for safety
    if idx >= 64 {
        return 0;
    }
    match piece.kind {
        PieceKind::Pawn => PAWN_TABLE[idx],
        PieceKind::Knight => KNIGHT_TABLE[idx],
        PieceKind::Bishop => BISHOP_TABLE[idx],
        PieceKind::Rook => ROOK_TABLE[idx],
        PieceKind::Queen => QUEEN_TABLE[idx],
        PieceKind::King => KING_MIDDLEGAME_TABLE[idx],
    }
}

/// Minimax with alpha-beta pruning.
/// Returns the evaluation score from the perspective of the side to move
/// at the root call.
fn minimax(board: &Board, depth: i32, mut alpha: i32, mut beta: i32, maximizing: bool) -> i32 {
    if depth == 0 {
        let eval = evaluate(board);
        return if maximizing { eval } else { -eval };
    }

    let moves = board.generate_legal_moves();

    if moves.is_empty() {
        if board.is_in_check(board.side_to_move) {
            // Checkmate — worst for the side to move
            return if maximizing {
                -KING_VALUE - depth
            } else {
                KING_VALUE + depth
            };
        }
        // Stalemate
        return 0;
    }

    if maximizing {
        let mut best = i32::MIN + 1;
        for mv in moves {
            let mut child = board.clone();
            child.make_move_unchecked(mv);
            let score = minimax(&child, depth - 1, alpha, beta, false);
            if score > best {
                best = score;
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
        let mut best = i32::MAX - 1;
        for mv in moves {
            let mut child = board.clone();
            child.make_move_unchecked(mv);
            let score = minimax(&child, depth - 1, alpha, beta, true);
            if score < best {
                best = score;
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

/// Choose the best move for the AI (Black).
fn ai_choose_move(board: &Board) -> Option<Move> {
    let moves = board.generate_legal_moves();
    if moves.is_empty() {
        return None;
    }

    let mut best_move = moves[0];
    let mut best_score = i32::MAX; // Black wants to minimize

    for mv in &moves {
        let mut child = board.clone();
        child.make_move_unchecked(*mv);
        // After Black's move, it's White's turn. Evaluate from White's perspective.
        let score = minimax(&child, AI_DEPTH - 1, i32::MIN + 1, i32::MAX - 1, true);
        if score < best_score {
            best_score = score;
            best_move = *mv;
        }
    }

    Some(best_move)
}

// ── Algebraic notation ──────────────────────────────────────────────

/// Convert a move to algebraic notation.
fn move_to_algebraic(board: &Board, mv: Move) -> String {
    let piece = match board.get(mv.from) {
        Some(p) => p,
        None => return String::new(),
    };

    // Castling
    if mv.is_castling {
        return if mv.to.col == 6 {
            "O-O".to_string()
        } else {
            "O-O-O".to_string()
        };
    }

    let mut notation = String::new();

    // Piece letter (not for pawns)
    let letter = piece.kind.letter();
    notation.push_str(letter);

    // Disambiguation for non-pawn pieces
    if piece.kind != PieceKind::Pawn {
        let legal_moves = board.generate_legal_moves();
        let same_dest: Vec<&Move> = legal_moves
            .iter()
            .filter(|m| {
                m.to == mv.to
                    && m.from != mv.from
                    && board.get(m.from).map(|p| p.kind) == Some(piece.kind)
            })
            .collect();
        if !same_dest.is_empty() {
            let same_col = same_dest.iter().any(|m| m.from.col == mv.from.col);
            let same_row = same_dest.iter().any(|m| m.from.row == mv.from.row);
            if !same_col {
                notation.push((b'a' + mv.from.col as u8) as char);
            } else if !same_row {
                notation.push((b'1' + mv.from.row as u8) as char);
            } else {
                notation.push((b'a' + mv.from.col as u8) as char);
                notation.push((b'1' + mv.from.row as u8) as char);
            }
        }
    }

    // Capture
    let is_capture = board.get(mv.to).is_some() || mv.is_en_passant;
    if is_capture {
        if piece.kind == PieceKind::Pawn {
            notation.push((b'a' + mv.from.col as u8) as char);
        }
        notation.push('x');
    }

    // Destination square
    notation.push_str(&mv.to.to_algebraic());

    // Promotion
    if let Some(promo) = mv.promotion {
        notation.push('=');
        notation.push_str(promo.letter());
    }

    // Check/checkmate suffix
    let mut test_board = board.clone();
    test_board.make_move_unchecked(mv);
    if test_board.is_in_check(test_board.side_to_move) {
        let legal = test_board.generate_legal_moves();
        if legal.is_empty() {
            notation.push('#');
        } else {
            notation.push('+');
        }
    }

    notation
}

// ── Chess App ───────────────────────────────────────────────────────

/// The main chess application state.
struct ChessApp {
    board: Board,
    selected: Option<Pos>,
    legal_moves_for_selected: Vec<Move>,
    last_move: Option<Move>,
    move_history: Vec<MoveRecord>,
    captured_white: Vec<Piece>, // White pieces captured by Black
    captured_black: Vec<Piece>, // Black pieces captured by White
    game_result: GameResult,
    status_message: String,
    /// Cursor position for keyboard navigation (row, col).
    cursor: Pos,
}

impl ChessApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            selected: None,
            legal_moves_for_selected: Vec::new(),
            last_move: None,
            move_history: Vec::new(),
            captured_white: Vec::new(),
            captured_black: Vec::new(),
            game_result: GameResult::Ongoing,
            status_message: "White to move".to_string(),
            cursor: Pos::new(0, 0),
        }
    }

    /// Reset to a new game.
    fn new_game(&mut self) {
        self.board = Board::new();
        self.selected = None;
        self.legal_moves_for_selected.clear();
        self.last_move = None;
        self.move_history.clear();
        self.captured_white.clear();
        self.captured_black.clear();
        self.game_result = GameResult::Ongoing;
        self.status_message = "White to move".to_string();
        self.cursor = Pos::new(0, 0);
    }

    /// Handle a click on a board square.
    fn click_square(&mut self, pos: Pos) {
        if self.game_result != GameResult::Ongoing {
            return;
        }
        // Only allow input when it's White's turn (human player)
        if self.board.side_to_move != Side::White {
            return;
        }

        if let Some(sel) = self.selected {
            // Try to make a move from selected to clicked square
            if let Some(mv) = self.find_legal_move(sel, pos) {
                self.execute_move(mv);
                // AI responds
                if self.game_result == GameResult::Ongoing {
                    self.ai_turn();
                }
                return;
            }
        }

        // Select a piece (must be own piece)
        if let Some(piece) = self.board.get(pos)
            && piece.side == Side::White {
                self.selected = Some(pos);
                self.legal_moves_for_selected = self
                    .board
                    .generate_legal_moves()
                    .into_iter()
                    .filter(|m| m.from == pos)
                    .collect();
                return;
            }

        // Clicked empty square or opponent piece without selection
        self.selected = None;
        self.legal_moves_for_selected.clear();
    }

    /// Find a legal move from `from` to `to`, preferring queen promotion.
    fn find_legal_move(&self, from: Pos, to: Pos) -> Option<Move> {
        let legal = self.board.generate_legal_moves();
        // First try queen promotion (most common choice)
        let queen_promo = legal
            .iter()
            .find(|m| m.from == from && m.to == to && m.promotion == Some(PieceKind::Queen));
        if let Some(mv) = queen_promo {
            return Some(*mv);
        }
        // Then any matching move
        legal.iter().find(|m| m.from == from && m.to == to).copied()
    }

    /// Execute a move on the board and update game state.
    fn execute_move(&mut self, mv: Move) {
        let notation = move_to_algebraic(&self.board, mv);
        let captured = if mv.is_en_passant {
            // The captured pawn in en passant
            let cap_row = mv.from.row;
            self.board.get(Pos::new(cap_row, mv.to.col))
        } else {
            self.board.get(mv.to)
        };

        // Track captures
        if let Some(cap) = captured {
            match cap.side {
                Side::White => self.captured_white.push(cap),
                Side::Black => self.captured_black.push(cap),
            }
        }

        // Record history
        // Reset halfmove clock on capture
        if captured.is_some() {
            self.board.halfmove_clock = 0;
        }

        self.move_history.push(MoveRecord {
            mv,
            notation,
            captured,
        });

        self.board.make_move_unchecked(mv);
        self.last_move = Some(mv);
        self.selected = None;
        self.legal_moves_for_selected.clear();

        self.update_game_state();
    }

    /// Let the AI (Black) make a move.
    fn ai_turn(&mut self) {
        if self.board.side_to_move != Side::Black {
            return;
        }
        if let Some(mv) = ai_choose_move(&self.board) {
            self.execute_move(mv);
        }
    }

    /// Update game state after a move (check, checkmate, stalemate).
    fn update_game_state(&mut self) {
        let legal = self.board.generate_legal_moves();
        let in_check = self.board.is_in_check(self.board.side_to_move);

        if legal.is_empty() {
            if in_check {
                // Checkmate
                match self.board.side_to_move {
                    Side::White => {
                        self.game_result = GameResult::BlackWins;
                        self.status_message = "Checkmate! Black wins.".to_string();
                    }
                    Side::Black => {
                        self.game_result = GameResult::WhiteWins;
                        self.status_message = "Checkmate! White wins.".to_string();
                    }
                }
            } else {
                self.game_result = GameResult::Stalemate;
                self.status_message = "Stalemate! Draw.".to_string();
            }
        } else if in_check {
            let side_name = match self.board.side_to_move {
                Side::White => "White",
                Side::Black => "Black",
            };
            self.status_message = format!("{side_name} is in check!");
        } else if self.board.halfmove_clock >= 100 {
            self.game_result = GameResult::Draw;
            self.status_message = "Draw by 50-move rule.".to_string();
        } else {
            let side_name = match self.board.side_to_move {
                Side::White => "White",
                Side::Black => "Black",
            };
            self.status_message = format!("{side_name} to move");
        }
    }

    /// Handle a keyboard event.
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

    /// Handle a mouse click event.
    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            // Convert mouse position to board square
            let board_x = event.x - BOARD_OFFSET_X;
            let board_y = event.y - BOARD_OFFSET_Y;

            if board_x >= 0.0
                && board_y >= 0.0
                && board_x < SQUARE_SIZE * 8.0
                && board_y < SQUARE_SIZE * 8.0
            {
                let col = (board_x / SQUARE_SIZE) as i8;
                // Flip row: screen y=0 is top, but row 7 = rank 8 should be at top
                let row = 7 - (board_y / SQUARE_SIZE) as i8;
                let pos = Pos::new(row, col);
                self.click_square(pos);
            }
        }
    }

    /// Handle an event.
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    /// Render the entire UI into a list of render commands.
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

        // Title
        commands.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 20.0,
            text: "Chess".to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status
        let status_color = match self.game_result {
            GameResult::Ongoing => {
                if self.board.is_in_check(self.board.side_to_move) {
                    RED
                } else {
                    SUBTEXT0
                }
            }
            GameResult::WhiteWins => GREEN,
            GameResult::BlackWins => RED,
            GameResult::Stalemate | GameResult::Draw => YELLOW,
        };
        commands.push(RenderCommand::Text {
            x: BOARD_OFFSET_X + 80.0,
            y: 24.0,
            text: self.status_message.clone(),
            color: status_color,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Draw the chess board
        self.render_board(&mut commands);

        // Rank labels (1-8 on left)
        for rank in 0..8 {
            let screen_row = 7 - rank;
            let y = BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0 - 6.0;
            commands.push(RenderCommand::Text {
                x: BOARD_OFFSET_X - 16.0,
                y,
                text: format!("{}", rank + 1),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // File labels (a-h on bottom)
        for file in 0..8 {
            let x = BOARD_OFFSET_X + file as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0 - 4.0;
            let y = BOARD_OFFSET_Y + SQUARE_SIZE * 8.0 + 8.0;
            commands.push(RenderCommand::Text {
                x,
                y,
                text: format!("{}", (b'a' + file as u8) as char),
                color: SUBTEXT0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Side panel
        self.render_panel(&mut commands);

        commands
    }

    /// Render the chess board squares and pieces.
    fn render_board(&self, commands: &mut Vec<RenderCommand>) {
        let king_pos = self.board.find_king(self.board.side_to_move);
        let in_check = self.board.is_in_check(self.board.side_to_move);

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                let screen_row = 7 - row;
                let sx = BOARD_OFFSET_X + col as f32 * SQUARE_SIZE;
                let sy = BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE;

                // Base square color
                let is_light = (row + col) % 2 != 0;
                let base_color = if is_light { LIGHT_SQUARE } else { DARK_SQUARE };

                commands.push(RenderCommand::FillRect {
                    x: sx,
                    y: sy,
                    width: SQUARE_SIZE,
                    height: SQUARE_SIZE,
                    color: base_color,
                    corner_radii: CornerRadii::ZERO,
                });

                // Last move highlight
                if let Some(last) = self.last_move
                    && (pos == last.from || pos == last.to) {
                        commands.push(RenderCommand::FillRect {
                            x: sx,
                            y: sy,
                            width: SQUARE_SIZE,
                            height: SQUARE_SIZE,
                            color: LAST_MOVE_HIGHLIGHT,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }

                // Check highlight on king
                if in_check && king_pos == Some(pos) {
                    commands.push(RenderCommand::FillRect {
                        x: sx,
                        y: sy,
                        width: SQUARE_SIZE,
                        height: SQUARE_SIZE,
                        color: CHECK_HIGHLIGHT,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Selected square highlight
                if self.selected == Some(pos) {
                    commands.push(RenderCommand::StrokeRect {
                        x: sx + 2.0,
                        y: sy + 2.0,
                        width: SQUARE_SIZE - 4.0,
                        height: SQUARE_SIZE - 4.0,
                        color: SELECTED_SQUARE,
                        line_width: 3.0,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Keyboard cursor highlight
                if self.cursor == pos && self.selected.is_none() {
                    commands.push(RenderCommand::StrokeRect {
                        x: sx + 1.0,
                        y: sy + 1.0,
                        width: SQUARE_SIZE - 2.0,
                        height: SQUARE_SIZE - 2.0,
                        color: MAUVE,
                        line_width: 2.0,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Draw piece
                if let Some(piece) = self.board.get(pos) {
                    commands.push(RenderCommand::Text {
                        x: sx + SQUARE_SIZE / 2.0 - PIECE_FONT_SIZE / 2.0 + 2.0,
                        y: sy + SQUARE_SIZE / 2.0 - PIECE_FONT_SIZE / 2.0 + 2.0,
                        text: piece.unicode().to_string(),
                        color: TEXT_COLOR,
                        font_size: PIECE_FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }
            }
        }

        // Legal move indicators (dots on target squares)
        for mv in &self.legal_moves_for_selected {
            let screen_row = 7 - mv.to.row;
            let cx = BOARD_OFFSET_X + mv.to.col as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0;
            let cy = BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE + SQUARE_SIZE / 2.0;

            // Use a small filled circle (approximated with a rounded rect)
            let has_piece = self.board.get(mv.to).is_some();
            if has_piece {
                // Capture indicator: ring around the square
                commands.push(RenderCommand::StrokeRect {
                    x: BOARD_OFFSET_X + mv.to.col as f32 * SQUARE_SIZE + 3.0,
                    y: BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE + 3.0,
                    width: SQUARE_SIZE - 6.0,
                    height: SQUARE_SIZE - 6.0,
                    color: LEGAL_MOVE_DOT,
                    line_width: 3.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            } else {
                // Empty square: small dot in center
                commands.push(RenderCommand::FillRect {
                    x: cx - DOT_RADIUS,
                    y: cy - DOT_RADIUS,
                    width: DOT_RADIUS * 2.0,
                    height: DOT_RADIUS * 2.0,
                    color: LEGAL_MOVE_DOT,
                    corner_radii: CornerRadii::all(DOT_RADIUS),
                });
            }
        }
    }

    /// Render the side panel (captured pieces, move history, controls).
    fn render_panel(&self, commands: &mut Vec<RenderCommand>) {
        let px = PANEL_X;
        let mut py = BOARD_OFFSET_Y;

        // Captured by White (Black pieces captured)
        commands.push(RenderCommand::Text {
            x: px,
            y: py,
            text: "Captured by White:".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        py += 20.0;

        let captured_text: String = self
            .captured_black
            .iter()
            .map(|p| p.unicode())
            .collect::<Vec<_>>()
            .join(" ");
        if !captured_text.is_empty() {
            commands.push(RenderCommand::Text {
                x: px,
                y: py,
                text: captured_text,
                color: TEXT_COLOR,
                font_size: INFO_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
            });
        }
        py += 24.0;

        // Captured by Black (White pieces captured)
        commands.push(RenderCommand::Text {
            x: px,
            y: py,
            text: "Captured by Black:".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        py += 20.0;

        let captured_text: String = self
            .captured_white
            .iter()
            .map(|p| p.unicode())
            .collect::<Vec<_>>()
            .join(" ");
        if !captured_text.is_empty() {
            commands.push(RenderCommand::Text {
                x: px,
                y: py,
                text: captured_text,
                color: TEXT_COLOR,
                font_size: INFO_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
            });
        }
        py += 30.0;

        // Move history header
        commands.push(RenderCommand::Text {
            x: px,
            y: py,
            text: "Moves:".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        py += 20.0;

        // Move list (paired: "1. e4 e5  2. Nf3 Nc6 ...")
        let mut move_idx = 0;
        while move_idx < self.move_history.len() {
            let move_num = move_idx / 2 + 1;
            let white_notation = &self.move_history[move_idx].notation;
            let mut line = format!("{move_num}. {white_notation}");

            if move_idx + 1 < self.move_history.len() {
                let black_notation = &self.move_history[move_idx + 1].notation;
                line.push_str(&format!(" {black_notation}"));
            }

            commands.push(RenderCommand::Text {
                x: px,
                y: py,
                text: line,
                color: TEXT_COLOR,
                font_size: MOVE_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(230.0),
            });
            py += 16.0;
            move_idx += 2;
        }

        // Controls hint at bottom of panel
        let controls_y = BOARD_OFFSET_Y + SQUARE_SIZE * 8.0 - 40.0;
        if py < controls_y {
            py = controls_y;
        }
        commands.push(RenderCommand::Text {
            x: px,
            y: py + 20.0,
            text: "Ctrl+N: New Game".to_string(),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        commands.push(RenderCommand::Text {
            x: px,
            y: py + 36.0,
            text: "Arrows/Enter: Navigate".to_string(),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        commands.push(RenderCommand::Text {
            x: px,
            y: py + 52.0,
            text: "Esc: Deselect".to_string(),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn main() {
    let _app = ChessApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Board setup helpers ─────────────────────────────────────────

    /// Place a piece on the board.
    fn place(board: &mut Board, row: i8, col: i8, side: Side, kind: PieceKind) {
        board.set(Pos::new(row, col), Some(Piece::new(side, kind)));
    }

    // ── Initial position tests ──────────────────────────────────────

    #[test]
    fn test_initial_board_setup() {
        let board = Board::new();
        // White pieces on rank 1
        assert_eq!(
            board.get(Pos::new(0, 0)),
            Some(Piece::new(Side::White, PieceKind::Rook))
        );
        assert_eq!(
            board.get(Pos::new(0, 4)),
            Some(Piece::new(Side::White, PieceKind::King))
        );
        assert_eq!(
            board.get(Pos::new(0, 3)),
            Some(Piece::new(Side::White, PieceKind::Queen))
        );
        // White pawns on rank 2
        for col in 0..8 {
            assert_eq!(
                board.get(Pos::new(1, col)),
                Some(Piece::new(Side::White, PieceKind::Pawn))
            );
        }
        // Black pieces on rank 8
        assert_eq!(
            board.get(Pos::new(7, 0)),
            Some(Piece::new(Side::Black, PieceKind::Rook))
        );
        assert_eq!(
            board.get(Pos::new(7, 4)),
            Some(Piece::new(Side::Black, PieceKind::King))
        );
        // Empty squares in the middle
        for row in 2..6 {
            for col in 0..8 {
                assert!(board.get(Pos::new(row, col)).is_none());
            }
        }
    }

    #[test]
    fn test_initial_side_to_move() {
        let board = Board::new();
        assert_eq!(board.side_to_move, Side::White);
    }

    #[test]
    fn test_initial_castling_rights() {
        let board = Board::new();
        assert!(board.castling.white_kingside);
        assert!(board.castling.white_queenside);
        assert!(board.castling.black_kingside);
        assert!(board.castling.black_queenside);
    }

    #[test]
    fn test_initial_no_en_passant() {
        let board = Board::new();
        assert!(board.en_passant.is_none());
    }

    // ── Position tests ──────────────────────────────────────────────

    #[test]
    fn test_pos_validity() {
        assert!(Pos::new(0, 0).is_valid());
        assert!(Pos::new(7, 7).is_valid());
        assert!(!Pos::new(-1, 0).is_valid());
        assert!(!Pos::new(0, 8).is_valid());
        assert!(!Pos::new(8, 0).is_valid());
    }

    #[test]
    fn test_pos_algebraic() {
        assert_eq!(Pos::new(0, 0).to_algebraic(), "a1");
        assert_eq!(Pos::new(7, 7).to_algebraic(), "h8");
        assert_eq!(Pos::new(3, 4).to_algebraic(), "e4");
    }

    #[test]
    fn test_pos_index() {
        assert_eq!(Pos::new(0, 0).index(), 0);
        assert_eq!(Pos::new(1, 0).index(), 8);
        assert_eq!(Pos::new(7, 7).index(), 63);
    }

    #[test]
    fn test_pos_mirror_index() {
        assert_eq!(Pos::new(0, 0).mirror_index(), 56);
        assert_eq!(Pos::new(7, 7).mirror_index(), 7);
    }

    // ── Piece type tests ────────────────────────────────────────────

    #[test]
    fn test_piece_values() {
        assert_eq!(PieceKind::Pawn.value(), 100);
        assert_eq!(PieceKind::Knight.value(), 320);
        assert_eq!(PieceKind::Bishop.value(), 330);
        assert_eq!(PieceKind::Rook.value(), 500);
        assert_eq!(PieceKind::Queen.value(), 900);
        assert_eq!(PieceKind::King.value(), 20_000);
    }

    #[test]
    fn test_side_opponent() {
        assert_eq!(Side::White.opponent(), Side::Black);
        assert_eq!(Side::Black.opponent(), Side::White);
    }

    #[test]
    fn test_piece_unicode() {
        let wp = Piece::new(Side::White, PieceKind::King);
        assert_eq!(wp.unicode(), "\u{2654}");
        let bp = Piece::new(Side::Black, PieceKind::King);
        assert_eq!(bp.unicode(), "\u{265A}");
    }

    #[test]
    fn test_piece_letter() {
        assert_eq!(PieceKind::King.letter(), "K");
        assert_eq!(PieceKind::Queen.letter(), "Q");
        assert_eq!(PieceKind::Pawn.letter(), "");
        assert_eq!(PieceKind::Knight.letter(), "N");
    }

    // ── Pawn movement tests ─────────────────────────────────────────

    #[test]
    fn test_pawn_single_push() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(3, 4) && m.to == Pos::new(4, 4))
        );
    }

    #[test]
    fn test_pawn_double_push() {
        let board = Board::new();
        let moves = board.generate_legal_moves();
        // e2-e4 should be available
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(1, 4) && m.to == Pos::new(3, 4))
        );
    }

    #[test]
    fn test_pawn_blocked() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 4, 4, Side::Black, PieceKind::Pawn); // blocking
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        // No forward moves for the pawn
        assert!(
            !moves
                .iter()
                .any(|m| m.from == Pos::new(3, 4) && m.to == Pos::new(4, 4))
        );
    }

    #[test]
    fn test_pawn_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 4, 5, Side::Black, PieceKind::Pawn); // capturable
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(3, 4) && m.to == Pos::new(4, 5))
        );
    }

    #[test]
    fn test_pawn_cant_capture_own() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 4, 5, Side::White, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            !moves
                .iter()
                .any(|m| m.from == Pos::new(3, 4) && m.to == Pos::new(4, 5))
        );
    }

    #[test]
    fn test_pawn_double_push_blocked() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 1, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 2, 4, Side::Black, PieceKind::Pawn); // blocking 1 square ahead
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        // Can't push at all when blocked one square ahead
        assert!(
            !moves
                .iter()
                .any(|m| m.from == Pos::new(1, 4) && m.to == Pos::new(3, 4))
        );
        assert!(
            !moves
                .iter()
                .any(|m| m.from == Pos::new(1, 4) && m.to == Pos::new(2, 4))
        );
    }

    // ── En passant tests ────────────────────────────────────────────

    #[test]
    fn test_en_passant_target_set() {
        let mut board = Board::new();
        let mv = Move::normal(Pos::new(1, 4), Pos::new(3, 4)); // e2-e4
        board.make_move_unchecked(mv);
        assert_eq!(board.en_passant, Some(Pos::new(2, 4))); // e3 is EP target
    }

    #[test]
    fn test_en_passant_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 4, 4, Side::White, PieceKind::Pawn); // e5
        place(&mut board, 4, 5, Side::Black, PieceKind::Pawn); // f5 (just double-pushed)
        board.en_passant = Some(Pos::new(5, 5)); // f6 is EP target
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let moves = board.generate_legal_moves();
        let ep_move = moves
            .iter()
            .find(|m| m.from == Pos::new(4, 4) && m.to == Pos::new(5, 5) && m.is_en_passant);
        assert!(ep_move.is_some());

        // Execute the EP capture
        let mut test_board = board.clone();
        test_board.make_move_unchecked(*ep_move.unwrap());
        // The captured pawn on f5 should be gone
        assert!(test_board.get(Pos::new(4, 5)).is_none());
        // The capturing pawn should be on f6
        assert_eq!(
            test_board.get(Pos::new(5, 5)),
            Some(Piece::new(Side::White, PieceKind::Pawn))
        );
    }

    #[test]
    fn test_en_passant_expires() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 1, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        board.en_passant = Some(Pos::new(5, 3));

        // Make a different move — en passant should expire
        let mv = Move::normal(Pos::new(1, 4), Pos::new(2, 4));
        board.make_move_unchecked(mv);
        assert!(board.en_passant.is_none());
    }

    // ── Knight movement tests ───────────────────────────────────────

    #[test]
    fn test_knight_moves_center() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Knight);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let knight_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(3, 3)).collect();
        assert_eq!(knight_moves.len(), 8); // Knight in center has 8 moves
    }

    #[test]
    fn test_knight_moves_corner() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 0, Side::White, PieceKind::Knight);
        place(&mut board, 4, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let knight_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(0, 0)).collect();
        assert_eq!(knight_moves.len(), 2); // Knight in corner has 2 moves
    }

    // ── Bishop movement tests ───────────────────────────────────────

    #[test]
    fn test_bishop_moves_empty_board() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Bishop);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let bishop_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(3, 3)).collect();
        // d4 bishop: diagonals reach many squares
        assert!(bishop_moves.len() >= 10);
    }

    #[test]
    fn test_bishop_blocked_by_own_piece() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 0, Side::White, PieceKind::Bishop);
        place(&mut board, 1, 1, Side::White, PieceKind::Pawn); // blocks diagonal
        place(&mut board, 4, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let bishop_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(0, 0)).collect();
        assert_eq!(bishop_moves.len(), 0); // Completely blocked
    }

    // ── Rook movement tests ─────────────────────────────────────────

    #[test]
    fn test_rook_moves_empty_board() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Rook);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let rook_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(3, 3)).collect();
        // Rook on d4: 7 up/down + 7 left/right = 14
        assert_eq!(rook_moves.len(), 14);
    }

    #[test]
    fn test_rook_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Rook);
        place(&mut board, 3, 6, Side::Black, PieceKind::Pawn); // capturable
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(3, 3) && m.to == Pos::new(3, 6))
        );
    }

    // ── Queen movement tests ────────────────────────────────────────

    #[test]
    fn test_queen_moves_center() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Queen);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let queen_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(3, 3)).collect();
        // Queen combines rook + bishop: 14 + 13 = 27 on empty center
        assert!(queen_moves.len() >= 25);
    }

    // ── King movement tests ─────────────────────────────────────────

    #[test]
    fn test_king_moves_center() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let king_moves: Vec<_> = moves.iter().filter(|m| m.from == Pos::new(3, 3)).collect();
        assert_eq!(king_moves.len(), 8);
    }

    #[test]
    fn test_king_cant_move_into_check() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 5, Side::Black, PieceKind::Rook); // Controls f-file
        place(&mut board, 7, 3, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        // King should not be able to move to f1 (attacked by rook)
        assert!(!moves.iter().any(|m| m.to == Pos::new(0, 5)));
    }

    // ── Castling tests ──────────────────────────────────────────────

    #[test]
    fn test_kingside_castling_available() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(0, 6))
        );
    }

    #[test]
    fn test_queenside_castling_available() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_queenside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(0, 2))
        );
    }

    #[test]
    fn test_castling_blocked_by_piece() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 5, Side::White, PieceKind::Bishop); // blocks
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            !moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(0, 6))
        );
    }

    #[test]
    fn test_castling_through_check() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 5, Side::Black, PieceKind::Rook); // attacks f1
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        // Can't castle through f1 which is attacked
        assert!(
            !moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(0, 6))
        );
    }

    #[test]
    fn test_castling_out_of_check() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 4, Side::Black, PieceKind::Rook); // king in check from e8
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(!moves.iter().any(|m| m.is_castling));
    }

    #[test]
    fn test_castling_rights_lost_on_king_move() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        board.castling.white_queenside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::Rook);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let mv = Move::normal(Pos::new(0, 4), Pos::new(0, 5));
        board.make_move_unchecked(mv);
        assert!(!board.castling.white_kingside);
        assert!(!board.castling.white_queenside);
    }

    #[test]
    fn test_castling_rights_lost_on_rook_move() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let mv = Move::normal(Pos::new(0, 7), Pos::new(0, 6));
        board.make_move_unchecked(mv);
        assert!(!board.castling.white_kingside);
    }

    #[test]
    fn test_castling_executes_rook_move() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let mv = Move::castling(Pos::new(0, 4), Pos::new(0, 6));
        board.make_move_unchecked(mv);
        assert_eq!(
            board.get(Pos::new(0, 6)),
            Some(Piece::new(Side::White, PieceKind::King))
        );
        assert_eq!(
            board.get(Pos::new(0, 5)),
            Some(Piece::new(Side::White, PieceKind::Rook))
        );
        assert!(board.get(Pos::new(0, 7)).is_none());
        assert!(board.get(Pos::new(0, 4)).is_none());
    }

    #[test]
    fn test_queenside_castling_executes() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_queenside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let mv = Move::castling(Pos::new(0, 4), Pos::new(0, 2));
        board.make_move_unchecked(mv);
        assert_eq!(
            board.get(Pos::new(0, 2)),
            Some(Piece::new(Side::White, PieceKind::King))
        );
        assert_eq!(
            board.get(Pos::new(0, 3)),
            Some(Piece::new(Side::White, PieceKind::Rook))
        );
        assert!(board.get(Pos::new(0, 0)).is_none());
    }

    // ── Pawn promotion tests ────────────────────────────────────────

    #[test]
    fn test_pawn_promotion_moves_generated() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 6, 4, Side::White, PieceKind::Pawn); // e7
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let promo_moves: Vec<_> = moves
            .iter()
            .filter(|m| m.from == Pos::new(6, 4) && m.promotion.is_some())
            .collect();
        // Should generate 4 promotion options (Q, R, B, N)
        assert_eq!(promo_moves.len(), 4);
    }

    #[test]
    fn test_pawn_promotion_executes() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 6, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 0, Side::Black, PieceKind::King);

        let mv = Move::promotion(Pos::new(6, 4), Pos::new(7, 4), PieceKind::Queen);
        board.make_move_unchecked(mv);
        assert_eq!(
            board.get(Pos::new(7, 4)),
            Some(Piece::new(Side::White, PieceKind::Queen))
        );
    }

    #[test]
    fn test_pawn_promotion_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 6, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 7, 5, Side::Black, PieceKind::Rook); // capturable
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        let promo_captures: Vec<_> = moves
            .iter()
            .filter(|m| m.from == Pos::new(6, 4) && m.to == Pos::new(7, 5) && m.promotion.is_some())
            .collect();
        assert_eq!(promo_captures.len(), 4);
    }

    // ── Check detection tests ───────────────────────────────────────

    #[test]
    fn test_check_by_rook() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 4, Side::Black, PieceKind::Rook); // check on e-file
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        assert!(board.is_in_check(Side::White));
    }

    #[test]
    fn test_check_by_bishop() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 3, 7, Side::Black, PieceKind::Bishop); // diagonal check
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        assert!(board.is_in_check(Side::White));
    }

    #[test]
    fn test_check_by_knight() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 2, 5, Side::Black, PieceKind::Knight); // knight check
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        assert!(board.is_in_check(Side::White));
    }

    #[test]
    fn test_check_by_pawn() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::King);
        place(&mut board, 4, 5, Side::Black, PieceKind::Pawn); // pawn check
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        assert!(board.is_in_check(Side::White));
    }

    #[test]
    fn test_not_in_check() {
        let board = Board::new();
        assert!(!board.is_in_check(Side::White));
        assert!(!board.is_in_check(Side::Black));
    }

    #[test]
    fn test_must_escape_check() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 4, Side::Black, PieceKind::Rook);
        place(&mut board, 7, 0, Side::Black, PieceKind::King);
        let moves = board.generate_legal_moves();
        // All moves must escape check (king must move off e-file)
        for mv in &moves {
            assert_ne!(mv.to.col, 4, "King should not stay on attacked file");
        }
    }

    // ── Checkmate detection tests ───────────────────────────────────

    #[test]
    fn test_checkmate_back_rank() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        // White king on a1, rook on a8, rook on b8 = back rank mate if it were Black's turn
        // Let's do: Black to move, king on h8, pawns on f7/g7/h7, white rook on a8 = mate
        board.side_to_move = Side::Black;
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        place(&mut board, 6, 5, Side::Black, PieceKind::Pawn);
        place(&mut board, 6, 6, Side::Black, PieceKind::Pawn);
        place(&mut board, 6, 7, Side::Black, PieceKind::Pawn);
        place(&mut board, 7, 0, Side::White, PieceKind::Rook); // Ra8 gives check
        place(&mut board, 0, 4, Side::White, PieceKind::King);

        assert!(board.is_in_check(Side::Black));
        let moves = board.generate_legal_moves();
        assert!(moves.is_empty(), "Should be checkmate (back rank mate)");
    }

    #[test]
    fn test_scholars_mate_position() {
        // Simplified: Black king on e8, white queen on f7 supported, no escape
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 6, 5, Side::White, PieceKind::Queen); // Qf7
        // Bc4 (on the a2-g8 diagonal a2-b3-c4-d5-e6-f7) defends the queen on f7,
        // so the king cannot capture it. c5 would NOT defend f7.
        place(&mut board, 3, 2, Side::White, PieceKind::Bishop); // Bc4 supports Qf7
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        // Block escape squares. d8/f8 hold the king's own queen/bishop; the d7
        // pawn blocks d7; e7 is empty but covered by the adjacent queen on f7.
        place(&mut board, 7, 3, Side::Black, PieceKind::Queen);
        place(&mut board, 7, 5, Side::Black, PieceKind::Bishop);
        place(&mut board, 6, 3, Side::Black, PieceKind::Pawn); // d7 blocked

        assert!(board.is_in_check(Side::Black));
        let moves = board.generate_legal_moves();
        assert!(moves.is_empty(), "Should be checkmate");
    }

    // ── Stalemate detection tests ───────────────────────────────────

    #[test]
    fn test_stalemate_king_only() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        place(&mut board, 5, 6, Side::White, PieceKind::Queen); // Controls g6
        place(&mut board, 6, 5, Side::White, PieceKind::King); // Controls f7, g7
        // Black king on h8 can't move: g8 attacked by queen/king, h7 attacked by queen
        let moves = board.generate_legal_moves();
        assert!(!board.is_in_check(Side::Black));
        assert!(moves.is_empty(), "Should be stalemate");
    }

    // ── Attack detection tests ──────────────────────────────────────

    #[test]
    fn test_square_attacked_by_rook() {
        let mut board = Board::empty();
        place(&mut board, 3, 0, Side::White, PieceKind::Rook);
        assert!(board.is_attacked_by(Pos::new(3, 5), Side::White));
        assert!(board.is_attacked_by(Pos::new(7, 0), Side::White));
        assert!(!board.is_attacked_by(Pos::new(4, 1), Side::White));
    }

    #[test]
    fn test_square_attacked_by_bishop() {
        let mut board = Board::empty();
        place(&mut board, 0, 0, Side::White, PieceKind::Bishop);
        assert!(board.is_attacked_by(Pos::new(3, 3), Side::White));
        assert!(!board.is_attacked_by(Pos::new(0, 3), Side::White));
    }

    #[test]
    fn test_square_attacked_by_knight() {
        let mut board = Board::empty();
        place(&mut board, 3, 3, Side::White, PieceKind::Knight);
        assert!(board.is_attacked_by(Pos::new(5, 4), Side::White));
        assert!(board.is_attacked_by(Pos::new(1, 2), Side::White));
        assert!(!board.is_attacked_by(Pos::new(4, 4), Side::White));
    }

    #[test]
    fn test_square_attacked_by_pawn() {
        let mut board = Board::empty();
        place(&mut board, 3, 3, Side::White, PieceKind::Pawn);
        // White pawn attacks diagonally forward (higher row)
        assert!(board.is_attacked_by(Pos::new(4, 4), Side::White));
        assert!(board.is_attacked_by(Pos::new(4, 2), Side::White));
        // Not straight ahead
        assert!(!board.is_attacked_by(Pos::new(4, 3), Side::White));
    }

    #[test]
    fn test_attack_blocked_by_piece() {
        let mut board = Board::empty();
        place(&mut board, 0, 0, Side::White, PieceKind::Rook);
        place(&mut board, 0, 3, Side::Black, PieceKind::Pawn); // blocking
        // Rook attack should be blocked at d1
        assert!(board.is_attacked_by(Pos::new(0, 3), Side::White));
        assert!(!board.is_attacked_by(Pos::new(0, 5), Side::White));
    }

    // ── Legal move count tests ──────────────────────────────────────

    #[test]
    fn test_initial_position_legal_moves() {
        let board = Board::new();
        let moves = board.generate_legal_moves();
        // 20 legal moves in starting position: 16 pawn moves + 4 knight moves
        assert_eq!(moves.len(), 20);
    }

    // ── Evaluation tests ────────────────────────────────────────────

    #[test]
    fn test_evaluate_starting_position() {
        let board = Board::new();
        let score = evaluate(&board);
        // Starting position should be roughly equal (close to 0)
        assert!(
            score.abs() < 50,
            "Starting position eval should be near 0, got {score}"
        );
    }

    #[test]
    fn test_evaluate_material_advantage() {
        let mut board = Board::empty();
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        place(&mut board, 3, 3, Side::White, PieceKind::Queen);
        let score = evaluate(&board);
        // White has a queen advantage
        assert!(
            score > 800,
            "Queen advantage should give high eval, got {score}"
        );
    }

    #[test]
    fn test_evaluate_black_advantage() {
        let mut board = Board::empty();
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        place(&mut board, 3, 3, Side::Black, PieceKind::Queen);
        let score = evaluate(&board);
        // Black has a queen advantage — score should be negative
        assert!(
            score < -800,
            "Black queen advantage should give negative eval, got {score}"
        );
    }

    #[test]
    fn test_piece_square_bonus() {
        // Knight in center should have higher bonus than in corner
        let center_bonus =
            piece_square_value(Piece::new(Side::White, PieceKind::Knight), Pos::new(3, 3));
        let corner_bonus =
            piece_square_value(Piece::new(Side::White, PieceKind::Knight), Pos::new(0, 0));
        assert!(
            center_bonus > corner_bonus,
            "Center knight should have better bonus: center={center_bonus}, corner={corner_bonus}"
        );
    }

    // ── AI tests ────────────────────────────────────────────────────

    #[test]
    fn test_ai_chooses_move() {
        let mut board = Board::new();
        board.side_to_move = Side::Black;
        let mv = ai_choose_move(&board);
        assert!(mv.is_some(), "AI should find a move from starting position");
    }

    #[test]
    fn test_ai_captures_free_piece() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 4, 3, Side::Black, PieceKind::Queen);
        place(&mut board, 3, 3, Side::White, PieceKind::Rook); // free rook
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        let mv = ai_choose_move(&board);
        assert!(mv.is_some());
        // AI should take the free rook
        let chosen = mv.unwrap();
        assert_eq!(chosen.to, Pos::new(3, 3), "AI should capture the free rook");
    }

    #[test]
    fn test_ai_no_moves_returns_none() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        // Stalemate position for Black
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        place(&mut board, 5, 6, Side::White, PieceKind::Queen);
        place(&mut board, 6, 5, Side::White, PieceKind::King);
        let mv = ai_choose_move(&board);
        assert!(mv.is_none(), "AI should return None when no legal moves");
    }

    // ── Minimax tests ───────────────────────────────────────────────

    #[test]
    fn test_minimax_depth_zero() {
        let board = Board::new();
        let score = minimax(&board, 0, i32::MIN + 1, i32::MAX - 1, true);
        // At depth 0, just returns evaluation (should be close to 0)
        assert!(score.abs() < 50);
    }

    #[test]
    fn test_minimax_finds_mate() {
        // Black king in corner, White to deliver mate
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 6, 0, Side::White, PieceKind::Rook);
        place(&mut board, 5, 1, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        // White can deliver mate: move rook to h-file
        let score = minimax(&board, 2, i32::MIN + 1, i32::MAX - 1, true);
        // Score should be very high (near mate value)
        assert!(score > 10000, "Should find forced mate, got {score}");
    }

    // ── Algebraic notation tests ────────────────────────────────────

    #[test]
    fn test_notation_pawn_move() {
        let board = Board::new();
        let mv = Move::normal(Pos::new(1, 4), Pos::new(3, 4));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "e4");
    }

    #[test]
    fn test_notation_knight_move() {
        let board = Board::new();
        let mv = Move::normal(Pos::new(0, 1), Pos::new(2, 2));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "Nc3");
    }

    #[test]
    fn test_notation_castling_kingside() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_kingside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 7, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let mv = Move::castling(Pos::new(0, 4), Pos::new(0, 6));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "O-O");
    }

    #[test]
    fn test_notation_castling_queenside() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.white_queenside = true;
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::Rook);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let mv = Move::castling(Pos::new(0, 4), Pos::new(0, 2));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "O-O-O");
    }

    #[test]
    fn test_notation_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Knight);
        place(&mut board, 5, 4, Side::Black, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let mv = Move::normal(Pos::new(3, 3), Pos::new(5, 4));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "Nxe6");
    }

    #[test]
    fn test_notation_pawn_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 4, 5, Side::Black, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);
        let mv = Move::normal(Pos::new(3, 4), Pos::new(4, 5));
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "exf5");
    }

    #[test]
    fn test_notation_promotion() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 6, 4, Side::White, PieceKind::Pawn);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        // Keep the black king off the 8th rank and the e-file (and off the new
        // queen's diagonals) so the promotion to e8 does NOT incidentally give
        // check — this test isolates the "=Q" promotion notation, not the check
        // suffix. a1 (0,0) is taken by the white king, so use h1 (0,7).
        place(&mut board, 0, 7, Side::Black, PieceKind::King);
        let mv = Move::promotion(Pos::new(6, 4), Pos::new(7, 4), PieceKind::Queen);
        let notation = move_to_algebraic(&board, mv);
        assert_eq!(notation, "e8=Q");
    }

    // ── Game state tests ────────────────────────────────────────────

    #[test]
    fn test_new_game_initial_state() {
        let app = ChessApp::new();
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert!(app.selected.is_none());
        assert!(app.move_history.is_empty());
        assert!(app.captured_white.is_empty());
        assert!(app.captured_black.is_empty());
        assert_eq!(app.board.side_to_move, Side::White);
    }

    #[test]
    fn test_reset_game() {
        let mut app = ChessApp::new();
        // Make a move to change state
        app.click_square(Pos::new(1, 4)); // select e2
        app.click_square(Pos::new(3, 4)); // move to e4
        assert!(!app.move_history.is_empty());

        app.new_game();
        assert!(app.move_history.is_empty());
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert_eq!(app.board.side_to_move, Side::White);
    }

    #[test]
    fn test_select_own_piece() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(0, 1)); // select b1 knight
        assert_eq!(app.selected, Some(Pos::new(0, 1)));
        assert!(!app.legal_moves_for_selected.is_empty());
    }

    #[test]
    fn test_cannot_select_opponent_piece() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(7, 1)); // try to select Black knight
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_click_empty_deselects() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(0, 1)); // select knight
        assert!(app.selected.is_some());
        app.click_square(Pos::new(4, 4)); // click empty square (not a legal target)
        assert!(app.selected.is_none());
    }

    // ── Move execution tests ────────────────────────────────────────

    #[test]
    fn test_execute_pawn_move() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(1, 4)); // select e2
        app.click_square(Pos::new(3, 4)); // move to e4
        // After human move + AI response, it should be White's turn again
        // (AI plays black automatically)
        assert_eq!(app.board.side_to_move, Side::White);
        assert!(app.move_history.len() >= 2); // White + Black moved
    }

    #[test]
    fn test_capture_tracked() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 3, 3, Side::White, PieceKind::Queen);
        place(&mut board, 5, 5, Side::Black, PieceKind::Rook);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::King);

        let mut app = ChessApp::new();
        app.board = board;
        app.click_square(Pos::new(3, 3)); // select queen
        app.click_square(Pos::new(5, 5)); // capture rook
        assert!(!app.captured_black.is_empty());
        assert_eq!(app.captured_black[0].kind, PieceKind::Rook);
    }

    // ── Keyboard handling tests ─────────────────────────────────────

    #[test]
    fn test_keyboard_navigation() {
        let mut app = ChessApp::new();
        assert_eq!(app.cursor, Pos::new(0, 0));

        let right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&right);
        assert_eq!(app.cursor, Pos::new(0, 1));

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&up);
        assert_eq!(app.cursor, Pos::new(1, 1));
    }

    #[test]
    fn test_keyboard_bounds() {
        let mut app = ChessApp::new();
        let left = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&left);
        // Should stay at 0, not go negative
        assert_eq!(app.cursor.col, 0);

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
    fn test_escape_deselects() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(0, 1)); // select knight
        assert!(app.selected.is_some());

        let esc = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&esc);
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_ctrl_n_new_game() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(1, 4));
        app.click_square(Pos::new(3, 4));

        let ctrl_n = KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        };
        app.handle_key(&ctrl_n);
        assert!(app.move_history.is_empty());
        assert_eq!(app.game_result, GameResult::Ongoing);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = ChessApp::new();
        let right = KeyEvent {
            key: Key::Right,
            pressed: false, // release
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&right);
        assert_eq!(app.cursor.col, 0); // Should not move
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = ChessApp::new();
        let commands = app.render();
        assert!(!commands.is_empty());
    }

    #[test]
    fn test_render_has_background() {
        let app = ChessApp::new();
        let commands = app.render();
        let has_bg = commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == BASE));
        assert!(has_bg, "Should render background");
    }

    #[test]
    fn test_render_has_title() {
        let app = ChessApp::new();
        let commands = app.render();
        let has_title = commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Chess"));
        assert!(has_title, "Should render title");
    }

    #[test]
    fn test_render_has_board_squares() {
        let app = ChessApp::new();
        let commands = app.render();
        // Should have at least 64 fill rects for board squares
        let fill_count = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert!(
            fill_count >= 64,
            "Should render 64 board squares, got {fill_count}"
        );
    }

    #[test]
    fn test_render_has_pieces() {
        let app = ChessApp::new();
        let commands = app.render();
        // 32 pieces in starting position
        let piece_texts = commands
            .iter()
            .filter(|c| {
                matches!(c, RenderCommand::Text { text, .. }
                    if text.chars().any(|ch| "\u{2654}\u{2655}\u{2656}\u{2657}\u{2658}\u{2659}\u{265A}\u{265B}\u{265C}\u{265D}\u{265E}\u{265F}".contains(ch)))
            })
            .count();
        assert_eq!(
            piece_texts, 32,
            "Should render 32 pieces, got {piece_texts}"
        );
    }

    #[test]
    fn test_render_selected_square() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(0, 1)); // select knight
        let commands = app.render();
        let has_selection = commands.iter().any(
            |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == SELECTED_SQUARE),
        );
        assert!(has_selection, "Should render selected square highlight");
    }

    #[test]
    fn test_render_legal_move_indicators() {
        let mut app = ChessApp::new();
        app.click_square(Pos::new(0, 1)); // select b1 knight (has 2 legal moves)
        let commands = app.render();
        let dot_count = commands
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == LEGAL_MOVE_DOT),
            )
            .count();
        assert!(
            dot_count >= 2,
            "Should show legal move dots for knight, got {dot_count}"
        );
    }

    // ── Mouse event tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_on_board() {
        let mut app = ChessApp::new();
        // Click on e2 (col=4, row=1). Screen: x = offset + 4*64 + 32, y = offset + 6*64 + 32
        let x = BOARD_OFFSET_X + 4.0 * SQUARE_SIZE + SQUARE_SIZE / 2.0;
        let y = BOARD_OFFSET_Y + 6.0 * SQUARE_SIZE + SQUARE_SIZE / 2.0; // row 1 = screen row 6
        let event = MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse(&event);
        // Should select the pawn on e2
        assert_eq!(app.selected, Some(Pos::new(1, 4)));
    }

    #[test]
    fn test_mouse_click_outside_board() {
        let mut app = ChessApp::new();
        let event = MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        app.handle_mouse(&event);
        assert!(app.selected.is_none());
    }

    // ── Move struct tests ───────────────────────────────────────────

    #[test]
    fn test_move_normal() {
        let mv = Move::normal(Pos::new(1, 4), Pos::new(3, 4));
        assert!(!mv.is_castling);
        assert!(!mv.is_en_passant);
        assert!(mv.promotion.is_none());
    }

    #[test]
    fn test_move_castling() {
        let mv = Move::castling(Pos::new(0, 4), Pos::new(0, 6));
        assert!(mv.is_castling);
        assert!(!mv.is_en_passant);
    }

    #[test]
    fn test_move_en_passant() {
        let mv = Move::en_passant(Pos::new(4, 4), Pos::new(5, 5));
        assert!(mv.is_en_passant);
        assert!(!mv.is_castling);
    }

    #[test]
    fn test_move_promotion() {
        let mv = Move::promotion(Pos::new(6, 4), Pos::new(7, 4), PieceKind::Queen);
        assert_eq!(mv.promotion, Some(PieceKind::Queen));
        assert!(!mv.is_castling);
    }

    // ── Empty board tests ───────────────────────────────────────────

    #[test]
    fn test_empty_board() {
        let board = Board::empty();
        for row in 0..8 {
            for col in 0..8 {
                assert!(board.get(Pos::new(row, col)).is_none());
            }
        }
        assert!(!board.castling.white_kingside);
    }

    #[test]
    fn test_find_king() {
        let board = Board::new();
        assert_eq!(board.find_king(Side::White), Some(Pos::new(0, 4)));
        assert_eq!(board.find_king(Side::Black), Some(Pos::new(7, 4)));
    }

    #[test]
    fn test_find_king_missing() {
        let board = Board::empty();
        assert!(board.find_king(Side::White).is_none());
    }

    // ── Side-to-move switching ──────────────────────────────────────

    #[test]
    fn test_side_switches_after_move() {
        let mut board = Board::new();
        assert_eq!(board.side_to_move, Side::White);
        let mv = Move::normal(Pos::new(1, 4), Pos::new(3, 4));
        board.make_move_unchecked(mv);
        assert_eq!(board.side_to_move, Side::Black);
    }

    #[test]
    fn test_fullmove_increments() {
        let mut board = Board::new();
        assert_eq!(board.fullmove_number, 1);
        // White moves
        board.make_move_unchecked(Move::normal(Pos::new(1, 4), Pos::new(3, 4)));
        assert_eq!(board.fullmove_number, 1); // Still 1 after White's move
        // Black moves
        board.make_move_unchecked(Move::normal(Pos::new(6, 4), Pos::new(4, 4)));
        assert_eq!(board.fullmove_number, 2); // Incremented after Black's move
    }

    // ── Event dispatch test ─────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = ChessApp::new();
        let event = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.cursor.col, 1);
    }

    #[test]
    fn test_handle_event_resize_ignored() {
        let mut app = ChessApp::new();
        let event = Event::Resize {
            width: 800,
            height: 600,
        };
        app.handle_event(&event);
        // Should not crash or change state
        assert_eq!(app.game_result, GameResult::Ongoing);
    }

    // ── Game over prevents input ────────────────────────────────────

    #[test]
    fn test_game_over_prevents_moves() {
        let mut app = ChessApp::new();
        app.game_result = GameResult::WhiteWins;
        app.click_square(Pos::new(1, 4)); // try to select
        assert!(
            app.selected.is_none(),
            "Should not allow selection when game is over"
        );
    }

    // ── Halfmove clock tests ────────────────────────────────────────

    #[test]
    fn test_halfmove_clock_resets_on_pawn_move() {
        let mut board = Board::new();
        board.halfmove_clock = 10;
        board.make_move_unchecked(Move::normal(Pos::new(1, 4), Pos::new(3, 4)));
        assert_eq!(board.halfmove_clock, 0);
    }

    #[test]
    fn test_halfmove_clock_increments() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        place(&mut board, 0, 1, Side::White, PieceKind::Knight);
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        board.make_move_unchecked(Move::normal(Pos::new(0, 1), Pos::new(2, 2)));
        assert_eq!(board.halfmove_clock, 1);
    }

    // ── Black castling tests ────────────────────────────────────────

    #[test]
    fn test_black_kingside_castling() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        board.castling.black_kingside = true;
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::Rook);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(7, 6))
        );
    }

    #[test]
    fn test_black_queenside_castling() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        board.castling.black_queenside = true;
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 7, 0, Side::Black, PieceKind::Rook);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        let moves = board.generate_legal_moves();
        assert!(
            moves
                .iter()
                .any(|m| m.is_castling && m.to == Pos::new(7, 2))
        );
    }

    // ── Black pawn tests ────────────────────────────────────────────

    #[test]
    fn test_black_pawn_moves_down() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 6, 4, Side::Black, PieceKind::Pawn);
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        let moves = board.generate_legal_moves();
        // Black pawn should move from row 6 to row 5 (and double push to row 4)
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(6, 4) && m.to == Pos::new(5, 4))
        );
        assert!(
            moves
                .iter()
                .any(|m| m.from == Pos::new(6, 4) && m.to == Pos::new(4, 4))
        );
    }

    #[test]
    fn test_black_pawn_promotion() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 1, 4, Side::Black, PieceKind::Pawn);
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 0, 0, Side::White, PieceKind::King);
        let moves = board.generate_legal_moves();
        let promos: Vec<_> = moves
            .iter()
            .filter(|m| m.from == Pos::new(1, 4) && m.promotion.is_some())
            .collect();
        assert_eq!(promos.len(), 4);
    }

    // ── Castling rights on rook capture ─────────────────────────────

    #[test]
    fn test_castling_rights_lost_on_rook_captured() {
        let mut board = Board::empty();
        board.side_to_move = Side::White;
        board.castling.black_kingside = true;
        place(&mut board, 0, 0, Side::White, PieceKind::Queen);
        place(&mut board, 0, 4, Side::White, PieceKind::King);
        place(&mut board, 7, 4, Side::Black, PieceKind::King);
        place(&mut board, 7, 7, Side::Black, PieceKind::Rook);

        // White queen captures Black's h8 rook
        board.make_move_unchecked(Move::normal(Pos::new(0, 0), Pos::new(7, 7)));
        assert!(!board.castling.black_kingside);
    }
}
