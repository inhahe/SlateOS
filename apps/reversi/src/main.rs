//! Slate OS Reversi (Othello) -- a full game against an AI opponent.
//!
//! An 8x8 board with the standard rules: legal-move validation, flipping in
//! all eight directions, passing when a player has no move, game end and
//! scoring. The opponent is a minimax search with alpha-beta pruning and a
//! positional evaluation. Alongside the board sit the score, the move count,
//! the last move and a scrolling history.
//!
//! The whole picture is solved from the size the window reports each frame:
//! there is no built-in size the drawing falls back on, and every box a click
//! is tested against is one the drawing pass recorded. Every square is
//! clickable as well as reachable by the arrow keys.
//!
//! Themed with the Catppuccin Mocha palette.

use std::cmp::Ordering;
use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const RED: Color = Color::from_hex(0xF38BA8);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Board colours ───────────────────────────────────────────────────
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

// ── The board's dimensions ──────────────────────────────────────────

/// Squares along one side.
const BOARD_SIZE: usize = 8;

/// The same count, as the signed type the row and column arithmetic uses.
///
/// Two spellings of one number is exactly the hazard this file was full of, so
/// `the_two_spellings_of_the_board_size_agree` pins them together rather than
/// a cast doing it silently: `BOARD_SIZE as i32` would need a suppressed lint
/// here and would still be a second place the size is written.
const SIDE: i32 = 8;

/// The highest legal row or column index.
const LAST: i32 = SIDE - 1;

/// How deep the search looks.
const AI_DEPTH: i32 = 4;

/// The size the window asks for when it opens. Everything afterwards comes
/// from the size the window reports, not from this.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// The eight directions a flank can run in.
const DIRECTIONS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// How much each square is worth to the evaluation.
///
/// Corners are unflippable and so are worth most; the squares diagonally
/// inside them hand a corner to the opponent and are worth least.
const POSITION_WEIGHTS: [[i32; 8]; 8] = [
    [100, -20, 10, 5, 5, 10, -20, 100],
    [-20, -50, -2, -2, -2, -2, -50, -20],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [5, -2, -1, -1, -1, -1, -2, 5],
    [10, -2, -1, -1, -1, -1, -2, 10],
    [-20, -50, -2, -2, -2, -2, -50, -20],
    [100, -20, 10, 5, 5, 10, -20, 100],
];

// ── What a click can land on ────────────────────────────────────────

/// Everything the drawing pass records a box for.
///
/// A click is answered by asking the frame what was drawn where the pointer
/// is, so a control that moves cannot leave its hit box behind. Before this
/// existed the board's screen position and the click arithmetic were two
/// copies of the same constants, kept in step by nothing but care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// One square, by row and column.
    Square(u8, u8),
    /// The whole board, behind its squares.
    Board,
    /// A column's `a`-`h` letter, by column.
    ColLabel(u8),
    /// A row's `1`-`8` number, by row.
    RowLabel(u8),
    Title,
    BlackScore,
    WhiteScore,
    Panel,
    Turn,
    ScoreBar,
    Moves,
    EmptyCount,
    LastMove,
    History,
    Help,
    Status,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, solved from the size the window reports.
///
/// Every measurement below is a share of the window. The old drawing had
/// `CELL_SIZE: f32 = 60.0`, `BOARD_OFFSET_X: f32 = 40.0` and a dozen more
/// like them, and the window was whatever those constants happened to add up
/// to -- which is to say the board was the wrong size at every window size but
/// one, and unreachable off the bottom of anything shorter.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    header: Rect,
    board_area: Rect,
    panel: Rect,
    status: Rect,
    pad: f32,
    title: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // A floor so a small window is not margined by a fraction of a pixel,
        // a ceiling so a 4K one is not margined by thirty, and a cap at half
        // the shorter side so a window narrower than twice its own margin is
        // not given a margin wider than the window it is a margin inside.
        let pad = (w.min(h) * 0.02).clamp(2.0, 20.0).min(w.min(h) / 2.0);
        let title = (h * 0.034).clamp(10.0, 30.0);
        let font = (h * 0.025).clamp(8.0, 20.0);
        let small = (h * 0.019).clamp(7.0, font);

        // Each band takes a share of what the bands before it left, and the
        // body is what is left when both have taken theirs. Written this way
        // the three heights sum to exactly `h` and none of them can come out
        // negative, so none of them needs a guard saying so.
        let header_h = h * 0.09;
        let rest = h - header_h;
        let status_h = rest * 0.08;
        let body_h = rest - status_h;

        let header = Rect::new(0.0, 0.0, w, header_h);
        let body = Rect::new(0.0, header.bottom(), w, body_h);
        let status = Rect::new(0.0, body.bottom(), w, status_h);

        // The panel is a share of the width, floored so its lines are still
        // legible on a narrow window, capped so a wide one does not hand a
        // quarter of the screen to six short lines, and then capped again at
        // half the window so it can never be wider than the board it is
        // beside.
        let panel_w = (w * 0.26).clamp(110.0, 260.0).min(w / 2.0);
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

// ── Board geometry ──────────────────────────────────────────────────

