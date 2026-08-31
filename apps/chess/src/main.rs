//! Slate OS Chess -- a full chess game against a minimax opponent, in a real
//! window.
//!
//! A complete engine: legal move generation including castling, en passant and
//! promotion, check/checkmate/stalemate detection, alpha-beta search, algebraic
//! notation, captured pieces.
//!
//! Two things about it are worth stating up front, because both were wrong
//! before it was wired to a window.
//!
//! [`Layout`] is solved from the size the compositor gave us, and owns both the
//! mapping from a square to its rectangle and the inverse. The board used to be
//! drawn from `SQUARE_SIZE = 64.0` and two offset constants and clicked through
//! a free function of the same three, so the arithmetic agreed with itself in
//! every window and with the picture in exactly one. The drawing pass also
//! records a hit box on every square it draws, so a click is answered by the
//! picture rather than by arithmetic the picture may not have been drawn from.
//!
//! The opponent searches on a clock rather than inside the click handler.
//! `click_square` used to call `ai_turn` directly, which ran an alpha-beta
//! search to depth three before the handler returned: the window was frozen for
//! the duration and "Black is thinking" was a state no frame could be drawn in.
//! Black's reply now arrives on an [`Event::Tick`], and the phase in between is
//! a phase [`ChessApp::frame`] can paint.

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
const SURFACE0: Color = Color::from_hex(0x313244);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Board colors ────────────────────────────────────────────────────
const LIGHT_SQUARE: Color = Color::from_hex(0x9CA0B0);
const DARK_SQUARE: Color = Color::from_hex(0x585B70);
const SELECTED_SQUARE: Color = Color::from_hex(0x89B4FA);
const LEGAL_MOVE_DOT: Color = Color::rgba(166, 227, 161, 140);
const LAST_MOVE_HIGHLIGHT: Color = Color::rgba(250, 179, 135, 80);
const CHECK_HIGHLIGHT: Color = Color::rgba(243, 139, 168, 120);

/// The window the program asks for, and the size its tests draw at.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 660.0;

/// What the pointer can land on.
///
/// The drawing pass records one of these for every square it draws and for the
/// New game button, so a click is answered by the picture rather than by
/// arithmetic over constants the picture may not have been drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A square, by rank row and file column, in board coordinates -- row 0 is
    /// White's back rank, as everywhere else in this program.
    Square(i8, i8),
    NewGame,
}

/// Every rectangle and type size the frame is drawn from, solved from the
/// window the compositor gave us.
///
/// This replaced eleven constants -- `SQUARE_SIZE = 64.0`, `BOARD_OFFSET_X`,
/// `BOARD_OFFSET_Y`, `PANEL_X` and five font sizes among them. `render` took no
/// width and no height at all, so the program drew the same 852x612 picture into
/// whatever window it was given, and `square_at` resolved the click from the
/// same constants: in any other window the board was drawn in one place and
/// clicked in another.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    /// The title and the status message.
    header: Rect,
    /// The square the board occupies, including the margins the rank and file
    /// labels need outside the grid.
    board: Rect,
    /// The information column beside the board. Zero-width when dropped.
    panel: Rect,
    /// The New game button at the foot of the panel. Empty when there is no
    /// panel to put it in.
    new_game: Rect,
    /// Top-left corner of the grid itself, inside the label margins.
    origin: (f32, f32),
    square: f32,
    /// The width of the rank-label column, which is also the height of the
    /// file-label row.
    margin: f32,
    pad: f32,
    title: f32,
    font: f32,
    label: f32,
    small: f32,
    piece: f32,
    dot: f32,
}

