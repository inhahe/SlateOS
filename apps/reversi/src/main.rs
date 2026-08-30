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
        // A player with no move passes, and the pass costs a ply exactly as a
        // move would -- so a search told to look four deep looks four deep
        // whether those four are moves or passes.
        //
        // It is *not* what stops the recursion: a board where both sides can
        // pass is a board where neither can move, which `is_game_over` above
        // has already returned on. The comment here used to claim otherwise,
        // and the mutation sweep found the claim out -- leaving the ply
        // un-spent changes the value the search returns but cannot make it
        // run forever.
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

        // Not gated on the phase: the dot is only drawn where the phase is
        // checked anyway, a few lines down, and a second copy of that check
        // here was a guard the mutation sweep could break without any test
        // noticing -- because breaking it changed nothing.
        let legal = self.board.legal_moves(self.current_turn);

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
        if black > 0 {
            // The division is inside the guard, not defended by a `.max(1.0)`
            // outside it: `black > 0` already means the total is at least one,
            // so the floor could never fire, and a defence that cannot fire is
            // a defence nothing can prove is there.
            let total = f32_from_i32(black.saturating_add(white));
            let black_w = f32_from_i32(black) / total * bar.w;
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

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    reason = "a test that panics on bad data is a test that failed"
)]
mod tests {
    use super::*;
    use guitk::probe;

    /// The sizes every geometry test is run at.
    ///
    /// The lopsided ones are the point: a board fitted to the width alone
    /// passes at 900x640 and runs off the bottom at 400x900, which is exactly
    /// the fault the old fixed layout had.
    const SIZES: [(f32, f32); 8] = [
        (900.0, 640.0),
        (640.0, 480.0),
        (1600.0, 1000.0),
        (400.0, 900.0),
        (1200.0, 400.0),
        (320.0, 240.0),
        (200.0, 200.0),
        (60.0, 60.0),
    ];

    /// A board holding only the pieces named, on otherwise empty squares.
    fn board_with(pieces: &[(i32, i32, Cell)]) -> Board {
        let mut board = Board::new();
        for pos in all_positions() {
            board.set(pos, Cell::Empty);
        }
        for &(row, col, cell) in pieces {
            board.set(Pos { row, col }, cell);
        }
        board
    }

    /// The box the drawing pass recorded for `target`, or a panic naming it.
    fn box_of(app: &ReversiApp, target: Target) -> Rect {
        box_of_sized(app, target, NATURAL)
    }

    /// [`box_of`] at a window size other than the natural one.
    ///
    /// Which is not a convenience: `probe::rect_of` reads the frame at
    /// `Probe::SIZE` whatever size the caller last drew at, so a test that
    /// draws a 900x400 window and then asks `box_of` for a box is comparing
    /// text from one window against boxes from another. That is exactly how
    /// `the_history_shows_the_newest_moves_and_never_runs_into_the_help` came
    /// to read an empty string out of a panel that had six lines in it.
    fn box_of_sized(app: &ReversiApp, target: Target, size: (f32, f32)) -> Rect {
        probe::rect_of_sized(app, target, size).unwrap_or_else(|| {
            panic!(
                "{target:?} was never drawn a hit box at {}x{}",
                size.0, size.1
            )
        })
    }