/// Where the eight-by-eight grid is drawn, fitted to the room it was given.
///
/// Where a square is painted and which square a click lands in used to be
/// computed by two copies of the same arithmetic. They are one thing here, and
/// the drawing pass hands the result to the frame as a hit box, so the board a
/// player sees is by construction the board a click resolves against.
#[derive(Debug, Clone, Copy)]
struct Grid {
    /// The top-left of square (0, 0).
    origin: (f32, f32),
    /// The side of one square.
    step: f32,
    /// Room reserved to the left of and above the squares for a-h and 1-8.
    label: f32,
}

impl Grid {
    /// Fit a labelled 8x8 board into `area`.
    ///
    /// The square is taken from *both* axes: the room has to hold the board
    /// and its labels either way round. Fitting to width alone is what let the
    /// old fixed layout run the board off the bottom of any window shorter
    /// than the one it was written for.
    fn fit(area: Rect, label_font: f32) -> Self {
        let side = f32_from_usize(BOARD_SIZE);
        let label = label_font * 1.7;
        let step = ((area.w - label).max(0.0) / side)
            .min((area.h - label).max(0.0) / side)
            .max(0.0);
        let board = step * side;
        // The labelled board, centred in what it was fitted to.
        let left = area.x + (area.w - board - label).max(0.0) / 2.0;
        let top = area.y + (area.h - board - label).max(0.0) / 2.0;
        Self {
            origin: (left + label, top + label),
            step,
            label,
        }
    }

    /// The square at `(row, col)`.
    fn square(self, row: i32, col: i32) -> Rect {
        Rect::new(
            self.origin.0 + f32_from_i32(col) * self.step,
            self.origin.1 + f32_from_i32(row) * self.step,
            self.step,
            self.step,
        )
    }

    /// The middle of the square at `(row, col)`, where its piece is drawn.
    fn centre(self, row: i32, col: i32) -> (f32, f32) {
        let r = self.square(row, col);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The whole board, edge to edge.
    fn board_rect(self) -> Rect {
        let board = self.step * f32_from_usize(BOARD_SIZE);
        Rect::new(self.origin.0, self.origin.1, board, board)
    }
}

/// `usize` to `f32` without a lint-suppressed cast at every call site.
///
/// The count of squares on a board is far below `f32`'s exact-integer range,
/// so the conversion is lossless here; it is written once so that is stated
/// once.
fn f32_from_usize(v: usize) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

/// `i32` to `f32`, for row and column indices.
fn f32_from_i32(v: i32) -> f32 {
    f32::from(i16::try_from(v).unwrap_or(i16::MAX))
}

/// `u32` to `f32`, for the size a resize event reports.
fn f32_from_u32(v: u32) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

/// A non-negative float to a count of rows, saturating rather than wrapping.
///
/// The ceiling is the number of squares on the board, which is the most moves
/// a game can hold; a panel tall enough to want more rows than that is a panel
/// asking for rows that do not exist.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=64 either side of the cast"
)]
fn count_from_f32(v: f32) -> usize {
    let clamped = v.floor().clamp(0.0, 64.0);
    clamped as usize
}

/// A byte index for a `Target`, saturating rather than wrapping.
fn byte(v: i32) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// `rect` shrunk by `pad` on every side, and never inside out.
///
/// A window narrower than twice its own padding would otherwise produce a
/// negative width, which is not a smaller box but a box that starts to the
/// right of where it ends.
fn inset(rect: Rect, pad: f32) -> Rect {
    Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    )
}

// ── Cells and positions ─────────────────────────────────────────────

/// The state of a single square.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Empty,
    Black,
    White,
}

impl Cell {
    /// The opposite colour, or `Empty` if empty.
    fn opponent(self) -> Self {
        match self {
            Cell::Black => Cell::White,
            Cell::White => Cell::Black,
            Cell::Empty => Cell::Empty,
        }
    }

    /// Whether this square is occupied.
    fn is_piece(self) -> bool {
        self != Cell::Empty
    }
}

/// A position on the board, each coordinate in `0..8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: i32,
    col: i32,
}

impl Pos {
    fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    /// Whether this position is on the board.
    fn in_bounds(self) -> bool {
        (0..SIDE).contains(&self.row) && (0..SIDE).contains(&self.col)
    }
}

/// Every position on the board, once each, in reading order.
fn all_positions() -> impl Iterator<Item = Pos> {
    (0..SIDE).flat_map(|row| (0..SIDE).map(move |col| Pos::new(row, col)))
}

// ── Board ───────────────────────────────────────────────────────────

/// The 8x8 Reversi board.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Board {
    cells: [[Cell; BOARD_SIZE]; BOARD_SIZE],
}

impl Board {
    /// The standard Othello opening: White on d4/e5, Black on d5/e4.
    fn new() -> Self {
        let mut board = Self::empty();
        board.set(Pos::new(3, 3), Cell::White);
        board.set(Pos::new(3, 4), Cell::Black);
        board.set(Pos::new(4, 3), Cell::Black);
        board.set(Pos::new(4, 4), Cell::White);
        board
    }

    /// A board with nothing on it.
    fn empty() -> Self {
        Self {
            cells: [[Cell::Empty; BOARD_SIZE]; BOARD_SIZE],
        }
    }

    /// What is on `pos`, or `Empty` for anything off the board.
    fn get(&self, pos: Pos) -> Cell {
        let (Ok(r), Ok(c)) = (usize::try_from(pos.row), usize::try_from(pos.col)) else {
            return Cell::Empty;
        };
        self.cells
            .get(r)
            .and_then(|row| row.get(c))
            .copied()
            .unwrap_or(Cell::Empty)
    }

