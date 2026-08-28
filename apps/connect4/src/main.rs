//! Connect Four — drop pieces down seven columns and get four of yours in a row.
//!
//! A 7x6 board. Pieces fall to the lowest free cell of the column they are
//! dropped into; four of one colour in a line — across, up, or either
//! diagonal — wins. The player is Red and moves first, an alpha-beta search
//! plays Yellow, and a board with no free cell and no line is a draw.
//!
//! ## What wiring this up found
//!
//! The board, the win scan and the search were sound. Everything between them
//! and a person was not:
//!
//! 1. **`main` was `let _app = Connect4App::new();`.** It built a board,
//!    dropped it and exited. No window was opened, nothing was drawn, and no
//!    key or click ever arrived.
//! 2. **`render` was not given the window at all.** It took `&self` and no
//!    size, and drew from constants: the board at a fixed `(40, 90)` with a
//!    72-pixel cell, and a background rectangle whose size was computed *from
//!    the board* — the drawing pass telling the window how big to be, which is
//!    backwards for a program that lives in a window the user drags. `Layout`
//!    is derived from the live window size every frame now.
//! 3. **Nothing was clickable.** There was no mouse code in the file: the only
//!    handler was `handle_key`. For a game whose whole interface is "put one in
//!    that column", a pointer could do nothing at all. Every column is a hit
//!    box now, recorded by the pass that paints it, and there is a footer with
//!    the three verbs that are not a move.
//! 4. **"AI thinking..." could never be seen.** The string was in `render`, but
//!    the search ran inside the key handler that started it — synchronously,
//!    before the frame that would have shown the message was drawn — so the
//!    status line went from the player's turn to the player's turn again, and
//!    the one moment the program had something to say about a pause of up to a
//!    second was the moment it could not speak. The AI's move is owed on the
//!    next tick now, so the frame that says it is thinking is painted first.
//! 5. **`Escape to quit` was in the module documentation and in no handler.**
//!    `handle_key` matched Left, Right, Enter, Space and N; Escape fell into
//!    the `_ => false` arm. A documented control that does nothing is worse
//!    than an undocumented one.
//! 6. **The game's state was kept twice.** `Connect4App::status` was maintained
//!    by hand after each drop while `Board::status` computed the same answer
//!    from the grid, and `find_winner` was called once for the status and again
//!    for the highlight. There is one function now — `Board::outcome` — that
//!    answers "how does this position stand, and which four cells say so" in a
//!    single scan, and the app stores what it returns rather than a second
//!    opinion.
//! 7. **A move could not be taken back.** The `undo_drop` the search leans on
//!    was there, but nothing offered it to the player, so a misclick — the
//!    exact accident a pointer interface introduces — ended the game. `U` and
//!    the footer button take back the whole round, the AI's reply and the move
//!    that prompted it, from a snapshot rather than by replaying: a state that
//!    is restored wholesale cannot restore four fields and forget the fifth.
//! 8. **The column numbers were centred by guessing** — `cx - 4.0`, which is
//!    half of one particular digit at one particular font size. They are
//!    measured now, like every other centred line in the file.
//! 9. **The crate did not pass the lane's clippy gate at all**, carrying
//!    `#![allow(dead_code)]` and nine more crate-wide allows. All ten are gone,
//!    and with them `Board::check_line`, which only the tests ever called.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

// ── Catppuccin Mocha, only the entries this program actually paints with ──
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ── The board ───────────────────────────────────────────────────────────────

const COLS: usize = 7;
const ROWS: usize = 6;
/// Total cells on the board. A `const` rather than `COLS * ROWS` written at
/// each use: a multiplication in a const initialiser is checked by the
/// compiler, so it cannot be the arithmetic that overflows at runtime.
const CELL_COUNT: usize = COLS * ROWS;
/// The column the AI prefers and the cursor starts on.
const CENTER_COL: usize = COLS / 2;
/// How many pieces in a row win the game.
const RUN: usize = 4;

// ── The window ──────────────────────────────────────────────────────────────

/// The size the window is asked to open at. A request, not a promise — every
/// rectangle is computed from the size the frame is actually given.
const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 700.0;

/// The smallest font the renderer will draw as asked.
///
/// `gui/font`'s `round_px` clamps a size to at least one whole pixel and then
/// rounds it, so a request below a pixel is not drawn small — it is drawn a
/// whole pixel high, *larger* than the layout asked for (`known-issues.md`
/// lesson 60). Every caller here shrinks its type to fit its band, so below
/// this point the renderer would silently overrule all of them.
const MIN_DRAWN_FONT: f32 = 1.0;

/// The share of the window height the board is guaranteed before any band is
/// allowed a pixel. Bands are dropped whole until the rest fit.
const BOARD_SHARE: f32 = 0.45;

/// Which band is given up first when they do not all fit: the chute, then the
/// status line, then the header, and the footer last of all.
///
/// The footer goes last because it is the pointer's only route to a new game
/// and to an undo — the board itself is the route to a *move*, and it is never
/// dropped. The chute goes first because it says only what is about to happen,
/// which the board says again a moment later. Bands are dropped whole rather
/// than shrunk together: a band scaled down to four pixels costs the board
/// four pixels and shows nothing.
const BAND_DROP_ORDER: [usize; 4] = [2, 1, 0, 3];

/// How long the program waits before playing the move the AI owes.
///
/// Short enough not to feel like a stall, long enough that the frame saying so
/// is painted first — which is the whole reason the move is deferred at all.
const AI_TICK: Duration = Duration::from_millis(60);

// ── AI ──────────────────────────────────────────────────────────────────────

const AI_DEPTH: i32 = 6;

const SCORE_WIN: i32 = 1_000_000;
const SCORE_THREE: i32 = 100;
const SCORE_TWO: i32 = 10;
const SCORE_CENTER: i32 = 6;
const SCORE_OPP_THREE: i32 = -80;

// ── Cell and player types ───────────────────────────────────────────────────

/// The contents of a single board cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Red,
    Yellow,
}

impl Cell {
    /// The other player's colour, or `Empty` for `Empty` — nobody's opponent
    /// is nobody.
    fn opponent(self) -> Self {
        match self {
            Self::Red => Self::Yellow,
            Self::Yellow => Self::Red,
            Self::Empty => Self::Empty,
        }
    }

    /// The face a piece of this colour is painted with. An empty cell is a
    /// hole in the board, and is painted the colour of what is behind it.
    fn face(self) -> Color {
        match self {
            Self::Red => COL_RED,
            Self::Yellow => COL_YELLOW,
            Self::Empty => COL_CRUST,
        }
    }

    /// The ink a caption written over this piece is drawn in. Both pieces are
    /// pale, so both take dark ink; a hole takes light, because it is dark.
    fn ink(self) -> Color {
        match self {
            Self::Red | Self::Yellow => COL_CRUST,
            Self::Empty => COL_SUBTEXT0,
        }
    }