    /// Every string the frame drew, with where it drew it.
    fn texts(frame: &Frame<Target>) -> Vec<(String, (f32, f32))> {
        frame
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::Text { x, y, text, .. } => Some((text.clone(), (*x, *y))),
                _ => None,
            })
            .collect()
    }

    /// Whether any string the frame drew contains `needle`.
    fn drew(frame: &Frame<Target>, needle: &str) -> bool {
        texts(frame).iter().any(|(t, _)| t.contains(needle))
    }

    /// Every string drawn with its origin inside `rect`, joined by newlines.
    fn text_in(frame: &Frame<Target>, rect: Rect) -> String {
        texts(frame)
            .into_iter()
            .filter(|(_, (x, y))| rect.contains(*x, *y))
            .map(|(t, _)| t)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The colours of every filled rect whose centre lies inside `rect`.
    fn fills_at(frame: &Frame<Target>, rect: Rect) -> Vec<Color> {
        frame
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => rect
                    .contains(x + width / 2.0, y + height / 2.0)
                    .then_some(*color),
                _ => None,
            })
            .collect()
    }

    /// The size the window opens at, which is the size the probe helpers use.
    const NATURAL: (f32, f32) = <ReversiApp as Probe>::SIZE;

    /// A bare key press.
    fn press(key: Key) -> KeyEvent {
        probe::press(key)
    }

    // ── The board's dimensions ──────────────────────────────────────

    #[test]
    fn the_two_spellings_of_the_board_size_agree() {
        assert_eq!(
            usize::try_from(SIDE).unwrap(),
            BOARD_SIZE,
            "the signed and unsigned spellings of the board size have drifted"
        );
        assert_eq!(LAST, SIDE - 1, "the last index is one short of the side");
        assert_eq!(POSITION_WEIGHTS.len(), BOARD_SIZE, "one weight row a row");
        for row in POSITION_WEIGHTS {
            assert_eq!(row.len(), BOARD_SIZE, "one weight a square");
        }
        assert_eq!(
            all_positions().count(),
            BOARD_SIZE * BOARD_SIZE,
            "every square is walked exactly once"
        );
    }

    // ── Layout ──────────────────────────────────────────────────────

    #[test]
    fn the_bands_run_down_the_window_in_order_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert_eq!(l.header.y, 0.0, "the header starts at the top at {w}x{h}");
            assert!(
                (l.header.bottom() - l.board_area.y).abs() < 0.01,
                "the body starts where the header ends at {w}x{h}"
            );
            assert!(
                (l.board_area.bottom() - l.status.y).abs() < 0.01,
                "the status bar starts where the body ends at {w}x{h}"
            );
            assert!(
                (l.status.bottom() - h).abs() < 0.01,
                "the status bar ends at the bottom of the window at {w}x{h}"
            );
        }
    }

    #[test]
    fn the_panel_sits_beside_the_board_and_neither_leaves_the_window() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert!(
                (l.board_area.right() - l.panel.x).abs() < 0.01,
                "the panel starts where the board area ends at {w}x{h}"
            );
            assert!(
                l.panel.right() <= w + 0.01,
                "the panel ends inside the window at {w}x{h}"
            );
            assert!(
                (l.panel.y - l.board_area.y).abs() < 0.01
                    && (l.panel.h - l.board_area.h).abs() < 0.01,
                "the panel is the full height of the body at {w}x{h}"
            );
        }
    }

    #[test]
    fn the_bands_are_shares_of_the_height_not_the_width() {
        let square = Layout::solve(600.0, 600.0);
        let wide = Layout::solve(1800.0, 600.0);
        assert!(
            (square.header.h - wide.header.h).abs() < 0.01,
            "tripling the width moved the header's height"
        );
        assert!(
            (square.status.h - wide.status.h).abs() < 0.01,
            "tripling the width moved the status bar's height"
        );
        let tall = Layout::solve(600.0, 1200.0);
        assert!(
            tall.header.h > square.header.h + 1.0,
            "doubling the height did not grow the header"
        );
    }

    #[test]
    fn the_status_bar_takes_its_share_of_what_the_header_left() {
        // Not of the whole window. Both spellings still sum to the height --
        // the body absorbs the difference -- so the bands stay in order and
        // nothing overruns either way, which is why the ordering test cannot
        // see the difference and this one has to.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert!(
                (l.status.h - (h - l.header.h) * 0.08).abs() < 0.01,
                "the status bar is {} of a {w}x{h} window, not 8% of the {} the header left",
                l.status.h,
                h - l.header.h
            );
        }
    }

    #[test]
    fn the_padding_never_vanishes_never_runs_away_and_never_outgrows_the_window() {
        assert_eq!(
            Layout::solve(60.0, 60.0).pad,
            2.0,
            "a tiny window still gets a whole pixel or two of margin"
        );
        assert_eq!(
            Layout::solve(4000.0, 3000.0).pad,
            20.0,
            "a 4K window is not margined by sixty pixels"
        );
        assert_eq!(
            Layout::solve(3.0, 3.0).pad,
            1.5,
            "a window narrower than twice its margin is not margined out of existence"
        );
    }

    #[test]
    fn the_panel_width_has_a_floor_a_ceiling_and_a_cap() {
        assert_eq!(
            Layout::solve(300.0, 600.0).panel.w,
            110.0,
            "the panel is legible on a narrow window"
        );
        assert_eq!(
            Layout::solve(2000.0, 600.0).panel.w,
            260.0,
            "a wide window does not hand a quarter of itself to six short lines"
        );
        assert_eq!(
            Layout::solve(100.0, 600.0).panel.w,
            50.0,
            "the panel is never wider than half the window"
        );
    }

    // ── Board geometry ──────────────────────────────────────────────

    #[test]
    fn the_board_fits_the_room_it_was_given_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let grid = Grid::fit(l.board_area, l.small);
            let board = grid.board_rect();
            assert!(
                board.x >= l.board_area.x - 0.01 && board.y >= l.board_area.y - 0.01,
                "the board starts inside its area at {w}x{h}"
            );
            assert!(
                board.right() <= l.board_area.right() + 0.01,
                "the board ends inside its area at {w}x{h}: {board:?} in {:?}",
                l.board_area
            );
            assert!(
                board.bottom() <= l.board_area.bottom() + 0.01,
                "the board runs off the bottom at {w}x{h}: {board:?} in {:?}",
                l.board_area
            );
        }
    }

    #[test]
    fn a_square_is_as_wide_as_it_is_tall() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let grid = Grid::fit(l.board_area, l.small);
            let sq = grid.square(3, 5);
            assert!(
                (sq.w - sq.h).abs() < 0.01,
                "a square came out {}x{} at {w}x{h}",
                sq.w,
                sq.h
            );
        }
    }

    #[test]
    fn the_squares_tile_the_board_edge_to_edge() {
        let l = Layout::solve(900.0, 640.0);
        let grid = Grid::fit(l.board_area, l.small);
        for row in 0..SIDE {
            for col in 0..LAST {
                let here = grid.square(row, col);
                let next = grid.square(row, col + 1);
                assert!(
                    (here.right() - next.x).abs() < 0.01,
                    "a gap between columns {col} and {} of row {row}",
                    col + 1
                );
            }
        }
        for col in 0..SIDE {
            for row in 0..LAST {
                let here = grid.square(row, col);
                let next = grid.square(row + 1, col);
                assert!(
                    (here.bottom() - next.y).abs() < 0.01,
                    "a gap between rows {row} and {} of column {col}",
                    row + 1
                );
            }
        }
        let board = grid.board_rect();
        let first = grid.square(0, 0);
        let last = grid.square(LAST, LAST);
        assert!(
            (board.x - first.x).abs() < 0.01 && (board.y - first.y).abs() < 0.01,
            "the board starts where its first square does"
        );
        assert!(
            (board.right() - last.right()).abs() < 0.01
                && (board.bottom() - last.bottom()).abs() < 0.01,
            "the board ends where its last square does"
        );
    }

    #[test]
    fn the_centre_of_a_square_is_its_middle() {
        let l = Layout::solve(900.0, 640.0);
        let grid = Grid::fit(l.board_area, l.small);
        for pos in all_positions() {
            let sq = grid.square(pos.row, pos.col);
            let (cx, cy) = grid.centre(pos.row, pos.col);
            assert!(
                (cx - (sq.x + sq.w / 2.0)).abs() < 0.01 && (cy - (sq.y + sq.h / 2.0)).abs() < 0.01,
                "the centre of {pos:?} is not its middle"
            );
        }
    }

    // ── The rules ───────────────────────────────────────────────────

    #[test]
    fn the_opening_position_is_the_standard_one() {
        let board = Board::new();
        assert_eq!(board.get(Pos::new(3, 3)), Cell::White, "d4 is White");
        assert_eq!(board.get(Pos::new(4, 4)), Cell::White, "e5 is White");
        assert_eq!(board.get(Pos::new(3, 4)), Cell::Black, "e4 is Black");
        assert_eq!(board.get(Pos::new(4, 3)), Cell::Black, "d5 is Black");
        assert_eq!(board.count(Cell::Black), 2, "two black pieces to open");
        assert_eq!(board.count(Cell::White), 2, "two white pieces to open");
        assert_eq!(board.empty_count(), 60, "sixty squares still empty");
    }

    #[test]
    fn the_empty_count_and_the_two_colours_account_for_every_square() {
        let mut app = ReversiApp::new();
        for _ in 0..6 {
            let Some(mv) = app.board.legal_moves(app.current_turn).first().copied() else {
                break;
            };
            app.cursor = mv;
            app.try_place_piece();
            let total = app.board.count(Cell::Black)
                + app.board.count(Cell::White)
                + app.board.empty_count();
            assert_eq!(
                total,
                i32::try_from(BOARD_SIZE * BOARD_SIZE).unwrap(),
                "the three counts stopped summing to the board"
            );
        }
    }

    #[test]
    fn the_opening_offers_black_exactly_four_moves() {
        let mut moves = Board::new().legal_moves(Cell::Black);
        moves.sort_by_key(|p| (p.row, p.col));
        assert_eq!(
            moves,
            vec![
                Pos::new(2, 3),
                Pos::new(3, 2),
                Pos::new(4, 5),
                Pos::new(5, 4)
            ],
            "the four openings of every game of Othello"
        );
    }

    #[test]
    fn a_flank_needs_ones_own_piece_to_close_it() {
        let closed = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
        ]);
        assert_eq!(
            closed.get_flips(Pos::new(0, 3), Cell::Black).len(),
            2,
            "a run of two whites closed by a black is flanked"
        );

        let open = board_with(&[(0, 1, Cell::White), (0, 2, Cell::White)]);
        assert!(
            open.get_flips(Pos::new(0, 3), Cell::Black).is_empty(),
            "a run with nothing closing it is not flanked"
        );

        let gapped = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            // (0, 2) empty -- the gap breaks the run
            (0, 3, Cell::White),
        ]);
        assert!(
            gapped.get_flips(Pos::new(0, 4), Cell::Black).is_empty(),
            "an empty square breaks the run rather than being walked through"
        );
    }

    #[test]
    fn a_run_that_reaches_the_edge_flips_nothing() {
        // Whites all the way to the wall with no black beyond them.
        let board = board_with(&[
            (0, 0, Cell::White),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
        ]);
        assert!(
            board.get_flips(Pos::new(0, 3), Cell::Black).is_empty(),
            "the walk ran off the board and flipped anyway"
        );
        assert!(
            !board.is_legal_move(Pos::new(0, 3), Cell::Black),
            "a move that flips nothing is not legal"
        );
    }

    #[test]
    fn a_move_flips_in_every_direction_at_once() {
        let mut pieces = Vec::new();
        for &(dr, dc) in &DIRECTIONS {
            pieces.push((4 + dr, 4 + dc, Cell::White));
            pieces.push((4 + dr * 2, 4 + dc * 2, Cell::Black));
        }
        let mut board = board_with(&pieces);
        assert_eq!(
            board.get_flips(Pos::new(4, 4), Cell::Black).len(),
            8,
            "eight directions, one flip each"
        );
        assert_eq!(
            board.make_move(Pos::new(4, 4), Cell::Black),
            8,
            "playing it turns all eight"
        );
        for &(dr, dc) in &DIRECTIONS {
            assert_eq!(
                board.get(Pos::new(4 + dr, 4 + dc)),
                Cell::Black,
                "the piece at ({dr}, {dc}) did not turn"
            );
        }
        assert_eq!(
            board.get(Pos::new(4, 4)),
            Cell::Black,
            "the played square is the mover's"
        );
    }

    #[test]
    fn an_occupied_or_off_board_square_is_never_legal() {
        let board = Board::new();
        assert!(
            !board.is_legal_move(Pos::new(3, 3), Cell::Black),
            "a square already holding a piece is not playable"
        );
        // (3, 3) alone cannot show the occupancy check is there: it flanks
        // nothing on the opening board, so it is refused twice over and
        // deleting one refusal changes no answer. This board is the case that
        // needs the check -- a1 is Black's, b1 and c1 are White's, and d1 is
        // White's too, so d1 *would* flank the pair if only it were empty.
        let flanking = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            (0, 2, Cell::White),
            (0, 3, Cell::White),
        ]);
        assert!(
            !flanking.is_legal_move(Pos::new(0, 3), Cell::Black),
            "a square that would flank if it were empty is still occupied"
        );
        assert!(
            flanking.get_flips(Pos::new(0, 3), Cell::Black).is_empty(),
            "an occupied square flipped what it would have flipped when empty"
        );
        for pos in [
            Pos::new(-1, 0),
            Pos::new(0, -1),
            Pos::new(SIDE, 0),
            Pos::new(0, SIDE),
        ] {
            assert!(
                !board.is_legal_move(pos, Cell::Black),
                "{pos:?} is off the board and was called legal"
            );
            assert!(
                board.get_flips(pos, Cell::Black).is_empty(),
                "{pos:?} is off the board and flipped something"
            );
        }
    }

    #[test]
    fn an_illegal_move_changes_nothing_at_all() {
        let mut board = Board::new();
        let before = board.clone();
        assert_eq!(
            board.make_move(Pos::new(0, 0), Cell::Black),
            0,
            "a move flipping nothing turns nothing"
        );
        assert_eq!(board, before, "the board was written to anyway");
    }

    #[test]
    fn the_winner_is_whoever_has_more_and_equal_is_neither() {
        assert_eq!(
            board_with(&[(0, 0, Cell::Black)]).winner(),
            Cell::Black,
            "more black pieces is a black win"
        );
        assert_eq!(
            board_with(&[(0, 0, Cell::White)]).winner(),
            Cell::White,
            "more white pieces is a white win"
        );
        assert_eq!(
            Board::new().winner(),
            Cell::Empty,
            "two apiece is a tie, not a win"
        );
    }

    #[test]
    fn the_game_is_over_only_when_neither_side_can_move() {
        assert!(
            !Board::new().is_game_over(),
            "the opening position is not a finished game"
        );
        assert!(
            Board::empty().is_game_over(),
            "a board nobody can play on is finished"
        );
        // One side can still move: not over.
        let one_sided = board_with(&[(0, 0, Cell::Black), (0, 1, Cell::White)]);
        assert!(
            one_sided.has_legal_move(Cell::Black),
            "black can play (0, 2) here"
        );
        assert!(
            !one_sided.is_game_over(),
            "a game one side can still play is not over"
        );
    }

    #[test]
    fn column_letters_run_a_to_h_and_refuse_everything_else() {
        assert_eq!(column_letter(0), 'a', "the first column is a");
        assert_eq!(column_letter(LAST), 'h', "the last column is h");
        for col in [-1, SIDE, 100] {
            assert_eq!(
                column_letter(col),
                '?',
                "column {col} is off the board and was named anyway"
            );
        }
    }

    #[test]
    fn notation_names_the_square_the_way_othello_does() {
        let rec = MoveRecord {
            pos: Pos::new(0, 0),
            color: Cell::Black,
            flipped: 3,
        };
        assert_eq!(rec.notation(), "B:a1(+3)", "rows are numbered from one");
        let rec = MoveRecord {
            pos: Pos::new(LAST, LAST),
            color: Cell::White,
            flipped: 1,
        };
        assert_eq!(rec.notation(), "W:h8(+1)", "the far corner is h8");
    }

    // ── The search ──────────────────────────────────────────────────

    #[test]
    fn the_evaluation_is_exactly_the_opposite_from_the_other_side() {
        let mut app = ReversiApp::new();
        for _ in 0..4 {
            let Some(mv) = app.board.legal_moves(app.current_turn).first().copied() else {
                break;
            };
            app.cursor = mv;
            app.try_place_piece();
            assert_eq!(
                evaluate(&app.board, Cell::Black),
                -evaluate(&app.board, Cell::White),
                "a position cannot be good for both sides"
            );
        }
    }

    #[test]
    fn the_evaluation_prefers_a_corner_to_the_square_that_gives_one_away() {
        let corner = board_with(&[(0, 0, Cell::Black)]);
        let beside = board_with(&[(1, 1, Cell::Black)]);
        assert!(
            evaluate(&corner, Cell::Black) > evaluate(&beside, Cell::Black),
            "the corner is the square that can never be taken back"
        );
        assert!(
            weight_at(Pos::new(0, 0)).unwrap() > 0,
            "a corner is worth having"
        );
        assert!(
            weight_at(Pos::new(1, 1)).unwrap() < 0,
            "the square diagonally inside a corner hands it over"
        );
        // The corner comparison above is satisfied by the separate corner
        // bonus, so it cannot tell whether the weight table is consulted at
        // all. Two squares that are neither corners nor adjacent to one can:
        // c1 is worth having and b2 is worth avoiding, and nothing but the
        // table says so.
        let edge = board_with(&[(0, 2, Cell::Black)]);
        let trap = board_with(&[(1, 1, Cell::Black)]);
        assert!(
            evaluate(&edge, Cell::Black) > evaluate(&trap, Cell::Black),
            "the position table is not being read: {} is no better than {}",
            evaluate(&edge, Cell::Black),
            evaluate(&trap, Cell::Black)
        );
        assert_eq!(
            weight_at(Pos::new(-1, 0)),
            None,
            "off the board is unweighted"
        );
        assert_eq!(
            weight_at(Pos::new(0, SIDE)),
            None,
            "off the board is unweighted"
        );
    }

    #[test]
    fn the_search_offers_a_legal_move_or_none_at_all() {
        assert_eq!(
            ai_best_move(&Board::empty(), Cell::White),
            None,
            "a board with no legal move has no best move"
        );
        let board = Board::new();
        let mv = ai_best_move(&board, Cell::White).expect("white has moves in the opening");
        assert!(
            board.is_legal_move(mv, Cell::White),
            "the search chose {mv:?}, which is not a legal move"
        );
    }

    #[test]
    fn the_search_takes_a_corner_when_one_is_offered() {
        // a1 is free, b1 is Black's, c1 is White's: playing a1 flanks b1 and
        // wins the one square that can never be flipped back.
        let board = board_with(&[
            (0, 1, Cell::Black),
            (0, 2, Cell::White),
            (3, 3, Cell::White),
            (3, 4, Cell::Black),
            (4, 3, Cell::Black),
            (4, 4, Cell::White),
        ]);
        assert_eq!(
            ai_best_move(&board, Cell::White),
            Some(Pos::new(0, 0)),
            "the search passed up a free corner"
        );
    }

    #[test]
    fn the_search_reads_its_reply_as_the_opponents_and_not_as_its_own() {
        // One ply into a real game: Black has played c4, flipping d4. Which
        // square White likes best depends on who the search thinks moves
        // after it -- reading the reply as White's own turn picks c5 here
        // instead of e3, because a side that gets to move twice likes the
        // squares that only pay off on the second move.
        //
        // The position was found by playing games out and comparing the two
        // spellings; it is the earliest board on which they disagree, which
        // is as close to the opening as this can be pinned.
        let board = board_with(&[
            (3, 2, Cell::Black),
            (3, 3, Cell::Black),
            (3, 4, Cell::Black),
            (4, 3, Cell::Black),
            (4, 4, Cell::White),
        ]);
        assert_eq!(
            ai_best_move(&board, Cell::White),
            Some(Pos::new(2, 4)),
            "the search answered as though the reply were a second move of its own"
        );
    }

    #[test]
    fn a_pass_costs_the_search_a_ply_exactly_as_a_move_does() {
        // Black at a1, White at b1: White has nothing to play, Black has c1.
        // Told to look one ply, the search spends that ply on White's pass
        // and scores the board as it stands. A pass that cost nothing would
        // let Black play inside the one ply the search was given, and return
        // the value of a board that is one move further on.
        let board = board_with(&[(0, 0, Cell::Black), (0, 1, Cell::White)]);
        assert!(
            !board.has_legal_move(Cell::White),
            "the fixture must leave white with nothing to play"
        );
        assert!(
            board.has_legal_move(Cell::Black),
            "the fixture must leave black something to play"
        );
        assert_eq!(
            minimax(&board, 1, i32::MIN, i32::MAX, true, Cell::White),
            evaluate(&board, Cell::White),
            "the pass did not cost the ply it was given"
        );
    }

    #[test]
    fn a_side_with_no_move_passes_and_the_search_still_terminates() {
        // Both sides can pass here; the ply is spent anyway, so the recursion
        // bottoms out instead of bouncing between two players forever.
        let stuck = Board::empty();
        assert_eq!(
            minimax(&stuck, AI_DEPTH, i32::MIN, i32::MAX, true, Cell::White),
            evaluate(&stuck, Cell::White),
            "a finished board scores as itself"
        );
    }

    // ── Turns ───────────────────────────────────────────────────────

    #[test]
    fn black_opens_with_the_cursor_on_the_board() {
        let app = ReversiApp::new();
        assert_eq!(app.current_turn, Cell::Black, "the human opens");
        assert_eq!(app.phase, Phase::Playing, "a new game is not over");
        assert!(app.cursor.in_bounds(), "the cursor starts on the board");
        assert_eq!(app.last_move, None, "nothing has been played yet");
        assert!(app.move_history.is_empty(), "the history starts empty");
    }

    #[test]
    fn the_arrow_keys_walk_the_cursor_and_stop_at_the_edges() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(0, 0);
        for (key, dr, dc) in [(Key::Down, 1, 0), (Key::Right, 0, 1)] {
            let before = app.cursor;
            assert_eq!(
                app.handle_key(key),
                EventResult::Consumed,
                "{key:?} moved the cursor and said it had not"
            );
            assert_eq!(
                app.cursor,
                Pos::new(before.row + dr, before.col + dc),
                "{key:?} walked the wrong way"
            );
        }
        app.cursor = Pos::new(0, 0);
        for key in [Key::Up, Key::Left] {
            assert_eq!(
                app.handle_key(key),
                EventResult::Ignored,
                "{key:?} at the edge claimed to have done something"
            );
            assert_eq!(app.cursor, Pos::new(0, 0), "{key:?} walked off the board");
        }
        app.cursor = Pos::new(LAST, LAST);
        for key in [Key::Down, Key::Right] {
            assert_eq!(
                app.handle_key(key),
                EventResult::Ignored,
                "{key:?} at the far edge"
            );
            assert_eq!(
                app.cursor,
                Pos::new(LAST, LAST),
                "{key:?} walked off the board"
            );
        }
    }

    #[test]
    fn an_illegal_square_is_refused_with_a_notice_and_no_piece() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(0, 0);
        let before = app.board.clone();
        app.handle_key(Key::Enter);
        assert_eq!(app.board, before, "an illegal move was played anyway");
        assert_eq!(
            app.current_turn,
            Cell::Black,
            "the turn was handed on anyway"
        );
        assert!(
            app.status().contains("Illegal move"),
            "the refusal was silent: {:?}",
            app.status()
        );
    }

    #[test]
    fn a_refusal_does_not_outlive_the_move_that_answers_it() {
        // Every move clears the notice before deciding anything, so a refusal
        // is a refusal of the move being made and not of the one before it.
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(0, 0);
        app.handle_key(Key::Enter);
        assert!(
            app.status().contains("Illegal move"),
            "the fixture did not produce a refusal to leave standing"
        );
        app.cursor = Pos::new(2, 3);
        app.handle_key(Key::Enter);
        assert_eq!(
            app.notice, None,
            "the refusal was left standing over the move that answered it"
        );
        assert!(
            app.status().starts_with("Your turn (Black)."),
            "the status line still carries the old notice: {:?}",
            app.status()
        );
    }

    #[test]
    fn a_legal_square_is_played_and_the_ai_answers_in_the_same_event() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(2, 3);
        app.handle_key(Key::Enter);
        assert_eq!(
            app.current_turn,
            Cell::Black,
            "the human is not left waiting on a reply that never comes"
        );
        assert_eq!(app.move_history.len(), 2, "one move each");
        assert_eq!(app.move_history[0].color, Cell::Black, "black played first");
        assert_eq!(
            app.move_history[0].pos,
            Pos::new(2, 3),
            "the square the cursor was on"
        );
        assert_eq!(app.move_history[1].color, Cell::White, "white answered");
        assert_eq!(
            app.last_move,
            Some(app.move_history[1].pos),
            "the last move is white's reply, not black's move"
        );
    }

    #[test]
    fn a_pass_notice_lives_long_enough_to_be_read() {
        // Black plays (0, 2), flipping (0, 1). White is left with one piece at
        // (7, 6), which flanks nothing, so White must pass -- while Black can
        // still play (7, 5). The old code composed this line and then had it
        // overwritten before any frame was drawn.
        let mut app = ReversiApp::new();
        app.board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::White),
            (7, 6, Cell::White),
            (7, 7, Cell::Black),
        ]);
        app.cursor = Pos::new(0, 2);
        app.handle_key(Key::Enter);
        assert_eq!(
            app.current_turn,
            Cell::Black,
            "black plays again when white cannot move"
        );
        assert_eq!(app.phase, Phase::Playing, "one side passing is not the end");
        assert_eq!(
            app.status(),
            "White cannot move -- Black plays again.",
            "the pass notice never reached the status line"
        );
    }

    #[test]
    fn the_game_ends_when_the_move_leaves_neither_side_a_move() {
        let mut app = ReversiApp::new();
        app.board = board_with(&[(0, 0, Cell::Black), (0, 1, Cell::White)]);
        app.cursor = Pos::new(0, 2);
        app.handle_key(Key::Enter);
        assert_eq!(
            app.phase,
            Phase::GameOver,
            "nobody can move and the game ran on"
        );
        assert!(
            app.status().starts_with("Game Over!") && app.status().contains("Black wins!"),
            "the result was not announced: {:?}",
            app.status()
        );
    }

    #[test]
    fn the_result_line_names_whoever_actually_won() {
        // The only finished game the other tests reach is one Black wins, so
        // the white and tie arms of the message were read by nothing. Three
        // crafted boards run all three.
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        for (pieces, expected) in [
            (
                vec![
                    (0, 0, Cell::Black),
                    (0, 1, Cell::Black),
                    (0, 2, Cell::White),
                ],
                "Black wins!",
            ),
            (
                vec![
                    (0, 0, Cell::White),
                    (0, 1, Cell::White),
                    (0, 2, Cell::Black),
                ],
                "White wins!",
            ),
            (
                vec![(0, 0, Cell::Black), (0, 1, Cell::White)],
                "It's a tie!",
            ),
        ] {
            app.board = board_with(&pieces);
            let line = app.game_over_message();
            assert!(
                line.contains(expected),
                "a game with {pieces:?} on the board was announced as {line:?}"
            );
            assert!(
                line.contains(&format!(
                    "(B:{} W:{})",
                    app.board.count(Cell::Black),
                    app.board.count(Cell::White)
                )),
                "the result does not carry the score it was decided by: {line:?}"
            );
            assert_eq!(
                app.status(),
                line,
                "a finished game says something other than its result"
            );
        }
    }

    #[test]
    fn an_ai_that_finds_no_move_hands_the_turn_back_rather_than_freezing() {
        let mut app = ReversiApp::new();
        app.board = Board::empty();
        app.current_turn = Cell::White;
        app.do_ai_move();
        assert_eq!(
            app.current_turn,
            Cell::Black,
            "white kept a turn it could not take, and nothing could unstick it"
        );
        assert_eq!(
            app.status(),
            "White did not move.",
            "the game went quiet instead of saying so"
        );
    }

    #[test]
    fn keys_are_ignored_while_it_is_not_blacks_turn() {
        let mut app = ReversiApp::new();
        app.current_turn = Cell::White;
        let before = app.cursor;
        assert_eq!(
            app.handle_key(Key::Down),
            EventResult::Ignored,
            "the board moved under the search"
        );
        assert_eq!(app.cursor, before, "the cursor moved on white's turn");
    }

    #[test]
    fn once_it_is_over_only_a_new_game_key_does_anything() {
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        assert_eq!(
            app.handle_key(Key::Down),
            EventResult::Ignored,
            "the cursor walked around a finished game"
        );
        assert_eq!(
            app.handle_key(Key::N),
            EventResult::Consumed,
            "N deals a new game"
        );
        assert_eq!(app.phase, Phase::Playing, "N did not deal");
    }

    #[test]
    fn a_new_game_forgets_the_position_and_keeps_the_window() {
        let mut app = ReversiApp::new();
        app.resize(1234.0, 567.0);
        app.cursor = Pos::new(2, 3);
        app.handle_key(Key::Enter);
        assert!(
            !app.move_history.is_empty(),
            "the fixture did not play a move"
        );
        app.handle_key(Key::N);
        assert_eq!(
            app.board,
            Board::new(),
            "the opening position was not dealt"
        );
        assert!(
            app.move_history.is_empty(),
            "the history survived the new game"
        );
        assert_eq!(app.last_move, None, "the last move survived the new game");
        assert_eq!(
            app.size,
            (1234.0, 567.0),
            "the new game snapped the window back to its opening size"
        );
    }

    #[test]
    fn a_negative_size_is_read_as_no_size_at_all() {
        let mut app = ReversiApp::new();
        app.resize(-100.0, -1.0);
        assert_eq!(
            app.size,
            (0.0, 0.0),
            "a window cannot be narrower than nothing"
        );
    }

    // ── Drawing ─────────────────────────────────────────────────────

    #[test]
    fn every_square_is_drawn_a_box_of_its_own_at_every_size() {
        let app = ReversiApp::new();
        for (w, h) in SIZES {
            let f = app.draw((w, h));
            for pos in all_positions() {
                let target = Target::Square(byte(pos.row), byte(pos.col));
                assert!(
                    f.hits().iter().any(|(t, _)| *t == target),
                    "{target:?} was not recorded at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_frame_paints_the_whole_window_and_closes_every_clip() {
        for (w, h) in SIZES {
            let f = ReversiApp::new().draw((w, h));
            assert!(
                f.is_balanced(),
                "a clip was pushed and never popped at {w}x{h}"
            );
            let painted = f.commands().iter().any(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect { x, y, width, height, color, .. }
                        if *x == 0.0 && *y == 0.0 && *width == w && *height == h && *color == BASE
                )
            });
            assert!(painted, "the window has no background at {w}x{h}");
        }
    }

    #[test]
    fn the_hit_box_of_a_square_is_the_square_that_was_painted() {
        let app = ReversiApp::new();
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let g = Grid::fit(inset(l.board_area, l.pad), l.small);
        for pos in all_positions() {
            let drawn = g.square(pos.row, pos.col);
            let hit = box_of(&app, Target::Square(byte(pos.row), byte(pos.col)));
            assert!(
                (hit.x - drawn.x).abs() < 0.01
                    && (hit.y - drawn.y).abs() < 0.01
                    && (hit.w - drawn.w).abs() < 0.01
                    && (hit.h - drawn.h).abs() < 0.01,
                "{pos:?} is painted at {drawn:?} and clicked at {hit:?}"
            );
        }
    }

    #[test]
    fn no_two_squares_share_a_pixel() {
        let app = ReversiApp::new();
        let mut seen: Vec<Rect> = Vec::new();
        for pos in all_positions() {
            let r = box_of(&app, Target::Square(byte(pos.row), byte(pos.col)));
            for other in &seen {
                let overlap = r.intersect(*other);
                assert!(
                    overlap.is_none_or(|o| o.w < 0.01 || o.h < 0.01),
                    "{pos:?} at {r:?} overlaps {other:?}"
                );
            }
            seen.push(r);
        }
    }

    #[test]
    fn the_board_is_lettered_and_numbered_the_othello_way() {
        // a1 is the upper-left square: files run a-h left to right, ranks run
        // 1-8 *downward*. Chess and go both number upward, which is the whole
        // reason this is pinned rather than left to a comment.
        let app = ReversiApp::new();
        let a = box_of(&app, Target::ColLabel(0));
        let hh = box_of(&app, Target::ColLabel(byte(LAST)));
        assert!(a.x < hh.x, "the a file is not left of the h file");
        let one = box_of(&app, Target::RowLabel(0));
        let eight = box_of(&app, Target::RowLabel(byte(LAST)));
        assert!(one.y < eight.y, "rank 1 is not above rank 8");

        // Each label is checked against a square *off* the diagonal -- the c
        // file against c1, rank 3 against a3 -- because the diagonal is the
        // one place a board whose rows and columns have been swapped still
        // looks right. The labels themselves are placed from `square(i, i)`,
        // so pinning them to a1 pinned them to the one square that cannot
        // tell the difference.
        let f = app.draw(NATURAL);
        for i in 0..SIDE {
            let letter = column_letter(i).to_string();
            let col = box_of(&app, Target::ColLabel(byte(i)));
            let top = box_of(&app, Target::Square(0, byte(i)));
            assert_eq!(
                text_in(&f, col),
                letter,
                "column {i} is not lettered {letter}"
            );
            assert!(
                col.x >= top.x - 0.01 && col.x < top.right(),
                "the {letter} label does not sit over the {letter} file: {col:?} against {top:?}"
            );

            // `i + 1`, not `i.saturating_add(1)`: the production line the
            // sweep anchors on is spelled the second way, and an anchor that
            // matches twice is one the sweep refuses to apply.
            let number = format!("{}", i + 1);
            let row = box_of(&app, Target::RowLabel(byte(i)));
            let left = box_of(&app, Target::Square(byte(i), 0));
            assert_eq!(text_in(&f, row), number, "row {i} is not numbered {number}");
            assert!(
                row.y >= left.y - 0.01 && row.y < left.bottom(),
                "the {number} label does not sit beside rank {number}: {row:?} against {left:?}"
            );
        }
    }

    #[test]
    fn the_labels_sit_outside_the_board_not_on_it() {
        let app = ReversiApp::new();
        let board = box_of(&app, Target::Board);
        for i in 0..SIDE {
            let col = box_of(&app, Target::ColLabel(byte(i)));
            assert!(
                col.bottom() <= board.y + 0.01,
                "the {i} column letter is drawn on the board"
            );
            let row = box_of(&app, Target::RowLabel(byte(i)));
            assert!(
                row.right() <= board.x + 0.01,
                "the {i} row number is drawn on the board"
            );
        }
    }

    #[test]
    fn the_pieces_that_are_on_the_board_are_the_pieces_that_are_drawn() {
        let app = ReversiApp::new();
        let f = app.draw(NATURAL);
        for pos in all_positions() {
            let square = box_of(&app, Target::Square(byte(pos.row), byte(pos.col)));
            let fills = fills_at(&f, square);
            let cell = app.board.get(pos);
            assert_eq!(
                fills.contains(&BLACK_PIECE),
                cell == Cell::Black,
                "{pos:?} holds {cell:?} and the frame disagrees"
            );
            assert_eq!(
                fills.contains(&WHITE_PIECE),
                cell == Cell::White,
                "{pos:?} holds {cell:?} and the frame disagrees"
            );
        }
    }

    #[test]
    fn a_piece_fits_inside_the_square_it_sits_on() {
        for (w, h) in SIZES {
            let app = ReversiApp::new();
            let f = app.draw((w, h));
            let l = Layout::solve(w, h);
            let g = Grid::fit(inset(l.board_area, l.pad), l.small);
            let square = g.square(3, 3);
            let disc = f.commands().iter().find_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == WHITE_PIECE => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            });
            let Some(disc) = disc else {
                assert!(g.step < 1.0, "d4's white piece was not drawn at {w}x{h}");
                continue;
            };
            assert!(
                disc.w <= square.w + 0.01 && disc.h <= square.h + 0.01,
                "the piece at {w}x{h} is bigger than its square: {disc:?} in {square:?}"
            );
        }
    }

    #[test]
    fn the_legal_squares_are_dotted_and_only_they_are() {
        let app = ReversiApp::new();
        let f = app.draw(NATURAL);
        let legal = app.board.legal_moves(Cell::Black);
        for pos in all_positions() {
            let square = box_of(&app, Target::Square(byte(pos.row), byte(pos.col)));
            let dotted = fills_at(&f, square).contains(&VALID_MOVE_DOT);
            assert_eq!(
                dotted,
                legal.contains(&pos),
                "{pos:?} is {}legal and {}dotted",
                if legal.contains(&pos) { "" } else { "not " },
                if dotted { "" } else { "not " }
            );
        }
    }

    #[test]
    fn the_cursor_is_ringed_where_it_stands_and_nowhere_else() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(5, 2);
        let f = app.draw(NATURAL);
        let rings: Vec<Rect> = f
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == CURSOR_COLOR => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect();
        assert_eq!(rings.len(), 1, "one cursor, one ring");
        let square = box_of(&app, Target::Square(5, 2));
        let ring = rings[0];
        assert!(
            square.contains(ring.x + ring.w / 2.0, ring.y + ring.h / 2.0),
            "the ring is not on the square the cursor is on"
        );
    }

    #[test]
    fn a_finished_game_shows_no_cursor_and_no_dots() {
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        let f = app.draw(NATURAL);
        assert!(
            !f.commands().iter().any(|cmd| matches!(
                cmd,
                RenderCommand::StrokeRect { color, .. } if *color == CURSOR_COLOR
            )),
            "a finished game still invites a move with a cursor"
        );
        assert!(
            !f.commands().iter().any(|cmd| matches!(
                cmd,
                RenderCommand::FillRect { color, .. } if *color == VALID_MOVE_DOT
            )),
            "a finished game still dots squares nobody may play"
        );
        assert_eq!(
            text_in(&f, box_of(&app, Target::Turn)),
            "Game Over",
            "the panel does not say the game is over"
        );
    }

    #[test]
    fn the_last_move_is_highlighted_on_the_square_it_was_played() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(2, 3);
        app.handle_key(Key::Enter);
        let played = app.last_move.expect("a move was played");
        let f = app.draw(NATURAL);
        for pos in all_positions() {
            let square = box_of(&app, Target::Square(byte(pos.row), byte(pos.col)));
            assert_eq!(
                fills_at(&f, square).contains(&LAST_MOVE_HIGHLIGHT),
                pos == played,
                "{pos:?} is highlighted and the last move was {played:?}"
            );
        }
    }

    #[test]
    fn the_panel_names_whose_turn_it_is_and_only_says_so_while_there_is_one() {
        let mut app = ReversiApp::new();
        let f = app.draw(NATURAL);
        assert_eq!(
            text_in(&f, box_of(&app, Target::Turn)),
            "Your turn (Black)",
            "the panel does not say it is the human's move"
        );
        app.current_turn = Cell::White;
        let f = app.draw(NATURAL);
        assert_eq!(
            text_in(&f, box_of(&app, Target::Turn)),
            "White to move",
            "the panel does not say the search has the move"
        );
        app.phase = Phase::GameOver;
        let f = app.draw(NATURAL);
        assert_eq!(
            text_in(&f, box_of(&app, Target::Turn)),
            "Game Over",
            "a finished game is still being offered a move"
        );
    }

    #[test]
    fn the_panel_says_the_score_the_board_holds() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(2, 3);
        app.handle_key(Key::Enter);
        let f = app.draw(NATURAL);
        let bar = text_in(&f, box_of(&app, Target::ScoreBar));
        assert!(
            bar.contains(&format!("B: {}", app.board.count(Cell::Black))),
            "the bar's black count is stale: {bar:?}"
        );
        assert!(
            bar.contains(&format!("W: {}", app.board.count(Cell::White))),
            "the bar's white count is stale: {bar:?}"
        );
        assert_eq!(
            text_in(&f, box_of(&app, Target::Moves)),
            format!("Moves: {}", app.move_history.len()),
            "the move count is not the history's length"
        );
        assert_eq!(
            text_in(&f, box_of(&app, Target::EmptyCount)),
            format!("Empty: {}", app.board.empty_count()),
            "the empty count is not the board's"
        );
        assert_eq!(
            text_in(&f, box_of(&app, Target::LastMove)),
            app.move_history.last().unwrap().notation(),
            "the last-move line is not the last move"
        );
    }

    #[test]
    fn the_score_bar_is_split_in_proportion_to_the_two_counts() {
        let mut app = ReversiApp::new();
        app.board = board_with(&[
            (0, 0, Cell::Black),
            (0, 1, Cell::Black),
            (0, 2, Cell::Black),
            (1, 0, Cell::White),
        ]);
        let f = app.draw(NATURAL);
        let bar = box_of(&app, Target::ScoreBar);
        let black = f
            .commands()
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x, width, color, ..
                } if *color == BLACK_PIECE && (*x - bar.x).abs() < 0.01 => Some(*width),
                _ => None,
            })
            .expect("the black share of the bar was not drawn");
        assert!(
            (black - bar.w * 0.75).abs() < 0.5,
            "three black to one white is not three quarters of the bar: {black} of {}",
            bar.w
        );
    }

    #[test]
    fn an_empty_board_does_not_divide_the_score_bar_by_zero() {
        let mut app = ReversiApp::new();
        app.board = Board::empty();
        let f = app.draw(NATURAL);
        assert!(
            !f.commands().iter().any(|cmd| matches!(
                cmd,
                RenderCommand::FillRect { color, width, .. }
                    if *color == BLACK_PIECE && !width.is_finite()
            )),
            "a board with no pieces produced a bar of infinite width"
        );
        assert!(
            drew(&f, "B: 0"),
            "a board with no pieces does not say the score is nothing"
        );
    }

    #[test]
    fn the_history_shows_the_newest_moves_and_never_runs_into_the_help() {
        let mut app = ReversiApp::new();
        for _ in 0..12 {
            let Some(mv) = app.board.legal_moves(app.current_turn).first().copied() else {
                break;
            };
            app.cursor = mv;
            app.try_place_piece();
        }
        assert!(
            app.move_history.len() > 6,
            "the fixture did not build a long enough history"
        );
        // A window short enough that the history cannot show every move.
        let size = (900.0, 400.0);
        let f = app.draw(size);
        let history = box_of_sized(&app, Target::History, size);
        let help = box_of_sized(&app, Target::Help, size);
        assert!(
            history.bottom() <= help.y + 0.01,
            "the history ran through the help text: {history:?} into {help:?}"
        );
        let shown = text_in(&f, history);
        let rows: Vec<&str> = shown.lines().skip(1).collect(); // past the heading
        let last = app.move_history.len();
        assert!(
            rows.last()
                .is_some_and(|r| r.starts_with(&format!("{last}. "))),
            "the newest move is not the bottom row: {shown:?}"
        );
        assert!(
            rows.len() < last,
            "a short window found room for all {last} moves: {shown:?}"
        );
        assert!(
            !rows.iter().any(|r| r.starts_with("1. ")),
            "a short window showed the oldest move as well as the newest: {shown:?}"
        );
    }

    #[test]
    fn the_help_sits_on_the_floor_of_the_panel_at_every_size() {
        for (w, h) in SIZES {
            let app = ReversiApp::new();
            let Some(help) = probe::rect_of_sized(&app, Target::Help, (w, h)) else {
                continue;
            };
            let panel = box_of_sized(&app, Target::Panel, (w, h));
            assert!(
                help.bottom() <= panel.bottom() + 0.01,
                "the help hangs out of the panel at {w}x{h}: {help:?} in {panel:?}"
            );
        }
    }

    #[test]
    fn the_status_line_is_drawn_and_is_the_line_the_state_derives() {
        let mut app = ReversiApp::new();
        for phase in [Phase::Playing, Phase::GameOver] {
            app.phase = phase;
            let f = app.draw(NATURAL);
            assert_eq!(
                text_in(&f, box_of(&app, Target::Status)),
                app.status(),
                "the status band shows something other than the status ({phase:?})"
            );
        }
    }

    #[test]
    fn the_panel_is_painted_in_the_room_the_layout_gave_it() {
        // Against the layout's own panel, not against the window. The frame
        // clips everything to the window, so a band shifted off the panel is
        // still painted somewhere the window allows -- and `Frame::hit`
        // intersects with that clip too, which is why the version of this test
        // that walked `hits()` and checked they were inside the window could
        // not fail however far the band was moved.
        for (w, h) in SIZES {
            let app = ReversiApp::new();
            let f = app.draw((w, h));
            let l = Layout::solve(w, h);
            let want = inset(l.panel, l.pad);
            let got = f
                .commands()
                .iter()
                .find_map(|cmd| match cmd {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                        ..
                    } if *color == SURFACE0 => Some(Rect::new(*x, *y, *width, *height)),
                    _ => None,
                })
                .expect("the panel's own band was never painted");
            assert!(
                (got.x - want.x).abs() < 0.01
                    && (got.y - want.y).abs() < 0.01
                    && (got.w - want.w).abs() < 0.01
                    && (got.h - want.h).abs() < 0.01,
                "the panel band is painted at {got:?} and the layout put the panel at {want:?} at {w}x{h}"
            );
        }
    }

    #[test]
    fn an_inset_never_turns_a_box_inside_out() {
        // A band can be thinner than the padding it is inset by: a 60x40
        // window gives a header 3.6 tall and a padding of 2 a side. Without
        // the floor that box comes back 0.4 wide the wrong way, and a box
        // whose right edge is left of its left edge answers every containment
        // question backwards for the rest of the frame.
        for (w, h) in [(100.0, 100.0), (4.0, 4.0), (3.6, 3.6), (0.0, 0.0)] {
            for pad in [0.0, 1.0, 2.0, 50.0] {
                let band = inset(Rect::new(10.0, 10.0, w, h), pad);
                assert!(
                    band.w >= 0.0 && band.h >= 0.0,
                    "a {w}x{h} box inset by {pad} came back {}x{}",
                    band.w,
                    band.h
                );
            }
        }
    }

    #[test]
    fn a_window_of_no_size_draws_without_panicking() {
        for (w, h) in [(0.0, 0.0), (0.0, 600.0), (600.0, 0.0), (1.0, 1.0)] {
            let mut app = ReversiApp::new();
            let f = app.draw((w, h));
            assert!(f.is_balanced(), "clips are unbalanced at {w}x{h}");
            // And the events that arrive at such a window are survivable too.
            let _ = app.click_at(0.0, 0.0, MouseButton::Left, (w, h));
            let _ = app.key_at(&press(Key::Enter), (w, h));
        }
    }

    // ── The wiring ──────────────────────────────────────────────────

    #[test]
    fn clicking_a_square_plays_it() {
        let mut app = ReversiApp::new();
        assert_eq!(
            probe::click(&mut app, Target::Square(2, 3)),
            EventResult::Consumed,
            "a click on a legal square was not acted on"
        );
        assert_eq!(
            app.move_history.first().map(|r| r.pos),
            Some(Pos::new(2, 3)),
            "the click played some other square"
        );
        assert_eq!(
            app.cursor,
            Pos::new(2, 3),
            "the cursor did not follow the click"
        );
    }

    #[test]
    fn a_click_lands_on_the_same_square_at_every_window_size() {
        for (w, h) in SIZES {
            let mut app = ReversiApp::new();
            let target = Target::Square(2, 3);
            let Some(rect) = probe::rect_of_sized(&app, target, (w, h)) else {
                continue;
            };
            if rect.w < 1.0 || rect.h < 1.0 {
                continue; // No room to click at this size.
            }
            probe::click_sized(&mut app, target, MouseButton::Left, (w, h));
            assert_eq!(
                app.move_history.first().map(|r| r.pos),
                Some(Pos::new(2, 3)),
                "a click on (2, 3) played somewhere else at {w}x{h}"
            );
        }
    }

    #[test]
    fn clicking_an_illegal_square_says_so_rather_than_doing_nothing() {
        let mut app = ReversiApp::new();
        assert_eq!(
            probe::click(&mut app, Target::Square(0, 0)),
            EventResult::Consumed,
            "the click fell through to the window"
        );
        assert!(app.move_history.is_empty(), "an illegal square was played");
        assert!(
            app.status().contains("Illegal move"),
            "a misclick cost the player a guess: {:?}",
            app.status()
        );
    }

    #[test]
    fn a_click_on_the_furniture_is_answered_and_changes_nothing() {
        for target in [
            Target::Title,
            Target::Panel,
            Target::Turn,
            Target::ScoreBar,
            Target::Moves,
            Target::EmptyCount,
            Target::Help,
            Target::Status,
        ] {
            let mut app = ReversiApp::new();
            let before = app.board.clone();
            assert_eq!(
                probe::click(&mut app, target),
                EventResult::Consumed,
                "a click on {target:?} fell through to the window"
            );
            assert_eq!(app.board, before, "a click on {target:?} moved a piece");
        }
    }

    #[test]
    fn only_the_left_button_plays() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut app = ReversiApp::new();
            assert_eq!(
                probe::click_sized(&mut app, Target::Square(2, 3), button, NATURAL),
                EventResult::Ignored,
                "{button:?} played a piece"
            );
            assert!(app.move_history.is_empty(), "{button:?} played a piece");
        }
    }

    #[test]
    fn a_click_on_nothing_at_all_is_left_to_the_window() {
        let mut app = ReversiApp::new();
        assert_eq!(
            app.click_at(-5.0, -5.0, MouseButton::Left, NATURAL),
            EventResult::Ignored,
            "a click outside the window was claimed"
        );
    }

    #[test]
    fn a_click_once_the_game_is_over_is_answered_and_plays_nothing() {
        let mut app = ReversiApp::new();
        app.phase = Phase::GameOver;
        let before = app.board.clone();
        assert_eq!(
            probe::click(&mut app, Target::Square(2, 3)),
            EventResult::Consumed,
            "the click fell through to the window"
        );
        assert_eq!(app.board, before, "a finished game accepted a move");
    }

    #[test]
    fn a_key_arriving_at_the_window_reaches_the_game() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(0, 0);
        assert_eq!(
            app.key_at(&press(Key::Down), NATURAL),
            EventResult::Consumed,
            "the arrow key never reached the board"
        );
        assert_eq!(
            app.cursor,
            Pos::new(1, 0),
            "the arrow key did not move the cursor"
        );
        assert_eq!(
            app.key_at(&probe::release(Key::Down), NATURAL),
            EventResult::Ignored,
            "releasing a key moved the cursor a second time"
        );
        assert_eq!(app.cursor, Pos::new(1, 0), "the release moved the cursor");
    }

    #[test]
    fn a_resize_event_is_the_size_the_next_click_is_read_against() {
        let mut app = ReversiApp::new();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1280,
                height: 720,
            },
        );
        assert_eq!(app.size, (1280.0, 720.0), "the resize was not noted");
        let target = Target::Square(2, 3);
        let rect = probe::rect_of_sized(&app, target, (1280.0, 720.0))
            .expect("(2, 3) has a box at 1280x720");
        let (cx, cy) = rect.centre();
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Mouse(MouseEvent {
                    x: cx,
                    y: cy,
                    kind: MouseEventKind::Press(MouseButton::Left),
                }),
            ),
            EventResult::Consumed,
            "the click was read against the old size"
        );
        assert_eq!(
            app.move_history.first().map(|r| r.pos),
            Some(Pos::new(2, 3)),
            "the click landed on the wrong square after a resize"
        );
    }

    #[test]
    fn rendering_notes_the_size_it_was_given() {
        let mut app = ReversiApp::new();
        let _ = app.render(1000.0, 800.0);
        assert_eq!(
            app.size,
            (1000.0, 800.0),
            "a frame was drawn at a size the clicks do not know about"
        );
    }

    #[test]
    fn the_window_closes_on_the_close_button_and_on_escape() {
        let mut app = ReversiApp::new();
        assert!(
            matches!(app.on_event(&Event::CloseRequested), Response::Exit),
            "the close button did not close the window"
        );
        assert!(
            matches!(
                app.on_event(&Event::Key(press(Key::Escape))),
                Response::Exit
            ),
            "escape did not close the window"
        );
    }

    #[test]
    fn a_key_that_changes_something_asks_for_a_redraw_and_one_that_does_not_does_not() {
        let mut app = ReversiApp::new();
        app.cursor = Pos::new(0, 0);
        assert!(
            matches!(
                app.on_event(&Event::Key(press(Key::Down))),
                Response::Redraw
            ),
            "the board moved and the window was not told to redraw"
        );
        assert!(
            matches!(app.on_event(&Event::Key(press(Key::Up))), Response::Redraw),
            "the board moved and the window was not told to redraw"
        );
        assert!(
            matches!(app.on_event(&Event::Key(press(Key::Up))), Response::Idle),
            "a key that did nothing woke the window anyway"
        );
    }

    #[test]
    fn the_window_names_itself() {
        let app = ReversiApp::new();
        assert_eq!(app.title(), "Reversi", "the title bar is wrong");
        assert_eq!(app.app_id(), "reversi", "the app id is wrong");
        assert_eq!(
            app.initial_size(),
            (900, 640),
            "the opening size is not the one the layout was written against"
        );
    }
}