    /// Put `cell` on `pos`, or do nothing if `pos` is off the board.
    ///
    /// The bounds check is the lookup itself rather than an `in_bounds` call
    /// before it: a second place that knows how big the board is, is a second
    /// place that can be wrong about it.
    fn set(&mut self, pos: Pos, cell: Cell) {
        let (Ok(r), Ok(c)) = (usize::try_from(pos.row), usize::try_from(pos.col)) else {
            return;
        };
        if let Some(slot) = self.cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            *slot = cell;
        }
    }

    /// How many squares hold `color`.
    fn count(&self, color: Cell) -> i32 {
        let n = self
            .cells
            .iter()
            .flat_map(|row| row.iter())
            .filter(|&&cell| cell == color)
            .count();
        i32::try_from(n).unwrap_or(i32::MAX)
    }

    /// How many squares are still empty.
    ///
    /// Counted, not subtracted. It used to be `64 - total_pieces()`, with the
    /// 64 written out as a literal and `total_pieces` existing for no other
    /// purpose -- a third place that knew how big the board is, and a helper
    /// that only served it.
    fn empty_count(&self) -> i32 {
        self.count(Cell::Empty)
    }

    /// The opponent pieces `color` would flank by playing `pos`, walking one
    /// direction only.
    ///
    /// Empty when the walk runs off the board or reaches an empty square: a
    /// run of opponent pieces is only flanked if one's own piece closes it.
    fn flips_in_direction(&self, pos: Pos, color: Cell, dr: i32, dc: i32) -> Vec<Pos> {
        let opponent = color.opponent();
        let mut flipped = Vec::new();
        let mut at = Pos::new(pos.row.saturating_add(dr), pos.col.saturating_add(dc));

        while at.in_bounds() {
            let current = self.get(at);
            if current == opponent {
                flipped.push(at);
            } else if current == color {
                // Our own piece closes the run: everything between is flanked.
                return flipped;
            } else {
                break;
            }
            at = Pos::new(at.row.saturating_add(dr), at.col.saturating_add(dc));
        }

        Vec::new()
    }

    /// Everything `color` would flip by playing `pos`, in all eight
    /// directions.
    fn get_flips(&self, pos: Pos, color: Cell) -> Vec<Pos> {
        if !pos.in_bounds() || self.get(pos) != Cell::Empty {
            return Vec::new();
        }
        DIRECTIONS
            .iter()
            .flat_map(|&(dr, dc)| self.flips_in_direction(pos, color, dr, dc))
            .collect()
    }

    /// Whether `color` may play `pos`: an empty square that flips something.
    fn is_legal_move(&self, pos: Pos, color: Cell) -> bool {
        if !pos.in_bounds() || self.get(pos) != Cell::Empty {
            return false;
        }
        DIRECTIONS
            .iter()
            .any(|&(dr, dc)| !self.flips_in_direction(pos, color, dr, dc).is_empty())
    }

    /// Every square `color` may play.
    fn legal_moves(&self, color: Cell) -> Vec<Pos> {
        all_positions()
            .filter(|&pos| self.is_legal_move(pos, color))
            .collect()
    }

    /// Play `pos` and flip what it flanks, returning how many pieces turned.
    ///
    /// Zero, and nothing changed, if the move is illegal.
    fn make_move(&mut self, pos: Pos, color: Cell) -> i32 {
        let flips = self.get_flips(pos, color);
        if flips.is_empty() {
            return 0;
        }
        self.set(pos, color);
        for flip_pos in &flips {
            self.set(*flip_pos, color);
        }
        i32::try_from(flips.len()).unwrap_or(i32::MAX)
    }

    /// Whether `color` has any move at all.
    fn has_legal_move(&self, color: Cell) -> bool {
        all_positions().any(|pos| self.is_legal_move(pos, color))
    }

    /// Whether neither player can move, which is how Reversi ends.
    fn is_game_over(&self) -> bool {
        !self.has_legal_move(Cell::Black) && !self.has_legal_move(Cell::White)
    }

    /// Who has more pieces, or `Cell::Empty` for a tie.
    fn winner(&self) -> Cell {
        match self.count(Cell::Black).cmp(&self.count(Cell::White)) {
            Ordering::Greater => Cell::Black,
            Ordering::Less => Cell::White,
            Ordering::Equal => Cell::Empty,
        }
    }
}

// ── AI ──────────────────────────────────────────────────────────────

/// The four corners, which are the squares that can never be flipped back.
fn corners() -> [Pos; 4] {
    [
        Pos::new(0, 0),
        Pos::new(0, LAST),
        Pos::new(LAST, 0),
        Pos::new(LAST, LAST),
    ]
}