    /// What this colour is called in the window.
    fn name(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Empty => "nobody",
        }
    }
}

// ── Game status ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    /// The game is in progress.
    Playing,
    /// A player has four in a row. Carries the winner.
    Won(Cell),
    /// Every cell is full and nobody has four in a row.
    Draw,
}

// ── Win line ────────────────────────────────────────────────────────────────

/// The coordinates of the four cells that won the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinLine {
    cells: [(usize, usize); RUN],
}

// ── Run geometry ────────────────────────────────────────────────────────────

/// The directions a run of four can take, as `(row_step, col_step)`.
///
/// Four, not eight: a run and the same run walked backwards are the same four
/// cells, so including the reverses would find every win twice and double
/// every window's contribution to the AI's evaluation.
const DIRECTIONS: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];

/// The coordinates of the `RUN` cells beginning at `(row, col)` and stepping
/// by `(dr, dc)`, or `None` if the run would leave the board.
///
/// This is the *only* place a run's bounds are checked. Win detection, the
/// AI's board evaluation and the win-line highlight all scan the same runs,
/// and each used to carry its own hand-written copy of the `col + 3 < COLS`
/// arithmetic — eight copies in all, four of them written out a second time
/// with the offsets spelled into the indices (`grid[row + 2][col - 2]`).
/// Eight copies of a bound are eight chances to get one of them wrong.
fn line_cells(row: usize, col: usize, dr: isize, dc: isize) -> Option<[(usize, usize); RUN]> {
    let mut cells = [(0usize, 0usize); RUN];
    for (i, cell) in cells.iter_mut().enumerate() {
        let step = isize::try_from(i).ok()?;
        let r = isize::try_from(row)
            .ok()?
            .checked_add(dr.checked_mul(step)?)?;
        let c = isize::try_from(col)
            .ok()?
            .checked_add(dc.checked_mul(step)?)?;
        // `try_from` is the negative-side bound: a run heading down or left
        // off the board produces a negative coordinate, which has no `usize`.
        let (r, c) = (usize::try_from(r).ok()?, usize::try_from(c).ok()?);
        if r >= ROWS || c >= COLS {
            return None;
        }
        *cell = (r, c);
    }
    Some(cells)
}

/// Every run of four that fits on the board: row-major, and within a cell in
/// `DIRECTIONS` order.
///
/// The order is observable — `outcome` reports the first winning run it yields
/// — and reproduces what the hand-written nested scans it replaced produced.
fn all_lines() -> impl Iterator<Item = [(usize, usize); RUN]> {
    (0..ROWS).flat_map(|row| {
        (0..COLS).flat_map(move |col| {
            DIRECTIONS
                .iter()
                .filter_map(move |&(dr, dc)| line_cells(row, col, dr, dc))
        })
    })
}

// ── Board ───────────────────────────────────────────────────────────────────

/// The 7x6 board. Column 0 is leftmost, row 0 is the bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    /// Indexed `grid[row][col]`, row 0 = bottom.
    grid: [[Cell; COLS]; ROWS],
    /// Pieces in each column, which is also the row the next one would land
    /// in.
    heights: [usize; COLS],
    /// Pieces on the board.
    piece_count: usize,
}

impl Board {
    /// An empty board.
    pub fn new() -> Self {
        Self {
            grid: [[Cell::Empty; COLS]; ROWS],
            heights: [0; COLS],
            piece_count: 0,
        }
    }

    /// The number of pieces in `col`, which is also the row the next piece
    /// dropped there would land in. `None` for a column not on the board.
    fn height(&self, col: usize) -> Option<usize> {
        self.heights.get(col).copied()
    }

    /// Can another piece go in this column?
    pub fn can_drop(&self, col: usize) -> bool {
        self.height(col).is_some_and(|filled| filled < ROWS)
    }

    /// The cell at `(row, col)`. Cells off the board read as `Empty` — there
    /// is nothing there to be anyone's piece.
    pub fn get(&self, row: usize, col: usize) -> Cell {
        self.grid
            .get(row)
            .and_then(|cells| cells.get(col))
            .copied()
            .unwrap_or(Cell::Empty)
    }

    /// Writes `piece` at `(row, col)`, returning `false` if that is not a cell
    /// on the board.
    fn set(&mut self, row: usize, col: usize, piece: Cell) -> bool {
        let Some(cell) = self.grid.get_mut(row).and_then(|cells| cells.get_mut(col)) else {
            return false;
        };
        *cell = piece;
        true
    }

    /// Drops a piece into `col`, returning the row it landed in, or `None` if
    /// the column is full or not on the board.
    pub fn drop_piece(&mut self, col: usize, piece: Cell) -> Option<usize> {
        let row = self.height(col)?;
        if row >= ROWS || !self.set(row, col, piece) {
            return None;
        }
        *self.heights.get_mut(col)? = row.saturating_add(1);
        self.piece_count = self.piece_count.saturating_add(1);
        Some(row)
    }

    /// Takes the top piece off `col` and returns it, or `None` if the column
    /// is empty or not on the board.
    fn undo_drop(&mut self, col: usize) -> Option<Cell> {
        // Via `height`, not `self.heights[col]`. This was the one place in the
        // drop/undo pair where the column index went unchecked: an off-board
        // column that `can_drop` merely declined would panic here.
        let row = self.height(col)?.checked_sub(1)?;
        let cell = self.get(row, col);
        if !self.set(row, col, Cell::Empty) {
            return None;
        }
        *self.heights.get_mut(col)? = row;
        self.piece_count = self.piece_count.saturating_sub(1);
        Some(cell)
    }

    /// Plays `piece` in `col`, evaluates `f` on the resulting position, then
    /// takes the piece back.
    ///
    /// Returns `None` — *without* calling `f` — if the drop was refused. The
    /// AI used to drop and undo as two separate statements and discard both
    /// results, so a refused drop would still have run the undo, taking back a
    /// piece some earlier move had put there.
    fn with_move<T>(
        &mut self,
        col: usize,
        piece: Cell,
        f: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        self.drop_piece(col, piece)?;
        let out = f(self);
        let undone = self.undo_drop(col);
        debug_assert!(undone.is_some(), "a drop that succeeded must be undoable");
        Some(out)
    }

    /// Is every cell full?
    pub fn is_full(&self) -> bool {
        self.piece_count >= CELL_COUNT
    }

    /// How many pieces are on the board.
    pub fn pieces(&self) -> usize {
        self.piece_count
    }

    /// The player whose pieces occupy all of `cells`, or `None` if the run is
    /// not one player's alone.
    fn line_owner(&self, cells: [(usize, usize); RUN]) -> Option<Cell> {
        let mut occupants = cells.into_iter().map(|(row, col)| self.get(row, col));
        let first = occupants.next()?;
        if first == Cell::Empty {
            return None;
        }
        occupants.all(|cell| cell == first).then_some(first)
    }

