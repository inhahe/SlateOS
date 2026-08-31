//! Slate OS Checkers -- American Checkers (English draughts) against an AI.
//!
//! An 8x8 board with the standard rules: mandatory jumps, multi-jump chains,
//! king promotion, and a minimax search with alpha-beta pruning for the
//! opponent. Alongside the board sit the piece counts, the captures, the move
//! number and a scrolling history.
//!
//! The whole picture is solved from the size the window reports each frame:
//! there is no built-in size the drawing falls back on, and every box a click
//! is tested against is one the drawing pass recorded. Every square is
//! clickable as well as reachable by the arrow keys.
//!
//! Themed with the Catppuccin Mocha palette.

use std::process::ExitCode;

use guitk::color::Color;
#[cfg(test)]
use guitk::event::Modifiers;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

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

// ── The size the window opens at ────────────────────────────────────
//
// The only two pixel counts in the file, and they are a *starting* size, not a
// layout: everything below is solved from whatever size the window reports,
// and these are merely what it asks the compositor for on the way up. What
// stood here was eleven of them -- a 64-pixel square, a 40-pixel left margin,
// a panel pinned to `BOARD_OFFSET_X + BOARD_PIXEL_SIZE + 20.0` -- and the
// window was whatever they happened to add up to. A board that cannot be
// resized is a board that is wrong at every size but one.
const WINDOW_WIDTH: f32 = 880.0;
const WINDOW_HEIGHT: f32 = 660.0;

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
/// Only dark squares are used in checkers; `Pos::is_dark` is the single place
/// that decides which those are.
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
    ///
    /// A checkers board is set up with a dark square in each player's
    /// lower-left corner -- the same orientation rule as chess -- and the
    /// pieces stand on the dark squares. So a1, which is `Pos::new(0, 0)`, is
    /// dark and playable; the parity follows from that, not the other way
    /// round.
    ///
    /// Getting this backwards mirrors the board left-to-right. The game still
    /// plays legally, because the playable squares are a mirror image of
    /// themselves -- which is exactly why the old parity survived unnoticed.
    /// The only symptom is that the board does not look like a checkers board:
    /// the double corner ends up on each player's left instead of their right.
    fn is_dark(self) -> bool {
        self.row.saturating_add(self.col) % 2 == 0
    }

    /// The square `dr` ranks and `dc` files from this one.
    ///
    /// Saturating rather than wrapping: a step off the end of an `i8` must stay
    /// off the board, not reappear on the far side of it. Every caller tests
    /// the result with `is_valid`, and saturation is what makes that test
    /// sufficient.
    fn offset(self, dr: i8, dc: i8) -> Self {
        Self::new(self.row.saturating_add(dr), self.col.saturating_add(dc))
    }

    /// This square as a pair of array subscripts, or `None` if it is off the
    /// board.
    ///
    /// The one place a `Pos` becomes a subscript. `is_valid` followed by a pair
    /// of `as usize` casts used to be written out at each of the four sites
    /// that indexed the board -- four chances for the check and the cast to
    /// drift apart, and four `[..]` that a bad `Pos` could panic on.
    fn index(self) -> Option<(usize, usize)> {
        if !self.is_valid() {
            return None;
        }
        Some((
            usize::try_from(self.row).ok()?,
            usize::try_from(self.col).ok()?,
        ))
    }

    /// Label for display (e.g. "a1").
    ///
    /// Off-board coordinates are clamped onto the board rather than allowed to
    /// run past `h` or `8`: this is a display label, and a label reading `` `9 ``
    /// would be a puzzle rather than a diagnosis.
    fn label(self) -> String {
        let file = file_letter(self.col);
        let rank = self.row.clamp(0, 7).saturating_add(1);
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
        let Some(first) = self.steps.first() else {
            return String::new();
        };
        let mut result = first.from.label();
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

// ── What a click can land on ────────────────────────────────────────

/// Every box the drawing pass records, and so everything a click can reach.
///
/// The old spelling had no such list: a click was resolved by `square_at`,
/// which re-derived the board's position from the same constants the drawing
/// used and undid the rank flip by hand. Two independent copies of one piece of
/// arithmetic is one copy too many -- nothing but care kept the board a player
/// sees in the same place as the board a click resolves against. Now the
/// drawing pass records where it actually put each square and the click is
/// tested against that, so the two cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// A square of the board, by `(row, col)` in board coordinates -- row 0 is
    /// rank 1, which is drawn at the *bottom*.
    Square(i8, i8),
    /// The board as a whole, behind the squares.
    Board,
    /// The title along the top.
    Title,
    /// The side panel's background.
    Panel,
    /// The new-game button in the panel.
    NewGame,
    /// One of the eight rank numbers down the left of the board.
    RankLabel(i8),
    /// One of the eight file letters along the bottom of the board.
    FileLabel(i8),
    /// The piece counts for a side.
    Count(Side),
    /// The line saying how many pieces each side has taken.
    Captures,
    /// The move number.
    MoveNumber,
    /// The move history, heading and rows together.
    History,
    /// The help line at the foot of the panel.
    Help,
    /// The status line along the bottom of the window.
    Status,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, worked out from the window size and nothing else.
///
/// Every field is derived; none is a constant. What this replaced was eleven
/// `const f32`s -- a 64-pixel square, a 40-pixel margin, a panel at
/// `BOARD_OFFSET_X + BOARD_PIXEL_SIZE + 20.0` -- which meant the picture was
/// right at exactly one window size and wrong at every other, and the window
/// was never resized because the program never opened one.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    header: Rect,
    board_area: Rect,
    panel: Rect,
    status: Rect,
    /// The gap left around and inside every band.
    pad: f32,
    /// The title's font size.
    title: f32,
    /// The body font size.
    font: f32,
    /// The font size for labels, history rows and help.
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Everything scales with the *smaller* side, so a wide-and-short window
        // gets small padding rather than padding that eats its whole height.
        let pad = (w.min(h) * 0.02).clamp(2.0, 18.0).min(w.min(h) / 2.0);
        let title = (h * 0.036).clamp(10.0, 30.0);
        let font = (h * 0.024).clamp(8.0, 18.0);
        let small = (h * 0.019).clamp(7.0, font);

        let header_h = h * 0.09;
        let rest = (h - header_h).max(0.0);
        let status_h = rest * 0.09;
        let body_h = (rest - status_h).max(0.0);

        let header = Rect::new(0.0, 0.0, w, header_h);
        let body = Rect::new(0.0, header.bottom(), w, body_h);
        let status = Rect::new(0.0, body.bottom(), w, status_h);

        // The panel takes a share of the width, floored so its text is legible
        // and capped so it never crowds the board off a narrow window.
        let panel_w = (w * 0.28).clamp(120.0, 280.0).min(w / 2.0);
        let board_area = Rect::new(body.x, body.y, (body.w - panel_w).max(0.0), body.h);
        let panel = Rect::new(board_area.right(), body.y, panel_w.min(body.w), body.h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board_area,
            panel,
            status,
            pad,
            title,
            font,
            small,
        }
    }
}

/// The board's squares, fitted to whatever room the layout gave them.
///
/// The board is square whatever shape its area is: the step is the smaller of
/// what the width and the height allow, and the leftovers become margins, so
/// the squares stay square in a tall window and in a wide one alike.
#[derive(Debug, Clone, Copy)]
struct Grid {
    /// Top-left of the board proper -- past the rank gutter, above the file
    /// gutter.
    origin: (f32, f32),
    /// The side of one square.
    step: f32,
    /// The width of the rank gutter and the height of the file gutter.
    label: f32,
}

impl Grid {
    fn fit(area: Rect, label_font: f32) -> Self {
        let side = 8.0;
        let label = label_font * 1.7;
        let step = ((area.w - label).max(0.0) / side)
            .min((area.h - label).max(0.0) / side)
            .max(0.0);
        let board = step * side;
        let left = area.x + (area.w - board - label).max(0.0) / 2.0;
        let top = area.y + (area.h - board - label).max(0.0) / 2.0;
        Self {
            origin: (left + label, top),
            step,
            label,
        }
    }

    /// The box a square occupies on screen.
    ///
    /// `Pos::new(0, 0)` is a1, and draughts -- like chess, and unlike Othello
    /// next door in `apps/reversi` -- puts rank 1 at the *bottom* of the board.
    /// So the screen row is the board row counted from the other end. That flip
    /// is written here and nowhere else; the file this replaced had it in three
    /// places, one of them inverted by hand in the hit test.
    fn square(self, row: i8, col: i8) -> Rect {
        let screen_row = f32::from(7i8.saturating_sub(row));
        Rect::new(
            self.origin.0 + f32::from(col) * self.step,
            self.origin.1 + screen_row * self.step,
            self.step,
            self.step,
        )
    }

    /// The middle of a square, where its piece is drawn.
    fn centre(self, row: i8, col: i8) -> (f32, f32) {
        let r = self.square(row, col);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The eight-by-eight box, gutters excluded.
    fn board_rect(self) -> Rect {
        Rect::new(
            self.origin.0,
            self.origin.1,
            self.step * 8.0,
            self.step * 8.0,
        )
    }
}

/// A rectangle shrunk by `pad` on every side, never past nothing.
fn inset(rect: Rect, pad: f32) -> Rect {
    Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    )
}