/// Score the board from `color`'s point of view: material, position,
/// mobility, corners.
///
/// Every term saturates. The honest bound is far below `i32`'s range -- 64
/// squares of at most 100 each, plus 64 moves of 5, plus four corners of 50 --
/// but saturating says so in the code rather than in a comment nobody has to
/// keep true.
fn evaluate(board: &Board, color: Cell) -> i32 {
    let opponent = color.opponent();
    let mut score = board
        .count(color)
        .saturating_sub(board.count(opponent))
        .saturating_mul(10);

    for pos in all_positions() {
        let Some(w) = weight_at(pos) else { continue };
        let cell = board.get(pos);
        if cell == color {
            score = score.saturating_add(w);
        } else if cell == opponent {
            score = score.saturating_sub(w);
        }
    }

    let my_moves = i32::try_from(board.legal_moves(color).len()).unwrap_or(i32::MAX);
    let opp_moves = i32::try_from(board.legal_moves(opponent).len()).unwrap_or(i32::MAX);
    score = score.saturating_add(my_moves.saturating_sub(opp_moves).saturating_mul(5));

    for corner in corners() {
        let cell = board.get(corner);
        if cell == color {
            score = score.saturating_add(50);
        } else if cell == opponent {
            score = score.saturating_sub(50);
        }
    }

    score
}

/// What `pos` is worth positionally, or `None` if it is off the board.
fn weight_at(pos: Pos) -> Option<i32> {
    let (Ok(r), Ok(c)) = (usize::try_from(pos.row), usize::try_from(pos.col)) else {
        return None;
    };
    POSITION_WEIGHTS.get(r).and_then(|row| row.get(c)).copied()
}

/// Minimax with alpha-beta pruning, `depth` plies from here.
fn minimax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
    ai_color: Cell,
) -> i32 {
    if depth <= 0 || board.is_game_over() {
        return evaluate(board, ai_color);
    }

    let current_color = if maximizing {
        ai_color
    } else {
        ai_color.opponent()
    };

    let moves = board.legal_moves(current_color);
    if moves.is_empty() {
        // A player with no move passes; the ply is still spent, which is what
        // stops a board where both sides can pass from recursing forever.
        return minimax(
            board,
            depth.saturating_sub(1),
            alpha,
            beta,
            !maximizing,
            ai_color,
        );
    }

    let mut best = if maximizing { i32::MIN } else { i32::MAX };
    for mv in &moves {
        let mut next = board.clone();
        next.make_move(*mv, current_color);
        let val = minimax(
            &next,
            depth.saturating_sub(1),
            alpha,
            beta,
            !maximizing,
            ai_color,
        );
        if maximizing {
            best = best.max(val);
            alpha = alpha.max(best);
        } else {
            best = best.min(val);
            beta = beta.min(best);
        }
        if alpha >= beta {
            break;
        }
    }
    best
}

/// The move the search likes best, or `None` if there is no legal move.
fn ai_best_move(board: &Board, ai_color: Cell) -> Option<Pos> {
    let mut best: Option<(i32, Pos)> = None;
    for mv in board.legal_moves(ai_color) {
        let mut next = board.clone();
        // The count is not the question here -- `legal_moves` already said the
        // move is legal -- but discarding it silently is how a caller that
        // *did* care came to ignore an illegal move. Naming it says which.
        let _flipped = next.make_move(mv, ai_color);
        let score = minimax(
            &next,
            AI_DEPTH.saturating_sub(1),
            i32::MIN,
            i32::MAX,
            false,
            ai_color,
        );
        if best.is_none_or(|(seen, _)| score > seen) {
            best = Some((score, mv));
        }
    }
    best.map(|(_, mv)| mv)
}

// ── Game state ──────────────────────────────────────────────────────

/// The game phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Playing,
    GameOver,
}

/// A record of a single move.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MoveRecord {
    pos: Pos,
    color: Cell,
    flipped: i32,
}

impl MoveRecord {
    /// Othello notation: column letter, row number, and the count turned.
    fn notation(&self) -> String {
        let color_str = match self.color {
            Cell::Black => "B",
            Cell::White => "W",
            Cell::Empty => "?",
        };
        format!(
            "{color_str}:{}{}(+{})",
            column_letter(self.pos.col),
            self.pos.row.saturating_add(1),
            self.flipped
        )
    }
}

/// The `a`-`h` that names a column, or `?` for a column off the board.
fn column_letter(col: i32) -> char {
    u8::try_from(col)
        .ok()
        .and_then(|c| b'a'.checked_add(c))
        .filter(|_| (0..SIDE).contains(&col))
        .map_or('?', char::from)
}

/// Main application state.
struct ReversiApp {
    board: Board,
    current_turn: Cell,
    phase: Phase,
    cursor: Pos,
    last_move: Option<Pos>,
    move_history: Vec<MoveRecord>,
    /// Something out of the ordinary to say instead of the standing status
    /// line: an illegal move, or a player who had to pass.
    ///
    /// The old code kept a `message: String` that every turn-handling branch
    /// wrote to in turn, and the last writer won. Two of the lines it composed
    /// -- the pass notice and "AI thinking (White)..." -- were overwritten
    /// before any frame could be drawn, so no player ever saw either of them.
    /// Deriving the standing line and keeping only the exception fixes that at
    /// the root: there is nothing left to overwrite.
    notice: Option<String>,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size: (f32, f32),
}