    /// How the position stands, and which four cells say so.
    ///
    /// One function, one scan, one answer. The app used to keep a `status`
    /// field it maintained by hand while the board could compute the same
    /// thing, and called `find_winner` a second time for the highlight — three
    /// statements of one rule, any two of which could disagree.
    pub fn outcome(&self) -> (GameStatus, Option<WinLine>) {
        if let Some((winner, cells)) =
            all_lines().find_map(|cells| Some((self.line_owner(cells)?, cells)))
        {
            return (GameStatus::Won(winner), Some(WinLine { cells }));
        }
        if self.is_full() {
            return (GameStatus::Draw, None);
        }
        (GameStatus::Playing, None)
    }

    /// Does `player` have four in a row?
    ///
    /// `outcome` answers the same question and also says *which* four; this is
    /// the form the AI's inner loop wants. They scan `all_lines` in the same
    /// order, so they cannot disagree — which they could when each carried its
    /// own hand-written scan, and a disagreement would have had the AI
    /// searching positions the game already considered won.
    fn has_won(&self, player: Cell) -> bool {
        all_lines().any(|cells| {
            cells
                .into_iter()
                .all(|(row, col)| self.get(row, col) == player)
        })
    }

    /// The columns that can take a piece, ordered from the centre outwards —
    /// which is better move ordering for alpha-beta, not a preference of the
    /// game's.
    fn valid_moves(&self) -> Vec<usize> {
        const ORDER: [usize; COLS] = [3, 2, 4, 1, 5, 0, 6];
        ORDER
            .iter()
            .copied()
            .filter(|&c| self.can_drop(c))
            .collect()
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

// ── AI: minimax with alpha-beta pruning ─────────────────────────────────────

/// Scores one window of four cells for `player`.
fn evaluate_window(window: &[Cell; RUN], player: Cell) -> i32 {
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

/// Scores a whole position from `player`'s side.
fn evaluate_board(board: &Board, player: Cell) -> i32 {
    let center_pieces = (0..ROWS)
        .filter(|&row| board.get(row, CENTER_COL) == player)
        .count();
    let mut score = i32::try_from(center_pieces)
        .unwrap_or(i32::MAX)
        .saturating_mul(SCORE_CENTER);

    // Every window on the board, in all four directions. The sum does not
    // depend on the order, so this is the same score the four hand-written
    // direction loops produced — from one scan instead of four.
    for cells in all_lines() {
        let window = cells.map(|(row, col)| board.get(row, col));
        score = score.saturating_add(evaluate_window(&window, player));
    }

    score
}

/// Is this position over — someone won, or the board is full?
fn is_terminal(board: &Board) -> bool {
    board.has_won(Cell::Red) || board.has_won(Cell::Yellow) || board.is_full()
}

/// Minimax with alpha-beta pruning, returning `(score, best column)`.
/// `maximizing` is `true` when it is the AI's turn.
fn minimax(
    board: &mut Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    maximizing: bool,
    ai_player: Cell,
) -> (i32, Option<usize>) {
    if depth == 0 || is_terminal(board) {
        // A win found sooner is worth more than the same win found later, so
        // the remaining depth is added to the magnitude.
        if board.has_won(ai_player) {
            return (SCORE_WIN.saturating_add(depth), None);
        }
        if board.has_won(ai_player.opponent()) {
            return (SCORE_WIN.saturating_add(depth).saturating_neg(), None);
        }
        if board.is_full() {
            return (0, None);
        }
        return (evaluate_board(board, ai_player), None);
    }

    let moves = board.valid_moves();
    // The first legal move is the fallback answer: alpha-beta can cut the loop
    // off before any move has beaten `best_score`, and no legal move at all
    // means there is nothing to report.
    let Some(&fallback) = moves.first() else {
        return (0, None);
    };
    let deeper = depth.saturating_sub(1);
    let mover = if maximizing {
        ai_player
    } else {
        ai_player.opponent()
    };

    let mut best_col = fallback;
    let mut best_score = if maximizing { i32::MIN } else { i32::MAX };
    for &col in &moves {
        let Some((score, _)) = board.with_move(col, mover, |after| {
            minimax(after, deeper, alpha, beta, !maximizing, ai_player)
        }) else {
            // `valid_moves` said this column had room, so a refusal here means
            // the board changed under us; skip rather than undo a move that
            // was never made.
            continue;
        };
        if maximizing {
            if score > best_score {
                best_score = score;
                best_col = col;
            }
            alpha = alpha.max(score);
        } else {
            if score < best_score {
                best_score = score;
                best_col = col;
            }
            beta = beta.min(score);
        }
        if alpha >= beta {
            break;
        }
    }
    (best_score, Some(best_col))
}

/// The column the AI plays, or `None` on a board with no room left.
fn ai_best_move(board: &mut Board, ai_player: Cell, depth: i32) -> Option<usize> {
    // A move that wins immediately, and then one that stops the opponent
    // winning immediately, both beat whatever the search would return — and
    // the two searches are the same search with a different piece in hand.
    for player in [ai_player, ai_player.opponent()] {
        for col in board.valid_moves() {
            if board.with_move(col, player, |after| after.has_won(player)) == Some(true) {
                return Some(col);
            }
        }
    }

    let (_, best_col) = minimax(board, depth, i32::MIN, i32::MAX, true, ai_player);
    best_col
}

// ── What a click can land on ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A column of the board, chute included: clicking anywhere down it drops
    /// a piece there. The board *is* the interface, so the column strips are
    /// the controls, and there is no separate row of buttons to keep in step
    /// with where the columns actually are.
    Column(usize),
    NewGame,
    Undo,
    Help,
    /// The help sheet itself. It swallows the clicks meant for what it covers
    /// and closes — a pointer that opened the sheet has to be able to shut it
    /// even in a window too small to still be drawing the button it used.
    HelpSheet,
}

/// The one thing a key or a click ultimately asks for.
///
/// Both routes go through here, so "what does clicking the undo button do" and
/// "what does pressing U do" cannot drift apart: they are the same line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Drop a piece in this column, wherever the cursor happens to be.
    Drop(usize),
    /// Drop a piece in the column the cursor is over.
    DropAtCursor,
    CursorLeft,
    CursorRight,
    NewGame,
    Undo,
    ToggleHelp,
    CloseHelp,
}

// ── Layout ──────────────────────────────────────────────────────────────────

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in, which
/// is the class of fault this file was built out of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    /// The one line that says whose turn it is, or who won.
    pub info: Rect,
    /// The strip above the board where the piece about to fall is shown, and
    /// where each column's number is written.
    pub chute: Rect,
    pub board: Rect,
    pub footer: Rect,
    pub help: Rect,
    /// The side of one cell of the board, gaps included.
    pub step: f32,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 40.0).clamp(8.0, 18.0);
        let small = (font - 3.0).max(7.0);
        // Padding is bounded above by a quarter of the smaller side so that in
        // a tiny window the padding cannot eat the thing it is padding.
        let pad = (w.min(h) * 0.02).clamp(2.0, 12.0).min(w.min(h) / 4.0);

        // What each band would like, in [header, info, chute, footer] order.
        let mut wants = [
            (h * 0.11).clamp(34.0, 84.0),
            (h * 0.05).clamp(14.0, 28.0),
            (h * 0.06).clamp(16.0, 44.0),
            (h * 0.075).clamp(24.0, 48.0),
        ];
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, chute_h, foot_h] = wants;

        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let info = if inf_h > 0.0 {
            Rect::new(0.0, hdr_h, w, inf_h)
        } else {
            Rect::EMPTY
        };
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        // The board keeps its cells square, so the side of a cell is whichever
        // of the two dimensions runs out first.
        let top = hdr_h + inf_h + chute_h;
        let bottom = h - foot_h;
        let avail_w = (w - pad * 2.0).max(0.0);
        let avail_h = (bottom - top - pad * 2.0).max(0.0);
        let step = (avail_w / COLS as f32).min(avail_h / ROWS as f32).max(0.0);
        let bw = step * COLS as f32;
        let bh = step * ROWS as f32;
        let board = Rect::new((w - bw) / 2.0, top + pad + (avail_h - bh) / 2.0, bw, bh);

        // The chute is placed against the board rather than at the height its
        // own band was reserved at: the board is centred in what the bands
        // left, so the two are only in the same place when the board happens
        // to fill its space exactly, and a chute that is not directly over the
        // column it names is pointing at nothing.
        let chute = if chute_h > 0.0 && !board.is_empty() {
            Rect::new(board.x, (board.y - chute_h).max(0.0), bw, chute_h)
        } else {
            Rect::EMPTY
        };

        let help_w = (w * 0.92).min(440.0);
        let help_h = (h * 0.92).min(320.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            chute,
            board,
            footer,
            help,
            step,
            font,
            small,
            pad,
        }
    }

    /// Whether a band survived the drop ladder and is worth drawing into.
    ///
    /// A band that did not fit is `Rect::EMPTY`, not a flat one.
    pub fn shows(&self, band: Rect) -> bool {
        band.w > 0.0 && band.h > 0.0
    }

    /// The `index`th of `count` evenly-spaced buttons filling `row`.
    ///
    /// There is no guard against an empty `row`: a band that did not fit is
    /// `Rect::EMPTY`, and the arithmetic turns that back into `Rect::EMPTY`
    /// unaided — a zero width leaves a zero gap and a zero button width, a
    /// zero height leaves a zero button height, and every offset is a multiple
    /// of those. A guard there would stand in front of a rule that already
    /// holds, which is a line no test can own (`known-issues.md` lesson 51).
    /// The range check is a different matter and does have to be made.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        if index >= count {
            return Rect::EMPTY;
        }
        let n = count.max(1) as f32;
        let gap = (row.w * 0.012).min(8.0);
        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);
        let bh = (row.h * 0.76).max(0.0);
        Rect::new(
            row.x + gap + index as f32 * (bw + gap),
            row.y + (row.h - bh) / 2.0,
            bw,
            bh,
        )
    }

    /// The footer buttons: new game, undo, help.
    pub fn footer_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, 3, index)
    }

    /// One of the three readouts at the right of the header: 0 is the player's
    /// wins, 1 the AI's, 2 the draws. Empty when the header did not survive,
    /// or when it is too narrow to hold the title and all three.
    pub fn score_box(&self, index: usize) -> Rect {
        if !self.shows(self.header) || index >= 3 {
            return Rect::EMPTY;
        }
        let bw = (self.header.w * 0.17).clamp(40.0, 110.0);
        let bh = (self.header.h * 0.7).max(1.0);
        let gap = self.pad;
        let right = self.header.right() - self.pad;
        // Laid out from the right edge inwards, so the box nearest the edge is
        // index 0 and adding a fourth would not move the other three.
        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;
        if x < self.header.x {
            return Rect::EMPTY;
        }
        Rect::new(x, self.header.y + (self.header.h - bh) / 2.0, bw, bh)
    }

    /// The strip a click lands in to drop a piece into `col`: the column of
    /// cells and the chute above it, as one rectangle.
    pub fn column(&self, col: usize) -> Rect {
        if col >= COLS || self.board.is_empty() {
            return Rect::EMPTY;
        }
        let top = if self.shows(self.chute) {
            self.chute.y
        } else {
            self.board.y
        };
        Rect::new(
            self.board.x + col as f32 * self.step,
            top,
            self.step,
            (self.board.bottom() - top).max(0.0),
        )
    }

    /// The chute slot above `col`, where its number is written and where the
    /// piece about to fall is shown.
    pub fn chute_slot(&self, col: usize) -> Rect {
        if col >= COLS || !self.shows(self.chute) {
            return Rect::EMPTY;
        }
        Rect::new(
            self.chute.x + col as f32 * self.step,
            self.chute.y,
            self.step,
            self.chute.h,
        )
    }

    /// One hole of the board. Row 0 is the bottom row, and is drawn at the
    /// bottom: the flip lives here, once, rather than at each drawing site.
    ///
    /// The gap is taken out of the hole rather than added to the step, so the
    /// last hole ends exactly at the board's edge.
    pub fn cell(&self, row: usize, col: usize) -> Rect {
        if self.board.is_empty() || row >= ROWS || col >= COLS {
            return Rect::EMPTY;
        }
        let visual_row = ROWS.saturating_sub(1).saturating_sub(row);
        let gap = (self.step * 0.14).min(12.0);
        Rect::new(
            self.board.x + col as f32 * self.step + gap / 2.0,
            self.board.y + visual_row as f32 * self.step + gap / 2.0,
            (self.step - gap).max(0.0),
            (self.step - gap).max(0.0),
        )
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────────────

pub type Frame = guitk::frame::Frame<Target>;

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: if radius > 0.0 {
            CornerRadii::all(radius)
        } else {
            CornerRadii::ZERO
        },
    });
}