impl Layout {
    /// Solve the layout for a window of `w` by `h`.
    ///
    /// The board is square and takes whatever the header and the panel leave,
    /// so a window of any shape gets a playable board rather than eight ranks
    /// of 64 px squares drawn off the bottom of a short one. The panel is
    /// dropped whole rather than drawn illegibly narrow when the window cannot
    /// pay for it, which is the rule the other wired apps use for their chrome.
    fn solve(w: f32, h: f32) -> Self {
        let window = Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0));
        let font = (h / 36.0).clamp(9.0, 17.0);
        let title = (font * 1.5).clamp(13.0, 26.0);
        let label = (font - 2.0).max(7.0);
        let small = (font - 4.0).max(6.0);
        let pad = (w.min(h) * 0.025).clamp(3.0, 16.0);

        let hdr_h = (h * 0.09).clamp(0.0, 46.0);
        let header = Rect::new(0.0, 0.0, w, hdr_h);

        // The panel is worth having only if it can hold its widest line. Below
        // that it is dropped and the board takes the whole width, rather than
        // squeezing the board to make room for a column too narrow to read.
        let panel_w_min =
            text::measure("Arrows/Enter: Navigate", label, FontWeightHint::Regular).max(
                text::measure("Captured by White:", label, FontWeightHint::Bold),
            ) + pad * 2.0;
        let want_panel = (w * 0.28).clamp(panel_w_min, 260.0);
        let panel_w = if w - want_panel >= h * 0.45 && want_panel <= w * 0.4 {
            want_panel
        } else {
            0.0
        };

        let free_w = (w - panel_w).max(0.0);
        let free_h = (h - header.bottom()).max(0.0);
        let side = free_w.min(free_h).max(0.0);
        let board = Rect::new(
            ((free_w - side) / 2.0).max(0.0),
            header.bottom() + ((free_h - side) / 2.0).max(0.0),
            side,
            side,
        );
        let panel = Rect::new(free_w, header.bottom(), panel_w, free_h);

        // Ranks are labelled down the left and files along the bottom, so one
        // margin's worth comes off each of those two edges and the grid is what
        // is left. Only two edges, unlike gomoku, because a chess label names a
        // rank or a file rather than an intersection's coordinate.
        let margin = (label * 1.8).min(side / 6.0);
        let grid = (side - margin).max(0.0);
        let square = grid / 8.0;
        let origin = (board.x + margin, board.y);

        // The button sits at the foot of the panel, above nothing, so a game
        // that has ended can be restarted with the pointer. It used to be
        // `Ctrl+N` and nothing else, which is keyboard-only at exactly the
        // moment a player reaches for the mouse.
        let btn_h = (font * 2.0).min(free_h);
        let new_game = if panel_w > 0.0 {
            Rect::new(
                panel.x + pad,
                (panel.bottom() - pad - btn_h).max(panel.y),
                (panel_w - pad * 2.0).max(0.0),
                btn_h,
            )
        } else {
            Rect::EMPTY
        };

        Self {
            window,
            header,
            board,
            panel,
            new_game,
            origin,
            square,
            margin,
            pad,
            title,
            font,
            label,
            small,
            piece: square * 0.62,
            dot: square * 0.13,
        }
    }

    /// The rectangle the square `pos` is drawn in, which is also the hit box
    /// the drawing pass records for it.
    ///
    /// Row 0 is White's back rank and belongs at the *bottom* of the window, so
    /// the row is flipped on the way to the screen. That flip is the one part
    /// of the mapping a reader cannot check by inspection, and the one part
    /// [`Layout::square_at`] has to undo.
    fn square_rect(&self, pos: Pos) -> Rect {
        // Row 0 is drawn at the bottom, so the screen row counts down.
        let screen_row = f32::from(7i8.saturating_sub(pos.row));
        Rect::new(
            self.origin.0 + f32::from(pos.col) * self.square,
            self.origin.1 + screen_row * self.square,
            self.square,
            self.square,
        )
    }

    /// Screen coordinates of the centre of `pos`.
    fn square_centre(&self, pos: Pos) -> (f32, f32) {
        self.square_rect(pos).centre()
    }
}

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
        (0..8).contains(&self.row) && (0..8).contains(&self.col)
    }

    /// The square `(d_row, d_col)` away, or `None` if that step leaves the
    /// board.
    ///
    /// Every move generator walks the board by a delta, and each of them used
    /// to write `Pos::new(pos.row + dr, pos.col + dc)` and rely on a later
    /// `is_valid` to catch what the addition produced. That is a bounds check
    /// after the fact rather than instead of the fault, and it made "did the
    /// step leave the board" a question answered at fourteen separate sites.
    /// It is answered here now, once, and the sum is checked rather than
    /// assumed.
    fn offset(self, d_row: i8, d_col: i8) -> Option<Self> {
        let p = Self {
            row: self.row.checked_add(d_row)?,
            col: self.col.checked_add(d_col)?,
        };
        p.is_valid().then_some(p)
    }

    /// The squares in direction `(d_row, d_col)`, nearest first, stopping at
    /// the edge of the board.
    ///
    /// Every sliding piece and every sliding attack walks one of these, and
    /// each of the five places that did so wrote its own `r += dr; c += dc`
    /// loop with its own copy of the bounds test.
    fn ray(self, d_row: i8, d_col: i8) -> impl Iterator<Item = Self> {
        std::iter::successors(self.offset(d_row, d_col), move |p| p.offset(d_row, d_col))
    }

    /// The letter this square's file is called, or `None` off the board.
    ///
    /// `b'a' + col as u8` was written at five sites -- here, three times in
    /// `move_to_algebraic`, and once more in the file labels along the bottom
    /// of the board -- each of them able to name a square that does not exist.
    fn file_char(self) -> Option<char> {
        u8::try_from(self.col)
            .ok()
            .filter(|c| *c < 8)
            .and_then(|c| b'a'.checked_add(c))
            .map(char::from)
    }

    /// The digit this square's rank is called, or `None` off the board.
    fn rank_char(self) -> Option<char> {
        u8::try_from(self.row)
            .ok()
            .filter(|r| *r < 8)
            .and_then(|r| b'1'.checked_add(r))
            .map(char::from)
    }

    /// Convert to algebraic notation (e.g. "e4").
    ///
    /// Off-board squares have no name, so they get an empty one rather than a
    /// character read off the end of the alphabet.
    fn to_algebraic(self) -> String {
        match (self.file_char(), self.rank_char()) {
            (Some(file), Some(rank)) => format!("{file}{rank}"),
            _ => String::new(),
        }
    }

    /// Index into a 64-element array (row * 8 + col), or `None` off the board.
    fn index(self) -> Option<usize> {
        self.is_valid().then(|| {
            usize::from(self.row.unsigned_abs())
                .saturating_mul(8)
                .saturating_add(usize::from(self.col.unsigned_abs()))
        })
    }

    /// Mirror index for black piece-square tables (flip rank).
    fn mirror_index(self) -> Option<usize> {
        Self::new(7i8.checked_sub(self.row)?, self.col).index()
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
    #[cfg(test)]
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

    /// What stands on `pos`, or `None` if nothing does -- or if `pos` is not
    /// a square.
    ///
    /// "Off the board" is an answer the move generators need rather than a
    /// fault to be guarded against: a ray walks until it leaves the board on
    /// purpose. So the bounds live in the accessor and are expressed by
    /// `slice::get`, rather than in an `is_valid` test in front of an index
    /// that would panic without it.
    fn get(&self, pos: Pos) -> Option<Piece> {
        let row = usize::try_from(pos.row).ok()?;
        let col = usize::try_from(pos.col).ok()?;
        self.squares
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .flatten()
    }

    fn set(&mut self, pos: Pos, piece: Option<Piece>) {
        if let Ok(row) = usize::try_from(pos.row)
            && let Ok(col) = usize::try_from(pos.col)
            && let Some(square) = self.squares.get_mut(row).and_then(|r| r.get_mut(col))
        {
            *square = piece;
        }
    }

    /// Find the king position for the given side.
    fn find_king(&self, side: Side) -> Option<Pos> {
        // Iterated rather than indexed: the bounds are the array's own, so
        // there is no arithmetic here that a check would have to rescue.
        self.squares.iter().enumerate().find_map(|(row, rank)| {
            rank.iter().enumerate().find_map(|(col, square)| {
                let p = (*square)?;
                if p.side != side || p.kind != PieceKind::King {
                    return None;
                }
                Some(Pos::new(i8::try_from(row).ok()?, i8::try_from(col).ok()?))
            })
        })
    }

    /// Check if a square is attacked by any piece of the given side.
    fn is_attacked_by(&self, pos: Pos, attacker: Side) -> bool {
        // A step from `pos` that lands on one of `kinds` belonging to the
        // attacker.  Every clause below is that same question, so it is asked
        // in one place rather than spelled out four times with four copies of
        // the side-and-kind test.
        let steps_to = |deltas: &[(i8, i8)], kinds: &[PieceKind]| {
            deltas.iter().any(|&(dr, dc)| {
                pos.offset(dr, dc)
                    .and_then(|p| self.get(p))
                    .is_some_and(|piece| piece.side == attacker && kinds.contains(&piece.kind))
            })
        };
        // The first piece along each ray, if any -- what is behind it is
        // blocked and does not attack `pos`.
        let slides_to = |dirs: &[(i8, i8)], kinds: &[PieceKind]| {
            dirs.iter().any(|&(dr, dc)| {
                pos.ray(dr, dc)
                    .find_map(|p| self.get(p))
                    .is_some_and(|piece| piece.side == attacker && kinds.contains(&piece.kind))
            })
        };

        // A pawn captures toward its own far rank, so from `pos` the pawns
        // that attack it lie in the direction they came from.
        let pawn_dir: i8 = if attacker == Side::White { -1 } else { 1 };

        steps_to(&KNIGHT_DELTAS, &[PieceKind::Knight])
            // The eight queen directions at one step are exactly the eight
            // squares adjacent to `pos`, which is where an enemy king attacks
            // from.
            || steps_to(&QUEEN_DIRS, &[PieceKind::King])
            || steps_to(&[(pawn_dir, -1), (pawn_dir, 1)], &[PieceKind::Pawn])
            || slides_to(&ROOK_DIRS, &[PieceKind::Rook, PieceKind::Queen])
            || slides_to(&BISHOP_DIRS, &[PieceKind::Bishop, PieceKind::Queen])
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

        // A pawn reaching the far rank promotes, and it may promote to any
        // of four pieces, so "record this pawn move" is not one push.
        let record = |to: Pos, moves: &mut Vec<Move>| {
            if to.row == promo_rank {
                for kind in PROMOTION_KINDS {
                    moves.push(Move::promotion(pos, to, kind));
                }
            } else {
                moves.push(Move::normal(pos, to));
            }
        };

        // Single push, and the double push that only follows a clear single
        // one.
        if let Some(one) = pos.offset(dir, 0)
            && self.get(one).is_none()
        {
            record(one, moves);
            if pos.row == start_rank
                && let Some(two) = one.offset(dir, 0)
                && self.get(two).is_none()
            {
                moves.push(Move::normal(pos, two));
            }
        }

        // Captures, including en passant.
        for dc in [-1i8, 1] {
            let Some(cap) = pos.offset(dir, dc) else {
                continue;
            };
            match self.get(cap) {
                Some(target) if target.side != side => record(cap, moves),
                Some(_) => {}
                None if self.en_passant == Some(cap) => moves.push(Move::en_passant(pos, cap)),
                None => {}
            }
        }
    }

    fn generate_knight_moves(&self, pos: Pos, side: Side, moves: &mut Vec<Move>) {
        for (dr, dc) in KNIGHT_DELTAS {
            let Some(to) = pos.offset(dr, dc) else {
                continue;
            };
            if self.get(to).is_none_or(|p| p.side != side) {
                moves.push(Move::normal(pos, to));
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
            for to in pos.ray(dr, dc) {
                match self.get(to) {
                    None => moves.push(Move::normal(pos, to)),
                    Some(p) => {
                        // The first piece in the way ends the ray; it can be
                        // captured only if it is not one of ours.
                        if p.side != side {
                            moves.push(Move::normal(pos, to));
                        }
                        break;
                    }
                }
            }
        }
    }

    fn generate_king_moves(&self, pos: Pos, side: Side, moves: &mut Vec<Move>) {
        // Normal king moves
        for (dr, dc) in QUEEN_DIRS {
            let Some(to) = pos.offset(dr, dc) else {
                continue;
            };
            if self.get(to).is_none_or(|p| p.side != side) {
                moves.push(Move::normal(pos, to));
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
            // The captured pawn is on the capturing pawn's own rank, beside
            // rather than on the square being moved to.
            self.set(Pos::new(mv.from.row, mv.to.col), None);
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
        if piece.kind == PieceKind::Pawn
            && let Some(diff) = mv.to.row.checked_sub(mv.from.row)
            && diff.abs() == 2
        {
            // The square passed over: one step back from where the pawn
            // landed, in the direction it came from.
            self.en_passant = mv.to.offset(diff.signum().saturating_neg(), 0);
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
            self.halfmove_clock = self.halfmove_clock.saturating_add(1);
        }

        // Update fullmove number
        if self.side_to_move == Side::Black {
            self.fullmove_number = self.fullmove_number.saturating_add(1);
        }

        self.side_to_move = self.side_to_move.opponent();
    }
}

/// The eight squares a knight reaches, as (rank, file) deltas.
const KNIGHT_DELTAS: [(i8, i8); 8] = [
    (-2, -1),
    (-2, 1),
    (-1, -2),
    (-1, 2),
    (1, -2),
    (1, 2),
    (2, -1),
    (2, 1),
];

/// What a pawn reaching the far rank may become. Written out once: the pawn
/// generator needs it for a push and again for a capture, and a list that
/// differs between the two is a promotion the player can make one way and not
/// the other.
const PROMOTION_KINDS: [PieceKind; 4] = [
    PieceKind::Queen,
    PieceKind::Rook,
    PieceKind::Bishop,
    PieceKind::Knight,
];

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
                let total = material.saturating_add(positional);
                score = match piece.side {
                    Side::White => score.saturating_add(total),
                    Side::Black => score.saturating_sub(total),
                };
            }
        }
    }

    score
}