impl ReversiApp {
    fn new() -> Self {
        Self {
            board: Board::new(),
            current_turn: Cell::Black, // Black, the human, opens.
            phase: Phase::Playing,
            cursor: Pos::new(3, 3),
            last_move: None,
            move_history: Vec::new(),
            notice: None,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// Deal a new game without forgetting how big the window is.
    ///
    /// `*self = Self::new()` was the whole of the old new-game handler, and
    /// with the window size now living in the state that would have snapped
    /// the board back to its opening size on every new game.
    fn restart(&mut self) {
        let size = self.size;
        *self = Self::new();
        self.size = size;
    }

    /// Note the size a frame was drawn at.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: Key) -> EventResult {
        match self.phase {
            Phase::Playing => self.handle_playing_key(key),
            Phase::GameOver => self.handle_game_over_key(key),
        }
    }

    /// Keys during play.
    fn handle_playing_key(&mut self, key: Key) -> EventResult {
        // The AI answers inside the same event that provokes it, so it is
        // always the human's turn by the time a key arrives. The guard stays
        // because "always" here is a property of `do_ai_move`, not of the
        // keyboard.
        if self.current_turn != Cell::Black {
            return EventResult::Ignored;
        }

        match key {
            Key::Up => self.move_cursor(-1, 0),
            Key::Down => self.move_cursor(1, 0),
            Key::Left => self.move_cursor(0, -1),
            Key::Right => self.move_cursor(0, 1),
            Key::Enter | Key::Space => {
                self.try_place_piece();
                EventResult::Consumed
            }
            Key::N => {
                self.restart();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Keys once the game is over.
    fn handle_game_over_key(&mut self, key: Key) -> EventResult {
        if key == Key::N || key == Key::Enter {
            self.restart();
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Walk the cursor, and refuse to walk it off the board.
    ///
    /// The old spelling put the bound in the match arm's guard
    /// (`Key::Up if self.cursor_row > 0`), which made a press at the edge fall
    /// through to the catch-all and read as a key the game has no use for.
    /// Four copies of one bound, and a keypress that meant two things.
    fn move_cursor(&mut self, dr: i32, dc: i32) -> EventResult {
        let want = Pos::new(
            self.cursor.row.saturating_add(dr),
            self.cursor.col.saturating_add(dc),
        );
        if want.in_bounds() && want != self.cursor {
            self.cursor = want;
            EventResult::Consumed
        } else {
            // The key was for us and we could not act on it; the window still
            // has no reason to redraw.
            EventResult::Ignored
        }
    }

    /// Try to play the square under the cursor.
    fn try_place_piece(&mut self) {
        self.notice = None;
        let pos = self.cursor;
        if !self.board.is_legal_move(pos, self.current_turn) {
            self.notice = Some(String::from(
                "Illegal move -- a move must flip at least one piece.",
            ));
            return;
        }

        let flipped = self.board.make_move(pos, self.current_turn);
        self.last_move = Some(pos);
        self.move_history.push(MoveRecord {
            pos,
            color: self.current_turn,
            flipped,
        });
        self.advance_turn();
    }

    /// Hand the turn on, passing or ending the game as the rules require.
    ///
    /// The old version kept a `pass_count`, incremented it, and tested it
    /// against 2 -- a value it could not reach, since every path into this
    /// function had just reset it to 0. The counter was read nowhere else, so
    /// the whole field was a guard for a case that could not arise. What
    /// actually ends the game is that neither player can move, which is what
    /// is asked here.
    fn advance_turn(&mut self) {
        let next = self.current_turn.opponent();

        if self.board.has_legal_move(next) {
            self.current_turn = next;
        } else if self.board.has_legal_move(self.current_turn) {
            self.notice = Some(format!(
                "{} cannot move -- {} plays again.",
                color_name(next),
                color_name(self.current_turn)
            ));
        } else {
            self.phase = Phase::GameOver;
            return;
        }

        if self.current_turn == Cell::White {
            self.do_ai_move();
        }
    }

    /// Play White's reply, and keep playing while Black has no answer.
    ///
    /// A loop rather than the recursion it replaces: a run of forced passes
    /// can be sixty plies long near the end of a game, and a stack that deep
    /// is a stack that can be made deeper by a board rather than by a bug.
    fn do_ai_move(&mut self) {
        loop {
            let Some(mv) = ai_best_move(&self.board, Cell::White) else {
                // Every caller checks `has_legal_move(White)` first, so this
                // is the search failing rather than the rules. Handing the
                // turn back is the only answer that leaves the game playable:
                // the old code returned silently and left White to move
                // forever, with no key or click that could unstick it.
                self.current_turn = Cell::Black;
                self.notice = Some(String::from("White did not move."));
                return;
            };

            let flipped = self.board.make_move(mv, Cell::White);
            self.last_move = Some(mv);
            self.move_history.push(MoveRecord {
                pos: mv,
                color: Cell::White,
                flipped,
            });
            self.current_turn = Cell::Black;

            if self.board.has_legal_move(Cell::Black) {
                return;
            }
            if !self.board.has_legal_move(Cell::White) {
                self.phase = Phase::GameOver;
                return;
            }
            self.notice = Some(String::from("Black cannot move -- White plays again."));
            self.current_turn = Cell::White;
        }
    }

    /// The line along the bottom of the window.
    ///
    /// Derived rather than stored. Everything in it -- whose turn it is, the
    /// score, the result -- is already on the board, so a copy kept in a field
    /// could only be a copy that went stale.
    fn status(&self) -> String {
        if self.phase == Phase::GameOver {
            return self.game_over_message();
        }
        if let Some(notice) = &self.notice {
            return notice.clone();
        }
        let black = self.board.count(Cell::Black);
        let white = self.board.count(Cell::White);
        format!("Your turn (Black). B:{black} W:{white}")
    }

    /// What the bottom line says once neither player can move.
    fn game_over_message(&self) -> String {
        let black = self.board.count(Cell::Black);
        let white = self.board.count(Cell::White);
        let result = match self.board.winner() {
            Cell::Black => "Black wins!",
            Cell::White => "White wins!",
            Cell::Empty => "It's a tie!",
        };
        format!("Game Over! {result} (B:{black} W:{white}) Press N for a new game.")
    }

    /// Act on a click, by asking the frame what was drawn there.
    ///
    /// An 8x8 board is the most click-natural thing a program can put on a
    /// screen. This one *was* clickable, but against arithmetic of its own
    /// rather than against anything the drawing pass had recorded, so the
    /// squares it answered were only ever accidentally the squares on screen.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(x, y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Square(row, col)
                if self.phase == Phase::Playing && self.current_turn == Cell::Black =>
            {
                // One action rather than two: the cursor goes to the square
                // and the move is attempted, exactly as arrows-then-Enter
                // would have done. An illegal square is refused with the same
                // notice the keys get, so a mis-click costs nothing.
                self.cursor = Pos::new(i32::from(row), i32::from(col));
                self.try_place_piece();
                EventResult::Consumed
            }
            // Every other box is answered and does nothing. A click on the
            // board once the game is over must not fall through to the
            // window, which would treat it as a click on nothing at all.
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
        // over its neighbours.
        f.clip(l.window);
        self.draw_header(&l, &mut f);
        self.draw_board(&l, &mut f);
        self.draw_panel(&l, &mut f);
        self.draw_status(&l, &mut f);
        f.unclip();
        f
    }

    /// The title, and the two score chips beside it.
    fn draw_header(&self, l: &Layout, f: &mut Frame<Target>) {
        let band = inset(l.header, l.pad);
        let title_ink = Ink::new(l.title, FontWeightHint::Bold, LAVENDER);
        let title = label_in(f, band, "Reversi", title_ink);
        f.hit(Target::Title, title);

        // The chips follow the title's *measured* width. They used to sit at
        // `BOARD_OFFSET_X + 120.0` and `+ 180.0`, two numbers that were right
        // for one font at one size and silently overlapped the title at any
        // other.
        let ink = Ink::new(l.font, FontWeightHint::Bold, TEXT_COLOR);
        let gap = l.pad * 1.5;
        let mut x = title.right() + gap;
        for (target, text) in [
            (
                Target::BlackScore,
                format!("\u{25CF} {}", self.board.count(Cell::Black)),
            ),
            (
                Target::WhiteScore,
                format!("\u{25CB} {}", self.board.count(Cell::White)),
            ),
        ] {
            let width = ink.width(&text);
            let area = Rect::new(x, band.y, width, band.h);
            let drawn = label_in(f, area, &text, ink);
            f.hit(target, drawn);
            x = area.right() + gap;
        }
    }

    /// The board: its border, its squares, the pieces on them, and its a-h
    /// and 1-8.
    fn draw_board(&self, l: &Layout, f: &mut Frame<Target>) {
        let g = Grid::fit(inset(l.board_area, l.pad), l.small);
        let board = g.board_rect();

        let edge = (g.step * 0.06).max(1.0);
        f.push(RenderCommand::FillRect {
            x: board.x - edge,
            y: board.y - edge,
            width: board.w + edge * 2.0,
            height: board.h + edge * 2.0,
            color: BOARD_BORDER,
            corner_radii: CornerRadii::all(edge),
        });
        // Recorded before the squares so a click that lands between them --
        // there is nothing between them, but the frame answers with whatever
        // was recorded last -- reaches the board rather than the window.
        f.hit(Target::Board, board);

        let legal = if self.phase == Phase::Playing {
            self.board.legal_moves(self.current_turn)
        } else {
            Vec::new()
        };

        for pos in all_positions() {
            let square = g.square(pos.row, pos.col);
            let shade = if (pos.row.saturating_add(pos.col)) % 2 == 0 {
                BOARD_GREEN
            } else {
                BOARD_GREEN_LIGHT
            };
            f.push(RenderCommand::FillRect {
                x: square.x,
                y: square.y,
                width: square.w,
                height: square.h,
                color: shade,
                corner_radii: CornerRadii::ZERO,
            });

            if self.last_move == Some(pos) {
                f.push(RenderCommand::FillRect {
                    x: square.x,
                    y: square.y,
                    width: square.w,
                    height: square.h,
                    color: LAST_MOVE_HIGHLIGHT,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            if pos == self.cursor && self.phase == Phase::Playing {
                let ring = (g.step * 0.05).max(1.0);
                f.push(RenderCommand::StrokeRect {
                    x: square.x + ring,
                    y: square.y + ring,
                    width: (square.w - ring * 2.0).max(0.0),
                    height: (square.h - ring * 2.0).max(0.0),
                    color: CURSOR_COLOR,
                    line_width: ring,
                    corner_radii: CornerRadii::all(ring),
                });
            }

            let cell = self.board.get(pos);
            if cell.is_piece() {
                let (cx, cy) = g.centre(pos.row, pos.col);
                draw_piece(f, cx, cy, g.step * 0.37, cell);
            } else if self.phase == Phase::Playing
                && self.current_turn == Cell::Black
                && legal.contains(&pos)
            {
                let (cx, cy) = g.centre(pos.row, pos.col);
                let r = g.step * 0.1;
                f.push(RenderCommand::FillRect {
                    x: cx - r,
                    y: cy - r,
                    width: r * 2.0,
                    height: r * 2.0,
                    color: VALID_MOVE_DOT,
                    corner_radii: CornerRadii::all(r),
                });
            }

            f.push(RenderCommand::StrokeRect {
                x: square.x,
                y: square.y,
                width: square.w,
                height: square.h,
                color: BOARD_BORDER,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
            f.hit(Target::Square(byte(pos.row), byte(pos.col)), square);
        }

        // Row 0 is the top of the window and also rank *1*: Othello numbers
        // its ranks downward from the top, so `a1` is the upper-left square.
        // Chess and go both run the other way, which is why
        // `the_board_is_lettered_and_numbered_the_othello_way` pins this to
        // the published rules rather than to this comment.
        let ink = Ink::new(l.small, FontWeightHint::Regular, SUBTEXT0);
        for i in 0..SIDE {
            let square = g.square(i, i);
            let letter = column_letter(i).to_string();
            let lw = ink.width(&letter);
            let drawn = label(
                f,
                square.x + (g.step - lw) / 2.0,
                board.y - g.label + (g.label - ink.height()) / 2.0,
                &letter,
                ink,
            );
            f.hit(Target::ColLabel(byte(i)), drawn);

            let number = format!("{}", i.saturating_add(1));
            let nw = ink.width(&number);
            let drawn = label(
                f,
                board.x - g.label + (g.label - nw) / 2.0,
                square.y + (g.step - ink.height()) / 2.0,
                &number,
                ink,
            );
            f.hit(Target::RowLabel(byte(i)), drawn);
        }
    }

    /// The side panel: turn, score bar, counts, last move, history, help.
    ///
    /// Every line is placed by walking a cursor down the panel, so the panel
    /// says as much as it has room for and no more. The old spelling put each
    /// line at a hand-counted offset from the panel's top -- `+ 55.0`,
    /// `+ 72.0`, `+ 110.0`, `+ 155.0`, `+ 205.0` -- and drew a fixed twelve
    /// history rows whatever the window's height, which a short window ran
    /// straight through its own help text.
    fn draw_panel(&self, l: &Layout, f: &mut Frame<Target>) {
        let band = inset(l.panel, l.pad);
        f.push(RenderCommand::FillRect {
            x: band.x,
            y: band.y,
            width: band.w,
            height: band.h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(l.pad),
        });
        f.hit(Target::Panel, band);

        let inner = inset(band, l.pad);
        let label_ink = Ink::new(l.small, FontWeightHint::Bold, SUBTEXT0);
        let body_ink = Ink::new(l.font, FontWeightHint::Regular, TEXT_COLOR);
        let mut y = inner.y;

        // Whose turn.
        let (turn_text, turn_color) = match (self.phase, self.current_turn) {
            (Phase::GameOver, _) => ("Game Over", RED),
            (Phase::Playing, Cell::White) => ("White to move", PEACH),
            (Phase::Playing, _) => ("Your turn (Black)", BLUE),
        };
        let drawn = panel_row(
            f,
            inner,
            &mut y,
            turn_text,
            Ink::new(l.font, FontWeightHint::Bold, turn_color),
        );
        f.hit(Target::Turn, drawn);
        y += l.small * 0.6;

        // The score, as a bar split in proportion to the two counts.
        panel_row(f, inner, &mut y, "Score", label_ink);
        let black = self.board.count(Cell::Black);
        let white = self.board.count(Cell::White);
        let bar_h = l.font * 1.4;
        let bar = Rect::new(inner.x, y, inner.w, bar_h);
        f.push(RenderCommand::FillRect {
            x: bar.x,
            y: bar.y,
            width: bar.w,
            height: bar.h,
            color: SURFACE1,
            corner_radii: CornerRadii::all(bar_h / 4.0),
        });
        let total = f32_from_i32(black.saturating_add(white)).max(1.0);
        let black_w = f32_from_i32(black) / total * bar.w;
        if black > 0 {
            f.push(RenderCommand::FillRect {
                x: bar.x,
                y: bar.y,
                width: black_w,
                height: bar.h,
                color: BLACK_PIECE,
                corner_radii: CornerRadii {
                    top_left: bar_h / 4.0,
                    bottom_left: bar_h / 4.0,
                    top_right: if white == 0 { bar_h / 4.0 } else { 0.0 },
                    bottom_right: if white == 0 { bar_h / 4.0 } else { 0.0 },
                },
            });
        }
        let chip = Ink::new(l.small, FontWeightHint::Bold, TEXT_COLOR);
        let b_text = format!("B: {black}");
        let w_text = format!("W: {white}");
        let chip_y = bar.y + (bar.h - chip.height()) / 2.0;
        label(f, bar.x + l.pad / 2.0, chip_y, &b_text, chip);
        // Right-aligned by measurement rather than by `px + 160.0`, which was
        // a number chosen for one font size and wrong at every other.
        label(
            f,
            (bar.right() - l.pad / 2.0 - chip.width(&w_text)).max(bar.x),
            chip_y,
            &w_text,
            chip,
        );
        f.hit(Target::ScoreBar, bar);
        y = bar.bottom() + l.small * 0.6;

        let drawn = panel_row(
            f,
            inner,
            &mut y,
            &format!("Moves: {}", self.move_history.len()),
            body_ink,
        );
        f.hit(Target::Moves, drawn);
        let drawn = panel_row(
            f,
            inner,
            &mut y,
            &format!("Empty: {}", self.board.empty_count()),
            body_ink,
        );
        f.hit(Target::EmptyCount, drawn);
        y += l.small * 0.6;

        if let Some(last) = self.move_history.last() {
            panel_row(f, inner, &mut y, "Last Move", label_ink);
            let drawn = panel_row(
                f,
                inner,
                &mut y,
                &last.notation(),
                Ink::new(l.font, FontWeightHint::Regular, PEACH),
            );
            f.hit(Target::LastMove, drawn);
            y += l.small * 0.6;
        }

        // The help sits on the floor of the panel, and the history fills
        // whatever is between the cursor and it -- so the two cannot collide
        // however long the game runs or however short the window is.
        let help_ink = Ink::new(l.small, FontWeightHint::Regular, OVERLAY0);
        let help_h = help_ink.height() * 2.0;
        let help_top = (inner.bottom() - help_h).max(y);

        let heading = panel_row(f, inner, &mut y, "History", label_ink);
        let row_ink = Ink::new(l.small, FontWeightHint::Regular, TEXT_COLOR);
        let rows = count_from_f32((help_top - y) / row_ink.height());
        let start = self.move_history.len().saturating_sub(rows);
        let mut history_box = Rect::new(heading.x, heading.y, heading.w, heading.h);
        for (idx, record) in self.move_history.iter().enumerate().skip(start) {
            let ink = Ink::new(
                l.small,
                FontWeightHint::Regular,
                if record.color == Cell::Black {
                    BLUE
                } else {
                    PEACH
                },
            );
            let text = format!("{}. {}", idx.saturating_add(1), record.notation());
            let drawn = panel_row(f, inner, &mut y, &text, ink);
            history_box = union(history_box, drawn);
        }
        f.hit(Target::History, history_box);

        let mut help_y = help_top;
        let first = panel_row(
            f,
            inner,
            &mut help_y,
            "Arrows: move   Enter: place",
            help_ink,
        );
        let second = panel_row(
            f,
            inner,
            &mut help_y,
            "N: new game   Click: place",
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
            if self.phase == Phase::GameOver {
                PEACH
            } else {
                TEXT_COLOR
            },
        );
        let drawn = label_in(f, band, &self.status(), ink);
        f.hit(Target::Status, drawn);
    }
}

/// A disc, drawn as a fully-rounded rect.
///
/// A free function rather than a method: it read `&self` and used nothing from
/// it, which is a method only in spelling.
fn draw_piece(f: &mut Frame<Target>, cx: f32, cy: f32, radius: f32, cell: Cell) {
    let (fill, border) = match cell {
        Cell::Black => (BLACK_PIECE, BLACK_PIECE_BORDER),
        Cell::White => (WHITE_PIECE, WHITE_PIECE_BORDER),
        Cell::Empty => return,
    };
    let shadow = (radius * 0.09).max(1.0);
    f.push(RenderCommand::FillRect {
        x: cx - radius + shadow,
        y: cy - radius + shadow,
        width: radius * 2.0,
        height: radius * 2.0,
        color: Color::rgba(0, 0, 0, 40),
        corner_radii: CornerRadii::all(radius),
    });
    f.push(RenderCommand::FillRect {
        x: cx - radius,
        y: cy - radius,
        width: radius * 2.0,
        height: radius * 2.0,
        color: fill,
        corner_radii: CornerRadii::all(radius),
    });
    f.push(RenderCommand::StrokeRect {
        x: cx - radius,
        y: cy - radius,
        width: radius * 2.0,
        height: radius * 2.0,
        color: border,
        line_width: (radius * 0.07).max(1.0),
        corner_radii: CornerRadii::all(radius),
    });
}

/// A display name for a colour.
fn color_name(cell: Cell) -> &'static str {
    match cell {
        Cell::Black => "Black",
        Cell::White => "White",
        Cell::Empty => "None",
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
fn panel_row(f: &mut Frame<Target>, band: Rect, y: &mut f32, s: &str, ink: Ink) -> Rect {
    let h = ink.height();
    let drawn = label_in(f, Rect::new(band.x, *y, band.w, h), s, ink);
    *y += h;
    drawn
}

/// The smallest box holding both, ignoring an empty one.
///
/// A section's hit box is its heading plus its rows, and a section with no
/// rows is its heading alone.
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

// ── The window ──────────────────────────────────────────────────────

/// The one body every event goes through, whichever side it arrives from.
///
/// The window calls it and the tests call it, so a key the tests prove works
/// is the same key the window delivers.
fn handle_event(app: &mut ReversiApp, event: &Event) -> EventResult {
    match event {
        Event::Key(KeyEvent {
            key, pressed: true, ..
        }) => app.handle_key(*key),
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

impl App for ReversiApp {
    fn title(&self) -> String {
        "Reversi".to_string()
    }

    fn app_id(&self) -> String {
        "reversi".to_string()
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
        if matches!(
            event,
            Event::Key(KeyEvent {
                key: Key::Escape,
                pressed: true,
                ..
            })
        ) {
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

impl Probe for ReversiApp {
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
    let mut game = ReversiApp::new();
    app::launch("reversi", &mut game)
}