/// A disc, as far as a renderer with rectangles and corner radii has one.
fn disc(f: &mut Frame, r: Rect, color: Color) {
    fill(f, r, color, r.w.min(r.h) / 2.0);
}

fn label(
    f: &mut Frame,
    x: f32,
    y: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
    max_width: Option<f32>,
) {
    // A width of nothing is not a narrow label, it is no label: elided to fit
    // in no space at all it would be an empty string sitting in the frame — a
    // text command that paints nothing and still counts as text drawn. The
    // check lives here, once, rather than at each call site.
    //
    // The floor under the size is the renderer's, not a taste in typography: a
    // request below a pixel is drawn a whole pixel high, *larger* than the
    // band it was sized to fit, so every caller that shrinks its type to fit
    // would be silently overruled (`known-issues.md` lesson 60). Refusing is
    // the honest answer — a window with no room for a legible line shows its
    // boxes and its colours and no words.
    if body.is_empty() || size < MIN_DRAWN_FONT || max_width.is_some_and(|w| w <= 0.0) {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: body.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width,
        overflow: TextOverflow::Ellipsis,
    });
}

/// A line centred in a box, both ways, and never started outside it.
///
/// The offsets are clamped at zero in *both* directions. A line wider or
/// taller than its box would otherwise centre to a negative offset and begin
/// above or to the left of the box it is supposed to be inside — which for a
/// box at the top of the window means beginning off the window.
fn centred(f: &mut Frame, r: Rect, body: &str, size: f32, color: Color, weight: FontWeightHint) {
    if r.is_empty() {
        return;
    }
    let tw = text::measure(body, size, weight);
    let th = text::line_height(size, weight);
    label(
        f,
        r.x + (r.w - tw).max(0.0) / 2.0,
        r.y + (r.h - th).max(0.0) / 2.0,
        body,
        size,
        color,
        weight,
        Some(r.w),
    );
}