/// The smallest box holding both, ignoring an empty one.
///
/// A section's hit box is its heading plus its rows, and a section with no rows
/// is its heading alone.
fn union(a: Rect, b: Rect) -> Rect {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

/// A `u32` as an `f32`, without a lossy cast.
fn f32_from_u32(v: u32) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

/// A `usize` as a `u32`, saturating rather than wrapping.
fn u32_from_usize(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// How many whole rows of some height fit in a span.
///
/// Saturates at zero for a negative or non-finite span, which is what a window
/// too short for its own panel produces.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=64 either side of the cast, so neither can fire"
)]
fn count_from_f32(v: f32) -> usize {
    if v.is_finite() && v > 0.0 {
        v.min(64.0) as usize
    } else {
        0
    }
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
    ///
    /// Built by `set` onto an empty board rather than by subscripting a local
    /// array, so the one bounds check in `Pos::index` covers the opening
    /// position too.
    fn new() -> Self {
        let mut board = Self::empty();
        // Black on rows 5-7 (the top three), Red on rows 0-2 (the bottom
        // three), dark squares only. Asking `is_dark` rather than restating its
        // parity is what keeps the pieces on the squares the board paints dark.
        for (rows, side) in [(5..=7i8, Side::Black), (0..=2i8, Side::Red)] {
            for row in rows {
                for col in 0..8i8 {
                    let pos = Pos::new(row, col);
                    if pos.is_dark() {
                        board.set(pos, Some(Piece::man(side)));
                    }
                }
            }
        }
        board
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
        let (row, col) = pos.index()?;
        self.squares.get(row)?.get(col).copied().flatten()
    }

    /// Set a piece at a position. A position off the board is ignored.
    fn set(&mut self, pos: Pos, piece: Option<Piece>) {
        let Some((row, col)) = pos.index() else {
            return;
        };
        if let Some(slot) = self.squares.get_mut(row).and_then(|r| r.get_mut(col)) {
            *slot = piece;
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
            let to = pos.offset(dr, dc);
            if to.is_valid() && self.get(to).is_none() {
                moves.push(CheckersMove::simple(pos, to));
            }
        }
        moves
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
            let mid = pos.offset(dr, dc);
            let to = pos.offset(dr.saturating_mul(2), dc.saturating_mul(2));
            if to.is_valid() {
                // Cannot jump a piece that was already captured in this chain
                if captured.contains(&mid) {
                    continue;
                }
                if let Some(mid_piece) = self.get(mid)
                    && mid_piece.side != piece.side
                {
                    // Landing square must be empty. It reads as occupied
                    // if it's the origin square (piece logically left),
                    // so treat that as empty.
                    let landing_empty = self.get(to).is_none() || (origin == Some(to));
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

        let next_jumps = self.generate_jumps_for_chain(pos, piece, captured, Some(origin));

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
            let sub = self.build_jump_sequences(jmp.to, &new_chain, &new_captured, is_king, origin);
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
                    let jump_seqs = self.build_jump_sequences(pos, &[], &[], piece.is_king, pos);
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
        self.move_count = self.move_count.saturating_add(1);
        if had_capture {
            self.no_capture_count = 0;
        } else {
            self.no_capture_count = self.no_capture_count.saturating_add(1);
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
                    let base = if piece.is_king { KING_VALUE } else { MAN_VALUE };

                    // Position bonuses
                    let mut bonus = 0i32;

                    // Center control bonus (columns 2-5, rows 2-5)
                    if (2..=5).contains(&col) && (2..=5).contains(&row) {
                        bonus = bonus.saturating_add(CENTER_BONUS);
                    }

                    if !piece.is_king {
                        // Advancement bonus for men, counted from each side own
                        // back rank, so both sides are scored on the same
                        // scale.
                        let advance = i32::from(match piece.side {
                            Side::Red => row,
                            Side::Black => 7i8.saturating_sub(row),
                        });
                        bonus = bonus.saturating_add(advance.saturating_mul(ADVANCE_BONUS));

                        // Back row defense bonus
                        let back_row = match piece.side {
                            Side::Red => 0,
                            Side::Black => 7,
                        };
                        if row == back_row {
                            bonus = bonus.saturating_add(BACK_ROW_BONUS);
                        }
                    }

                    // Saturating throughout: the search adds at most 32 pieces
                    // worth of a few hundred points each, so nothing here can
                    // reach the ends of an i32 -- but an evaluation that
                    // silently wrapped would make the AI prefer the position it
                    // wrapped in, which is the worst possible way to find out.
                    let val = base.saturating_add(bonus);
                    match piece.side {
                        Side::Black => score = score.saturating_add(val),
                        Side::Red => score = score.saturating_sub(val),
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
        GameResult::BlackWins => return (100_000i32.saturating_add(depth), None),
        GameResult::RedWins => return ((-100_000i32).saturating_sub(depth), None),
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
            let (eval, _) = minimax(&new_board, depth.saturating_sub(1), alpha, beta, false);
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
            let (eval, _) = minimax(&new_board, depth.saturating_sub(1), alpha, beta, true);
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
    move_history: Vec<String>,
    last_move_from: Option<Pos>,
    last_move_to: Option<Pos>,
    /// How many pieces Red has taken.
    ///
    /// Named for the side that did the taking. The old pair was named for the
    /// side that was taken *from* -- `red_captured` counted Red's losses -- and
    /// then printed as `Captured: Red {red_captured}`, which any reader takes
    /// to mean the pieces Red has captured. The panel said the opposite of what
    /// it counted, and swapping the names is the fix rather than swapping the
    /// two at the point of printing, because the printing was not the thing
    /// that was wrong.
    red_takes: u32,
    /// How many pieces Black has taken.
    black_takes: u32,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size: (f32, f32),
}

impl CheckersApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            cursor: Pos::new(0, 0), // a1: dark, so playable
            selected: None,
            legal_moves_for_selected: Vec::new(),
            game_result: GameResult::Ongoing,
            move_history: Vec::new(),
            last_move_from: None,
            last_move_to: None,
            red_takes: 0,
            black_takes: 0,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Note the size a frame was drawn at.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// Start a new game.
    ///
    /// Keeps the window size, which is not part of the game. Reversi had the
    /// same shape and the same trap: `*self = Self::new()` would have snapped
    /// the board back to its opening size on every new game.
    fn new_game(&mut self) {
        self.board = Board::new();
        self.cursor = Pos::new(0, 0);
        self.selected = None;
        self.legal_moves_for_selected.clear();
        self.game_result = GameResult::Ongoing;
        self.move_history.clear();
        self.last_move_from = None;
        self.last_move_to = None;
        self.red_takes = 0;
        self.black_takes = 0;
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
                && piece.side == Side::Red
            {
                self.select_piece(pos);
                return;
            }

            // Clicked elsewhere: deselect
            self.selected = None;
            self.legal_moves_for_selected.clear();
        } else {
            // No piece selected: try to select one
            if let Some(piece) = self.board.get(pos)
                && piece.side == Side::Red
            {
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

    /// Execute Red's move, and let Black answer it.
    fn execute_move(&mut self, mv: &MoveSequence) {
        let notation = mv.notation();
        let captured = mv.captured_count();

        self.last_move_from = Some(mv.origin_pos());
        self.last_move_to = Some(mv.to_pos());

        self.board.apply_move_in_place(mv);
        // Red made this move, so the pieces it took are Red's takings. The old
        // spelling credited them to `black_captured` and the panel printed that
        // beside the word "Black".
        self.red_takes = self.red_takes.saturating_add(u32_from_usize(captured));

        self.move_history.push(notation);

        self.selected = None;
        self.legal_moves_for_selected.clear();

        self.game_result = self.board.check_result();
        if self.game_result != GameResult::Ongoing {
            return;
        }

        // What stood here was `status_message = "Black thinking..."`, written
        // one line before `do_ai_move` overwrote it. The search runs inside
        // this same event, so no frame is ever drawn between the two: the
        // message could not be seen, and a notice nobody can see is not a
        // notice. Reversi's pass notice was the same fault; see roadmap.md.
        self.do_ai_move();
    }

    /// Execute Black's reply.
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
            self.black_takes = self.black_takes.saturating_add(u32_from_usize(captured));

            self.move_history.push(notation);
        }
        // Whether or not a move was found, the position decides the result: a
        // side with no move has lost, and `check_result` says so. The old
        // spelling had two arms here that differed only in which of them also
        // set a status string.
        self.game_result = self.board.check_result();
    }

    /// The line along the bottom of the window.
    ///
    /// Derived from the position every frame rather than stored in a field. The
    /// stored `status_message` was written at five sites, one of which
    /// ("Black thinking...") could never be read, and one arm of the function
    /// that set it -- `GameResult::Ongoing` -- was unreachable from all five.
    /// A string that is a function of the state should be a function.
    fn status(&self) -> String {
        match self.game_result {
            GameResult::RedWins => "Red wins!".to_string(),
            GameResult::BlackWins => "Black wins!".to_string(),
            GameResult::Draw => "Draw!".to_string(),
            GameResult::Ongoing => match self.selected {
                Some(pos) => format!("{} selected -- click a marked square", pos.label()),
                None => format!("{} to move", self.board.side_to_move.name()),
            },
        }
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }

        match event.key {
            Key::N if event.modifiers.ctrl => self.new_game(),
            Key::Left => self.cursor.col = self.cursor.col.saturating_sub(1).max(0),
            Key::Right => self.cursor.col = self.cursor.col.saturating_add(1).min(7),
            Key::Up => self.cursor.row = self.cursor.row.saturating_add(1).min(7),
            Key::Down => self.cursor.row = self.cursor.row.saturating_sub(1).max(0),
            Key::Enter | Key::Space => self.click_square(self.cursor),
            Key::Escape => {
                self.selected = None;
                self.legal_moves_for_selected.clear();
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Resolve a click against the boxes the drawing pass recorded.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(x, y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Square(row, col) => {
                // One action rather than two: the cursor goes to the square and
                // the square is acted on, exactly as arrows-then-Enter would
                // have done.
                self.cursor = Pos::new(row, col);
                self.click_square(self.cursor);
                EventResult::Consumed
            }
            Target::NewGame => {
                self.new_game();
                EventResult::Consumed
            }
            // Every other recorded box is answered and does nothing. A click on
            // the panel must not fall through to the window, which would treat
            // it as a click on nothing at all.
            _ => EventResult::Consumed,
        }
    }

    // ── Drawing ─────────────────────────────────────────────────────

    /// One frame at the given size: what to draw, and what a click there hits.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(l.window.w, l.window.h);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.window.w,
            height: l.window.h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        // A window too small for its contents crops them rather than painting
        // over its neighbours. The old background was
        // `PANEL_X + 250.0` by `BOARD_OFFSET_Y + BOARD_PIXEL_SIZE + 80.0` --
        // the size the constants happened to add up to, painted whatever the
        // window's actual size was.
        f.clip(l.window);
        self.draw_header(&l, &mut f);
        self.draw_board(&l, &mut f);
        self.draw_panel(&l, &mut f);
        self.draw_status(&l, &mut f);
        f.unclip();
        f
    }

    /// The title along the top, and the piece counts beside it.
    fn draw_header(&self, l: &Layout, f: &mut Frame<Target>) {
        let band = inset(l.header, l.pad);
        let title = label_in(
            f,
            band,
            "Checkers",
            Ink::new(l.title, FontWeightHint::Bold, LAVENDER),
        );
        f.hit(Target::Title, title);

        // The chips follow the title's *measured* width rather than a
        // hand-counted offset from it, so they cannot slide under the title at
        // a font size nobody tried.
        let gap = l.pad * 1.5;
        let mut x = title.right() + gap;
        for side in [Side::Red, Side::Black] {
            let ink = Ink::new(
                l.font,
                FontWeightHint::Bold,
                match side {
                    Side::Red => RED_PIECE,
                    Side::Black => SUBTEXT0,
                },
            );
            let kings = self.board.count_kings(side);
            let text = if kings == 0 {
                format!("{}: {}", side.name(), self.board.count_pieces(side))
            } else {
                format!(
                    "{}: {} ({}K)",
                    side.name(),
                    self.board.count_pieces(side),
                    kings
                )
            };
            let area = Rect::new(x, band.y, ink.width(&text), band.h);
            let drawn = label_in(f, area, &text, ink);
            f.hit(Target::Count(side), drawn);
            x = area.right() + gap;
        }
    }

    /// The board: its border, its squares, the pieces on them, and its a-h and
    /// 1-8.
    fn draw_board(&self, l: &Layout, f: &mut Frame<Target>) {
        let g = Grid::fit(inset(l.board_area, l.pad), l.small);
        let board = g.board_rect();

        let edge = (g.step * 0.06).max(1.0);
        f.push(RenderCommand::StrokeRect {
            x: board.x - edge,
            y: board.y - edge,
            width: board.w + edge * 2.0,
            height: board.h + edge * 2.0,
            color: SURFACE1,
            line_width: edge,
            corner_radii: CornerRadii::ZERO,
        });
        // Recorded before the squares so a click that lands between them --
        // there is nothing between them, but the frame answers with whatever
        // was recorded last -- reaches the board rather than the window.
        f.hit(Target::Board, board);

        let legal_dests: Vec<Pos> = self
            .legal_moves_for_selected
            .iter()
            .map(MoveSequence::to_pos)
            .collect();

        for row in 0..8i8 {
            for col in 0..8i8 {
                let pos = Pos::new(row, col);
                let square = g.square(row, col);

                let mut shade = if pos.is_dark() {
                    DARK_SQUARE
                } else {
                    LIGHT_SQUARE
                };
                if self.last_move_from == Some(pos) || self.last_move_to == Some(pos) {
                    shade = LAST_MOVE_HIGHLIGHT;
                }
                f.push(RenderCommand::FillRect {
                    x: square.x,
                    y: square.y,
                    width: square.w,
                    height: square.h,
                    color: shade,
                    corner_radii: CornerRadii::ZERO,
                });

                if self.selected == Some(pos) {
                    let ring = (g.step * 0.05).max(1.0);
                    f.push(RenderCommand::StrokeRect {
                        x: square.x + ring,
                        y: square.y + ring,
                        width: (square.w - ring * 2.0).max(0.0),
                        height: (square.h - ring * 2.0).max(0.0),
                        color: SELECTED_SQUARE,
                        line_width: ring,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if self.cursor == pos {
                    let ring = (g.step * 0.032).max(1.0);
                    f.push(RenderCommand::StrokeRect {
                        x: square.x + ring * 2.0,
                        y: square.y + ring * 2.0,
                        width: (square.w - ring * 4.0).max(0.0),
                        height: (square.h - ring * 4.0).max(0.0),
                        color: YELLOW,
                        line_width: ring,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                if let Some(piece) = self.board.get(pos) {
                    let (cx, cy) = g.centre(row, col);
                    draw_piece(f, cx, cy, g.step * 0.37, piece);
                }

                if legal_dests.contains(&pos) {
                    let (cx, cy) = g.centre(row, col);
                    let r = g.step * 0.11;
                    f.push(RenderCommand::FillRect {
                        x: cx - r,
                        y: cy - r,
                        width: r * 2.0,
                        height: r * 2.0,
                        color: LEGAL_MOVE_DOT,
                        corner_radii: CornerRadii::all(r),
                    });
                }

                f.hit(Target::Square(row, col), square);
            }
        }

        // The rank numbers run 1..8 *upwards*, which is why they are drawn
        // against `g.square(i, ...)` rather than counted down the screen: the
        // grid owns the flip, and a label placed by its own arithmetic is a
        // second copy of it waiting to disagree.
        let ink = Ink::new(l.small, FontWeightHint::Regular, SUBTEXT0);
        for i in 0..8i8 {
            let square = g.square(i, 0);
            let number = format!("{}", i.saturating_add(1));
            let drawn = label(
                f,
                board.x - g.label + (g.label - ink.width(&number)) / 2.0,
                square.y + (g.step - ink.height()) / 2.0,
                &number,
                ink,
            );
            f.hit(Target::RankLabel(i), drawn);

            let square = g.square(0, i);
            let letter = file_letter(i).to_string();
            let drawn = label(
                f,
                square.x + (g.step - ink.width(&letter)) / 2.0,
                board.bottom() + (g.label - ink.height()) / 2.0,
                &letter,
                ink,
            );
            f.hit(Target::FileLabel(i), drawn);
        }
    }

    /// The side panel: the new-game button, the captures, the move number, the
    /// history and the help.
    ///
    /// Every line is placed by walking a cursor down the panel, so the panel
    /// says as much as it has room for and no more. The old spelling put each
    /// line at a hand-counted offset from the panel's top -- `+ 15.0`,
    /// `+ 50.0`, `+ 85.0`, `+ 110.0`, `+ 145.0`, `+ 170.0`, `+ 195.0`,
    /// `+ 210.0`, `+ 235.0` -- and drew a fixed eighteen history rows at a
    /// fixed eighteen pixels apart, which in a short window ran straight
    /// through its own help text.
    fn draw_panel(&self, l: &Layout, f: &mut Frame<Target>) {
        let band = inset(l.panel, l.pad);
        f.push(RenderCommand::FillRect {
            x: band.x,
            y: band.y,
            width: band.w,
            height: band.h,
            color: MANTLE,
            corner_radii: CornerRadii::all(l.pad),
        });
        f.hit(Target::Panel, band);

        let inner = inset(band, l.pad);
        let label_ink = Ink::new(l.small, FontWeightHint::Bold, SUBTEXT0);
        let body_ink = Ink::new(l.font, FontWeightHint::Regular, TEXT_COLOR);
        let mut y = inner.y;

        // The new-game button. `Ctrl+N` still works, but a control that exists
        // only as a line of help text is a control half the players never find.
        let button_ink = Ink::new(l.font, FontWeightHint::Bold, CRUST);
        let button = Rect::new(inner.x, y, inner.w, button_ink.height() + l.pad);
        // Same rule as `panel_row`, which the button cannot use because it is a
        // filled control rather than a line of text: a button that hangs off
        // the panel is a button drawn on the board.
        if button.bottom() <= inner.bottom() + 0.01 {
            f.push(RenderCommand::FillRect {
                x: button.x,
                y: button.y,
                width: button.w,
                height: button.h,
                color: if self.game_result == GameResult::Ongoing {
                    SURFACE2
                } else {
                    GREEN
                },
                corner_radii: CornerRadii::all(l.pad / 2.0),
            });
            let text = "New Game";
            label(
                f,
                button.x + (button.w - button_ink.width(text)).max(0.0) / 2.0,
                button.y + (button.h - button_ink.height()).max(0.0) / 2.0,
                text,
                button_ink,
            );
            f.hit(Target::NewGame, button);
        }
        y = button.bottom() + l.small * 0.8;

        panel_row(f, inner, &mut y, "Pieces Taken", label_ink);
        let drawn = panel_row(
            f,
            inner,
            &mut y,
            &format!("Red {}   Black {}", self.red_takes, self.black_takes),
            Ink::new(l.font, FontWeightHint::Regular, PEACH),
        );
        f.hit(Target::Captures, drawn);
        y += l.small * 0.6;

        let drawn = panel_row(
            f,
            inner,
            &mut y,
            &format!("Move: {}", self.board.move_count),
            body_ink,
        );
        f.hit(Target::MoveNumber, drawn);

        let rule_y = y + l.small * 0.4;
        if rule_y >= inner.y && rule_y <= inner.bottom() {
            f.push(RenderCommand::Line {
                x1: inner.x,
                y1: rule_y,
                x2: inner.right(),
                y2: rule_y,
                color: SURFACE0,
                width: 1.0,
            });
        }
        y += l.small;

        // The help sits on the floor of the panel and the history fills
        // whatever is between the cursor and it, so the two cannot collide
        // however long the game runs or however short the window is.
        let help_ink = Ink::new(l.small, FontWeightHint::Regular, OVERLAY0);
        let help_h = help_ink.height() * 2.0;
        let help_top = (inner.bottom() - help_h).max(y);

        let heading = panel_row(f, inner, &mut y, "Move History", label_ink);
        let row_h = Ink::new(l.small, FontWeightHint::Regular, TEXT_COLOR).height();
        let rows = count_from_f32((help_top - y) / row_h);
        let start = self.move_history.len().saturating_sub(rows);
        let mut history_box = heading;
        for (idx, notation) in self.move_history.iter().enumerate().skip(start) {
            // Red plays every even-numbered ply, so the ply number decides both
            // the colour and whether the line carries a move number.
            let red = idx % 2 == 0;
            let text = if red {
                format!("{}. {notation}", (idx / 2).saturating_add(1))
            } else {
                format!("   {notation}")
            };
            let ink = Ink::new(
                l.small,
                FontWeightHint::Regular,
                if red { RED_PIECE } else { SUBTEXT0 },
            );
            let drawn = panel_row(f, inner, &mut y, &text, ink);
            history_box = union(history_box, drawn);
        }
        f.hit(Target::History, history_box);

        let mut help_y = help_top;
        let first = panel_row(f, inner, &mut help_y, "Arrows: move   Enter: act", help_ink);
        let second = panel_row(
            f,
            inner,
            &mut help_y,
            "Esc: deselect   Ctrl+N: new",
            help_ink,
        );
        f.hit(Target::Help, union(first, second));
    }

    /// The line along the bottom of the window.
    fn draw_status(&self, l: &Layout, f: &mut Frame<Target>) {
        let band = inset(l.status, l.pad);
        let ink = Ink::new(
            l.font,
            FontWeightHint::Regular,
            match self.game_result {
                GameResult::RedWins => GREEN,
                GameResult::BlackWins => RED,
                GameResult::Draw => YELLOW,
                GameResult::Ongoing => TEXT_COLOR,
            },
        );
        let drawn = label_in(f, band, &self.status(), ink);
        f.hit(Target::Status, drawn);
    }
}

/// The file letter for a column, `a` through `h`.
fn file_letter(col: i8) -> char {
    // Saturating rather than wrapping, and clamped to the board, so a column
    // outside 0..8 yields a letter on the board rather than an arbitrary byte.
    let col = col.clamp(0, 7);
    #[expect(
        clippy::cast_sign_loss,
        reason = "clamped to 0..=7 on the line above, so it cannot be negative"
    )]
    let offset = col as u8;
    char::from(b'a'.saturating_add(offset))
}

/// A checker, drawn as two concentric discs and, for a king, a crown.
///
/// A free function rather than a method: it read `&self` and used nothing from
/// it, which is a method only in spelling.
fn draw_piece(f: &mut Frame<Target>, cx: f32, cy: f32, radius: f32, piece: Piece) {
    let (outer, inner) = match piece.side {
        Side::Red => (RED_PIECE, RED_PIECE_DARK),
        Side::Black => (BLACK_PIECE, BLACK_PIECE_DARK),
    };
    f.push(RenderCommand::FillRect {
        x: cx - radius,
        y: cy - radius,
        width: radius * 2.0,
        height: radius * 2.0,
        color: outer,
        corner_radii: CornerRadii::all(radius),
    });
    let core = radius * 0.62;
    f.push(RenderCommand::FillRect {
        x: cx - core,
        y: cy - core,
        width: core * 2.0,
        height: core * 2.0,
        color: inner,
        corner_radii: CornerRadii::all(core),
    });

    if piece.is_king {
        // Centred by measurement. The old spelling drew the crown at
        // `cx - 8.0, cy - 10.0`: two nudges chosen for one font size, which put
        // the crown off its own piece at any other.
        let ink = Ink::new(radius * 1.1, FontWeightHint::Bold, KING_CROWN);
        let crown = "\u{265A}";
        label(
            f,
            cx - ink.width(crown) / 2.0,
            cy - ink.height() / 2.0,
            crown,
            ink,
        );
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A font size, weight and colour, together, because they always travel
/// together.
#[derive(Debug, Clone, Copy)]
struct Ink {
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

impl Ink {
    const fn new(size: f32, weight: FontWeightHint, color: Color) -> Self {
        Self {
            size,
            weight,
            color,
        }
    }

    /// How wide `s` is when drawn in this ink.
    fn width(self, s: &str) -> f32 {
        text::measure(s, self.size, self.weight)
    }

    /// How tall one line of this ink is.
    fn height(self) -> f32 {
        text::line_height(self.size, self.weight)
    }
}

/// Draw `s` at `(x, y)` and hand back the box its glyphs occupy.
fn label(f: &mut Frame<Target>, x: f32, y: f32, s: &str, ink: Ink) -> Rect {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    Rect::new(x, y, ink.width(s), ink.height())
}

/// Draw `s` at the left of `area`, vertically centred, elided to fit.
fn label_in(f: &mut Frame<Target>, area: Rect, s: &str, ink: Ink) -> Rect {
    let y = area.y + (area.h - ink.height()).max(0.0) / 2.0;
    f.push(RenderCommand::Text {
        x: area.x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: Some(area.w.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    Rect::new(area.x, y, ink.width(s).min(area.w.max(0.0)), ink.height())
}

/// Draw one line at `*y` down the panel, and move the cursor past it.
///
/// A row that will not fit inside `band` is not drawn, and comes back empty.
/// The panel stacks downwards from a fixed top, so at a small enough window
/// every row after the first hangs off the bottom of the panel and paints over
/// the status line; half a line of text on the wrong background is worse than
/// no line. An empty rect is the right answer rather than a special case,
/// because both [`union`] and [`Frame::hit`] already ignore one.
fn panel_row(f: &mut Frame<Target>, band: Rect, y: &mut f32, s: &str, ink: Ink) -> Rect {
    let h = ink.height();
    let row = Rect::new(band.x, *y, band.w, h);
    *y += h;
    if row.y < band.y - 0.01 || row.bottom() > band.bottom() + 0.01 {
        return Rect::new(row.x, row.y, 0.0, 0.0);
    }
    label_in(f, row, s, ink)
}

// ── The window ──────────────────────────────────────────────────────

/// The one body every event goes through, whichever side it arrives from.
///
/// The window calls it and the tests call it, so a key the tests prove works is
/// the same key the window delivers.
fn handle_event(app: &mut CheckersApp, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }) => app.click(*x, *y, *button),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for CheckersApp {
    fn title(&self) -> String {
        "Checkers".to_string()
    }

    fn app_id(&self) -> String {
        "checkers".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the natural size is two small positive whole numbers"
    )]
    fn initial_size(&self) -> (u32, u32) {
        // Converted from the float pair rather than written out again: two
        // spellings of one size are two things that can drift apart.
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
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for CheckersApp {
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
    let mut game = CheckersApp::new();
    app::launch("checkers", &mut game)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes past the end, or unwraps a `None`, is a test that
    // has already failed; panicking is the reporting mechanism, not a fault.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::too_many_lines,
        clippy::unwrap_used
    )]

    use super::*;
    use guitk::probe;

    // ── The window ──────────────────────────────────────────────────
    //
    // Nothing below asks the production code where it *would* have drawn a
    // square. `square_origin` and `square_at` used to be each other's inverse,
    // so a test that clicked where the code said it drew something agreed with
    // any mapping, right or wrong. These read the boxes the drawing pass
    // actually recorded, and compare them against the window, against each
    // other, and against the way a draughts board is numbered.

    /// The sizes every geometry test is run at.
    ///
    /// The lopsided ones are the point: a board fitted to the width alone
    /// passes at 900x640 and runs off the bottom at 400x900, which is exactly
    /// the fault the old fixed layout had.
    const SIZES: [(f32, f32); 8] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (640.0, 480.0),
        (1600.0, 1000.0),
        (400.0, 900.0),
        (1200.0, 400.0),
        (320.0, 240.0),
        (200.0, 200.0),
        (60.0, 60.0),
    ];

    /// A fresh game.
    fn app() -> CheckersApp {
        CheckersApp::new()
    }

    /// The box the drawing pass recorded for `target` at `size`, or a panic
    /// naming it.
    fn box_at(app: &CheckersApp, target: Target, size: (f32, f32)) -> Rect {
        probe::rect_of_sized(app, target, size)
            .unwrap_or_else(|| panic!("{target:?} was not drawn at {size:?}"))
    }

    /// The box recorded for `target` at the natural size.
    fn box_of(app: &CheckersApp, target: Target) -> Rect {
        box_at(app, target, CheckersApp::SIZE)
    }

    /// Whether two rectangles agree to within a rounding error.
    fn same(a: Rect, b: Rect) -> bool {
        (a.x - b.x).abs() < 0.01
            && (a.y - b.y).abs() < 0.01
            && (a.w - b.w).abs() < 0.01
            && (a.h - b.h).abs() < 0.01
    }

    /// A left click at a window coordinate.
    ///
    /// `probe::click_sized` clicks a *target* — it asks the hit map where the
    /// control is and clicks there. That is the wrong instrument for these
    /// tests, which exist to check the mapping from a raw point back to a
    /// control, so they go in at the point.
    fn tap(app: &mut CheckersApp, x: f32, y: f32, size: (f32, f32)) -> EventResult {
        app.click_at(x, y, MouseButton::Left, size)
    }

    /// A click at a window coordinate with a nominated button.
    fn tap_with(
        app: &mut CheckersApp,
        x: f32,
        y: f32,
        button: MouseButton,
        size: (f32, f32),
    ) -> EventResult {
        app.click_at(x, y, button, size)
    }

    /// Whether `inner` lies within `outer`, allowing a rounding error.
    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }

    #[test]
    fn every_square_is_drawn_at_every_window_size() {
        // The first draft asked only whether each square had a recorded hit
        // box, which is known-issues.md lesson 81: the drawing pass fills the
        // rectangle *and* hands it to `f.hit`, so deleting the fill leaves
        // every box in place and every `is_some()` still answering yes. It
        // cannot be repaired by asking whether some fill covers the square's
        // centre either -- lesson 83 -- because the window's own background
        // fill covers every point in the app. The claim has to run the other
        // way: a fill in one of the board's own shades, at the square.
        for size in SIZES {
            let app = app();
            let f = app.draw(size);
            let shades: Vec<(f32, f32, f32, f32)> = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                        ..
                    } if *color == LIGHT_SQUARE
                        || *color == DARK_SQUARE
                        || *color == LAST_MOVE_HIGHLIGHT =>
                    {
                        Some((*x, *y, *width, *height))
                    }
                    _ => None,
                })
                .collect();
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let square = box_at(&app, Target::Square(row, col), size);
                    // Exactly one, not at least one. A count over the whole
                    // frame cannot say this: `SURFACE2` and `DARK_SQUARE` are
                    // both 0x585B70, so every panel drawn in the one answers
                    // to a test looking for the other, and "64 shaded fills"
                    // came back 65. Matching on the square's own box is the
                    // claim that does not care what else the window paints in
                    // the same colour.
                    let painted = shades
                        .iter()
                        .filter(|&&(x, y, w, h)| {
                            (x - square.x).abs() < 0.01
                                && (y - square.y).abs() < 0.01
                                && (w - square.w).abs() < 0.01
                                && (h - square.h).abs() < 0.01
                        })
                        .count();
                    assert_eq!(
                        painted, 1,
                        "square ({row}, {col}) at {square:?} was painted {painted} \
                         time(s) at {size:?}, want exactly one"
                    );
                }
            }
        }
    }

    #[test]
    fn the_squares_are_square_and_all_the_same_size() {
        for size in SIZES {
            let app = app();
            let first = box_at(&app, Target::Square(0, 0), size);
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let r = box_at(&app, Target::Square(row, col), size);
                    assert!(
                        (r.w - r.h).abs() < 0.01,
                        "square ({row}, {col}) is {}x{} at {size:?} -- not square",
                        r.w,
                        r.h
                    );
                    assert!(
                        (r.w - first.w).abs() < 0.01,
                        "square ({row}, {col}) is {} across at {size:?}, a1 is {}",
                        r.w,
                        first.w
                    );
                }
            }
        }
    }

    #[test]
    fn the_squares_tile_the_board_edge_to_edge() {
        for size in SIZES {
            let app = app();
            let step = box_at(&app, Target::Square(0, 0), size).w;
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let here = box_at(&app, Target::Square(row, col), size);
                    if col < 7 {
                        let right = box_at(&app, Target::Square(row, col + 1), size);
                        assert!(
                            (right.x - here.right()).abs() < 0.01,
                            "a gap of {} between ({row}, {col}) and its neighbour at {size:?}",
                            right.x - here.right()
                        );
                    }
                    if row < 7 {
                        // Rank `row + 1` is drawn one step *above* rank `row`:
                        // draughts puts rank 1 at the bottom.
                        let above = box_at(&app, Target::Square(row + 1, col), size);
                        assert!(
                            (here.y - above.bottom()).abs() < 0.01,
                            "rank {} is not directly above rank {row} at {size:?}",
                            row + 1
                        );
                    }
                    assert!(
                        (here.w - step).abs() < 0.01,
                        "({row}, {col}) is not one step across at {size:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_board_box_is_exactly_the_squares_it_holds() {
        // `Target::Board` is recorded from `Grid::board_rect`, which is a
        // second expression of the same geometry `Grid::square` lays out.
        // Two expressions of one thing drift; this is what notices.
        for size in SIZES {
            let app = app();
            let mut squares: Option<Rect> = None;
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let square = box_at(&app, Target::Square(row, col), size);
                    squares = Some(match squares {
                        Some(so_far) => union(so_far, square),
                        None => square,
                    });
                }
            }
            let squares = squares.expect("the board draws sixty-four squares");
            let board = box_at(&app, Target::Board, size);
            assert!(
                same(board, squares),
                "at {size:?} the board box is {board:?} but its squares fill {squares:?}"
            );
        }
    }

    #[test]
    fn rank_one_is_at_the_bottom_and_file_a_at_the_left() {
        // The orientation rule, checked against the published one rather than
        // against the code that implements it. Getting it wrong turns the board
        // upside down, and every piece of arithmetic in the file stays
        // self-consistent while it does.
        for size in SIZES {
            let app = app();
            let a1 = box_at(&app, Target::Square(0, 0), size);
            let a8 = box_at(&app, Target::Square(7, 0), size);
            let h1 = box_at(&app, Target::Square(0, 7), size);
            assert!(
                a1.y > a8.y,
                "rank 1 is drawn above rank 8 at {size:?} -- the board is upside down"
            );
            assert!(
                h1.x > a1.x,
                "file h is drawn left of file a at {size:?} -- the board is mirrored"
            );
        }
    }

    #[test]
    fn the_board_stays_inside_the_window() {
        for (w, h) in SIZES {
            let app = app();
            let window = Rect::new(0.0, 0.0, w, h);
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let r = box_at(&app, Target::Square(row, col), (w, h));
                    assert!(
                        inside(r, window),
                        "square ({row}, {col}) at {r:?} escapes a {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn the_board_and_the_panel_do_not_overlap() {
        for size in SIZES {
            let app = app();
            let panel = box_at(&app, Target::Panel, size);
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let r = box_at(&app, Target::Square(row, col), size);
                    assert!(
                        panel.intersect(r).is_none(),
                        "square ({row}, {col}) at {r:?} runs under the panel at {panel:?}, size {size:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn widening_the_window_moves_the_board_rather_than_the_gap_beside_it() {
        // The board is square and the room it is given usually is not, so one
        // axis has slack, and the board is centred in it. Pinning the board to
        // the corner of that room instead passes every other geometry test here
        // -- the squares still tile, still stay in the window, still miss the
        // panel -- and looks wrong on screen: the board shoved against the left
        // edge with all the empty space in one stripe down the right.
        //
        // Stated as an absolute this is awkward, because neither edge of the
        // room is recorded in the frame: the left edge is the window's less the
        // padding, and the right edge is the panel band's plus it. Stated as a
        // difference the padding cancels. Both sizes below are 400 tall, so the
        // padding, the fonts and the height-limited square size are identical
        // at each, and the only thing that changes is how much room there is
        // across. Half of that extra room should end up on the board's left.
        let app = app();
        let narrow = (1200.0, 400.0);
        let wide = (1600.0, 400.0);

        let a = box_at(&app, Target::Board, narrow);
        let b = box_at(&app, Target::Board, wide);
        assert!(
            (a.w - b.w).abs() < 0.01,
            "the premise is wrong: the board is {} across at {narrow:?} and {} at {wide:?}",
            a.w,
            b.w
        );

        let extra = box_at(&app, Target::Panel, wide).x - box_at(&app, Target::Panel, narrow).x;
        assert!(extra > 1.0, "the wider window gave the board no extra room");
        assert!(
            ((b.x - a.x) - extra / 2.0).abs() < 0.01,
            "the window grew by {extra} across and the board moved {} right; \
             centred, it should have moved {}",
            b.x - a.x,
            extra / 2.0
        );
    }

    #[test]
    fn a_click_in_a_square_reaches_that_square() {
        // The whole point of the rewrite: the click is resolved against the box
        // the drawing pass recorded, so this cannot pass by agreeing with a
        // wrong formula that both sides share.
        for size in SIZES {
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let mut app = app();
                    let r = box_at(&app, Target::Square(row, col), size);
                    if r.w < 2.0 {
                        // At 60x60 the squares are sub-pixel; the centre of one
                        // is not distinguishable from its neighbour's, and a
                        // test that insisted otherwise would be testing float
                        // rounding.
                        continue;
                    }
                    let (cx, cy) = r.centre();
                    tap(&mut app, cx, cy, size);
                    assert_eq!(
                        app.cursor,
                        Pos::new(row, col),
                        "a click in the middle of ({row}, {col}) at {size:?} moved the cursor to {:?}",
                        app.cursor
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_in_each_corner_of_a_square_reaches_that_square() {
        // A click a hair inside each corner, which is where an off-by-one in
        // the hit box shows up first.
        let size = CheckersApp::SIZE;
        for row in 0..8i8 {
            for col in 0..8i8 {
                let r = box_at(&app(), Target::Square(row, col), size);
                for (x, y) in [
                    (r.x + 0.5, r.y + 0.5),
                    (r.right() - 0.5, r.y + 0.5),
                    (r.x + 0.5, r.bottom() - 0.5),
                    (r.right() - 0.5, r.bottom() - 0.5),
                ] {
                    let mut app = app();
                    tap(&mut app, x, y, size);
                    assert_eq!(
                        app.cursor,
                        Pos::new(row, col),
                        "a click at ({x}, {y}), inside ({row}, {col}), moved the cursor to {:?}",
                        app.cursor
                    );
                }
            }
        }
    }

    #[test]
    fn a_click_off_the_board_moves_no_cursor() {
        // The old `square_at` checked its bounds before its cast, which is what
        // kept a click to the left of the board from resolving as column 0.
        // The frame's hit map has no cast to get wrong -- but a box recorded
        // wider than the square it stands for would have the same symptom, so
        // the check is still worth making.
        // The cursor is walked off a1 before each click. It starts on a1, and
        // a1 is also the first square any "resolve a miss to *something*"
        // mistake lands on -- `hit_test(..).unwrap_or(Target::Square(0, 0))` --
        // so a version of this test that clicked from the opening cursor could
        // not tell a click that was ignored from a click that was misread.
        let size = CheckersApp::SIZE;
        let board = box_of(&app(), Target::Board);
        let elsewhere = Pos::new(3, 3);
        let perch = box_of(&app(), Target::Square(elsewhere.row, elsewhere.col)).centre();
        for (x, y) in [
            (board.x - 4.0, board.centre().1),
            (board.right() + 4.0, board.centre().1),
            (board.centre().0, board.y - 4.0),
            (board.centre().0, board.bottom() + 4.0),
        ] {
            let mut app = app();
            tap(&mut app, perch.0, perch.1, size);
            assert_eq!(
                app.cursor, elsewhere,
                "the cursor would not move to d4 to begin with"
            );

            tap(&mut app, x, y, size);
            assert_eq!(
                app.cursor, elsewhere,
                "a click at ({x}, {y}), outside the board at {board:?}, moved the cursor"
            );
        }
    }

    #[test]
    fn the_rank_numbers_line_up_with_the_ranks_they_name() {
        for size in SIZES {
            let app = app();
            for row in 0..8i8 {
                let square = box_at(&app, Target::Square(row, 0), size);
                let label = box_at(&app, Target::RankLabel(row), size);
                assert!(
                    label.right() <= square.x + 0.01,
                    "rank {} sits on the board rather than beside it at {size:?}",
                    row + 1
                );
                let (_, ly) = label.centre();
                let (_, sy) = square.centre();
                assert!(
                    (ly - sy).abs() <= square.h / 2.0 + 0.01,
                    "rank {} is centred at {ly}, but its rank is centred at {sy}, at {size:?}",
                    row + 1
                );
            }
        }
    }

    #[test]
    fn the_file_letters_line_up_with_the_files_they_name() {
        for size in SIZES {
            let app = app();
            for col in 0..8i8 {
                let square = box_at(&app, Target::Square(0, col), size);
                let label = box_at(&app, Target::FileLabel(col), size);
                assert!(
                    label.y >= square.bottom() - 0.01,
                    "file {} sits on the board rather than below it at {size:?}",
                    file_letter(col)
                );
                let (lx, _) = label.centre();
                let (sx, _) = square.centre();
                assert!(
                    (lx - sx).abs() <= square.w / 2.0 + 0.01,
                    "file {} is centred at {lx}, but its file is centred at {sx}, at {size:?}",
                    file_letter(col)
                );
            }
        }
    }

    #[test]
    fn the_ranks_read_one_to_eight_upwards_and_the_files_a_to_h_rightwards() {
        // The numbering itself, read out of the drawn text rather than assumed
        // from the target names.
        let f = app().draw(CheckersApp::SIZE);
        let text_at = |want: &str| -> Option<(f32, f32)> {
            f.commands().iter().find_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if text == want => Some((*x, *y)),
                _ => None,
            })
        };
        let mut last_y = f32::INFINITY;
        for rank in 1..=8 {
            let (_, y) =
                text_at(&rank.to_string()).unwrap_or_else(|| panic!("rank {rank} was never drawn"));
            assert!(y < last_y, "rank {rank} is drawn below rank {}", rank - 1);
            last_y = y;
        }
        let mut last_x = f32::NEG_INFINITY;
        for col in 0..8i8 {
            let letter = file_letter(col).to_string();
            let (x, _) =
                text_at(&letter).unwrap_or_else(|| panic!("file {letter} was never drawn"));
            assert!(
                x > last_x,
                "file {letter} is drawn left of the one before it"
            );
            last_x = x;
        }
    }

    #[test]
    fn a_piece_is_drawn_in_the_middle_of_the_square_it_stands_on() {
        for size in SIZES {
            let app = app();
            let f = app.draw(size);
            // Every disc the renderer emitted, as a centre point.
            let discs: Vec<(f32, f32)> = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                        corner_radii,
                    } if corner_radii.top_left > 0.0
                        && (*color == RED_PIECE || *color == BLACK_PIECE) =>
                    {
                        Some((x + width / 2.0, y + height / 2.0))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                discs.len(),
                24,
                "the opening position has 24 pieces, {} were drawn at {size:?}",
                discs.len()
            );
            for row in 0..8i8 {
                for col in 0..8i8 {
                    let pos = Pos::new(row, col);
                    if app.board.get(pos).is_none() {
                        continue;
                    }
                    let square = box_at(&app, Target::Square(row, col), size);
                    let (cx, cy) = square.centre();
                    assert!(
                        discs
                            .iter()
                            .any(|&(dx, dy)| (dx - cx).abs() < 0.01 && (dy - cy).abs() < 0.01),
                        "no piece is centred on ({row}, {col}) at {size:?}, whose middle is ({cx}, {cy})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_legal_move_dot_marks_a_square_the_selected_piece_can_reach() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        // c3 (row 2, col 2) is a Red man with two moves in the opening
        // position.
        app.select_piece(Pos::new(2, 2));
        assert!(
            !app.legal_moves_for_selected.is_empty(),
            "the fixture selected a piece with no moves"
        );
        let f = app.draw(size);
        let dots: Vec<(f32, f32)> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == LEGAL_MOVE_DOT => Some((x + width / 2.0, y + height / 2.0)),
                _ => None,
            })
            .collect();
        assert_eq!(
            dots.len(),
            app.legal_moves_for_selected.len(),
            "one dot per legal move"
        );
        for (dx, dy) in dots {
            let hit = f
                .hit_test(dx, dy)
                .unwrap_or_else(|| panic!("the dot at ({dx}, {dy}) is on no recorded box"));
            let Target::Square(row, col) = hit else {
                panic!("the dot at ({dx}, {dy}) is on {hit:?}, not a square");
            };
            assert!(
                app.legal_moves_for_selected
                    .iter()
                    .any(|seq| seq.to_pos() == Pos::new(row, col)),
                "a dot marks ({row}, {col}), which the selected piece cannot reach"
            );
        }
    }

    #[test]
    fn the_whole_frame_is_clipped_to_the_window() {
        // Only the clip is asserted, not that every recorded box lies inside
        // the window: `Frame::hit` trims a box to the clip in force and drops
        // it if nothing survives, so a box half a window away would come back
        // cropped and wave such an assertion through. See known-issues Lesson
        // 80. That the boxes are where the layout put them is covered by
        // `the_board_stays_inside_the_window` and its neighbours, which compare
        // against the layout rather than against the clip.
        for (w, h) in SIZES {
            let f = app().draw((w, h));
            let outer = f.commands().iter().find_map(|c| match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some((*x, *y, *width, *height)),
                _ => None,
            });
            assert_eq!(
                outer,
                Some((0.0, 0.0, w, h)),
                "a {w}x{h} window was not clipped to itself"
            );
            assert!(f.is_balanced(), "a clip was pushed and never popped");
        }
    }

    #[test]
    fn the_panel_keeps_its_history_clear_of_its_help() {
        // The old panel drew eighteen history rows at eighteen pixels apart
        // whatever the window's height, so a long game ran its history straight
        // through the help line at the bottom.
        for size in SIZES {
            let mut app = app();
            for i in 0..60 {
                app.move_history.push(format!("m{i}"));
            }
            // Below a certain window there is no room for either, and a panel
            // that drew them anyway would be drawing them on the board. Where
            // both appear they must not touch; where one is missing there is
            // nothing to collide with. That is not a hole in the test: the
            // sizes it actually cares about are the ones where both are drawn,
            // and `the_panel_draws_its_rows_while_they_fit` is what stops the
            // panel escaping this test by drawing nothing at all.
            let (Some(history), Some(help)) = (
                probe::rect_of_sized(&app, Target::History, size),
                probe::rect_of_sized(&app, Target::Help, size),
            ) else {
                continue;
            };
            assert!(
                history.bottom() <= help.y + 0.01,
                "the history runs to {} and the help starts at {}, at {size:?}",
                history.bottom(),
                help.y
            );
        }
    }

    #[test]
    fn the_panel_draws_its_rows_while_they_fit() {
        // The companion to the two tests either side: they permit a panel to
        // omit a row, and this is what stops it omitting rows it has room for.
        // Every size the app is meant to be used at holds the whole panel.
        //
        // A recorded box is not enough. `panel_row` returns the rect whether or
        // not it drew in it, and the caller hands that rect to `f.hit` either
        // way -- so a panel that recorded all five boxes and painted no text in
        // any of them would satisfy a test that only asked whether the boxes
        // were there. That is a panel which answers clicks and shows nothing,
        // and it is what the mutation sweep produced. So each box is also
        // required to contain the origin of a line of text.
        for size in [
            CheckersApp::SIZE,
            (640.0, 480.0),
            (1600.0, 1000.0),
            (400.0, 900.0),
        ] {
            let app = app();
            let f = app.draw(size);
            let text: Vec<(f32, f32)> = f
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { x, y, .. } => Some((*x, *y)),
                    _ => None,
                })
                .collect();
            for target in [
                Target::NewGame,
                Target::Captures,
                Target::MoveNumber,
                Target::History,
                Target::Help,
            ] {
                let Some(r) = probe::rect_of_sized(&app, target, size) else {
                    panic!("{target:?} was not drawn at {size:?}, which has room for it");
                };
                assert!(
                    text.iter().any(|&(x, y)| x >= r.x - 0.01
                        && x <= r.right() + 0.01
                        && y >= r.y - 0.01
                        && y <= r.bottom() + 0.01),
                    "{target:?} at {size:?} recorded the box {r:?} and wrote nothing in it"
                );
            }
        }
    }

    #[test]
    fn the_panel_holds_everything_it_draws() {
        for size in SIZES {
            let app = app();
            let panel = box_at(&app, Target::Panel, size);
            for target in [
                Target::NewGame,
                Target::Captures,
                Target::MoveNumber,
                Target::History,
                Target::Help,
            ] {
                // Nothing is clipped to the panel, so this is a real question
                // and not Lesson 80's tautology: a row drawn past the bottom of
                // the panel is still inside the window, so the frame keeps its
                // box at full size and it shows up here.
                let Some(r) = probe::rect_of_sized(&app, target, size) else {
                    continue;
                };
                assert!(
                    inside(r, panel),
                    "{target:?} at {r:?} escapes the panel at {panel:?}, size {size:?}"
                );
            }
        }
    }

    #[test]
    fn the_new_game_button_starts_a_new_game() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        app.move_history.push("a1-b2".to_string());
        app.red_takes = 4;
        app.selected = Some(Pos::new(2, 2));
        let button = box_of(&app, Target::NewGame);
        let (cx, cy) = button.centre();
        tap(&mut app, cx, cy, size);
        assert!(
            app.move_history.is_empty(),
            "the history survived a new game"
        );
        assert_eq!(app.red_takes, 0, "the captures survived a new game");
        assert_eq!(app.selected, None, "the selection survived a new game");
    }

    #[test]
    fn a_new_game_keeps_the_window_size() {
        let mut app = app();
        app.resize(1234.0, 567.0);
        app.new_game();
        assert_eq!(
            app.size,
            (1234.0, 567.0),
            "a new game snapped the window back to its opening size"
        );
    }

    #[test]
    fn a_resize_event_is_what_the_next_click_is_read_against() {
        let mut app = app();
        let _ = handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 900,
            },
        );
        assert_eq!(app.size, (1200.0, 900.0));
        // And a click now resolves against the boxes at *that* size.
        let r = box_at(&app, Target::Square(3, 3), (1200.0, 900.0));
        let (cx, cy) = r.centre();
        let _ = handle_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert_eq!(app.cursor, Pos::new(3, 3));
    }

    #[test]
    fn the_window_opens_at_the_size_the_layout_is_written_for() {
        let app = app();
        assert_eq!(
            app.initial_size(),
            (880, 660),
            "the window asks for a size the natural-size constants do not name"
        );
        assert_eq!(CheckersApp::SIZE, (WINDOW_WIDTH, WINDOW_HEIGHT));
    }

    #[test]
    fn the_window_is_asked_to_redraw_only_when_something_changed() {
        // `handle_event` says whether the app used the event; `on_event` turns
        // that into what the window should do about it. Answering `Redraw` to
        // everything is invisible in a test of `handle_event` alone, and on
        // screen it is a repaint per key release and per mouse button the app
        // does not use -- the whole board redrawn to change nothing.
        let mut closing = app();
        assert_eq!(
            closing.on_event(&Event::CloseRequested),
            Response::Exit,
            "the close button did not close the window"
        );

        let mut app = app();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Right))),
            Response::Redraw,
            "moving the cursor did not ask for a repaint"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::release(Key::Right))),
            Response::Idle,
            "letting a key up repainted the window"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Tab))),
            Response::Idle,
            "a key the game does not use repainted the window"
        );
        assert_eq!(
            app.on_event(&Event::Mouse(MouseEvent {
                x: -50.0,
                y: -50.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            Response::Idle,
            "a click outside the window repainted it"
        );
    }

    #[test]
    fn the_render_pass_draws_at_the_size_the_window_hands_it() {
        // Every other drawing test in this file goes through `Probe::draw`,
        // which calls `frame` directly. `App::render` is the path the real
        // window uses and the only one that can be wrong on its own: remember
        // the new size, then hand `frame` the old one, and the app resizes its
        // hit boxes while drawing the previous size's picture -- a board that
        // does not follow the window, and clicks that land on nothing.
        //
        // The background is the first thing drawn and is the full window, so
        // the size the pass actually used is readable off command zero.
        let mut app = app();
        for (w, h) in [
            (1200.0, 900.0),
            (640.0, 480.0),
            (WINDOW_WIDTH, WINDOW_HEIGHT),
        ] {
            let tree = app.render(w, h);
            let first = tree.commands.first().expect("render drew nothing at all");
            let RenderCommand::FillRect { width, height, .. } = first else {
                panic!("the first thing drawn is no longer the background: {first:?}");
            };
            assert!(
                (width - w).abs() < 0.01 && (height - h).abs() < 0.01,
                "asked to render at {w}x{h}, the pass painted a {width}x{height} background"
            );
            assert_eq!(app.size, (w, h), "the render pass did not remember {w}x{h}");
        }
    }

    #[test]
    fn a_click_on_the_panel_is_answered_rather_than_dropped() {
        // A click the app ignores falls through to the window, which treats it
        // as a click on nothing at all -- so every box the app draws must
        // answer, even the ones that do nothing.
        let size = CheckersApp::SIZE;
        let mut app = app();
        let panel = box_of(&app, Target::Panel);
        let outcome = tap(&mut app, panel.x + 2.0, panel.bottom() - 2.0, size);
        assert_eq!(outcome, EventResult::Consumed);
    }

    #[test]
    fn a_right_click_is_not_a_move() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        let r = box_of(&app, Target::Square(2, 2));
        let (cx, cy) = r.centre();
        let outcome = tap_with(&mut app, cx, cy, MouseButton::Right, size);
        assert_eq!(outcome, EventResult::Ignored);
        assert_eq!(app.selected, None, "a right click selected a piece");
    }

    #[test]
    fn the_status_line_says_whose_turn_it_is() {
        let app = app();
        assert_eq!(app.status(), "Red to move");
        let f = app.draw(CheckersApp::SIZE);
        assert!(
            f.commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Red to move")),
            "the status the app reports is not the status it draws"
        );
    }

    #[test]
    fn no_frame_ever_says_black_is_thinking() {
        // `execute_move` used to set "Black thinking..." on the line before it
        // called `do_ai_move`, which overwrote it. The search runs inside the
        // same event, so no frame was ever drawn between the two and the
        // message could not be seen. It is gone; this is what keeps it gone.
        let mut app = app();
        for _ in 0..6 {
            let moves = app.board.generate_legal_moves();
            let Some(mv) = moves.first().cloned() else {
                break;
            };
            app.execute_move(&mv);
            let f = app.draw(CheckersApp::SIZE);
            for c in f.commands() {
                if let RenderCommand::Text { text, .. } = c {
                    assert!(
                        !text.contains("thinking"),
                        "a frame drew {text:?}, a notice no player can ever see"
                    );
                }
            }
        }
    }

    #[test]
    fn the_captures_line_credits_the_side_that_did_the_taking() {
        // The old pair was named for the side taken *from* -- `red_captured`
        // counted Red's losses -- and then printed as `Captured: Red {n}`,
        // which reads as the pieces Red has captured. The panel said the
        // opposite of what it counted.
        let mut app = app();
        let mut board = Board::empty();
        place(&mut board, 2, 2, Side::Red, false);
        place(&mut board, 3, 3, Side::Black, false);
        place(&mut board, 7, 7, Side::Black, false);
        board.side_to_move = Side::Red;
        app.board = board;
        let jump = app
            .board
            .generate_legal_moves()
            .into_iter()
            .find(|seq| seq.is_jump())
            .expect("the fixture set up a jump");
        app.execute_move(&jump);
        assert_eq!(
            app.red_takes, 1,
            "Red took a piece and the count credited to Red is {}",
            app.red_takes
        );
        let f = app.draw(CheckersApp::SIZE);
        assert!(
            f.commands().iter().any(
                |c| matches!(c, RenderCommand::Text { text, .. } if text == "Red 1   Black 0")
            ),
            "the panel does not report Red's single capture"
        );
    }

    #[test]
    fn the_cursor_walks_the_board_and_stops_at_its_edges() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        // Rank 1 is at the bottom, so Up raises the rank.
        let up = probe::press(Key::Up);
        for _ in 0..12 {
            app.key_at(&up, size);
        }
        assert_eq!(app.cursor.row, 7, "Up did not stop at rank 8");
        let down = probe::press(Key::Down);
        for _ in 0..12 {
            app.key_at(&down, size);
        }
        assert_eq!(app.cursor.row, 0, "Down did not stop at rank 1");
    }

    #[test]
    fn the_cursor_is_drawn_on_the_square_it_is_on() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        app.cursor = Pos::new(5, 3);
        let f = app.draw(size);
        let square = box_at(&app, Target::Square(5, 3), size);
        let ring = f.commands().iter().find_map(|c| match c {
            RenderCommand::StrokeRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } if *color == YELLOW => Some(Rect::new(*x, *y, *width, *height)),
            _ => None,
        });
        let ring = ring.expect("the cursor was not drawn");
        assert!(
            inside(ring, square),
            "the cursor at {ring:?} is not on its square at {square:?}"
        );
    }

    #[test]
    fn the_selection_ring_is_drawn_on_the_selected_square() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        app.select_piece(Pos::new(2, 2));
        assert_eq!(app.selected, Some(Pos::new(2, 2)));
        let f = app.draw(size);
        let square = box_at(&app, Target::Square(2, 2), size);
        let ring = f
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == SELECTED_SQUARE => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .expect("the selection was not drawn");
        assert!(
            inside(ring, square),
            "the selection ring at {ring:?} is not on its square at {square:?}"
        );
    }

    #[test]
    fn a_click_selects_a_piece_and_a_second_click_moves_it() {
        let size = CheckersApp::SIZE;
        let mut app = app();
        let from = Pos::new(2, 2);
        let r = box_of(&app, Target::Square(from.row, from.col));
        let (cx, cy) = r.centre();
        tap(&mut app, cx, cy, size);
        assert_eq!(app.selected, Some(from), "the first click did not select");

        let to = app
            .legal_moves_for_selected
            .first()
            .expect("the selected piece has no moves")
            .to_pos();
        let r = box_of(&app, Target::Square(to.row, to.col));
        let (cx, cy) = r.centre();
        tap(&mut app, cx, cy, size);
        assert_eq!(app.selected, None, "the move did not clear the selection");
        assert!(
            app.board.get(from).is_none(),
            "the piece is still on the square it left"
        );
        assert!(
            !app.move_history.is_empty(),
            "the move was not recorded in the history"
        );
    }

    #[test]
    fn the_header_counts_the_pieces_each_side_has_left() {
        let opening = app();
        let f = opening.draw(CheckersApp::SIZE);
        let drawn: Vec<&String> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(
            drawn.iter().any(|t| t.as_str() == "Red: 12"),
            "the header does not report Red's twelve pieces: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t.as_str() == "Black: 12"),
            "the header does not report Black's twelve pieces"
        );

        // The opening is twelve against twelve, so a header that reported each
        // side the *other* side's count would read correctly there and be
        // wrong for the rest of the game. Take three black pieces off and the
        // two counts stop agreeing.
        let mut app = app();
        for col in [0i8, 2, 4] {
            app.board.set(Pos::new(7, col + 1), None);
        }
        let f = app.draw(CheckersApp::SIZE);
        let drawn: Vec<&String> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(app.board.count_pieces(Side::Red), 12);
        assert_eq!(app.board.count_pieces(Side::Black), 9);
        assert!(
            drawn.iter().any(|t| t.as_str() == "Red: 12"),
            "Red still has twelve, and the header says otherwise: {drawn:?}"
        );
        assert!(
            drawn.iter().any(|t| t.as_str() == "Black: 9"),
            "Black is down to nine, and the header says otherwise: {drawn:?}"
        );
    }

    #[test]
    fn the_piece_counts_follow_the_title_rather_than_a_fixed_offset() {
        // The chips used to start at a hand-counted offset from the header's
        // left edge, chosen to clear the title at one font size. The title
        // scales with the window, so at a large enough one the chips slid under
        // it and at a small enough one they floated off on their own.
        for size in SIZES {
            let app = app();
            let Some(title) = probe::rect_of_sized(&app, Target::Title, size) else {
                continue;
            };
            let Some(red) = probe::rect_of_sized(&app, Target::Count(Side::Red), size) else {
                continue;
            };
            assert!(
                red.x > title.right(),
                "at {size:?} the Red count starts at {} and the title runs to {}",
                red.x,
                title.right()
            );
            // And not so far past it that the gap is unrelated to the title:
            // a fixed offset clears a small title by an arbitrary margin.
            assert!(
                red.x - title.right() < title.w,
                "at {size:?} the gap between the title and the Red count is {}, \
                 wider than the {} title it is supposed to be clearing",
                red.x - title.right(),
                title.w
            );
            if let Some(black) = probe::rect_of_sized(&app, Target::Count(Side::Black), size) {
                assert!(
                    black.x > red.right(),
                    "at {size:?} the Black count at {} overlaps the Red one ending at {}",
                    black.x,
                    red.right()
                );
            }
        }
    }

    #[test]
    fn the_header_counts_kings_once_there_are_any() {
        let mut app = app();
        app.board.set(Pos::new(4, 4), Some(Piece::king(Side::Red)));
        let f = app.draw(CheckersApp::SIZE);
        assert!(
            f.commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Red: 13 (1K)")),
            "the header does not report Red's king"
        );
    }

    #[test]
    fn a_king_wears_a_crown_centred_on_its_own_piece() {
        for size in SIZES {
            let mut app = app();
            app.board.set(Pos::new(4, 4), Some(Piece::king(Side::Red)));
            let square = box_at(&app, Target::Square(4, 4), size);
            let f = app.draw(size);
            let crown = f.commands().iter().find_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    font_weight,
                    ..
                } if text == "\u{265A}" => Some(Rect::new(
                    *x,
                    *y,
                    text::measure(text, *font_size, *font_weight),
                    text::line_height(*font_size, *font_weight),
                )),
                _ => None,
            });
            let crown = crown.expect("the king was drawn without a crown");
            let (kx, ky) = crown.centre();
            let (sx, sy) = square.centre();
            // The old crown was drawn at `cx - 8.0, cy - 10.0`: two nudges
            // chosen for one font size, which put it off its own piece at any
            // other. Half a square's tolerance is what "on its own piece"
            // means.
            assert!(
                (kx - sx).abs() < square.w / 2.0 && (ky - sy).abs() < square.h / 2.0,
                "the crown is centred at ({kx}, {ky}) and its square at ({sx}, {sy}), at {size:?}"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_at_a_zero_sized_window() {
        // A compositor can hand a window a zero size while it is being mapped.
        // The layout must survive it rather than dividing by it.
        let app = app();
        let f = app.draw((0.0, 0.0));
        assert!(f.is_balanced(), "a clip was pushed and never popped");
        for (_, r) in f.hits() {
            assert!(
                r.w <= 0.01 && r.h <= 0.01,
                "a box of {r:?} was recorded in a zero-sized window"
            );
        }
    }

    // ── Helper ──────────────────────────────────────────

    /// Place a piece on the board.
    fn place(board: &mut Board, row: i8, col: i8, side: Side, is_king: bool) {
        let piece = if is_king {
            Piece::king(side)
        } else {
            Piece::man(side)
        };
        board.set(Pos::new(row, col), Some(piece));
    }

    /// Every capture available to the piece standing on `pos`.
    ///
    /// `Board::generate_jumps_for` used to exist for this and was deleted: it
    /// was a strict subset of `generate_jumps_for_chain` with the chain state
    /// zeroed, called by nothing but itself and `has_jumps`. The zeroing is
    /// four words, so it lives here rather than as a second production entry
    /// point that only the tests reach.
    fn jumps_from(board: &Board, pos: Pos) -> Vec<CheckersMove> {
        match board.get(pos) {
            Some(piece) => board.generate_jumps_for_chain(pos, piece, &[], None),
            None => Vec::new(),
        }
    }

    // ── Pos tests ───────────────────────────────────────────────────

    #[test]
    fn test_pos_validity() {
        assert!(Pos::new(0, 7).is_valid());
        assert!(Pos::new(7, 0).is_valid());
        assert!(!Pos::new(-1, 0).is_valid());
        assert!(!Pos::new(0, 8).is_valid());
        assert!(!Pos::new(8, 0).is_valid());
    }

    #[test]
    fn test_pos_dark_squares() {
        // Stated as squares of a real board rather than as "row + col is even",
        // which would only restate the implementation.
        assert!(Pos::new(0, 0).is_dark(), "a1 is dark");
        assert!(!Pos::new(0, 1).is_dark(), "b1 is light");
        assert!(!Pos::new(1, 0).is_dark(), "a2 is light");
        assert!(Pos::new(1, 1).is_dark(), "b2 is dark");
        assert!(!Pos::new(3, 4).is_dark(), "e4 is light");
    }

    #[test]
    fn the_board_is_oriented_the_way_a_real_checkers_board_is() {
        // Two independent statements of the same convention, so a mirrored
        // board cannot satisfy both:
        //
        //   * a dark square in each player's lower-left corner -- a1 for Red,
        //     and h8 for Black, who sits at the far side;
        //   * the double corner (the two playable squares that meet at a
        //     corner) on each player's right, which means the lower-right
        //     corner h1 is light and both squares touching it are playable.
        assert!(
            Pos::new(0, 0).is_dark(),
            "a1, Red's lower left, must be dark"
        );
        assert!(
            Pos::new(7, 7).is_dark(),
            "h8, Black's lower left, must be dark"
        );
        assert!(!Pos::new(0, 7).is_dark(), "h1 must be light");
        assert!(
            Pos::new(0, 6).is_dark(),
            "g1 is one half of the double corner"
        );
        assert!(Pos::new(1, 7).is_dark(), "h2 is the other half");
    }

    #[test]
    fn the_board_is_painted_the_colour_it_says_each_square_is() {
        // Measured against the window rather than against `is_dark`: the
        // bottom-left square of the drawn board is the leftmost one at the
        // *largest* y, and it has to come out dark. Reading the colour back off
        // the paint is what stops `is_dark` and the renderer agreeing with each
        // other while both disagree with a checkers board.
        //
        // The square is found by geometry, not by asking the hit map for
        // `Square(0, 0)`: naming the target would let a board drawn upside down
        // pass, because the flipped board records the flipped box under the
        // same name.
        let size = CheckersApp::SIZE;
        let app = CheckersApp::new();
        let f = app.draw(size);
        let step = box_at(&app, Target::Square(0, 0), size).w;
        let bottom_left = f
            .commands()
            .iter()
            .filter_map(|c| match *c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (width - step).abs() < 0.01 && (height - step).abs() < 0.01 => {
                    Some((x, y, color))
                }
                _ => None,
            })
            // Sorted on y first so that "the bottom row" is settled before
            // "the left of it": a lexicographic sort the other way round would
            // pick the leftmost column's *top* square.
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.total_cmp(&a.0)))
            .map(|(_, _, color)| color);
        // Asserted as an Option so that painting no board at all fails here
        // rather than unwrapping into a panic with a different message.
        assert_eq!(
            bottom_left,
            Some(DARK_SQUARE),
            "the lower-left square of a checkers board is dark"
        );
    }

    #[test]
    fn every_row_has_four_playable_squares() {
        for row in 0..8i8 {
            let dark = (0..8i8).filter(|&col| Pos::new(row, col).is_dark()).count();
            assert_eq!(dark, 4, "row {row} should have four playable squares");
        }
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
        place(&mut board, 3, 5, Side::Red, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 5));
        // Red man moves diagonally forward (up): (4,6) and (4,4)
        assert_eq!(moves.len(), 2);
        assert!(moves.iter().any(|m| m.to == Pos::new(4, 6)));
        assert!(moves.iter().any(|m| m.to == Pos::new(4, 4)));
    }

    #[test]
    fn test_black_man_moves_forward() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 4, 4, Side::Black, false);
        let moves = board.generate_simple_moves_for(Pos::new(4, 4));
        // Black man moves diagonally forward (down): (3,5) and (3,3)
        assert_eq!(moves.len(), 2);
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 5)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 3)));
    }

    #[test]
    fn test_man_blocked_by_own_piece() {
        let mut board = Board::empty();
        place(&mut board, 3, 5, Side::Red, false);
        place(&mut board, 4, 4, Side::Red, false); // blocks forward-right
        let moves = board.generate_simple_moves_for(Pos::new(3, 5));
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, Pos::new(4, 6));
    }

    #[test]
    fn test_man_blocked_by_opponent() {
        let mut board = Board::empty();
        place(&mut board, 3, 5, Side::Red, false);
        place(&mut board, 4, 6, Side::Black, false);
        place(&mut board, 4, 4, Side::Black, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 5));
        // Both diagonal squares occupied
        assert_eq!(moves.len(), 0);
    }

    #[test]
    fn test_man_at_edge() {
        let mut board = Board::empty();
        place(&mut board, 3, 7, Side::Red, false);
        let moves = board.generate_simple_moves_for(Pos::new(3, 7));
        // Only one diagonal (right) is on the board
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to, Pos::new(4, 6));
    }

    #[test]
    fn test_king_moves_all_directions() {
        let mut board = Board::empty();
        place(&mut board, 4, 4, Side::Red, true);
        let moves = board.generate_simple_moves_for(Pos::new(4, 4));
        // King can move in 4 diagonal directions
        assert_eq!(moves.len(), 4);
        assert!(moves.iter().any(|m| m.to == Pos::new(5, 3)));
        assert!(moves.iter().any(|m| m.to == Pos::new(5, 5)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 3)));
        assert!(moves.iter().any(|m| m.to == Pos::new(3, 5)));
    }

    #[test]
    fn test_king_at_corner() {
        let mut board = Board::empty();
        place(&mut board, 0, 6, Side::Red, true);
        let moves = board.generate_simple_moves_for(Pos::new(0, 6));
        // Corner king has limited moves: (1,7) and (1,5)
        assert_eq!(moves.len(), 2);
    }

    // ── Jump generation ─────────────────────────────────────────────

    #[test]
    fn test_red_man_single_jump() {
        let mut board = Board::empty();
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        let jumps = jumps_from(&board, Pos::new(2, 6));
        assert_eq!(jumps.len(), 1);
        assert_eq!(jumps[0].to, Pos::new(4, 4));
        assert_eq!(jumps[0].captured, Some(Pos::new(3, 5)));
    }

    #[test]
    fn test_no_jump_over_own_piece() {
        let mut board = Board::empty();
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Red, false); // own piece
        let jumps = jumps_from(&board, Pos::new(2, 6));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_no_jump_when_landing_occupied() {
        let mut board = Board::empty();
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        place(&mut board, 4, 4, Side::Red, false); // landing blocked
        let jumps = jumps_from(&board, Pos::new(2, 6));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_no_jump_off_board() {
        let mut board = Board::empty();
        place(&mut board, 6, 1, Side::Red, false);
        place(&mut board, 7, 0, Side::Black, false);
        // Jump would land at (8,8) which is off the board
        let jumps = jumps_from(&board, Pos::new(6, 1));
        assert!(jumps.is_empty());
    }

    #[test]
    fn test_king_jumps_backward() {
        let mut board = Board::empty();
        place(&mut board, 4, 4, Side::Red, true); // king
        place(&mut board, 3, 5, Side::Black, false);
        let jumps = jumps_from(&board, Pos::new(4, 4));
        // King can jump backward
        assert!(jumps.iter().any(|j| j.to == Pos::new(2, 6)));
    }

    #[test]
    fn test_king_jumps_all_directions() {
        let mut board = Board::empty();
        place(&mut board, 4, 4, Side::Red, true); // king
        place(&mut board, 5, 3, Side::Black, false);
        place(&mut board, 3, 5, Side::Black, false);
        let jumps = jumps_from(&board, Pos::new(4, 4));
        assert_eq!(jumps.len(), 2);
    }

    // ── Multi-jump chains ───────────────────────────────────────────

    #[test]
    fn test_double_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 6, Side::Red, false);
        place(&mut board, 1, 5, Side::Black, false);
        place(&mut board, 3, 3, Side::Black, false);
        // Red at (0,6) jumps (1,5) to (2,4), then jumps (3,3) to (4,2)
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
        place(&mut board, 0, 6, Side::Red, false);
        place(&mut board, 1, 5, Side::Black, false);
        place(&mut board, 3, 3, Side::Black, false);
        place(&mut board, 5, 1, Side::Black, false);
        let moves = board.generate_legal_moves();
        let has_triple = moves.iter().any(|m| m.steps.len() == 3);
        assert!(has_triple, "Should find a triple-jump sequence");
    }

    #[test]
    fn test_multi_jump_captures_all() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 6, Side::Red, false);
        place(&mut board, 1, 5, Side::Black, false);
        place(&mut board, 3, 3, Side::Black, false);
        let moves = board.generate_legal_moves();
        let double = moves.iter().find(|m| m.steps.len() == 2).unwrap();
        assert_eq!(double.captured_count(), 2);
    }

    // ── Mandatory capture ───────────────────────────────────────────

    #[test]
    fn test_mandatory_capture() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 2, 2, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        // Piece at (2,6) can jump, piece at (2,2) can only move forward.
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
        place(&mut board, 2, 6, Side::Red, false);
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
        place(&mut board, 6, 7, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(6, 7), Pos::new(7, 6)));
        board.apply_move_in_place(&mv);
        let promoted = board.get(Pos::new(7, 6));
        assert!(promoted.is_some());
        assert!(promoted.unwrap().is_king, "Red should be promoted at row 7");
    }

    #[test]
    fn test_black_promotion() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 1, 7, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(1, 7), Pos::new(0, 6)));
        board.apply_move_in_place(&mv);
        let promoted = board.get(Pos::new(0, 6));
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
        place(&mut board, 3, 5, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(3, 5), Pos::new(4, 4)));
        board.apply_move_in_place(&mv);
        let piece = board.get(Pos::new(4, 4));
        assert!(piece.is_some());
        assert!(!piece.unwrap().is_king, "Should not promote in the middle");
    }

    #[test]
    fn test_promotion_via_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 5, 5, Side::Red, false);
        place(&mut board, 6, 4, Side::Black, false);
        // Red jumps from (5,5) over (6,4) to (7,3) -> promotion
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(5, 5),
            Pos::new(7, 3),
            Pos::new(6, 4),
        ));
        board.apply_move_in_place(&mv);
        let piece = board.get(Pos::new(7, 3));
        assert!(piece.is_some());
        assert!(piece.unwrap().is_king, "Should promote after jump to row 7");
        // Captured piece should be removed
        assert!(board.get(Pos::new(6, 4)).is_none());
    }

    #[test]
    fn test_promotion_stops_multi_jump() {
        // When a man reaches the promotion row mid-chain, the turn ends in American checkers.
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 5, 7, Side::Red, false);
        place(&mut board, 6, 6, Side::Black, false);
        // Place another black piece that could be jumped after promotion
        place(&mut board, 6, 4, Side::Black, false);
        let moves = board.generate_legal_moves();
        // The jump (5,7) -> (7,5) should stop at promotion, not continue
        let jump_from = moves
            .iter()
            .filter(|m| m.origin_pos() == Pos::new(5, 7) && m.is_jump())
            .collect::<Vec<_>>();
        assert!(!jump_from.is_empty());
        // All chains from this piece should end at row 7
        for jm in &jump_from {
            assert_eq!(jm.to_pos().row, 7, "Chain should stop at promotion row");
            assert_eq!(
                jm.steps.len(),
                1,
                "Should be a single jump (stops at promotion)"
            );
        }
    }

    // ── Move application ────────────────────────────────────────────

    #[test]
    fn test_simple_move_applies() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5)));
        board.apply_move_in_place(&mv);
        assert!(board.get(Pos::new(2, 6)).is_none());
        assert!(board.get(Pos::new(3, 5)).is_some());
        assert_eq!(board.side_to_move, Side::Black);
    }

    #[test]
    fn test_jump_removes_captured() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 6),
            Pos::new(4, 4),
            Pos::new(3, 5),
        ));
        board.apply_move_in_place(&mv);
        assert!(board.get(Pos::new(2, 6)).is_none());
        assert!(
            board.get(Pos::new(3, 5)).is_none(),
            "Captured piece removed"
        );
        assert!(board.get(Pos::new(4, 4)).is_some());
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
        place(&mut board, 2, 6, Side::Red, false);
        assert_eq!(board.move_count, 0);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5)));
        board.apply_move_in_place(&mv);
        assert_eq!(board.move_count, 1);
    }

    #[test]
    fn test_no_capture_count_increments() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5)));
        board.apply_move_in_place(&mv);
        assert_eq!(board.no_capture_count, 1);
    }

    #[test]
    fn test_capture_resets_no_capture_count() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        board.no_capture_count = 10;
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 6),
            Pos::new(4, 4),
            Pos::new(3, 5),
        ));
        board.apply_move_in_place(&mv);
        assert_eq!(board.no_capture_count, 0);
    }

    // ── Game result detection ───────────────────────────────────────

    #[test]
    fn test_red_wins_no_black_pieces() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 0, 6, Side::Red, false);
        // No black pieces -> Red wins
        assert_eq!(board.check_result(), GameResult::RedWins);
    }

    #[test]
    fn test_correct_win_detection() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 6, Side::Red, false);
        // No black pieces -> Red wins
        assert_eq!(board.check_result(), GameResult::RedWins);
    }

    #[test]
    fn test_black_wins_no_red_pieces() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 7, 7, Side::Black, false);
        // No red pieces -> Black wins
        assert_eq!(board.check_result(), GameResult::BlackWins);
    }

    #[test]
    fn test_draw_by_no_capture_rule() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        board.no_capture_count = 80;
        place(&mut board, 0, 6, Side::Red, true);
        place(&mut board, 7, 7, Side::Black, true);
        assert_eq!(board.check_result(), GameResult::Draw);
    }

    #[test]
    fn test_no_moves_loses() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        // Red piece in corner, blocked
        place(&mut board, 7, 7, Side::Red, false);
        // Red can't move forward (already at top row for a man),
        // so Red has no moves -> Black wins
        place(&mut board, 0, 6, Side::Black, false);
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
        place(&mut board, 4, 4, Side::Black, false);
        place(&mut board, 3, 3, Side::Red, false);
        // Give red a piece so game is not over
        place(&mut board, 0, 6, Side::Red, false);
        let ai_mv = ai_pick_move(&board);
        assert!(ai_mv.is_some());
        assert!(ai_mv.unwrap().is_jump(), "AI should prefer capture");
    }

    #[test]
    fn test_ai_returns_none_when_no_moves() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        // No black pieces
        place(&mut board, 0, 6, Side::Red, false);
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
        place(&mut board, 7, 7, Side::Black, false);
        // No red pieces -> Black wins
        let (score, _) = minimax(&board, 2, i32::MIN, i32::MAX, true);
        assert!(score > 0, "Black win should have positive score");
    }

    #[test]
    fn test_minimax_terminal_red_wins() {
        let mut board = Board::empty();
        board.side_to_move = Side::Black;
        place(&mut board, 0, 6, Side::Red, false);
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
        place(&mut board, 4, 4, Side::Black, false);
        place(&mut board, 4, 2, Side::Black, false);
        place(&mut board, 0, 6, Side::Red, false);
        let score = board.evaluate();
        assert!(
            score > 0,
            "Black should have positive score with more pieces"
        );
    }

    #[test]
    fn test_evaluate_king_worth_more() {
        let mut board = Board::empty();
        place(&mut board, 4, 4, Side::Black, true); // king
        place(&mut board, 0, 6, Side::Red, false); // man
        let score = board.evaluate();
        assert!(
            score > MAN_VALUE,
            "King should be worth significantly more than man"
        );
    }

    // ── MoveSequence tests ──────────────────────────────────────────

    #[test]
    fn test_move_sequence_notation_simple() {
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5)));
        let notation = mv.notation();
        assert_eq!(notation, "g3-f4");
    }

    #[test]
    fn test_move_sequence_notation_jump() {
        let mv = MoveSequence::single(CheckersMove::jump(
            Pos::new(2, 6),
            Pos::new(4, 4),
            Pos::new(3, 5),
        ));
        let notation = mv.notation();
        assert_eq!(notation, "g3xe5");
    }

    #[test]
    fn test_move_sequence_notation_double_jump() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 6), Pos::new(2, 4), Pos::new(1, 5)),
            CheckersMove::jump(Pos::new(2, 4), Pos::new(4, 2), Pos::new(3, 3)),
        ]);
        let notation = mv.notation();
        assert_eq!(notation, "g1xe3xc5");
    }

    #[test]
    fn test_move_sequence_from_to() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 6), Pos::new(2, 4), Pos::new(1, 5)),
            CheckersMove::jump(Pos::new(2, 4), Pos::new(4, 2), Pos::new(3, 3)),
        ]);
        assert_eq!(mv.origin_pos(), Pos::new(0, 6));
        assert_eq!(mv.to_pos(), Pos::new(4, 2));
    }

    #[test]
    fn test_move_sequence_captured_count() {
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 6), Pos::new(2, 4), Pos::new(1, 5)),
            CheckersMove::jump(Pos::new(2, 4), Pos::new(4, 2), Pos::new(3, 3)),
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
        app.red_takes = 5;
        app.new_game();
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert!(app.move_history.is_empty());
        assert_eq!(app.red_takes, 0);
    }

    #[test]
    fn test_click_light_square_does_nothing() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(0, 7)); // light square
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_click_empty_dark_square_does_nothing() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(3, 5)); // empty dark square
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_click_own_piece_selects() {
        let mut app = CheckersApp::new();
        // Red piece at (2,6)
        app.click_square(Pos::new(2, 6));
        assert_eq!(app.selected, Some(Pos::new(2, 6)));
        assert!(!app.legal_moves_for_selected.is_empty());
    }

    #[test]
    fn test_click_opponent_piece_no_select() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(5, 7)); // Black piece
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_escape_deselects() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 6)); // select
        assert!(app.selected.is_some());
        let event = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&event);
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_game_over_prevents_moves() {
        let mut app = CheckersApp::new();
        app.game_result = GameResult::RedWins;
        app.click_square(Pos::new(2, 6));
        assert!(
            app.selected.is_none(),
            "Should not select when game is over"
        );
    }

    #[test]
    fn test_cursor_movement() {
        let mut app = CheckersApp::new();
        let right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&right);
        assert_eq!(app.cursor.col, 1);

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&up);
        assert_eq!(app.cursor.row, 1);

        let left = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&left);
        assert_eq!(app.cursor.col, 0);

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&down);
        assert_eq!(app.cursor.row, 0);
    }

    #[test]
    fn test_cursor_bounds() {
        let mut app = CheckersApp::new();
        // This test is about the clamp, so the coordinates are the board's
        // limits rather than any particular square, and do not move with the
        // playable-square parity.
        app.cursor = Pos::new(0, 0);
        let left = KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&left);
        assert_eq!(app.cursor.col, 0, "Cursor should not go below 0");

        let down = KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&down);
        assert_eq!(app.cursor.row, 0, "Cursor should not go below 0");
    }

    #[test]
    fn test_cursor_upper_bounds() {
        let mut app = CheckersApp::new();
        // As above: the far corner, not a square chosen for its colour.
        app.cursor = Pos::new(7, 7);
        let right = KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&right);
        assert_eq!(app.cursor.col, 7, "Cursor should not exceed 7");

        let up = KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&up);
        assert_eq!(app.cursor.row, 7);
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = CheckersApp::new();
        let frame = app.draw(CheckersApp::SIZE);
        let commands = frame.commands();
        assert!(!commands.is_empty(), "Render should produce commands");
        // Should have at least: background + 64 squares + board border + pieces
        assert!(commands.len() > 70, "Should produce many render commands");
    }

    #[test]
    fn test_render_selected_square_highlight() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 6)); // select a red piece
        let frame = app.draw(CheckersApp::SIZE);
        let commands = frame.commands();
        let has_selection = commands.iter().any(
            |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == SELECTED_SQUARE),
        );
        assert!(has_selection, "Should render selected square highlight");
    }

    #[test]
    fn test_render_legal_move_indicators() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 6)); // select a piece with moves
        let frame = app.draw(CheckersApp::SIZE);
        let commands = frame.commands();
        let dot_count = commands
            .iter()
            .filter(
                |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == LEGAL_MOVE_DOT),
            )
            .count();
        assert!(
            dot_count >= 1,
            "Should show legal move dots, got {dot_count}"
        );
    }

    #[test]
    fn test_render_cursor_highlight() {
        let app = CheckersApp::new();
        let frame = app.draw(CheckersApp::SIZE);
        let commands = frame.commands();
        let has_cursor = commands
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == YELLOW));
        assert!(has_cursor, "Should render cursor highlight");
    }

    #[test]
    fn test_render_pieces_on_board() {
        let app = CheckersApp::new();
        let frame = app.draw(CheckersApp::SIZE);
        let commands = frame.commands();
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
        let size = CheckersApp::SIZE;
        let mut app = CheckersApp::new();
        // The middle of c3 -- board row 2, column 6 -- taken from the box the
        // drawing pass recorded, not from a second copy of the board
        // arithmetic. `square_center` was that second copy, and it and
        // `square_at` were each other's inverse, so this test used to agree
        // with whatever mapping the pair happened to share.
        let (x, y) = box_at(&app, Target::Square(2, 6), size).centre();
        tap(&mut app, x, y, size);
        assert_eq!(app.selected, Some(Pos::new(2, 6)));
    }

    #[test]
    fn test_mouse_click_outside_board() {
        let size = CheckersApp::SIZE;
        let mut app = CheckersApp::new();
        tap(&mut app, 0.0, 0.0, size);
        assert!(app.selected.is_none());
    }

    // ── Event dispatch tests ────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = CheckersApp::new();
        let event = Event::Key(probe::press(Key::Right));
        handle_event(&mut app, &event);
        assert_eq!(app.cursor.col, 1, "cursor starts at a1 and steps right");
    }

    #[test]
    fn a_resize_changes_the_size_without_disturbing_the_game() {
        // This was `test_handle_event_resize_ignored`, and the name was
        // accurate: the old app ignored resizes because it drew at eleven
        // fixed pixel counts. The resize now has to land -- and still has to
        // leave the game alone.
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 6));
        let selected = app.selected;
        handle_event(
            &mut app,
            &Event::Resize {
                width: 800,
                height: 600,
            },
        );
        assert_eq!(app.size, (800.0, 600.0), "the resize did not land");
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert_eq!(app.selected, selected, "the resize disturbed the selection");
    }

    #[test]
    fn test_handle_key_not_pressed_ignored() {
        let mut app = CheckersApp::new();
        let event = KeyEvent {
            key: Key::Right,
            pressed: false, // key released, not pressed
            modifiers: Modifiers::NONE,
            text: String::new(),
        };
        app.handle_key(&event);
        // Cursor should not have moved
        assert_eq!(app.cursor.col, 0);
    }

    // ── Full game flow tests ────────────────────────────────────────

    #[test]
    fn test_make_move_and_ai_responds() {
        let mut app = CheckersApp::new();
        // Select a red piece and make a valid move
        app.click_square(Pos::new(2, 6)); // select
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
        app.click_square(Pos::new(2, 6)); // select first piece
        assert_eq!(app.selected, Some(Pos::new(2, 6)));
        app.click_square(Pos::new(2, 4)); // select different piece
        assert_eq!(app.selected, Some(Pos::new(2, 4)));
    }

    #[test]
    fn test_click_invalid_deselects() {
        let mut app = CheckersApp::new();
        app.click_square(Pos::new(2, 6)); // select
        assert!(app.selected.is_some());
        app.click_square(Pos::new(4, 4)); // empty square, not a legal move
        assert!(app.selected.is_none());
    }

    #[test]
    fn test_ctrl_n_new_game() {
        let mut app = CheckersApp::new();
        app.game_result = GameResult::RedWins;
        app.red_takes = 3;
        let event = KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        };
        app.handle_key(&event);
        assert_eq!(app.game_result, GameResult::Ongoing);
        assert_eq!(app.red_takes, 0);
    }

    // ── Forced capture ──────────────────────────────────────────────
    //
    // `Board::has_jumps` used to answer "is a capture available?" and was
    // deleted along with `generate_jumps_for`, its only caller. The question is
    // still worth asking, but the answer that matters is the one the move list
    // gives: in draughts an available capture is not merely available, it is
    // *compulsory*, so a board with a capture on it must offer nothing else.

    #[test]
    fn the_opening_position_offers_no_capture() {
        let board = Board::new();
        assert!(
            !board
                .generate_legal_moves()
                .iter()
                .any(MoveSequence::is_jump),
            "no piece can be taken from the opening position"
        );
    }

    #[test]
    fn a_capture_on_offer_is_the_only_move_allowed() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        place(&mut board, 3, 5, Side::Black, false);
        // The red man could otherwise step to (3, 7); the capture takes that
        // choice away, which is what "forced capture" means.
        let moves = board.generate_legal_moves();
        assert!(!moves.is_empty(), "red has a capture and so has a move");
        assert!(
            moves.iter().all(MoveSequence::is_jump),
            "a quiet move was offered alongside a capture: {moves:?}"
        );
    }

    // ── CheckersMove tests ──────────────────────────────────────────

    #[test]
    fn test_checkers_move_simple() {
        let mv = CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5));
        assert!(!mv.is_jump());
        assert!(mv.captured.is_none());
    }

    #[test]
    fn test_checkers_move_jump() {
        let mv = CheckersMove::jump(Pos::new(2, 6), Pos::new(4, 4), Pos::new(3, 5));
        assert!(mv.is_jump());
        assert_eq!(mv.captured, Some(Pos::new(3, 5)));
    }

    // ── Board::apply_move (non-destructive) test ────────────────────

    #[test]
    fn test_apply_move_returns_new_board() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 2, 6, Side::Red, false);
        let mv = MoveSequence::single(CheckersMove::simple(Pos::new(2, 6), Pos::new(3, 5)));
        let new_board = board.apply_move(&mv);
        // Original board unchanged
        assert!(board.get(Pos::new(2, 6)).is_some());
        // New board has the move applied
        assert!(new_board.get(Pos::new(2, 6)).is_none());
        assert!(new_board.get(Pos::new(3, 5)).is_some());
    }

    // ── Legal moves from specific position ──────────────────────────

    #[test]
    fn test_generate_legal_moves_from() {
        let board = Board::new();
        let moves = board.generate_legal_moves_from(Pos::new(2, 6));
        // Piece at (2,6) can move to (3,7) and (3,5)
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn test_generate_legal_moves_from_empty() {
        let board = Board::new();
        let moves = board.generate_legal_moves_from(Pos::new(4, 4));
        assert!(moves.is_empty(), "No piece = no moves");
    }

    // ── Initial legal moves ─────────────────────────────────────────

    #[test]
    fn test_initial_red_legal_moves_count() {
        let board = Board::new();
        let moves = board.generate_legal_moves();
        // Red has pieces on row 2 that can move. Row 2 dark squares: (2,6),(2,4),(2,2),(2,0)
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
        place(&mut board, 2, 6, Side::Red, false); // no jump available
        place(&mut board, 2, 2, Side::Red, false);
        place(&mut board, 3, 1, Side::Black, false); // jump available for (2,2)
        // (2,2) can jump to (4,0). Mandatory capture.
        let mut app = CheckersApp::new();
        app.board = board;
        app.cursor = Pos::new(0, 6);
        // Try to select (2,6) which has no jump
        app.click_square(Pos::new(2, 6));
        assert!(
            app.selected.is_none(),
            "Should not select a piece that cannot participate in mandatory capture"
        );
        // Select (2,2) which has a jump
        app.click_square(Pos::new(2, 2));
        assert_eq!(app.selected, Some(Pos::new(2, 2)));
    }

    // ── King in multi-jump ──────────────────────────────────────────

    #[test]
    fn test_king_multi_jump() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 4, 4, Side::Red, true); // king
        place(&mut board, 3, 5, Side::Black, false);
        place(&mut board, 3, 3, Side::Black, false);
        // King at (4,4) can jump (3,5) to (2,6), then potentially (3,3)... wait,
        // from (2,6) to jump (3,3) would require being at adjacent diagonal which doesn't work.
        // Let's set up a proper king multi-jump:
        // King at (4,4), black at (5,3) and (3,3). King jumps (5,3) to (6,2), then backward
        // to jump... that won't chain either. Let's use:
        // King at (4,4), black at (3,5) and (1,5).
        // Jump (3,5) -> (2,6), jump (1,5) -> (0,4)
        let mut board2 = Board::empty();
        board2.side_to_move = Side::Red;
        place(&mut board2, 4, 4, Side::Red, true);
        place(&mut board2, 3, 5, Side::Black, false);
        place(&mut board2, 1, 5, Side::Black, false);
        let moves = board2.generate_legal_moves();
        let multi = moves.iter().find(|m| m.steps.len() == 2);
        assert!(multi.is_some(), "King should be able to do multi-jump");
    }

    // ── Apply move multi-jump ───────────────────────────────────────

    #[test]
    fn test_apply_multi_jump_removes_all_captured() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        place(&mut board, 0, 6, Side::Red, false);
        place(&mut board, 1, 5, Side::Black, false);
        place(&mut board, 3, 3, Side::Black, false);
        let mv = MoveSequence::new(vec![
            CheckersMove::jump(Pos::new(0, 6), Pos::new(2, 4), Pos::new(1, 5)),
            CheckersMove::jump(Pos::new(2, 4), Pos::new(4, 2), Pos::new(3, 3)),
        ]);
        board.apply_move_in_place(&mv);
        assert!(
            board.get(Pos::new(1, 5)).is_none(),
            "First captured removed"
        );
        assert!(
            board.get(Pos::new(3, 3)).is_none(),
            "Second captured removed"
        );
        assert!(board.get(Pos::new(4, 2)).is_some(), "Piece at destination");
        assert!(board.get(Pos::new(0, 6)).is_none(), "Origin cleared");
    }

    // ── Game result: stalemate (no moves) ───────────────────────────

    #[test]
    fn test_stalemate_red_blocked() {
        let mut board = Board::empty();
        board.side_to_move = Side::Red;
        // Red man at top row can't move forward
        place(&mut board, 7, 1, Side::Red, false);
        // Black piece to keep game going
        place(&mut board, 0, 6, Side::Black, false);
        let result = board.check_result();
        assert_eq!(result, GameResult::BlackWins, "Blocked player loses");
    }
}