/// Get piece-square table bonus for a piece at a given position.
fn piece_square_value(piece: Piece, pos: Pos) -> i32 {
    // Black reads the tables through a mirrored index, because they are
    // written from White's side of the board. An off-board square has no
    // index, and so no bonus -- which is what the `if idx >= 64` guard below
    // used to say after the fact, having already computed the index.
    let idx = match piece.side {
        Side::White => pos.index(),
        Side::Black => pos.mirror_index(),
    };
    let Some(idx) = idx else {
        return 0;
    };
    let table = match piece.kind {
        PieceKind::Pawn => &PAWN_TABLE,
        PieceKind::Knight => &KNIGHT_TABLE,
        PieceKind::Bishop => &BISHOP_TABLE,
        PieceKind::Rook => &ROOK_TABLE,
        PieceKind::Queen => &QUEEN_TABLE,
        PieceKind::King => &KING_MIDDLEGAME_TABLE,
    };
    table.get(idx).copied().unwrap_or(0)
}

/// Minimax with alpha-beta pruning.
/// Returns the evaluation score from the perspective of the side to move
/// at the root call.
fn minimax(board: &Board, depth: i32, mut alpha: i32, mut beta: i32, maximizing: bool) -> i32 {
    if depth <= 0 {
        let eval = evaluate(board);
        return if maximizing {
            eval
        } else {
            eval.saturating_neg()
        };
    }

    let moves = board.generate_legal_moves();

    if moves.is_empty() {
        if board.is_in_check(board.side_to_move) {
            // Checkmate — worst for the side to move
            // Deeper mates are worth less, so a mate in one is preferred
            // over a mate in three.
            return if maximizing {
                KING_VALUE.saturating_add(depth).saturating_neg()
            } else {
                KING_VALUE.saturating_add(depth)
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
            let score = minimax(&child, depth.saturating_sub(1), alpha, beta, false);
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
            let score = minimax(&child, depth.saturating_sub(1), alpha, beta, true);
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
    // No separate emptiness test: "there is a first move" and "the list is
    // not empty" are the same question, and asking it twice leaves one copy
    // no test can reach.
    let moves = board.generate_legal_moves();
    let mut best_move = *moves.first()?;
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
            // A file letter is enough when no other candidate shares the
            // file; a rank digit when none shares the rank; both otherwise.
            if !same_col {
                notation.extend(mv.from.file_char());
            } else if !same_row {
                notation.extend(mv.from.rank_char());
            } else {
                notation.extend(mv.from.file_char());
                notation.extend(mv.from.rank_char());
            }
        }
    }

    // Capture
    let is_capture = board.get(mv.to).is_some() || mv.is_en_passant;
    if is_capture {
        if piece.kind == PieceKind::Pawn {
            notation.extend(mv.from.file_char());
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
    /// The moves so far, in algebraic notation, which is the only form
    /// anything reads them in.
    ///
    /// This was a `Vec<MoveRecord>` carrying the `Move` and the captured piece
    /// beside the notation, and nothing ever read either: the last move is kept
    /// in `last_move` and the captures in `captured_white`/`captured_black`.
    /// `#![allow(dead_code)]` is what let two unread fields sit here.
    move_history: Vec<String>,
    captured_white: Vec<Piece>, // White pieces captured by Black
    captured_black: Vec<Piece>, // Black pieces captured by White
    game_result: GameResult,
    status_message: String,
    /// Cursor position for keyboard navigation (row, col).
    cursor: Pos,
    /// True while Black owes a reply and the search has not run yet.
    ///
    /// `click_square` used to call `ai_turn()` inline, so an alpha-beta search
    /// to [`AI_DEPTH`] ran to completion before the click handler returned: the
    /// window did not repaint for its duration, and "Black is thinking" was a
    /// string no frame could ever show. The search runs on a tick now, and this
    /// flag is what a frame drawn in between paints.
    thinking: bool,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size: (f32, f32),
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
            thinking: false,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Reset to a new game.
    ///
    /// Built from [`ChessApp::new`] rather than by clearing eleven fields by
    /// hand, which is what it used to do: a field added to the struct and not
    /// to that list is a piece of the finished game that survives into the next
    /// one, and `thinking` -- added when the search moved onto a tick -- would
    /// have been exactly that. The window size is the one thing carried over,
    /// because it describes the window rather than the game.
    fn new_game(&mut self) {
        let size = self.size;
        *self = Self::new();
        self.size = size;
    }

    /// Remember the size the frame was last drawn at.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width, height);
    }

    /// Handle a click on a board square.
    ///
    /// Returns whether the click changed anything, which is what the pointer
    /// and the keyboard are both answered with: a click on a square that
    /// selects nothing and deselects nothing has not been acted on, and saying
    /// it was consumed would ask the compositor for a repaint that draws the
    /// same picture.
    fn click_square(&mut self, pos: Pos) -> bool {
        // The human plays White. While Black owes a reply the board is not
        // White's to touch, which is a different thing from the game being
        // over: `thinking` is a state the player can see and wait out.
        if self.game_result != GameResult::Ongoing
            || self.thinking
            || self.board.side_to_move != Side::White
        {
            return false;
        }

        if let Some(sel) = self.selected
            && let Some(mv) = self.find_legal_move(sel, pos)
        {
            self.execute_move(mv);
            return true;
        }

        // Select a piece (must be own piece)
        if let Some(piece) = self.board.get(pos)
            && piece.side == Side::White
        {
            let already = self.selected == Some(pos);
            self.selected = Some(pos);
            self.legal_moves_for_selected = self
                .board
                .generate_legal_moves()
                .into_iter()
                .filter(|m| m.from == pos)
                .collect();
            return !already;
        }

        // Clicked empty square or opponent piece without selection.
        self.deselect()
    }

    /// Drop the selection, reporting whether there was one to drop.
    fn deselect(&mut self) -> bool {
        let had = self.selected.is_some();
        self.selected = None;
        self.legal_moves_for_selected.clear();
        had
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

        self.move_history.push(notation);

        self.board.make_move_unchecked(mv);
        self.last_move = Some(mv);
        self.selected = None;
        self.legal_moves_for_selected.clear();

        self.update_game_state();
    }

    /// Run Black's search if one is owed, and report whether it ran.
    ///
    /// This is the only place the search is called from, and the only place
    /// `thinking` is tested. It used to be called `ai_turn` and invoked
    /// directly from `click_square`, which meant an alpha-beta search to
    /// [`AI_DEPTH`] ran inside the click handler and no frame was ever drawn
    /// while it was running.
    ///
    /// The `Event::Tick` arm is answered by the value returned rather than by
    /// asking `self.thinking` a second time: a condition written down in a
    /// caller and again in the callee has one copy no test can reach (see
    /// known-issues lesson 92).
    fn think(&mut self) -> bool {
        if !self.thinking {
            return false;
        }
        // `thinking` is not cleared here: both arms below end in
        // `update_game_state`, which derives it afresh from the position. A
        // second place that decides it is a second place that can decide it
        // wrongly.
        if let Some(mv) = ai_choose_move(&self.board) {
            self.execute_move(mv);
        } else {
            // No legal reply: the position is mate or stalemate, and it is
            // `update_game_state` that says which. Reaching here without it
            // having said so would leave the status stuck on "Black is
            // thinking" forever.
            self.update_game_state();
        }
        true
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

        // Black owes a reply exactly when the game is live and the move is
        // Black's. Derived here, once, from the state the branches above have
        // just settled -- rather than set at each place a move can be made,
        // which is how the score came to be credited twice over in gomoku.
        self.thinking =
            self.game_result == GameResult::Ongoing && self.board.side_to_move == Side::Black;
        if self.thinking {
            self.status_message = "Black is thinking".to_string();
        }
    }

    /// Move the keyboard cursor by `(d_row, d_col)`, reporting whether it
    /// moved.
    ///
    /// A cursor already against the edge does not move, and saying the key was
    /// consumed would ask for a repaint of an identical picture.
    fn step_cursor(&mut self, d_row: i8, d_col: i8) -> bool {
        let row = self.cursor.row.saturating_add(d_row).clamp(0, 7);
        let col = self.cursor.col.saturating_add(d_col).clamp(0, 7);
        let moved = (row, col) != (self.cursor.row, self.cursor.col);
        self.cursor = Pos::new(row, col);
        moved
    }

    /// Handle a keyboard event.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        let acted = match event.key {
            Key::N if event.modifiers.ctrl => {
                self.new_game();
                true
            }
            Key::Left => self.step_cursor(0, -1),
            Key::Right => self.step_cursor(0, 1),
            // Row 0 is White's back rank, drawn at the *bottom*, so Up walks
            // toward rank 8 and away from the bottom of the window.
            Key::Up => self.step_cursor(1, 0),
            Key::Down => self.step_cursor(-1, 0),
            Key::Enter | Key::Space => self.click_square(self.cursor),
            Key::Escape => self.deselect(),
            _ => false,
        };
        if acted {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Handle a mouse click event.
    ///
    /// The square is read out of the frame's hit boxes rather than computed
    /// from the layout a second time, so a square that was not drawn cannot be
    /// clicked and a square that was drawn is clickable exactly where its ink
    /// is.
    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return EventResult::Ignored;
        };
        let acted = match self
            .frame(self.size.0, self.size.1)
            .hit_test(event.x, event.y)
        {
            Some(Target::Square(row, col)) => self.click_square(Pos::new(row, col)),
            Some(Target::NewGame) => {
                self.new_game();
                true
            }
            // Off the board and off the button. The selection survives, which
            // is what it did before there was a frame to ask.
            None => false,
        };
        if acted {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Handle an event.
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

    /// Draw the whole window into a frame sized to it.
    ///
    /// Every rectangle comes from `l`, which was solved from `width` and
    /// `height`, and every square records the hit box the click handler reads
    /// back. `render(&self) -> Vec<RenderCommand>` took no size at all and
    /// painted a fixed 852x612 picture into whatever window it was given.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::solve(width, height);
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
        self.draw_board(&mut f, &l);
        self.draw_labels(&mut f, &l);
        self.draw_panel(&mut f, &l);
        f
    }

    /// The title, and the one line that says what the game is doing.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        let name = "Chess";
        let name_w = text::measure(name, l.title, FontWeightHint::Bold);
        let baseline = l.header.y + (l.header.h - l.title).max(0.0) / 2.0;
        text_at(
            f,
            name,
            l.pad,
            baseline,
            l.title,
            FontWeightHint::Bold,
            LAVENDER,
            Some((l.header.w - l.pad * 2.0).max(0.0)),
        );

        // The status shares the header with the title, so its column starts
        // where the title's ink ends rather than at a constant offset that was
        // only wide enough for one type size.
        let sx = l.pad + name_w + l.pad * 2.0;
        let status_color = match self.game_result {
            GameResult::Ongoing => {
                if self.thinking {
                    BLUE
                } else if self.board.is_in_check(self.board.side_to_move) {
                    RED
                } else {
                    SUBTEXT0
                }
            }
            GameResult::WhiteWins => GREEN,
            GameResult::BlackWins => RED,
            GameResult::Stalemate | GameResult::Draw => YELLOW,
        };
        text_at(
            f,
            &self.status_message,
            sx,
            l.header.y + (l.header.h - l.font).max(0.0) / 2.0,
            l.font,
            FontWeightHint::Regular,
            status_color,
            Some((l.header.right() - l.pad - sx).max(0.0)),
        );
    }

    /// The sixty-four squares, what stands on them, and what may be done to
    /// them.
    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.square <= 0.0 {
            return;
        }
        let king_pos = self.board.find_king(self.board.side_to_move);
        let in_check = self.board.is_in_check(self.board.side_to_move);

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                let r = l.square_rect(pos);

                // A light square is the one whose coordinates differ in parity,
                // which puts a dark square in each player's lower left -- the
                // rule a chess board is checked against.
                let base_color = if row.saturating_add(col) % 2 != 0 {
                    LIGHT_SQUARE
                } else {
                    DARK_SQUARE
                };
                f.push(RenderCommand::FillRect {
                    x: r.x,
                    y: r.y,
                    width: r.w,
                    height: r.h,
                    color: base_color,
                    corner_radii: CornerRadii::ZERO,
                });

                if let Some(last) = self.last_move
                    && (pos == last.from || pos == last.to)
                {
                    f.push(RenderCommand::FillRect {
                        x: r.x,
                        y: r.y,
                        width: r.w,
                        height: r.h,
                        color: LAST_MOVE_HIGHLIGHT,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if in_check && king_pos == Some(pos) {
                    f.push(RenderCommand::FillRect {
                        x: r.x,
                        y: r.y,
                        width: r.w,
                        height: r.h,
                        color: CHECK_HIGHLIGHT,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if self.selected == Some(pos) {
                    let inset = (l.square * 0.04).max(1.0);
                    f.push(RenderCommand::StrokeRect {
                        x: r.x + inset,
                        y: r.y + inset,
                        width: (r.w - inset * 2.0).max(0.0),
                        height: (r.h - inset * 2.0).max(0.0),
                        color: SELECTED_SQUARE,
                        line_width: (l.square * 0.05).max(1.0),
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if self.cursor == pos && self.selected.is_none() {
                    let inset = (l.square * 0.02).max(1.0);
                    f.push(RenderCommand::StrokeRect {
                        x: r.x + inset,
                        y: r.y + inset,
                        width: (r.w - inset * 2.0).max(0.0),
                        height: (r.h - inset * 2.0).max(0.0),
                        color: MAUVE,
                        line_width: (l.square * 0.035).max(1.0),
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if let Some(piece) = self.board.get(pos) {
                    let glyph = piece.unicode();
                    let w = text::measure(glyph, l.piece, FontWeightHint::Regular);
                    text_at(
                        f,
                        glyph,
                        r.x + (r.w - w) / 2.0,
                        r.y + (r.h - l.piece) / 2.0,
                        l.piece,
                        FontWeightHint::Regular,
                        TEXT_COLOR,
                        None,
                    );
                }

                // Recorded last, so the box covers everything drawn in the
                // square rather than whatever happened to be pushed after it.
                f.hit(Target::Square(row, col), r);
            }
        }

        // Where the selected piece may go.
        for mv in &self.legal_moves_for_selected {
            let r = l.square_rect(mv.to);
            if self.board.get(mv.to).is_some() {
                // A capture is ringed rather than dotted, because a dot in the
                // middle of an occupied square lands on the piece.
                let inset = (l.square * 0.05).max(1.0);
                f.push(RenderCommand::StrokeRect {
                    x: r.x + inset,
                    y: r.y + inset,
                    width: (r.w - inset * 2.0).max(0.0),
                    height: (r.h - inset * 2.0).max(0.0),
                    color: LEGAL_MOVE_DOT,
                    line_width: (l.square * 0.05).max(1.0),
                    corner_radii: CornerRadii::all(l.square * 0.06),
                });
            } else {
                let (cx, cy) = l.square_centre(mv.to);
                f.push(RenderCommand::FillRect {
                    x: cx - l.dot,
                    y: cy - l.dot,
                    width: l.dot * 2.0,
                    height: l.dot * 2.0,
                    color: LEGAL_MOVE_DOT,
                    corner_radii: CornerRadii::all(l.dot),
                });
            }
        }
    }

    /// Rank numbers down the left of the grid and file letters along the
    /// bottom, each centred on the rank or file it names.
    ///
    /// They are dropped whole when the margin the layout could spare is too
    /// small to hold them, rather than drawn overlapping the board.
    fn draw_labels(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.square <= 0.0 || l.margin < l.label {
            return;
        }
        for rank in 0..8i8 {
            let r = l.square_rect(Pos::new(rank, 0));
            let Some(s) = Pos::new(rank, 0).rank_char().map(String::from) else {
                continue;
            };
            let w = text::measure(&s, l.label, FontWeightHint::Regular);
            text_at(
                f,
                &s,
                l.board.x + (l.margin - w) / 2.0,
                r.y + (r.h - l.label) / 2.0,
                l.label,
                FontWeightHint::Regular,
                SUBTEXT0,
                None,
            );
        }
        for file in 0..8i8 {
            let r = l.square_rect(Pos::new(0, file));
            let Some(s) = Pos::new(0, file).file_char().map(String::from) else {
                continue;
            };
            let w = text::measure(&s, l.label, FontWeightHint::Regular);
            text_at(
                f,
                &s,
                r.x + (r.w - w) / 2.0,
                r.bottom() + (l.margin - l.label) / 2.0,
                l.label,
                FontWeightHint::Regular,
                SUBTEXT0,
                None,
            );
        }
    }

    /// The captured pieces, the moves so far, the controls, and the New game
    /// button.
    ///
    /// Every row is placed from `l` and stops at the button rather than
    /// running past it: the move list used to grow downward without a bound
    /// and painted straight off the bottom of the window after about thirty
    /// moves.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.w <= 0.0 {
            return;
        }
        let x = l.panel.x + l.pad;
        let w = (l.panel.w - l.pad * 2.0).max(0.0);
        let line = l.font * 1.35;
        let mut y = l.panel.y + l.pad;

        // Everything above the controls has to fit above them, and the
        // controls above the button.
        let controls: [&str; 3] = ["Ctrl+N: New game", "Arrows/Enter: Move", "Esc: Deselect"];
        let controls_h = l.small * 1.5 * controls.len() as f32;
        let floor = (l.new_game.y - l.pad - controls_h).max(l.panel.y);

        for (heading, pieces) in [
            ("Captured by White:", &self.captured_black),
            ("Captured by Black:", &self.captured_white),
        ] {
            if y + line > floor {
                break;
            }
            text_at(
                f,
                heading,
                x,
                y,
                l.label,
                FontWeightHint::Bold,
                SUBTEXT0,
                Some(w),
            );
            y += line;
            let taken: String = pieces
                .iter()
                .map(|p| p.unicode())
                .collect::<Vec<_>>()
                .join(" ");
            if !taken.is_empty() && y + line <= floor {
                text_at(
                    f,
                    &taken,
                    x,
                    y,
                    l.font,
                    FontWeightHint::Regular,
                    TEXT_COLOR,
                    Some(w),
                );
            }
            y += line;
        }

        if y + line <= floor {
            text_at(
                f,
                "Moves:",
                x,
                y,
                l.label,
                FontWeightHint::Bold,
                SUBTEXT0,
                Some(w),
            );
            y += line;
        }

        // The last pairs rather than the first: a player watching a long game
        // wants the move just played, and the panel can only hold so many.
        let rows = ((floor - y) / (l.small * 1.3)).floor().max(0.0) as usize;
        let pairs: Vec<String> = self
            .move_history
            .chunks(2)
            .enumerate()
            .filter_map(|(n, pair)| {
                let white = pair.first()?;
                let mut s = format!("{}. {white}", n.saturating_add(1));
                if let Some(black) = pair.get(1) {
                    s.push(' ');
                    s.push_str(black);
                }
                Some(s)
            })
            .collect();
        for s in pairs.iter().skip(pairs.len().saturating_sub(rows)) {
            text_at(
                f,
                s,
                x,
                y,
                l.small,
                FontWeightHint::Regular,
                TEXT_COLOR,
                Some(w),
            );
            y += l.small * 1.3;
        }

        let mut cy = (l.new_game.y - l.pad - controls_h).max(l.panel.y);
        for hint in controls {
            text_at(
                f,
                hint,
                x,
                cy,
                l.small,
                FontWeightHint::Regular,
                OVERLAY0,
                Some(w),
            );
            cy += l.small * 1.5;
        }

        self.draw_button(f, l);
    }

    /// The one thing in the panel that can be clicked.
    fn draw_button(&self, f: &mut Frame<Target>, l: &Layout) {
        let r = l.new_game;
        if r.is_empty() {
            return;
        }
        f.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(l.pad * 0.4),
        });
        let label = "New game";
        let tw = text::measure(label, l.font, FontWeightHint::Bold);
        text_at(
            f,
            label,
            r.x + (r.w - tw).max(0.0) / 2.0,
            r.y + (r.h - l.font).max(0.0) / 2.0,
            l.font,
            FontWeightHint::Bold,
            TEXT_COLOR,
            Some(r.w),
        );
        f.hit(Target::NewGame, r);
    }
}

/// Push a string, or nothing when there is no room for it.
///
/// A `max_width` of zero or less is not a narrow column but no column at all,
/// and text drawn into one is ink outside the box that was meant to hold it.
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
    if s.is_empty() || font_size <= 0.0 || max_width.is_some_and(|w| w <= 0.0) {
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

impl App for ChessApp {
    fn title(&self) -> String {
        String::from("Chess")
    }

    fn app_id(&self) -> String {
        String::from("chess")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Black's search runs on this tick rather than inside the click handler.
    /// Without an interval here the game would enter `thinking` after White's
    /// first move and stay there.
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

impl Probe for ChessApp {
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
        // Nothing scrolls: the board is sized to the window rather than panned
        // inside it, and the move list is trimmed to what the panel holds.
        None
    }
}

fn main() -> ExitCode {
    let mut app = ChessApp::new();
    app::launch("chess", &mut app)
}

// ── Tests ───────────────────────────────────────────────────────────

// A test that indexes past the end, or unwraps a `None`, is a test that has
// already failed; panicking is the reporting mechanism, not a fault. `expect`
// rather than `allow` so that a lint the tests stop tripping is reported
// rather than silently kept.
#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "a panicking test is a failing test, which is the point"
)]
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
        assert_eq!(Pos::new(0, 0).index(), Some(0));
        assert_eq!(Pos::new(1, 0).index(), Some(8));
        assert_eq!(Pos::new(7, 7).index(), Some(63));
    }

    #[test]
    fn off_board_squares_have_no_index() {
        // The index is what reads the piece-square tables, so a square that is
        // not on the board must not produce one -- there is no entry for it,
        // and an index computed anyway would name some other square's entry.
        assert_eq!(Pos::new(-1, 0).index(), None);
        assert_eq!(Pos::new(0, 8).index(), None);
        assert_eq!(Pos::new(8, 0).mirror_index(), None);
        assert_eq!(Pos::new(0, -1).mirror_index(), None);
    }

    #[test]
    fn test_pos_mirror_index() {
        assert_eq!(Pos::new(0, 0).mirror_index(), Some(56));
        assert_eq!(Pos::new(7, 7).mirror_index(), Some(7));
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
        // The click plays White's move and hands the turn over; it does not
        // also run the search, because a search that runs inside the click
        // freezes the window until it finishes.
        assert_eq!(app.board.side_to_move, Side::Black);
        assert_eq!(app.move_history.len(), 1);
        assert!(app.thinking);

        // The tick is what runs it.
        assert!(app.think());
        assert_eq!(app.board.side_to_move, Side::White);
        assert_eq!(app.move_history.len(), 2);
        assert!(!app.thinking);
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