/// A button: a filled box with a hit box on it and a centred label.
fn button(f: &mut Frame, r: Rect, target: Target, body: &str, size: f32, face: Color, ink: Color) {
    if r.is_empty() {
        return;
    }
    fill(f, r, face, (r.h * 0.22).min(8.0));
    // Recorded by the pass that paints it, so a button that moved took its hit
    // box with it and there is no second copy of the geometry to disagree.
    f.hit(target, r);
    centred(f, r, body, size, ink, FontWeightHint::Bold);
}

// ── The help sheet's contents ───────────────────────────────────────────────

const HELP_TITLE: &str = "Connect Four";

const HELP_ROWS: [(&str, &str); 6] = [
    ("Left / Right", "choose a column"),
    ("Enter / Space", "drop a piece there"),
    ("1 - 7", "drop straight into that column"),
    ("U / Ctrl+Z", "take back your last move"),
    ("N", "start a new game"),
    ("H", "show or hide this sheet"),
];

// ── The program ─────────────────────────────────────────────────────────────

/// The whole of a game, kept so a move can be taken back.
///
/// Everything a drop changes, in one value. Restoring a snapshot cannot put
/// four fields back and forget the fifth, which is the fault an
/// undo-by-field-list keeps having — this program's neighbour, 2048, shipped
/// an undo that restored the grid and the score and left the *status* alone,
/// so taking back the winning move left "You win!" over a board with no win
/// on it.
#[derive(Debug, Clone)]
struct Snapshot {
    board: Board,
    current_player: Cell,
    status: GameStatus,
    win_line: Option<WinLine>,
    /// How long `move_history` was, which is what it is truncated back to.
    moves: usize,
    human_wins: u32,
    ai_wins: u32,
    draws: u32,
}

pub struct Connect4 {
    board: Board,
    /// The column the keyboard cursor is over.
    cursor_col: usize,
    current_player: Cell,
    status: GameStatus,
    win_line: Option<WinLine>,
    /// The player is Red and moves first; the search plays Yellow.
    human_player: Cell,
    ai_player: Cell,
    ai_depth: i32,
    human_wins: u32,
    ai_wins: u32,
    draws: u32,
    /// Every move played this game, as `(column, player)`.
    move_history: Vec<(usize, Cell)>,
    /// The position before each drop, oldest first.
    ///
    /// Uncapped, and it does not need a cap: a game is over after at most
    /// `CELL_COUNT` drops and a new game clears it, so the deepest this can
    /// ever go is 42 boards.
    history: Vec<Snapshot>,
    show_help: bool,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    width: f32,
    height: f32,
}

impl Connect4 {
    pub fn new() -> Self {
        Self {
            board: Board::new(),
            cursor_col: CENTER_COL,
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
            history: Vec::new(),
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// Throws the position away and keeps the tally. A new game is a new
    /// board, not a new session.
    pub fn new_game(&mut self) {
        self.board = Board::new();
        self.cursor_col = CENTER_COL;
        self.current_player = Cell::Red;
        self.status = GameStatus::Playing;
        self.win_line = None;
        self.move_history.clear();
        self.history.clear();
    }

    /// Whether the AI owes a move.
    ///
    /// A question asked of the state rather than a flag kept beside it. A
    /// `pending_ai: bool` would be a second copy of exactly this expression,
    /// and the two could disagree — which is the shape of half the faults this
    /// file was rewritten to remove.
    pub fn ai_to_play(&self) -> bool {
        self.status == GameStatus::Playing && self.current_player == self.ai_player
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn win_line(&self) -> Option<WinLine> {
        self.win_line
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    pub fn help_is_open(&self) -> bool {
        self.show_help
    }

    pub fn moves(&self) -> &[(usize, Cell)] {
        &self.move_history
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// The state to come back to if the move about to be played is taken back.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            board: self.board.clone(),
            current_player: self.current_player,
            status: self.status,
            win_line: self.win_line,
            moves: self.move_history.len(),
            human_wins: self.human_wins,
            ai_wins: self.ai_wins,
            draws: self.draws,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.move_history.truncate(s.moves);
        self.board = s.board;
        self.current_player = s.current_player;
        self.status = s.status;
        self.win_line = s.win_line;
        self.human_wins = s.human_wins;
        self.ai_wins = s.ai_wins;
        self.draws = s.draws;
    }

    /// Drops a piece for whoever is to move. `false` if the game is over or
    /// the column will not take one.
    pub fn drop_at(&mut self, col: usize) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        let player = self.current_player;
        let before = self.snapshot();
        // `drop_piece` states the "is there room in this column" bound itself
        // and reports it; a `can_drop` guard here would be the same bound
        // written a second time, one statement away from the read it guards.
        if self.board.drop_piece(col, player).is_none() {
            return false;
        }
        self.history.push(before);
        self.move_history.push((col, player));

        let (status, line) = self.board.outcome();
        self.status = status;
        self.win_line = line;
        match status {
            // Who scores turns only on whether the mover was the human: the
            // winner of a game that has just ended is whoever moved last.
            GameStatus::Won(_) => {
                if player == self.human_player {
                    self.human_wins = self.human_wins.saturating_add(1);
                } else {
                    self.ai_wins = self.ai_wins.saturating_add(1);
                }
            }
            GameStatus::Draw => self.draws = self.draws.saturating_add(1),
            GameStatus::Playing => self.current_player = player.opponent(),
        }
        true
    }

    /// Plays the move the AI owes, and reports which column it chose.
    ///
    /// `None` when it is not the AI's turn — including when the game is over,
    /// which `ai_to_play` folds into the same question.
    pub fn ai_turn(&mut self) -> Option<usize> {
        if !self.ai_to_play() {
            return None;
        }
        let col = ai_best_move(&mut self.board, self.ai_player, self.ai_depth)?;
        if !self.drop_at(col) {
            return None;
        }
        Some(col)
    }

    /// Takes back moves until it is the player's turn on a playable board.
    ///
    /// That is one drop when the AI has not replied yet and two when it has,
    /// and the loop says so rather than the count: a rule written as "pop two"
    /// is wrong on the first move of the game and wrong again the moment
    /// anything else can move.
    pub fn undo(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }
        while let Some(s) = self.history.pop() {
            self.restore(s);
            if self.status == GameStatus::Playing && self.current_player == self.human_player {
                break;
            }
        }
        true
    }

    fn drop_for_human(&mut self, col: usize) -> EventResult {
        // Two rules, not one written twice: this is "it is not your turn", and
        // `drop_at`'s is "the game is over". A won game leaves the winner as
        // the current player, so on a game the human won only the second of
        // them refuses.
        if self.current_player != self.human_player {
            return EventResult::Ignored;
        }
        if self.drop_at(col) {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    /// Does what a key or a click asked for.
    pub fn apply(&mut self, intent: Intent) -> EventResult {
        // The sheet is modal to the game, and has to be, because it is drawn
        // over the board. Without this an arrow key would move a cursor the
        // player cannot see and Enter would drop a piece behind the help text.
        // Anything the game would otherwise act on shuts the sheet instead,
        // which is the same answer a click gets and the one the sheet's own
        // closing line promises. The two help intents fall through, because
        // shutting the sheet is what they were going to do anyway.
        if self.show_help && !matches!(intent, Intent::ToggleHelp | Intent::CloseHelp) {
            self.show_help = false;
            return EventResult::Consumed;
        }
        match intent {
            Intent::Drop(col) => {
                // The cursor follows the column that was played, so the
                // keyboard picks up where the pointer left off.
                if col < COLS {
                    self.cursor_col = col;
                }
                self.drop_for_human(col)
            }
            Intent::DropAtCursor => self.drop_for_human(self.cursor_col),
            Intent::CursorLeft => self.move_cursor(-1),
            Intent::CursorRight => self.move_cursor(1),
            Intent::NewGame => {
                self.new_game();
                EventResult::Consumed
            }
            Intent::Undo => {
                if self.undo() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Intent::ToggleHelp => {
                self.show_help = !self.show_help;
                EventResult::Consumed
            }
            Intent::CloseHelp => {
                if self.show_help {
                    self.show_help = false;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    /// Moves the cursor one column, and says whether it went anywhere.
    ///
    /// A cursor already against the wall reports `Ignored`, because nothing
    /// changed and a redraw would be a frame spent painting the same picture.
    fn move_cursor(&mut self, step: isize) -> EventResult {
        let Some(next) = self
            .cursor_col
            .checked_add_signed(step)
            .filter(|&c| c < COLS)
        else {
            return EventResult::Ignored;
        };
        self.cursor_col = next;
        EventResult::Consumed
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        match key_intent(ev) {
            Some(intent) => self.apply(intent),
            None => EventResult::Ignored,
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        // Hit-tested against a frame drawn at the size the last one was drawn
        // at, so a click is read against the picture the player is looking at.
        let frame = self.frame(self.width, self.height);
        match frame.hit_test(ev.x, ev.y) {
            Some(target) => self.apply(target_intent(target)),
            None => EventResult::Ignored,
        }
    }

    /// Remembers the size the window is now, which is the size the next click
    /// will be read against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// The line the info band shows.
    fn status_line(&self) -> String {
        match self.status {
            GameStatus::Playing => {
                if self.ai_to_play() {
                    "Yellow is thinking...".to_string()
                } else {
                    format!("Your turn ({})", self.human_player.name())
                }
            }
            GameStatus::Won(winner) if winner == self.human_player => "You win!".to_string(),
            GameStatus::Won(_) => "Yellow wins!".to_string(),
            GameStatus::Draw => "A draw — the board is full".to_string(),
        }
    }

    fn status_colour(&self) -> Color {
        match self.status {
            GameStatus::Playing => COL_BLUE,
            GameStatus::Won(winner) if winner == self.human_player => COL_GREEN,
            GameStatus::Won(_) => COL_RED,
            GameStatus::Draw => COL_PEACH,
        }
    }

    /// The whole window, as commands and the hit boxes the same pass recorded.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_info(&mut f, &l);
        self.draw_chute(&mut f, &l);
        self.draw_board(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.header) {
            return;
        }
        fill(f, l.header, COL_MANTLE, 0.0);

        let boxes = [
            ("You", self.human_wins, COL_RED),
            ("AI", self.ai_wins, COL_YELLOW),
            ("Draw", self.draws, COL_OVERLAY0),
        ];
        // Index 0 is the box nearest the right edge, so the readouts are drawn
        // right to left and read left to right.
        let mut leftmost = l.header.right();
        for (i, &(name, count, ink)) in boxes.iter().enumerate() {
            let r = l.score_box(boxes.len().saturating_sub(1).saturating_sub(i));
            if r.is_empty() {
                continue;
            }
            leftmost = leftmost.min(r.x);
            fill(f, r, COL_SURFACE0, (r.h * 0.2).min(8.0));
            let size = (r.h * 0.3).min(l.small);
            centred(
                f,
                Rect::new(r.x, r.y, r.w, r.h * 0.5),
                name,
                size,
                COL_SUBTEXT0,
                FontWeightHint::Regular,
            );
            centred(
                f,
                Rect::new(r.x, r.y + r.h * 0.42, r.w, r.h * 0.58),
                &count.to_string(),
                (r.h * 0.4).min(l.font),
                ink,
                FontWeightHint::Bold,
            );
        }

        // The title takes whatever the readouts left, and is elided rather
        // than drawn over them.
        let title_w = leftmost - l.header.x - l.pad * 2.0;
        label(
            f,
            l.header.x + l.pad,
            l.header.y + (l.header.h - text::line_height(l.font, FontWeightHint::Bold)) / 2.0,
            "Connect Four",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_w),
        );
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.info) {
            return;
        }
        centred(
            f,
            l.info,
            &self.status_line(),
            (l.info.h * 0.7).min(l.font),
            self.status_colour(),
            FontWeightHint::Bold,
        );
    }

    /// The strip above the board: each column's number, and under the cursor's
    /// number the piece that is about to fall.
    fn draw_chute(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.chute) {
            return;
        }
        for col in 0..COLS {
            let slot = l.chute_slot(col);
            if slot.is_empty() {
                continue;
            }
            // The piece is drawn only where it can actually be dropped: on the
            // cursor's column, while the game is the player's to play. A disc
            // over a column on a finished board would be promising a move the
            // program would then refuse.
            let waiting = col == self.cursor_col
                && self.status == GameStatus::Playing
                && self.current_player == self.human_player
                && self.board.can_drop(col);
            let ink = if waiting {
                let d = (slot.w.min(slot.h) * 0.8).max(0.0);
                let (cx, cy) = slot.centre();
                disc(
                    f,
                    Rect::new(cx - d / 2.0, cy - d / 2.0, d, d),
                    self.human_player.face(),
                );
                self.human_player.ink()
            } else {
                COL_OVERLAY0
            };
            centred(
                f,
                slot,
                &col.saturating_add(1).to_string(),
                (slot.h * 0.5).min(l.small),
                ink,
                FontWeightHint::Bold,
            );
        }
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        fill(f, l.board, COL_BLUE, (l.step * 0.2).min(10.0));
        for row in 0..ROWS {
            for col in 0..COLS {
                disc(f, l.cell(row, col), self.board.get(row, col).face());
            }
        }

        // The winning four are ringed, and a line is drawn between the ends of
        // the run so the direction is readable at a glance.
        if let Some(line) = self.win_line {
            for &(row, col) in &line.cells {
                let r = l.cell(row, col);
                if r.is_empty() {
                    continue;
                }
                f.push(RenderCommand::StrokeRect {
                    x: r.x,
                    y: r.y,
                    width: r.w,
                    height: r.h,
                    color: COL_GREEN,
                    line_width: (l.step * 0.06).clamp(1.0, 5.0),
                    corner_radii: CornerRadii::all(r.w.min(r.h) / 2.0),
                });
            }
            // Destructuring the fixed-size array rather than indexing it:
            // `[first, .., last]` is irrefutable for `[_; RUN]`, so the
            // compiler — not a comment about `RUN` being 4 — is what says the
            // ends exist.
            let [(r0, c0), .., (r3, c3)] = line.cells;
            let (x1, y1) = l.cell(r0, c0).centre();
            let (x2, y2) = l.cell(r3, c3).centre();
            f.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: COL_GREEN,
                width: (l.step * 0.06).clamp(1.0, 5.0),
            });
        }

        // The columns take clicks last of the board's parts, so a click that
        // lands on a piece still drops into the column that piece is in. A
        // full column keeps its hit box: the answer to clicking it is "no",
        // and a control that silently stops existing cannot say so.
        for col in 0..COLS {
            let strip = l.column(col);
            if !strip.is_empty() {
                f.hit(Target::Column(col), strip);
            }
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        if !l.shows(l.footer) {
            return;
        }
        let size = (l.footer.h * 0.34).min(l.small);
        button(
            f,
            l.footer_button(0),
            Target::NewGame,
            "New game",
            size,
            COL_SURFACE1,
            COL_TEXT,
        );
        // Greyed rather than gone. The button keeps its hit box with nothing
        // to undo, so the answer to pressing it is a refusal the player can
        // see rather than a control that moved.
        let (undo_face, undo_ink) = if self.can_undo() {
            (COL_SURFACE1, COL_TEXT)
        } else {
            (COL_SURFACE0, COL_OVERLAY0)
        };
        button(
            f,
            l.footer_button(1),
            Target::Undo,
            "Undo",
            size,
            undo_face,
            undo_ink,
        );
        button(
            f,
            l.footer_button(2),
            Target::Help,
            "Help",
            size,
            COL_SURFACE1,
            COL_TEXT,
        );
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        let h = l.help;
        if h.is_empty() {
            return;
        }
        fill(f, h, COL_SURFACE0, (h.h * 0.04).min(10.0));
        // The hit box is the whole *window*, not the sheet's own rectangle,
        // and the sheet's last line is the reason: it says "click anywhere to
        // close", and anywhere means anywhere. Claiming only its own rectangle
        // would leave the columns and the footer live underneath a sheet that
        // covers the board — a click meant to shut the sheet would drop a
        // piece the player could not see. Recorded last, so it lies over every
        // control drawn before it and takes their clicks (`Frame::hit_test`
        // answers with the last box painted). This also means a pointer can
        // shut the sheet in a window too small to be drawing the button it
        // opened from.
        f.hit(Target::HelpSheet, l.window);

        let head_h = (h.h * 0.16).max(0.0);
        centred(
            f,
            Rect::new(h.x, h.y, h.w, head_h),
            HELP_TITLE,
            (head_h * 0.6).min(l.font * 1.2),
            COL_LAVENDER,
            FontWeightHint::Bold,
        );

        // One band per row and one more for the line that says how to shut the
        // sheet, which is why the ladder divides by `rows + 1`: measured back
        // from the sheet's own bottom edge instead, that line would sit in the
        // last row's band, written across the last row.
        let rows = HELP_ROWS.len() as f32;
        let body_h = (h.h - head_h - l.pad * 2.0).max(0.0);
        let step = body_h / (rows + 1.0);
        // Sized to the band it is written in, not to the sheet: a row taller
        // than its own band overwrites the row beneath it.
        let size = l.small.min(step * 0.7);
        let key_w = (h.w * 0.42).max(0.0);
        for (i, &(key, meaning)) in HELP_ROWS.iter().enumerate() {
            let y = h.y + head_h + l.pad + i as f32 * step;
            label(
                f,
                h.x + l.pad,
                y,
                key,
                size,
                COL_TEXT,
                FontWeightHint::Bold,
                Some(key_w),
            );
            label(
                f,
                h.x + l.pad + key_w,
                y,
                meaning,
                size,
                COL_SUBTEXT0,
                FontWeightHint::Regular,
                Some((h.w - key_w - l.pad * 2.0).max(0.0)),
            );
        }

        centred(
            f,
            Rect::new(h.x, h.y + head_h + l.pad + rows * step, h.w, step),
            "Click anywhere to close",
            size * 0.9,
            COL_OVERLAY0,
            FontWeightHint::Regular,
        );
    }
}

impl Default for Connect4 {
    fn default() -> Self {
        Self::new()
    }
}

/// The digit keys name columns one to seven, so there have to be seven of
/// them. A compile-time check rather than a comment: a board with another
/// column would otherwise leave a column no digit reaches, silently.
const _: () = assert!(COLS == 7, "the digit keys 1-7 name every column");

/// What a key asks for, if anything.
///
/// A free function rather than a method, so the mapping can be read and tested
/// without a game to read it against.
pub fn key_intent(ev: &KeyEvent) -> Option<Intent> {
    if ev.key == Key::Z && ev.modifiers.ctrl {
        return Some(Intent::Undo);
    }
    // Ctrl and Alt combinations belong to the window, not to the board: a
    // Ctrl+Left that moves the cursor is a Ctrl+Left the desktop cannot have.
    if ev.modifiers.ctrl || ev.modifiers.alt {
        return None;
    }
    match ev.key {
        Key::Left | Key::A => Some(Intent::CursorLeft),
        Key::Right | Key::D => Some(Intent::CursorRight),
        Key::Enter | Key::Space => Some(Intent::DropAtCursor),
        Key::Num1 => Some(Intent::Drop(0)),
        Key::Num2 => Some(Intent::Drop(1)),
        Key::Num3 => Some(Intent::Drop(2)),
        Key::Num4 => Some(Intent::Drop(3)),
        Key::Num5 => Some(Intent::Drop(4)),
        Key::Num6 => Some(Intent::Drop(5)),
        Key::Num7 => Some(Intent::Drop(6)),
        Key::N => Some(Intent::NewGame),
        Key::U => Some(Intent::Undo),
        Key::H => Some(Intent::ToggleHelp),
        Key::Escape => Some(Intent::CloseHelp),
        _ => None,
    }
}

/// What clicking a control asks for.
pub fn target_intent(target: Target) -> Intent {
    match target {
        Target::Column(col) => Intent::Drop(col),
        Target::NewGame => Intent::NewGame,
        Target::Undo => Intent::Undo,
        Target::Help => Intent::ToggleHelp,
        Target::HelpSheet => Intent::CloseHelp,
    }
}

pub fn handle_event(app: &mut Connect4, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The move the AI owes is played here rather than inside the handler
        // for the key that prompted it, so the frame that says it is thinking
        // is painted before the search that makes it wait begins.
        Event::Tick { .. } => {
            if app.ai_turn().is_some() {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

impl App for Connect4 {
    fn title(&self) -> String {
        "Connect Four".to_string()
    }

    fn app_id(&self) -> String {
        "connect4".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// A clock only while the AI owes a move, and none at all otherwise.
    ///
    /// Consulted after every event, so this starts when the player moves and
    /// stops when the reply lands. A game waiting on a person holds no timer,
    /// and the desktop is not kept awake by a board nobody is playing.
    fn tick_interval(&self) -> Option<Duration> {
        self.ai_to_play().then_some(AI_TICK)
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
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Connect4 {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
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
    let mut game = Connect4::new();
    app::launch("connect4", &mut game)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

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

    // ── Board status tests ──────────────────────────────────────────

    // ── Cell tests ──────────────────────────────────────────────────

    #[test]
    fn test_cell_opponent() {
        assert_eq!(Cell::Red.opponent(), Cell::Yellow);
        assert_eq!(Cell::Yellow.opponent(), Cell::Red);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
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
            [
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
            ],
            [
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
            ],
            [
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
            ],
            [
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
            ],
            [
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
            ],
            [
                Cell::Red,
                Cell::Red,
                Cell::Red,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Yellow,
                Cell::Red,
            ],
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

    // ── Rendering tests ─────────────────────────────────────────────

    // ── Diagonal win variants ───────────────────────────────────────

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
        let pieces = [
            Cell::Red,
            Cell::Yellow,
            Cell::Red,
            Cell::Yellow,
            Cell::Red,
            Cell::Yellow,
        ];
        for (i, &piece) in pieces.iter().enumerate() {
            let row = board.drop_piece(4, piece);
            assert_eq!(row, Some(i));
        }
        assert!(!board.can_drop(4));
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

    // ── Run geometry ────────────────────────────────────────────────
    //
    // `all_lines` replaced eight hand-written nested scans. These tests pin
    // the set it yields, because every other scan on the board now trusts it.

    #[test]
    fn all_lines_yields_every_run_of_four_exactly_once() {
        let lines: Vec<_> = all_lines().collect();
        // 24 horizontal (6 rows x 4 starts) + 21 vertical (7 cols x 3) +
        // 12 up-right + 12 up-left. This is the standard count of
        // four-in-a-row windows on a 7x6 board.
        assert_eq!(lines.len(), 69, "wrong number of runs on the board");

        let mut seen: Vec<[(usize, usize); RUN]> = Vec::new();
        for line in &lines {
            assert!(
                !seen.contains(line),
                "run {line:?} was yielded twice -- it would be scored twice"
            );
            seen.push(*line);
        }
    }

    #[test]
    fn every_run_all_lines_yields_is_on_the_board() {
        for line in all_lines() {
            for (row, col) in line {
                assert!(
                    row < ROWS && col < COLS,
                    "run left the board at ({row}, {col})"
                );
            }
        }
    }

    #[test]
    fn a_run_that_would_leave_the_board_has_no_cells() {
        // Off the right edge, off the top, off the left edge going up-left,
        // and starting outside the board entirely.
        assert_eq!(line_cells(0, 4, 0, 1), None);
        assert_eq!(line_cells(3, 0, 1, 0), None);
        assert_eq!(line_cells(0, 2, 1, -1), None);
        assert_eq!(line_cells(ROWS, 0, 0, 1), None);
    }

    // ── Bounds made total, not merely unreached ─────────────────────

    #[test]
    fn an_off_board_column_is_declined_rather_than_indexed() {
        let mut board = Board::new();
        // `can_drop` always declined an off-board column; `undo_drop` used to
        // index `heights` with it and panic. Both now answer the same way.
        assert!(!board.can_drop(COLS));
        assert_eq!(board.drop_piece(COLS, Cell::Red), None);
        assert_eq!(board.undo_drop(COLS), None);
        assert_eq!(board.height(COLS), None);
        assert_eq!(board.piece_count, 0, "a declined drop changes nothing");
    }

    #[test]
    fn cells_off_the_board_are_empty_and_cannot_be_written() {
        let mut board = Board::new();
        assert_eq!(board.get(ROWS, 0), Cell::Empty);
        assert_eq!(board.get(0, COLS), Cell::Empty);
        assert!(!board.set(ROWS, 0, Cell::Red));
        assert!(!board.set(0, COLS, Cell::Red));
        assert!(board.set(0, 0, Cell::Red));
        assert_eq!(board.get(0, 0), Cell::Red);
    }

    #[test]
    fn with_move_leaves_the_board_exactly_as_it_found_it() {
        let mut board = Board::new();
        board.drop_piece(3, Cell::Red);
        board.drop_piece(3, Cell::Yellow);
        board.drop_piece(4, Cell::Red);
        let before = board.clone();

        let saw = board.with_move(3, Cell::Yellow, |after| after.get(2, 3));
        assert_eq!(saw, Some(Cell::Yellow), "the closure sees the played move");
        assert_eq!(board.grid, before.grid);
        assert_eq!(board.heights, before.heights);
        assert_eq!(board.piece_count, before.piece_count);
    }

    #[test]
    fn with_move_on_a_full_column_runs_nothing_and_undoes_nothing() {
        let mut board = Board::new();
        for _ in 0..ROWS {
            board.drop_piece(0, Cell::Red);
        }
        board.drop_piece(1, Cell::Yellow);
        let before = board.clone();

        // The old code dropped and undid as two statements: a refused drop
        // still ran the undo, which would have taken back the piece in
        // column 0 that a different move had put there.
        let mut ran = false;
        let out = board.with_move(0, Cell::Yellow, |_| {
            ran = true;
        });
        // The board first: an undo without a matching drop is what actually
        // damages the position, and it is what this test exists to catch.
        assert_eq!(board.grid, before.grid, "a refused drop moved a piece");
        assert_eq!(board.heights, before.heights);
        assert_eq!(board.piece_count, before.piece_count);
        assert_eq!(out, None, "a full column is refused");
        assert!(!ran, "the closure must not run on a refused drop");
    }

    // ── The two win detectors agree ─────────────────────────────────
}
