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
//! 10. **`has_won(Cell::Empty)` was true on an empty board.** The scan asked
//!     whether four cells in a line all held the colour it was handed, and on
//!     a board of nothing they all hold `Empty` — so the position "nobody has
//!     played yet" answered yes to "has this player won". Nothing in the
//!     shipped program passed `Empty` to it, which is why it went unseen, but
//!     the function is one caller away from crowning a winner before the first
//!     move. Absence is not a colour: the scan refuses `Empty` up front now.
//! 11. **`minimax`'s emptiness guard was a duplicate of the test above it.**
//!     It returned early when the move list was empty — but the line before it
//!     had already returned on `is_terminal`, which is true of exactly those
//!     positions, so the guard was unreachable. What it was standing in for,
//!     the "the loop improved on nothing" case, was instead papered over by a
//!     `fallback` column that claimed a choice had been made when none had.
//!     The chosen column is an `Option` now, `None` only when there was
//!     genuinely nothing to choose.
//! 12. **…and the `Option` was then seeded with a column anyway.** The rewrite
//!     for fault 11 initialised the chosen column to the first legal move, as
//!     "the answer for a search alpha-beta cut off before anything beat the
//!     best score" — a case that cannot arise, because the first move
//!     compares against `i32::MIN`/`i32::MAX` and always wins that comparison,
//!     and the cut happens after the assignment. So the `None` that fault 11
//!     introduced was never returned, and a search that chose nothing was
//!     indistinguishable from one that chose the column it happened to try
//!     first. Found by mutation: replacing the seed with `None` changed no
//!     test's verdict.
//! 13. **The help sheet excused the two keys that are about the help sheet
//!     from its own modal rule** — `H` fell through to a toggle and Escape to
//!     a conditional close — on the reasoning that both would shut the sheet
//!     anyway. Which is precisely why the exemption was worth nothing: no
//!     keystroke could tell the two paths apart, and it left `ToggleHelp`'s
//!     toggle and `CloseHelp`'s open-sheet branch unreachable behind it.
//!     Anything at all shuts an open sheet now, and the two arms below say
//!     what they really are: "open it" and "there is nothing to close".
//! 14. **The undo snapshot carried two fields that could only hold one value.**
//!     A snapshot is taken in `drop_at`, past its refusal to move on a finished
//!     game, so the status it recorded was always `Playing` and the winning
//!     line always `None`. Restoring them *looked* like what puts a taken-back
//!     win back in play while being a copy of a constant. `restore` now sets
//!     the game live explicitly and says why.
//! 15. **All five drawing passes opened with a "did this band fit?" guard that
//!     nothing needed.** `fill`, `centred`, `label`, `Frame::hit`, `score_box`
//!     and `chute_slot` all refuse an empty box already, so deleting the
//!     guards changed not one command in any frame — lesson 51 five times
//!     over, in a file whose own `nth_of` refuses to write that guard for
//!     exactly that reason. Four went at once. The fifth, `draw_board`'s, was
//!     kept for a whole day behind a paragraph explaining why the board was
//!     different: the line joining the winning four is pushed unguarded, so a
//!     board with no room would leave a zero-length stroke in the corner of
//!     the window. The paragraph was wrong. The board is the one band the drop
//!     ladder cannot take — the padding is capped at a quarter of the smaller
//!     side, so the width survives, and the ladder empties all four other
//!     bands before it would eat into `BOARD_SHARE`, so the height does too —
//!     and there is no window down to 1x1 in which `l.board` is empty.
//!     `the_board_is_drawn_in_every_window_however_small` is that claim as a
//!     test rather than as prose, and is now the only thing licensing the
//!     pass to go without a guard. See `known-issues.md` lesson 61: the
//!     exception you carve out of a rule is the one place a comment ends up
//!     doing the work of a test.
//! 16. **The AI's move was witnessed only by the AI's own report of it.**
//!     `the_search_plays_the_move_it_owes_and_nothing_else` checked that the
//!     column `ai_turn` returned was the column that appeared in the move
//!     list — but both come out of the same two lines, so a fault between
//!     choosing the column and dropping into it moves the witness with the
//!     move. Mutation shifted the played column one to the right and no test
//!     failed. The test now asks `ai_best_move` what it would choose, on a
//!     clone of the board, before letting `ai_turn` play at all.

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
        // Nobody wins by not being there. Without this, every run on an empty
        // board is four cells of one "colour" and `has_won(Cell::Empty)` is
        // true before a piece has been played — the same trap `line_owner`
        // sidesteps, and what lets the two scans be claimed to agree for
        // *every* argument rather than only for the two the callers pass.
        if player == Cell::Empty {
            return false;
        }
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
    // `None` until a move has genuinely been chosen. This was seeded with the
    // first legal move, described as the answer for a loop that alpha-beta cut
    // off before anything beat `best_score` — a case that cannot arise: the
    // first move compares against `i32::MIN`/`i32::MAX`, which no score can
    // equal, so the first move always takes the seat, and the cut below is
    // reached only after that assignment. The seed was therefore a value no
    // caller could ever read, and worse, it made "the search chose nothing"
    // indistinguishable from "the search chose the first column it tried".
    let mut best_col = None;
    let deeper = depth.saturating_sub(1);
    let mover = if maximizing {
        ai_player
    } else {
        ai_player.opponent()
    };

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
                best_col = Some(col);
            }
            alpha = alpha.max(score);
        } else {
            if score < best_score {
                best_score = score;
                best_col = Some(col);
            }
            beta = beta.min(score);
        }
        if alpha >= beta {
            break;
        }
    }
    (best_score, best_col)
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
    // No `r.is_empty()` guard, and none is needed: `fill` and `centred` return
    // on an empty box, and `Frame::hit` refuses to record one — so a button
    // with no box paints nothing and takes no clicks whichever way round it is
    // written. It had one, and mutation testing could not tell it from its own
    // absence, which is lesson 51's signature.
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
/// Everything a drop changes *and could have found in more than one state*, in
/// one value. Restoring a snapshot cannot put four fields back and forget the
/// fifth, which is the fault an undo-by-field-list keeps having — this
/// program's neighbour, 2048, shipped an undo that restored the grid and the
/// score and left the *status* alone, so taking back the winning move left
/// "You win!" over a board with no win on it.
///
/// The status and the winning line are deliberately *not* among the fields.
/// A snapshot is only ever taken in `drop_at`, past its refusal to move on a
/// game that is over, so the position recorded is always one still in play:
/// storing those two would be storing a constant, and `restore` sets them back
/// to that constant instead. See `restore`.
#[derive(Debug, Clone)]
struct Snapshot {
    board: Board,
    current_player: Cell,
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
            moves: self.move_history.len(),
            human_wins: self.human_wins,
            ai_wins: self.ai_wins,
            draws: self.draws,
        }
    }

    /// Puts a snapshot back, and puts the game back *in play*.
    ///
    /// The status is set rather than restored: every snapshot is taken of a
    /// live game (see `Snapshot`), so the position being returned to is one
    /// still being played by construction. This is the line that takes the
    /// "You win!" off the board when the winning move is taken back — and
    /// while the status travelled in the snapshot it was a field that could
    /// only ever hold `Playing`, so nothing could tell a restore that read it
    /// from one that ignored it.
    fn restore(&mut self, s: Snapshot) {
        self.move_history.truncate(s.moves);
        self.board = s.board;
        self.current_player = s.current_player;
        self.status = GameStatus::Playing;
        self.win_line = None;
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
        // *Anything* shuts it — which is the answer a click gets and the one
        // the sheet's own closing line promises.
        //
        // The two intents that are about the sheet used to be excused from
        // this rule and left to fall through to their own arms, on the
        // reasoning that shutting the sheet was what they were going to do
        // anyway. That reasoning is exactly why the exemption was worth
        // nothing: `H` toggled a shown sheet to hidden and Escape closed it,
        // both ending where this line ends, so no key could tell the two paths
        // apart. All the exemption bought was two branches below that could
        // never be taken with the sheet up.
        if self.show_help {
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
            // Past the guard above the sheet is always down, so these two are
            // "open it" and "there is nothing to close" — not a toggle and a
            // conditional close, which is how they were written while the
            // guard let them through.
            Intent::ToggleHelp => {
                self.show_help = true;
                EventResult::Consumed
            }
            Intent::CloseHelp => EventResult::Ignored,
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
        // No "did the header fit?" guard. A band that did not fit is
        // `Rect::EMPTY`, and every call below already refuses one: `fill` and
        // `centred` return on an empty box, `score_box` returns empty boxes of
        // its own, and the title's width comes out negative and `label`
        // declines it. A guard here would restate a rule that already holds in
        // four places — `nth_of` refuses the same guard for the same reason,
        // and `known-issues.md` lesson 51 is what happens when one is written
        // anyway: a line no test can own, because deleting it changes nothing.
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
        // No band guard, for the reason given in `draw_header`: `centred`
        // refuses an empty box, and there is nothing else here to refuse.
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
        // No band guard: `chute_slot` asks `shows` itself and hands back
        // `Rect::EMPTY` for every column when the chute did not fit, which the
        // skip below already handles. See `draw_header`.
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
        // No band guard here either, and this one took two tries to see. The
        // guard was kept on the reasoning that the line joining the winning
        // four is pushed unconditionally below, so a board with no room would
        // leave a zero-length stroke in the corner of the window. The premise
        // is false: the board is the one band the drop ladder cannot take, and
        // `the_board_is_drawn_in_every_window_however_small` is the test that
        // says so — the padding is capped at a quarter of the smaller side, so
        // the width always survives, and the ladder empties every band before
        // it would touch the board's `BOARD_SHARE`, so the height does too.
        // A guard against a case the layout cannot produce is lesson 51 again,
        // and the comment defending it was doing the same work as the guard.
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
        // As in `draw_header`: no guard on the band. `footer_button` turns an
        // empty footer into empty buttons unaided, and an empty button paints
        // nothing and records no hit — so a window with no footer has no
        // footer controls without anything here saying so.
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
    // Presses only. A `KeyEvent` carries `pressed`, and the compositor sends one
    // with `pressed: false` for every key-up (`gui/compositor/src/lib.rs`), so a
    // handler that does not read it runs each action twice — once when the key
    // goes down and once when it comes back up. For this game that is two pieces
    // per keystroke: the player's move, then the same move again into the next
    // free cell, played on behalf of the AI. This file shipped without the
    // check while `maze`, `life`, `game2048`, `sudoku`, `minesweeper` and
    // `wordsearch` all had it — the sixth app in a row to lose the same line,
    // which is what a rule kept by copying looks like from the inside.
    if !ev.pressed {
        return None;
    }
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    /// Windows to check the layout against, from a desktop down to something
    /// no sane person would resize to.
    ///
    /// Each of the last five earns its place by making a rule bind that binds
    /// nowhere else in the list:
    ///
    /// - `(120, 800)` is tall enough for every band and too narrow for the
    ///   third score readout, which is the only condition under which
    ///   `score_box`'s "ran off the left edge" refusal fires.
    /// - `(600, 150)` and `(300, 110)` are short enough that the drop ladder
    ///   runs, which is the only condition under which `BOARD_SHARE` and
    ///   `BAND_DROP_ORDER` have any effect at all.
    /// - `(24, 24)` drops every band, so the board is drawn with no chrome
    ///   above or below it -- the case where `column`'s "no chute, start at the
    ///   board" arm is the one taken.
    /// - `(4, 4)` is here for the padding and the font floor: at every other
    ///   size the padding lands on its 2px floor, so the clamp's upper bound --
    ///   a quarter of the smaller side, which stops the padding eating the
    ///   thing it pads -- never binds; and it is the only size at which the
    ///   help sheet's type is driven below one pixel.
    const WINDOWS: [(f32, f32); 12] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (480.0, 400.0),
        (400.0, 640.0),
        (120.0, 800.0),
        (600.0, 150.0),
        (640.0, 200.0),
        (300.0, 110.0),
        (90.0, 120.0),
        (24.0, 24.0),
        (4.0, 4.0),
    ];

    // ── Fixtures ──

    fn game() -> Connect4 {
        Connect4::new()
    }

    /// A game whose stored window is `(width, height)`, which is the size a
    /// click on it will be read against.
    fn windowed(width: f32, height: f32) -> Connect4 {
        let mut app = game();
        app.resize(width, height);
        app
    }

    /// A game with a search shallow enough to run inside a test.
    ///
    /// The depth is the AI's strength, not its rules: every test that drives a
    /// tick is asking whether the move is *played*, not whether it is good, and
    /// the two tests that do ask about the move's quality build the position so
    /// that `ai_best_move` answers from its immediate win/block scan without
    /// searching at all.
    fn shallow() -> Connect4 {
        let mut app = game();
        app.ai_depth = 2;
        app
    }

    /// A full board with no four in a row anywhere: rows in pairs, each pair
    /// offset by one column from the pair below.
    ///
    /// Checked by `a_full_board_with_no_line_on_it_is_a_draw` rather than
    /// asserted here, so the fixture's own claim is a test and not a comment.
    fn drawn_board() -> Board {
        let mut b = Board::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                let piece = if (row / 2 + col) % 2 == 0 {
                    Cell::Red
                } else {
                    Cell::Yellow
                };
                assert!(b.set(row, col, piece), "({row}, {col}) is on the board");
            }
        }
        b.heights = [ROWS; COLS];
        b.piece_count = CELL_COUNT;
        b
    }

    /// Four of `player`'s pieces along the bottom row, starting at column 0.
    fn won_board(player: Cell) -> Board {
        let mut b = Board::new();
        for col in 0..RUN {
            b.drop_piece(col, player);
        }
        b
    }

    fn press(app: &mut Connect4, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn click(app: &mut Connect4, x: f32, y: f32) -> EventResult {
        let size = (app.width, app.height);
        app.click_at(x, y, MouseButton::Left, size)
    }

    /// Click the middle of the named control, at the app's current size.
    fn click_on(app: &mut Connect4, target: Target) -> EventResult {
        let size = (app.width, app.height);
        probe::click_sized(app, target, MouseButton::Left, size)
    }

    fn tick(app: &mut Connect4) -> EventResult {
        handle_event(app, &Event::Tick { elapsed_ms: 60 })
    }

    fn texts(f: &Frame) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every line in the frame with the box it will actually cover.
    ///
    /// The width is the one that reaches the screen, not the one the whole
    /// string measures to: a label carries a maximum width and is elided to fit
    /// it, so measuring the body would report an overflow that is never painted
    /// -- and would make every one of these tests a test of the string rather
    /// than of the layout.
    fn text_boxes(f: &Frame) -> Vec<(String, Rect, f32)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    x,
                    y,
                    font_size,
                    font_weight,
                    max_width,
                    ..
                } => {
                    let full = text::measure(text, *font_size, *font_weight);
                    let w = max_width.map_or(full, |m| full.min(m));
                    Some((
                        text.clone(),
                        Rect::new(*x, *y, w, text::line_height(*font_size, *font_weight)),
                        *font_size,
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// The colour the frame last painted the given box, read out of the
    /// commands rather than out of the function that chose it.
    fn fill_at(f: &Frame, r: Rect) -> Option<Color> {
        f.commands().iter().rev().find_map(|c| match c {
            RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color,
                ..
            } if (*x - r.x).abs() < 0.01
                && (*y - r.y).abs() < 0.01
                && (*width - r.w).abs() < 0.01
                && (*height - r.h).abs() < 0.01 =>
            {
                Some(*color)
            }
            _ => None,
        })
    }

    fn hits_for(f: &Frame, target: Target) -> Vec<Rect> {
        f.hits()
            .iter()
            .filter(|(t, _)| *t == target)
            .map(|&(_, r)| r)
            .collect()
    }

    // ── Run geometry: the one place a run's bounds are checked ──

    #[test]
    fn a_run_that_fits_reports_the_four_cells_it_covers() {
        assert_eq!(
            line_cells(1, 2, 0, 1),
            Some([(1, 2), (1, 3), (1, 4), (1, 5)]),
            "a horizontal run steps the column and holds the row"
        );
    }

    #[test]
    fn a_run_that_would_leave_the_right_edge_is_refused() {
        // Column 4 is on the board and so is a run starting there in every
        // other direction -- it is the fourth cell, column 7, that is not.
        assert!(line_cells(0, 4, 0, 1).is_none(), "a run ran off the right");
        assert!(line_cells(0, 3, 0, 1).is_some(), "the last run that fits");
    }

    #[test]
    fn a_run_that_would_leave_the_top_is_refused() {
        assert!(line_cells(3, 0, 1, 0).is_none(), "a run ran off the top");
        assert!(line_cells(2, 0, 1, 0).is_some(), "the last run that fits");
    }

    #[test]
    fn a_run_that_would_leave_the_left_edge_is_refused() {
        // The down-left direction is the only one that can produce a negative
        // coordinate, and a negative coordinate has no `usize` -- which is the
        // bound, not a comparison written next to it.
        assert!(line_cells(0, 2, 1, -1).is_none(), "a run ran off the left");
        assert!(line_cells(0, 3, 1, -1).is_some(), "the last run that fits");
    }

    #[test]
    fn a_run_starting_off_the_board_is_refused() {
        assert!(line_cells(ROWS, 0, 0, 1).is_none(), "row past the top");
        assert!(line_cells(0, COLS, 1, 0).is_none(), "column past the right");
    }

    #[test]
    fn the_board_holds_sixty_nine_runs_of_four() {
        // 24 across (6 rows x 4 starts), 21 up (7 columns x 3 starts) and 12
        // on each diagonal. Counted here so that a change to `ROWS`, `COLS` or
        // `RUN` has to be looked at rather than absorbed.
        assert_eq!(all_lines().count(), 69);
    }

    #[test]
    fn no_run_is_yielded_twice() {
        let mut seen: Vec<[(usize, usize); RUN]> = Vec::new();
        for cells in all_lines() {
            assert!(
                !seen.contains(&cells),
                "{cells:?} was yielded more than once"
            );
            seen.push(cells);
        }
    }

    #[test]
    fn a_run_and_the_same_run_walked_backwards_are_not_both_listed() {
        // The reverse of a run is the same four cells in the other order, so a
        // direction table carrying both would find every win twice and double
        // every window's contribution to the AI's score.
        let mut sorted: Vec<Vec<(usize, usize)>> = all_lines()
            .map(|cells| {
                let mut v = cells.to_vec();
                v.sort_unstable();
                v
            })
            .collect();
        let before = sorted.len();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "a run is listed in both directions");
    }

    #[test]
    fn every_run_lies_entirely_on_the_board() {
        for cells in all_lines() {
            for (row, col) in cells {
                assert!(row < ROWS && col < COLS, "{cells:?} leaves the board");
            }
        }
    }

    // ── The board ──

    #[test]
    fn a_new_board_is_empty_everywhere() {
        let b = Board::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                assert_eq!(b.get(row, col), Cell::Empty, "({row}, {col})");
            }
        }
        assert_eq!(b.pieces(), 0);
        assert!(!b.is_full());
    }

    #[test]
    fn a_dropped_piece_lands_on_the_floor_of_an_empty_column() {
        let mut b = Board::new();
        assert_eq!(b.drop_piece(3, Cell::Red), Some(0), "row 0 is the bottom");
        assert_eq!(b.get(0, 3), Cell::Red);
    }

    #[test]
    fn a_dropped_piece_lands_on_top_of_what_is_already_there() {
        let mut b = Board::new();
        b.drop_piece(3, Cell::Red);
        assert_eq!(b.drop_piece(3, Cell::Yellow), Some(1));
        assert_eq!(b.get(0, 3), Cell::Red, "the piece underneath moved");
        assert_eq!(b.get(1, 3), Cell::Yellow);
    }

    #[test]
    fn a_drop_lands_in_the_column_it_was_aimed_at_and_no_other() {
        let mut b = Board::new();
        b.drop_piece(2, Cell::Red);
        for col in 0..COLS {
            let expected = if col == 2 { Cell::Red } else { Cell::Empty };
            assert_eq!(b.get(0, col), expected, "bottom of column {col}");
        }
    }

    #[test]
    fn a_full_column_takes_no_more() {
        let mut b = Board::new();
        for _ in 0..ROWS {
            assert!(b.drop_piece(0, Cell::Red).is_some());
        }
        assert!(!b.can_drop(0), "a column with six pieces still says yes");
        assert_eq!(b.drop_piece(0, Cell::Yellow), None);
        assert_eq!(b.pieces(), ROWS, "the refused drop was counted");
    }

    #[test]
    fn a_column_that_is_not_on_the_board_takes_nothing() {
        let mut b = Board::new();
        assert!(!b.can_drop(COLS));
        assert_eq!(b.drop_piece(COLS, Cell::Red), None);
        assert_eq!(b.pieces(), 0);
    }

    #[test]
    fn a_cell_off_the_board_reads_as_empty_rather_than_panicking() {
        let b = won_board(Cell::Red);
        assert_eq!(b.get(ROWS, 0), Cell::Empty, "past the top");
        assert_eq!(b.get(0, COLS), Cell::Empty, "past the right");
        assert_eq!(b.get(usize::MAX, usize::MAX), Cell::Empty);
    }

    #[test]
    fn taking_a_piece_back_returns_the_piece_that_was_taken() {
        let mut b = Board::new();
        b.drop_piece(1, Cell::Red);
        b.drop_piece(1, Cell::Yellow);
        assert_eq!(b.undo_drop(1), Some(Cell::Yellow), "the top piece");
        assert_eq!(b.undo_drop(1), Some(Cell::Red));
        assert_eq!(b.undo_drop(1), None, "an empty column had a piece to give");
    }

    #[test]
    fn taking_a_piece_back_empties_the_cell_it_came_from() {
        let mut b = Board::new();
        b.drop_piece(1, Cell::Red);
        b.undo_drop(1);
        assert_eq!(b.get(0, 1), Cell::Empty, "the cell kept its piece");
        assert_eq!(b.pieces(), 0);
        assert!(b.can_drop(1));
    }

    #[test]
    fn taking_a_piece_off_a_column_that_is_not_on_the_board_is_refused() {
        // This was the one place in the drop/undo pair where the column index
        // went unchecked: an off-board column that `can_drop` merely declined
        // would panic here.
        let mut b = Board::new();
        assert_eq!(b.undo_drop(COLS), None);
        assert_eq!(b.undo_drop(usize::MAX), None);
    }

    #[test]
    fn a_drop_and_the_undo_of_it_leave_the_board_exactly_as_it_was() {
        let mut b = Board::new();
        b.drop_piece(0, Cell::Red);
        b.drop_piece(3, Cell::Yellow);
        let before = b.clone();
        b.drop_piece(3, Cell::Red);
        b.undo_drop(3);
        assert_eq!(b, before, "the round trip changed the board");
    }

    #[test]
    fn a_board_is_full_only_when_every_cell_is() {
        // Filled by dropping, not by `drawn_board`, which sets `piece_count`
        // by hand: asked about a board whose tally the *fixture* wrote, this
        // test could not tell whether `drop_piece` keeps one at all
        // (`known-issues.md` lesson 52). It now counts every drop on the way
        // up, so the tally is the code's own.
        let mut b = Board::new();
        for col in 0..COLS {
            for row in 0..ROWS {
                assert!(!b.is_full(), "full at {} pieces", b.pieces());
                assert_eq!(b.pieces(), col * ROWS + row);
                b.drop_piece(col, Cell::Red).expect("the column has room");
            }
        }
        assert!(b.is_full());
        assert_eq!(b.pieces(), CELL_COUNT);
        b.undo_drop(0);
        assert!(!b.is_full(), "a board with a hole in it claimed to be full");
    }

    #[test]
    fn with_move_hands_the_position_the_move_makes_to_its_caller() {
        let mut b = Board::new();
        let seen = b.with_move(4, Cell::Yellow, |after| after.get(0, 4));
        assert_eq!(seen, Some(Cell::Yellow), "the move was not there to see");
    }

    #[test]
    fn with_move_leaves_the_board_exactly_as_it_found_it() {
        let mut b = won_board(Cell::Red);
        let before = b.clone();
        b.with_move(6, Cell::Yellow, |_| ());
        assert_eq!(b, before);
    }

    #[test]
    fn with_move_on_a_full_column_runs_nothing_and_undoes_nothing() {
        // The AI used to drop and undo as two separate statements and discard
        // both results, so a refused drop still ran the undo -- taking back a
        // piece some earlier move had put there.
        let mut b = Board::new();
        for _ in 0..ROWS {
            b.drop_piece(2, Cell::Red);
        }
        let before = b.clone();
        let mut ran = false;
        let out = b.with_move(2, Cell::Yellow, |_| {
            ran = true;
        });
        assert_eq!(out, None, "a refused move reported a result");
        assert!(!ran, "the body ran on a move that was never made");
        assert_eq!(b, before, "the undo of a move never made took a piece");
    }

    #[test]
    fn nobodys_opponent_is_nobody() {
        assert_eq!(Cell::Red.opponent(), Cell::Yellow);
        assert_eq!(Cell::Yellow.opponent(), Cell::Red);
        assert_eq!(Cell::Empty.opponent(), Cell::Empty);
    }

    #[test]
    fn the_playable_columns_are_ordered_from_the_centre_outwards() {
        // Move ordering for alpha-beta, not a preference of the game's: the
        // centre column is on more runs than any other, so trying it first is
        // what makes the search's cutoffs happen early.
        assert_eq!(Board::new().valid_moves(), vec![3, 2, 4, 1, 5, 0, 6]);
    }

    #[test]
    fn a_full_column_is_not_offered_as_a_move() {
        let mut b = Board::new();
        for _ in 0..ROWS {
            b.drop_piece(3, Cell::Red);
        }
        let moves = b.valid_moves();
        assert!(!moves.contains(&3), "a full column was still offered");
        assert_eq!(moves.len(), COLS - 1, "another column went missing with it");
    }

    #[test]
    fn a_full_board_offers_no_move_at_all() {
        assert!(drawn_board().valid_moves().is_empty());
    }

    // ── Outcome: one function, one scan, one answer ──

    #[test]
    fn an_empty_board_is_still_being_played() {
        assert_eq!(Board::new().outcome(), (GameStatus::Playing, None));
    }

    #[test]
    fn three_in_a_row_is_not_a_win() {
        let mut b = Board::new();
        for col in 0..3 {
            b.drop_piece(col, Cell::Red);
        }
        assert_eq!(b.outcome().0, GameStatus::Playing, "three won the game");
    }

    #[test]
    fn four_across_the_bottom_wins_and_names_its_four_cells() {
        let (status, line) = won_board(Cell::Red).outcome();
        assert_eq!(status, GameStatus::Won(Cell::Red));
        assert_eq!(
            line.expect("a win with no line to show").cells,
            [(0, 0), (0, 1), (0, 2), (0, 3)]
        );
    }

    #[test]
    fn four_across_wins_away_from_the_left_edge_too() {
        let mut b = Board::new();
        for col in 3..COLS {
            b.drop_piece(col, Cell::Yellow);
        }
        let (status, line) = b.outcome();
        assert_eq!(status, GameStatus::Won(Cell::Yellow));
        assert_eq!(
            line.unwrap().cells,
            [(0, 3), (0, 4), (0, 5), (0, 6)],
            "the run against the right edge"
        );
    }

    #[test]
    fn four_up_a_column_wins() {
        let mut b = Board::new();
        for _ in 0..RUN {
            b.drop_piece(5, Cell::Yellow);
        }
        let (status, line) = b.outcome();
        assert_eq!(status, GameStatus::Won(Cell::Yellow));
        assert_eq!(line.unwrap().cells, [(0, 5), (1, 5), (2, 5), (3, 5)]);
    }

    #[test]
    fn four_up_a_column_wins_when_it_does_not_start_at_the_floor() {
        // A vertical scan anchored at row 0 rather than at every row would miss
        // this: the run is rows 2..5, on top of two pieces of the other colour.
        let mut b = Board::new();
        b.drop_piece(5, Cell::Red);
        b.drop_piece(5, Cell::Red);
        for _ in 0..RUN {
            b.drop_piece(5, Cell::Yellow);
        }
        assert_eq!(b.outcome().0, GameStatus::Won(Cell::Yellow));
        assert_eq!(
            b.outcome().1.unwrap().cells,
            [(2, 5), (3, 5), (4, 5), (5, 5)]
        );
    }

    #[test]
    fn four_up_and_to_the_right_wins() {
        // Built by dropping, so every piece is supported the way a real game's
        // would be -- a diagonal written straight into the grid could sit on
        // nothing and would not test the position a game can actually reach.
        let mut b = Board::new();
        let staircase = [
            (0, Cell::Red),
            (1, Cell::Yellow),
            (1, Cell::Red),
            (2, Cell::Yellow),
            (2, Cell::Yellow),
            (2, Cell::Red),
            (3, Cell::Yellow),
            (3, Cell::Yellow),
            (3, Cell::Yellow),
            (3, Cell::Red),
        ];
        for (col, piece) in staircase {
            b.drop_piece(col, piece);
        }
        let (status, line) = b.outcome();
        assert_eq!(status, GameStatus::Won(Cell::Red));
        assert_eq!(line.unwrap().cells, [(0, 0), (1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn four_up_and_to_the_left_wins() {
        let mut b = Board::new();
        let staircase = [
            (6, Cell::Red),
            (5, Cell::Yellow),
            (5, Cell::Red),
            (4, Cell::Yellow),
            (4, Cell::Yellow),
            (4, Cell::Red),
            (3, Cell::Yellow),
            (3, Cell::Yellow),
            (3, Cell::Yellow),
            (3, Cell::Red),
        ];
        for (col, piece) in staircase {
            b.drop_piece(col, piece);
        }
        let (status, line) = b.outcome();
        assert_eq!(status, GameStatus::Won(Cell::Red));
        assert_eq!(
            line.unwrap().cells,
            [(0, 6), (1, 5), (2, 4), (3, 3)],
            "the run is reported from its lowest cell"
        );
    }

    #[test]
    fn a_line_broken_by_the_other_colour_is_not_a_win() {
        let mut b = Board::new();
        b.drop_piece(0, Cell::Red);
        b.drop_piece(1, Cell::Red);
        b.drop_piece(2, Cell::Yellow);
        b.drop_piece(3, Cell::Red);
        b.drop_piece(4, Cell::Red);
        assert_eq!(b.outcome().0, GameStatus::Playing);
    }

    #[test]
    fn a_run_of_four_empty_cells_is_not_a_win_for_nobody() {
        // Every run on an empty board is four cells of one "colour", and that
        // colour is `Empty`. A scan that only checked "are these four alike"
        // would report `Won(Empty)` before the first piece was played.
        assert_eq!(Board::new().outcome().0, GameStatus::Playing);
        assert!(
            Board::new()
                .line_owner([(0, 0), (0, 1), (0, 2), (0, 3)])
                .is_none()
        );
    }

    #[test]
    fn a_full_board_with_no_line_on_it_is_a_draw() {
        let (status, line) = drawn_board().outcome();
        assert_eq!(status, GameStatus::Draw, "the draw fixture has a win on it");
        assert_eq!(line, None, "a draw named a winning line");
    }

    #[test]
    fn a_full_board_with_a_line_on_it_is_a_win_and_not_a_draw() {
        // The order of the two questions is the rule: a board can be both full
        // and won, and the game it was won on did not end in a draw.
        let mut b = drawn_board();
        for col in 0..RUN {
            assert!(b.set(0, col, Cell::Red));
        }
        assert_eq!(b.outcome().0, GameStatus::Won(Cell::Red));
    }

    #[test]
    fn the_win_scan_and_the_ai_s_form_of_it_agree() {
        // `outcome` says which four cells; `has_won` says only whether. They
        // scan the same runs in the same order, and a disagreement would have
        // the search exploring positions the game already considers finished.
        for board in [
            Board::new(),
            won_board(Cell::Red),
            won_board(Cell::Yellow),
            drawn_board(),
        ] {
            let by_outcome = match board.outcome().0 {
                GameStatus::Won(winner) => Some(winner),
                _ => None,
            };
            for player in [Cell::Red, Cell::Yellow] {
                assert_eq!(
                    board.has_won(player),
                    by_outcome == Some(player),
                    "the two scans disagree about {player:?}"
                );
            }
        }
    }

    #[test]
    fn nobody_has_won_an_empty_board_including_nobody() {
        let b = Board::new();
        assert!(!b.has_won(Cell::Red));
        assert!(!b.has_won(Cell::Yellow));
        assert!(!b.has_won(Cell::Empty), "the empty cells won the game");
    }

    // ── The AI ──

    #[test]
    fn a_window_of_four_is_worth_a_win() {
        let w = [Cell::Red; RUN];
        assert_eq!(evaluate_window(&w, Cell::Red), SCORE_WIN);
        assert_eq!(evaluate_window(&w, Cell::Yellow), 0, "the loser scored it");
    }

    #[test]
    fn three_with_a_gap_is_worth_more_than_two_with_two() {
        let three = [Cell::Red, Cell::Red, Cell::Red, Cell::Empty];
        let two = [Cell::Red, Cell::Red, Cell::Empty, Cell::Empty];
        assert_eq!(evaluate_window(&three, Cell::Red), SCORE_THREE);
        assert_eq!(evaluate_window(&two, Cell::Red), SCORE_TWO);
        // Scored, not named: comparing the two constants is a claim about the
        // ladder the compiler can settle, and clippy rightly refuses it.
        assert!(
            evaluate_window(&three, Cell::Red) > evaluate_window(&two, Cell::Red),
            "the ladder is upside down"
        );
    }

    #[test]
    fn the_opponents_three_with_a_gap_scores_against_you() {
        let w = [Cell::Yellow, Cell::Yellow, Cell::Yellow, Cell::Empty];
        assert_eq!(evaluate_window(&w, Cell::Red), SCORE_OPP_THREE);
        assert!(
            evaluate_window(&w, Cell::Red) < 0,
            "a threat against you scored for you"
        );
    }

    #[test]
    fn a_window_both_colours_are_in_is_worth_nothing_to_either() {
        // Neither can complete it, so it is not a threat and not a chance.
        let w = [Cell::Red, Cell::Yellow, Cell::Red, Cell::Empty];
        assert_eq!(evaluate_window(&w, Cell::Red), 0);
        assert_eq!(evaluate_window(&w, Cell::Yellow), 0);
    }

    #[test]
    fn an_empty_window_is_worth_nothing() {
        assert_eq!(evaluate_window(&[Cell::Empty; RUN], Cell::Red), 0);
    }

    #[test]
    fn three_with_no_gap_to_complete_them_is_worth_nothing() {
        // Three of yours and one of theirs is a dead run, not a near-win. The
        // `empty_count == 1` clause is what tells the two apart.
        let w = [Cell::Red, Cell::Red, Cell::Red, Cell::Yellow];
        assert_eq!(evaluate_window(&w, Cell::Red), 0);
    }

    #[test]
    fn a_piece_in_the_centre_column_is_worth_more_than_one_at_the_edge() {
        let mut centre = Board::new();
        centre.drop_piece(CENTER_COL, Cell::Red);
        let mut edge = Board::new();
        edge.drop_piece(0, Cell::Red);
        assert!(
            evaluate_board(&centre, Cell::Red) > evaluate_board(&edge, Cell::Red),
            "the centre bonus is not being paid"
        );
    }

    #[test]
    fn the_centre_bonus_is_paid_to_the_player_it_is_evaluated_for() {
        let mut b = Board::new();
        b.drop_piece(CENTER_COL, Cell::Red);
        assert!(
            evaluate_board(&b, Cell::Red) > evaluate_board(&b, Cell::Yellow),
            "the centre piece counted for the player who did not play it"
        );
    }

    #[test]
    fn an_empty_board_is_worth_the_same_to_both_players() {
        let b = Board::new();
        assert_eq!(
            evaluate_board(&b, Cell::Red),
            evaluate_board(&b, Cell::Yellow)
        );
    }

    #[test]
    fn a_position_is_scored_by_the_runs_on_it_and_not_only_by_the_centre_column() {
        // Two boards with the same number of pieces and the same number of them
        // in the centre column -- none, so the centre bonus is equal — and a
        // line of three on one of them. Without the window scan the two score
        // the same, and a search told to maximise that score has no reason to
        // build a run at all.
        let mut threat = Board::new();
        for col in 0..3 {
            threat.drop_piece(col, Cell::Red);
        }
        let mut scattered = Board::new();
        for col in [0, 2, 4] {
            scattered.drop_piece(col, Cell::Red);
        }
        assert_eq!(threat.pieces(), scattered.pieces());
        assert!(
            evaluate_board(&threat, Cell::Red) > evaluate_board(&scattered, Cell::Red),
            "three in a line is worth no more than three pieces in none"
        );
    }

    #[test]
    fn a_position_is_over_when_someone_has_won() {
        assert!(is_terminal(&won_board(Cell::Red)));
        assert!(is_terminal(&won_board(Cell::Yellow)));
    }

    #[test]
    fn a_position_is_over_when_the_board_is_full() {
        assert!(is_terminal(&drawn_board()));
    }

    #[test]
    fn a_position_with_room_and_no_win_is_not_over() {
        assert!(!is_terminal(&Board::new()));
    }

    #[test]
    fn the_ai_plays_the_move_that_wins_now() {
        let mut b = Board::new();
        for col in 0..3 {
            b.drop_piece(col, Cell::Yellow);
        }
        assert_eq!(ai_best_move(&mut b, Cell::Yellow, 1), Some(3));
        // And at depth 0, where the search returns no column at all: the scan
        // for a win on the board *now* is a rule of its own, not an
        // optimisation of the search, and at any depth that can see the win
        // the two agree — which is why deleting the scan changed nothing here
        // until this line was added.
        assert_eq!(
            ai_best_move(&mut b, Cell::Yellow, 0),
            Some(3),
            "with no search left, the win in front of it went unplayed"
        );
    }

    #[test]
    fn the_ai_blocks_the_move_that_would_lose_now() {
        let mut b = Board::new();
        for col in 0..3 {
            b.drop_piece(col, Cell::Red);
        }
        assert_eq!(ai_best_move(&mut b, Cell::Yellow, 1), Some(3));
        // As above: at depth 0 only the scan can answer, so this is the line
        // that holds the *block* half of it. At depth 1 the search reaches the
        // same column by way of `SCORE_OPP_THREE` and covers for its absence.
        assert_eq!(
            ai_best_move(&mut b, Cell::Yellow, 0),
            Some(3),
            "with no search left, the loss in front of it went unblocked"
        );
    }

    #[test]
    fn the_ai_takes_its_own_win_rather_than_blocking_theirs() {
        // Yellow can complete a column at 6 and Red can complete a row at 3.
        // Winning ends the game; blocking only postpones theirs.
        let mut b = Board::new();
        for col in 0..3 {
            b.drop_piece(col, Cell::Red);
        }
        for _ in 0..3 {
            b.drop_piece(6, Cell::Yellow);
        }
        assert_eq!(
            ai_best_move(&mut b, Cell::Yellow, 1),
            Some(6),
            "it blocked a loss it could have avoided by winning"
        );
    }

    #[test]
    fn the_ai_finds_a_move_on_any_board_with_room() {
        let mut b = Board::new();
        b.drop_piece(0, Cell::Red);
        let col = ai_best_move(&mut b, Cell::Yellow, 2).expect("no move on a board with room");
        assert!(b.can_drop(col), "it chose a column it cannot play");
    }

    #[test]
    fn the_ai_has_no_move_on_a_full_board() {
        assert_eq!(ai_best_move(&mut drawn_board(), Cell::Yellow, 2), None);
    }

    #[test]
    fn the_ai_leaves_the_board_it_was_asked_about_alone() {
        // It searches by playing and taking back on the caller's board, so a
        // single missed undo would leave a phantom piece in the real game.
        let mut b = Board::new();
        b.drop_piece(3, Cell::Red);
        let before = b.clone();
        ai_best_move(&mut b, Cell::Yellow, 3);
        assert_eq!(b, before, "the search left something behind");
    }

    #[test]
    fn a_search_that_can_see_the_win_scores_it_as_a_win() {
        // The win is a stack in column 6, which is *last* in the move order —
        // so "the search chose this column" and "the search handed back the
        // column it happened to try first" are different answers here. They
        // were not while the winning move was in column 3, which is where the
        // move order starts, and a search that named its first move regardless
        // of the score passed this test for that reason alone.
        let mut b = Board::new();
        for _ in 0..3 {
            b.drop_piece(6, Cell::Yellow);
        }
        let order = Board::new().valid_moves();
        assert_eq!(
            order.first(),
            Some(&CENTER_COL),
            "the move order no longer starts in the centre"
        );
        assert_eq!(order.last(), Some(&6), "column 6 is no longer tried last");
        let (score, col) = minimax(&mut b, 2, i32::MIN, i32::MAX, true, Cell::Yellow);
        assert!(score >= SCORE_WIN, "a forced win scored {score}");
        assert_eq!(col, Some(6));
    }

    #[test]
    fn the_side_that_is_not_the_ai_picks_the_move_that_hurts_it_most() {
        // Every production caller enters the search on the AI's turn and reads
        // only the root's column, so the minimising half of it — the half that
        // makes the search a search and not a one-ply greedy pick — is only
        // ever exercised through what it returns upwards. Entered directly, it
        // has to name the move as well: Red has three in column 0 and it is
        // Red to play, and column 0 is neither the first move the order tries
        // (3) nor the last (6), so naming it is a real answer.
        let mut b = Board::new();
        for _ in 0..3 {
            b.drop_piece(0, Cell::Red);
        }
        let (score, col) = minimax(&mut b, 1, i32::MIN, i32::MAX, false, Cell::Yellow);
        assert!(
            score <= -SCORE_WIN,
            "the opponent's forced win scored {score} to the searcher"
        );
        assert_eq!(col, Some(0), "the losing move was not the one reported");
    }

    #[test]
    fn a_threat_the_search_cannot_block_is_scored_as_a_loss() {
        // Red holds the middle three of the bottom row with both ends open, so
        // whichever end Yellow blocks, Red takes the other. A search in which
        // both plies play the AI's own pieces never sees the reply and scores
        // this comfortable.
        let mut b = Board::new();
        for col in 1..4 {
            b.drop_piece(col, Cell::Red);
        }
        let (score, col) = minimax(&mut b, 2, i32::MIN, i32::MAX, true, Cell::Yellow);
        assert!(
            score <= -SCORE_WIN,
            "an open-ended three scored {score} to the side that cannot stop it"
        );
        let col = col.expect("a board with room reported no move");
        assert!(b.can_drop(col), "the search named a column it cannot play");
    }

    #[test]
    fn a_win_found_sooner_is_worth_more_than_the_same_win_found_later() {
        // Without the depth bonus every winning line scores identically and the
        // search has no reason to prefer the one that ends the game now.
        let won = won_board(Cell::Yellow);
        let (near, _) = minimax(&mut won.clone(), 5, i32::MIN, i32::MAX, true, Cell::Yellow);
        let (far, _) = minimax(&mut won.clone(), 1, i32::MIN, i32::MAX, true, Cell::Yellow);
        assert!(near > far, "the search is indifferent to how soon it wins");
    }

    #[test]
    fn a_position_the_opponent_has_already_won_scores_against_the_searcher() {
        let mut b = won_board(Cell::Red);
        let (score, _) = minimax(&mut b, 3, i32::MIN, i32::MAX, true, Cell::Yellow);
        assert!(score <= -SCORE_WIN, "a lost position scored {score}");
    }

    #[test]
    fn a_full_position_scores_level() {
        let mut b = drawn_board();
        assert_eq!(
            minimax(&mut b, 3, i32::MIN, i32::MAX, true, Cell::Yellow).0,
            0
        );
    }

    #[test]
    fn a_search_with_no_move_to_make_reports_no_move() {
        // The board is full, so there is no column to report. It is reported as
        // `None` rather than as a column nobody can play: `best_col` starts as
        // the first legal move *if there is one*, which is where the emptiness
        // is handled -- there is no separate `is_empty` arm, because a full
        // board is terminal and such an arm could never run
        // (`known-issues.md` lesson 51).
        let mut b = drawn_board();
        assert_eq!(
            minimax(&mut b, 2, i32::MIN, i32::MAX, true, Cell::Yellow).1,
            None
        );
    }

    #[test]
    fn a_search_that_reaches_its_depth_reports_a_score_and_no_column() {
        // Depth zero is a leaf: it is worth something, but no move was tried
        // in it, so naming a column would be naming one nothing looked at.
        let mut b = Board::new();
        let (score, col) = minimax(&mut b, 0, i32::MIN, i32::MAX, true, Cell::Yellow);
        assert_eq!(col, None);
        assert_eq!(score, evaluate_board(&b, Cell::Yellow));
    }

    #[test]
    fn the_column_a_search_reports_is_always_one_that_can_be_played() {
        // The column is filled in alternating colours: six of one colour would
        // be four in a column, which is a won position and returns before any
        // move is tried.
        let mut b = Board::new();
        for i in 0..ROWS {
            b.drop_piece(3, if i % 2 == 0 { Cell::Red } else { Cell::Yellow });
        }
        let (_, col) = minimax(&mut b, 2, i32::MIN, i32::MAX, true, Cell::Yellow);
        let col = col.expect("a board with room reported no move");
        assert!(b.can_drop(col), "the search named the full column");
    }

    // ── Layout ──

    #[test]
    fn the_layout_covers_the_window_it_was_given() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert_eq!(l.window, Rect::new(0.0, 0.0, w, h), "at {w}x{h}");
        }
    }

    #[test]
    fn a_window_of_no_size_still_produces_a_layout_of_some_size() {
        // A zero-sized window is a real event -- a compositor sends one while a
        // window is being mapped -- and every division in the layout would be
        // by zero. The floor is one pixel, so the arithmetic stays finite.
        let l = Layout::new(0.0, 0.0);
        assert_eq!(l.window, Rect::new(0.0, 0.0, 1.0, 1.0));
        assert!(l.step.is_finite() && l.step >= 0.0, "step {}", l.step);
        assert!(l.pad.is_finite(), "pad {}", l.pad);
    }

    #[test]
    fn every_band_stays_inside_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, band) in [
                ("header", l.header),
                ("info", l.info),
                ("chute", l.chute),
                ("board", l.board),
                ("footer", l.footer),
                ("help", l.help),
            ] {
                if band.is_empty() {
                    continue;
                }
                assert!(
                    band.x >= -0.01
                        && band.y >= -0.01
                        && band.right() <= w + 0.01
                        && band.bottom() <= h + 0.01,
                    "{name} {band:?} leaves the {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_bands_are_stacked_in_the_order_they_are_read_in() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.shows(l.header) && l.shows(l.info) {
                assert!(
                    l.info.y >= l.header.bottom() - 0.01,
                    "info above header at {w}x{h}"
                );
            }
            if l.shows(l.chute) && !l.board.is_empty() {
                assert!(
                    l.chute.bottom() <= l.board.y + 0.01,
                    "chute below the board"
                );
            }
            if l.shows(l.footer) && !l.board.is_empty() {
                assert!(
                    l.footer.y >= l.board.bottom() - 0.01,
                    "footer above the board at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_chute_sits_directly_on_top_of_the_board_and_over_the_same_columns() {
        // The board is centred in what the bands left, so it is only where the
        // chute's own reserved band is when it happens to fill that space
        // exactly. A chute drawn at its band's height would float above the
        // board at every other size, pointing at nothing.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if !l.shows(l.chute) {
                continue;
            }
            assert_eq!(l.chute.x, l.board.x, "chute not over the board at {w}x{h}");
            assert_eq!(l.chute.w, l.board.w, "chute not as wide as the board");
            assert!(
                (l.chute.bottom() - l.board.y).abs() < 0.01,
                "a gap of {} between chute and board at {w}x{h}",
                l.board.y - l.chute.bottom()
            );
        }
    }

    #[test]
    fn the_boards_cells_are_square() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            assert!(
                (l.board.w / COLS as f32 - l.board.h / ROWS as f32).abs() < 0.01,
                "cells are {}x{} at {w}x{h}",
                l.board.w / COLS as f32,
                l.board.h / ROWS as f32
            );
            assert_eq!(l.board.w, l.step * COLS as f32);
            assert_eq!(l.board.h, l.step * ROWS as f32);
        }
    }

    #[test]
    fn the_board_is_centred_across_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            assert!(
                (l.board.x - (w - l.board.right())).abs() < 0.01,
                "the margins differ at {w}x{h}: {} and {}",
                l.board.x,
                w - l.board.right()
            );
        }
    }

    #[test]
    fn the_board_never_runs_into_the_footer_or_the_bands_above_it() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            let top = l.header.h + l.info.h + l.chute.h;
            assert!(
                l.board.y >= top - 0.01,
                "the board is under the bands at {w}x{h}"
            );
            assert!(
                l.board.bottom() <= h - l.footer.h + 0.01,
                "the board is under the footer at {w}x{h}"
            );
        }
    }

    #[test]
    fn a_window_too_short_for_the_chrome_drops_the_chute_first_and_the_footer_last() {
        // The order is the ladder in `BAND_DROP_ORDER`: the chute is decoration
        // over a board whose columns are still clickable, the status line and
        // the title are context, and the footer holds the only controls that
        // are not the board itself -- so it is the last thing to go.
        let mut seen_stages = 0_u32;
        for h in [700.0_f32, 260.0, 200.0, 150.0, 120.0, 90.0, 40.0] {
            let l = Layout::new(600.0, h);
            let bands = [
                l.shows(l.header),
                l.shows(l.info),
                l.shows(l.chute),
                l.shows(l.footer),
            ];
            if bands[2] {
                assert!(
                    bands[0] && bands[1] && bands[3],
                    "chute went last at 600x{h}"
                );
            }
            if bands[1] {
                assert!(bands[0] && bands[3], "info outlived the header at 600x{h}");
            }
            if bands[0] {
                assert!(bands[3], "the header outlived the footer at 600x{h}");
            }
            seen_stages |= 1 << bands.iter().filter(|b| **b).count();
        }
        assert!(
            seen_stages.count_ones() >= 3,
            "every height in the sweep dropped the same number of bands"
        );
    }

    #[test]
    fn a_band_that_did_not_fit_is_nothing_rather_than_a_flat_strip() {
        // A flat band would still be at some `y`, and everything measured from
        // it would be measured from a rectangle nobody can see.
        let l = Layout::new(24.0, 24.0);
        assert_eq!(l.header, Rect::EMPTY);
        assert_eq!(l.info, Rect::EMPTY);
        assert_eq!(l.chute, Rect::EMPTY);
        assert_eq!(l.footer, Rect::EMPTY);
        assert!(!l.board.is_empty(), "the board went too");
    }

    #[test]
    fn shows_answers_no_for_a_band_with_no_area() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(l.shows(l.header));
        assert!(!l.shows(Rect::EMPTY));
        assert!(
            !l.shows(Rect::new(10.0, 10.0, 40.0, 0.0)),
            "a flat band shows"
        );
        assert!(
            !l.shows(Rect::new(10.0, 10.0, 0.0, 40.0)),
            "a thin band shows"
        );
    }

    #[test]
    fn the_board_keeps_nearly_half_the_window_when_everything_fits() {
        // What `BOARD_SHARE` promises. It is only a promise where the ladder
        // runs at all -- at a comfortable size the bands take far less than
        // their share and the board takes the rest.
        let l = Layout::new(600.0, 150.0);
        assert!(
            l.board.h >= 150.0 * BOARD_SHARE * 0.9,
            "the board got {} of a 150px window",
            l.board.h
        );
    }

    #[test]
    fn the_board_is_drawn_in_every_window_however_small() {
        // The board is the one band the drop ladder cannot take, and this is
        // the test that lets `draw_board` go without a "did it fit?" guard.
        // Two rules hold it up and both are swept here rather than argued:
        // the padding is capped at a quarter of the smaller side, so a window
        // can never be narrower than its own margins; and the ladder empties
        // all four bands before it would eat into `BOARD_SHARE`, so whatever
        // the bands take, the height left over is still positive.
        //
        // The sweep is a grid rather than `WINDOWS` on purpose: `WINDOWS` is a
        // list of sizes chosen because each makes some *other* rule bind, and
        // a rule about every size has to be asked about sizes nobody picked.
        for w in [1.0, 2.0, 3.0, 4.0, 7.0, 13.0, 24.0, 90.0, 300.0, 1920.0] {
            for h in [1.0, 2.0, 3.0, 4.0, 7.0, 13.0, 24.0, 90.0, 300.0, 1080.0] {
                let l = Layout::new(w, h);
                assert!(
                    !l.board.is_empty(),
                    "a {w}x{h} window drew no board at all ({:?})",
                    l.board
                );
                assert!(l.step > 0.0, "a {w}x{h} window has cells of no size");
            }
        }
    }

    #[test]
    fn the_padding_never_eats_more_than_a_quarter_of_the_smaller_side() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                l.pad <= w.min(h) / 4.0 + 0.01,
                "padding {} in a {w}x{h} window",
                l.pad
            );
        }
    }

    #[test]
    fn the_type_size_is_bounded_at_both_ends() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!((8.0..=18.0).contains(&l.font), "font {} at {w}x{h}", l.font);
            assert!(l.small >= 7.0, "small {} at {w}x{h}", l.small);
            assert!(l.small < l.font, "the small size is not smaller");
        }
    }

    // ── Cells, columns and chute slots ──

    #[test]
    fn row_zero_is_drawn_at_the_bottom_of_the_board() {
        // Row 0 is the floor a piece lands on; drawn at the top it would show
        // every game upside down. The flip lives in `cell` alone.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let bottom = l.cell(0, 0);
        let top = l.cell(ROWS - 1, 0);
        assert!(bottom.y > top.y, "row 0 was drawn above row {}", ROWS - 1);
        assert!(bottom.bottom() <= l.board.bottom() + 0.01);
        assert!(top.y >= l.board.y - 0.01);
    }

    #[test]
    fn the_holes_of_the_board_tile_it_without_overlapping() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut boxes = Vec::new();
        for row in 0..ROWS {
            for col in 0..COLS {
                let r = l.cell(row, col);
                assert!(!r.is_empty(), "({row}, {col}) has no box");
                assert!(
                    r.x >= l.board.x - 0.01
                        && r.y >= l.board.y - 0.01
                        && r.right() <= l.board.right() + 0.01
                        && r.bottom() <= l.board.bottom() + 0.01,
                    "({row}, {col}) at {r:?} leaves the board {:?}",
                    l.board
                );
                for other in &boxes {
                    assert!(
                        r.intersect(*other).is_none(),
                        "({row}, {col}) overlaps {other:?}"
                    );
                }
                boxes.push(r);
            }
        }
    }

    #[test]
    fn the_holes_are_round_because_they_are_square() {
        // They are drawn as discs, and a disc is a rectangle with a corner
        // radius of half its side -- which is only a circle when the sides are
        // equal.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let r = l.cell(2, 2);
        assert!((r.w - r.h).abs() < 0.01, "hole {r:?} is not square");
    }

    #[test]
    fn the_gap_between_holes_comes_out_of_them_and_not_off_the_boards_edge() {
        // Taken out of the hole rather than added to the step, so the last hole
        // ends where the board does instead of a gap past it.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let last = l.cell(0, COLS - 1);
        let gap = l.step - last.w;
        assert!(gap > 0.0, "the holes touch");
        assert!(
            (last.right() + gap / 2.0 - l.board.right()).abs() < 0.01,
            "the last column ends {} short of the board",
            l.board.right() - last.right()
        );
    }

    #[test]
    fn a_cell_that_is_not_on_the_board_has_no_box() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.cell(ROWS, 0), Rect::EMPTY, "past the top row");
        assert_eq!(l.cell(0, COLS), Rect::EMPTY, "past the last column");
    }

    #[test]
    fn a_column_strip_covers_the_column_it_names_and_the_chute_above_it() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            for col in 0..COLS {
                let strip = l.column(col);
                let cell = l.cell(0, col);
                assert!(!strip.is_empty(), "column {col} has no strip at {w}x{h}");
                assert!(
                    strip.x <= cell.x + 0.01 && strip.right() >= cell.right() - 0.01,
                    "strip {strip:?} does not cover cell {cell:?} at {w}x{h}"
                );
                assert!(
                    (strip.bottom() - l.board.bottom()).abs() < 0.01,
                    "the strip stops short of the board's floor at {w}x{h}"
                );
                let top = if l.shows(l.chute) {
                    l.chute.y
                } else {
                    l.board.y
                };
                assert!(
                    (strip.y - top).abs() < 0.01,
                    "strip starts at {} not {top}",
                    strip.y
                );
            }
        }
    }

    #[test]
    fn the_column_strips_tile_the_board_without_overlapping() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for col in 1..COLS {
            let left = l.column(col - 1);
            let right = l.column(col);
            assert!(
                (left.right() - right.x).abs() < 0.01,
                "a seam between columns {} and {col}",
                col - 1
            );
        }
        assert_eq!(l.column(0).x, l.board.x);
        assert!((l.column(COLS - 1).right() - l.board.right()).abs() < 0.01);
    }

    #[test]
    fn a_column_that_is_not_on_the_board_has_no_strip() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.column(COLS), Rect::EMPTY);
        assert_eq!(l.column(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn a_chute_slot_sits_over_the_column_it_names() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for col in 0..COLS {
            let slot = l.chute_slot(col);
            let cell = l.cell(ROWS - 1, col);
            assert!(!slot.is_empty(), "column {col} has no chute slot");
            let (sx, _) = slot.centre();
            let (cx, _) = cell.centre();
            assert!((sx - cx).abs() < 0.01, "slot {sx} is not over cell {cx}");
            assert!(slot.bottom() <= l.board.y + 0.01);
        }
    }

    #[test]
    fn there_are_no_chute_slots_when_the_chute_did_not_fit() {
        let l = Layout::new(24.0, 24.0);
        assert!(!l.shows(l.chute));
        for col in 0..COLS {
            assert_eq!(l.chute_slot(col), Rect::EMPTY, "slot {col}");
        }
    }

    #[test]
    fn a_chute_slot_for_a_column_that_is_not_there_is_nothing() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.chute_slot(COLS), Rect::EMPTY);
    }

    // ── The footer's buttons and the header's readouts ──

    #[test]
    fn the_three_footer_buttons_are_side_by_side_inside_the_footer() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut previous: Option<Rect> = None;
        for i in 0..3 {
            let r = l.footer_button(i);
            assert!(!r.is_empty(), "button {i} has no box");
            assert!(
                r.y >= l.footer.y - 0.01 && r.bottom() <= l.footer.bottom() + 0.01,
                "button {i} is outside the footer"
            );
            assert!(r.x >= l.footer.x && r.right() <= l.footer.right() + 0.01);
            if let Some(p) = previous {
                assert!(r.x > p.right(), "button {i} overlaps the one before it");
                assert!(
                    (r.w - p.w).abs() < 0.01,
                    "the buttons are not the same width"
                );
            }
            previous = Some(r);
        }
    }

    #[test]
    fn there_is_no_fourth_footer_button() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.footer_button(3), Rect::EMPTY);
        assert_eq!(l.footer_button(usize::MAX), Rect::EMPTY);
    }

    #[test]
    fn a_footer_that_did_not_fit_has_no_buttons_rather_than_flat_ones() {
        let l = Layout::new(24.0, 24.0);
        for i in 0..3 {
            assert!(l.footer_button(i).is_empty(), "button {i} at 24x24");
        }
    }

    #[test]
    fn the_score_readouts_are_laid_out_from_the_right_edge_inwards() {
        // Index 0 is the box nearest the edge, so adding a fourth would not
        // move the other three.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let first = l.score_box(0);
        let second = l.score_box(1);
        let third = l.score_box(2);
        assert!(!first.is_empty() && !second.is_empty() && !third.is_empty());
        assert!(
            first.x > second.x && second.x > third.x,
            "not right to left"
        );
        assert!(
            first.right() <= l.header.right() - l.pad + 0.01,
            "past the edge"
        );
        assert!(second.right() < first.x, "the boxes overlap");
    }

    #[test]
    fn the_score_readouts_sit_inside_the_header() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..3 {
            let r = l.score_box(i);
            assert!(
                r.y >= l.header.y - 0.01 && r.bottom() <= l.header.bottom() + 0.01,
                "readout {i} at {r:?} is outside the header"
            );
        }
    }

    #[test]
    fn a_readout_that_would_run_off_the_left_edge_is_dropped() {
        // A header this narrow holds two of the three. The third is refused
        // rather than drawn at a negative x, where it would be half off the
        // window and over the title.
        let l = Layout::new(120.0, 800.0);
        assert!(l.shows(l.header), "the header did not survive at 120x800");
        assert!(!l.score_box(0).is_empty(), "the first readout went");
        assert_eq!(l.score_box(2), Rect::EMPTY, "the third readout was drawn");
    }

    #[test]
    fn there_is_no_fourth_readout() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(l.score_box(3), Rect::EMPTY);
    }

    #[test]
    fn a_header_that_did_not_fit_has_no_readouts() {
        let l = Layout::new(24.0, 24.0);
        for i in 0..3 {
            assert_eq!(l.score_box(i), Rect::EMPTY, "readout {i}");
        }
        // A dropped band is `Rect::EMPTY`, so the case above is also caught by
        // the "runs off the left edge" check further down `score_box` — the
        // right edge of a band with no width is to the left of its own left
        // edge. `shows` is asked about a band with *no height and full width*,
        // which that check would let through: a row of readouts one pixel tall
        // in a header that is not there.
        let mut flat = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        flat.header = Rect::new(0.0, 0.0, WINDOW_WIDTH, 0.0);
        assert!(!flat.shows(flat.header));
        for i in 0..3 {
            assert_eq!(flat.score_box(i), Rect::EMPTY, "readout {i} on a flat band");
        }
    }

    #[test]
    fn the_help_sheet_is_centred_in_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.help.x - (w - l.help.right())).abs() < 0.01
                    && (l.help.y - (h - l.help.bottom())).abs() < 0.01,
                "the sheet is off-centre at {w}x{h}"
            );
        }
    }

    // ── A new game ──

    #[test]
    fn a_new_game_starts_empty_with_the_player_to_move_and_the_cursor_in_the_middle() {
        let app = game();
        assert_eq!(app.board.pieces(), 0);
        assert_eq!(app.status(), GameStatus::Playing);
        assert_eq!(app.win_line(), None);
        assert_eq!(app.current_player, app.human_player);
        assert_eq!(app.cursor_col(), CENTER_COL);
        assert!(app.moves().is_empty());
        assert!(!app.can_undo(), "there is a move to take back already");
        assert!(!app.help_is_open());
    }

    #[test]
    fn the_player_is_red_and_moves_first_and_the_search_plays_yellow() {
        let app = game();
        assert_eq!(app.human_player, Cell::Red);
        assert_eq!(app.ai_player, Cell::Yellow);
        assert_ne!(app.human_player, app.ai_player, "both sides are one player");
    }

    #[test]
    fn a_new_game_clears_the_board_and_keeps_the_tally() {
        // A new game is a new board, not a new session: the score of the match
        // so far is the reason to press the button twice.
        let mut app = game();
        app.human_wins = 3;
        app.ai_wins = 2;
        app.draws = 1;
        app.drop_at(0);
        app.new_game();
        assert_eq!(app.board.pieces(), 0);
        assert!(app.moves().is_empty());
        assert!(!app.can_undo(), "a new game can undo into the old one");
        assert_eq!((app.human_wins, app.ai_wins, app.draws), (3, 2, 1));
    }

    #[test]
    fn a_new_game_puts_the_cursor_back_in_the_middle() {
        let mut app = game();
        app.cursor_col = 0;
        app.new_game();
        assert_eq!(app.cursor_col(), CENTER_COL);
    }

    #[test]
    fn a_new_game_clears_a_finished_games_result() {
        let mut app = game();
        app.board = won_board(Cell::Red);
        app.status = GameStatus::Won(Cell::Red);
        app.win_line = app.board.outcome().1;
        app.new_game();
        assert_eq!(app.status(), GameStatus::Playing);
        assert_eq!(app.win_line(), None, "the old game's line is still ringed");
        assert_eq!(app.current_player, Cell::Red, "the loser is not to move");
    }

    // ── Dropping ──

    #[test]
    fn a_drop_puts_the_moving_players_piece_on_the_board() {
        let mut app = game();
        assert!(app.drop_at(2));
        assert_eq!(app.board.get(0, 2), Cell::Red);
        assert_eq!(app.moves(), [(2, Cell::Red)]);
    }

    #[test]
    fn a_drop_hands_the_turn_to_the_other_player() {
        let mut app = game();
        app.drop_at(2);
        assert_eq!(app.current_player, Cell::Yellow);
        app.drop_at(3);
        assert_eq!(app.current_player, Cell::Red, "the turn did not come back");
        assert_eq!(
            app.board.get(0, 3),
            Cell::Yellow,
            "the wrong piece was played"
        );
    }

    #[test]
    fn a_drop_into_a_full_column_changes_nothing() {
        let mut app = game();
        for _ in 0..ROWS {
            app.drop_at(0);
        }
        let before = app.current_player;
        let pieces = app.board.pieces();
        let history = app.moves().len();
        assert!(!app.drop_at(0), "a full column took a piece");
        assert_eq!(app.board.pieces(), pieces, "the refused drop landed");
        assert_eq!(app.current_player, before, "the refused drop took the turn");
        assert_eq!(app.moves().len(), history, "the refused drop was recorded");
        assert!(!app.can_undo() || app.moves().len() == history);
    }

    #[test]
    fn a_refused_drop_leaves_nothing_to_take_back() {
        // The snapshot is taken before the drop is attempted, so a drop that is
        // then refused must not push it -- an undo that restored a position
        // identical to the current one would look like a control that does
        // nothing.
        let mut app = game();
        for _ in 0..ROWS {
            app.drop_at(0);
        }
        let depth = app.history.len();
        app.drop_at(0);
        assert_eq!(app.history.len(), depth, "a refused drop was made undoable");
    }

    #[test]
    fn a_drop_on_a_finished_game_is_refused() {
        let mut app = game();
        app.status = GameStatus::Won(Cell::Red);
        assert!(!app.drop_at(1), "a won game took another move");
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn a_drop_on_a_drawn_game_is_refused() {
        let mut app = game();
        app.status = GameStatus::Draw;
        assert!(!app.drop_at(1));
    }

    #[test]
    fn a_drop_that_wins_ends_the_game_and_leaves_the_winner_to_move() {
        // The winner staying as the current player is what lets the tally know
        // who won without a second field saying so.
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        assert!(app.drop_at(3));
        assert_eq!(app.status(), GameStatus::Won(Cell::Red));
        assert_eq!(app.current_player, Cell::Red);
    }

    #[test]
    fn a_drop_that_wins_shows_which_four_cells_did_it() {
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        assert_eq!(
            app.win_line().expect("a win with no line").cells,
            [(0, 0), (0, 1), (0, 2), (0, 3)]
        );
    }

    #[test]
    fn a_win_by_the_player_is_scored_to_the_player() {
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        assert_eq!(app.human_wins, 1);
        assert_eq!(app.ai_wins, 0, "the AI was paid for the player's win");
        assert_eq!(app.draws, 0);
    }

    #[test]
    fn a_win_by_the_search_is_scored_to_the_search() {
        let mut app = game();
        app.current_player = Cell::Yellow;
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Yellow;
        }
        app.drop_at(3);
        assert_eq!(app.status(), GameStatus::Won(Cell::Yellow));
        assert_eq!(app.ai_wins, 1);
        assert_eq!(app.human_wins, 0, "the player was paid for the AI's win");
    }

    #[test]
    fn a_drop_that_fills_the_last_hole_without_a_line_is_a_draw() {
        let mut app = game();
        app.board = drawn_board();
        app.board.undo_drop(6);
        // The colour the fixture wanted in that hole, so the board the drop
        // completes is the drawn one and not a won one.
        app.current_player = if ((ROWS - 1) / 2 + 6).is_multiple_of(2) {
            Cell::Red
        } else {
            Cell::Yellow
        };
        assert!(app.drop_at(6));
        assert_eq!(app.status(), GameStatus::Draw);
        assert_eq!(app.draws, 1);
        assert_eq!(app.win_line(), None);
    }

    #[test]
    fn the_tally_only_moves_when_a_game_ends() {
        let mut app = game();
        app.drop_at(0);
        app.drop_at(1);
        assert_eq!((app.human_wins, app.ai_wins, app.draws), (0, 0, 0));
    }

    // ── Whose turn it is ──

    #[test]
    fn the_search_owes_a_move_only_when_it_is_its_turn_on_a_live_game() {
        let mut app = game();
        assert!(!app.ai_to_play(), "the AI moves before the player has");
        app.drop_at(3);
        assert!(app.ai_to_play(), "the AI does not owe a reply");
        app.status = GameStatus::Won(Cell::Yellow);
        assert!(!app.ai_to_play(), "the AI moves on a finished game");
    }

    #[test]
    fn the_search_plays_the_move_it_owes_and_nothing_else() {
        let mut app = shallow();
        app.drop_at(3);
        // What the search chooses, asked independently of the code that plays
        // it. Without this the test's only witness is `ai_turn`'s own return
        // value, and a fault between the search and the drop moves *both*: the
        // played column and the reported column go on agreeing with each other
        // and stop agreeing with the search. Mutation found this by shifting
        // the column one to the right between the two lines, and nothing
        // failed. `known-issues.md` lesson 62 -- a witness the code under test
        // produced moves with the fault.
        let mut board = app.board.clone();
        let want = ai_best_move(&mut board, app.ai_player, app.ai_depth)
            .expect("the search had a move to make");
        let col = app.ai_turn().expect("the AI passed on its turn");
        assert_eq!(col, want, "the AI played a column the search did not pick");
        assert_eq!(app.moves().len(), 2, "it played more than one piece");
        assert_eq!(app.moves()[1], (col, Cell::Yellow));
        assert_eq!(app.current_player, Cell::Red, "the turn did not come back");
    }

    #[test]
    fn the_search_declines_a_turn_that_is_not_its_own() {
        let mut app = shallow();
        assert_eq!(app.ai_turn(), None, "the AI moved on the player's turn");
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn the_search_declines_to_move_on_a_finished_game() {
        let mut app = shallow();
        app.board = won_board(Cell::Red);
        app.status = GameStatus::Won(Cell::Red);
        app.current_player = Cell::Yellow;
        assert_eq!(app.ai_turn(), None, "the AI played on after the game ended");
    }

    #[test]
    fn a_click_by_the_player_out_of_turn_is_ignored() {
        // Two rules, not one written twice: this is "it is not your turn", and
        // `drop_at`'s is "the game is over".
        let mut app = shallow();
        app.drop_at(3);
        assert_eq!(app.apply(Intent::Drop(0)), EventResult::Ignored);
        assert_eq!(app.board.pieces(), 1, "the out-of-turn drop landed");
    }

    #[test]
    fn a_drop_on_a_game_the_player_won_is_refused_by_the_game_being_over() {
        // The won game leaves the human as the current player, so the "not your
        // turn" rule passes and only the "game is over" rule can refuse. If
        // those two were one rule this move would be played.
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        assert_eq!(app.current_player, app.human_player, "the fixture is wrong");
        assert_eq!(app.apply(Intent::Drop(5)), EventResult::Ignored);
        assert_eq!(app.board.pieces(), 4, "a piece was played after the win");
    }

    // ── Taking a move back ──

    #[test]
    fn there_is_nothing_to_take_back_before_the_first_move() {
        let mut app = game();
        assert!(!app.can_undo());
        assert!(!app.undo(), "an empty game had something to undo");
    }

    #[test]
    fn taking_back_the_first_move_leaves_an_empty_board_with_the_player_to_move() {
        let mut app = game();
        app.drop_at(3);
        assert!(app.can_undo());
        assert!(app.undo());
        assert_eq!(app.board.pieces(), 0);
        assert!(app.moves().is_empty());
        assert_eq!(app.current_player, Cell::Red);
        assert!(!app.can_undo(), "there is still something to take back");
    }

    #[test]
    fn taking_back_takes_the_whole_round_and_not_half_of_it() {
        // One drop when the AI has not replied and two when it has. A rule
        // written as "pop two" is wrong on the first move of the game.
        let mut app = shallow();
        app.drop_at(3);
        app.ai_turn();
        assert_eq!(app.moves().len(), 2);
        assert!(app.undo());
        assert_eq!(app.board.pieces(), 0, "the AI's reply was left behind");
        assert_eq!(app.current_player, Cell::Red);
    }

    #[test]
    fn taking_back_stops_at_the_players_turn_rather_than_unwinding_the_game() {
        let mut app = shallow();
        for _ in 0..3 {
            app.drop_at(app.cursor_col());
            app.ai_turn();
        }
        let before = app.moves().len();
        app.undo();
        assert_eq!(
            app.moves().len(),
            before - 2,
            "one undo took back more than one round"
        );
    }

    #[test]
    fn taking_back_the_winning_move_puts_the_game_back_in_play() {
        // 2048 -- this program's neighbour -- shipped an undo that restored the
        // grid and the score and left the *status* alone, so taking back the
        // winning move left "You win!" over a board with no win on it. A whole
        // snapshot cannot restore four fields and forget the fifth.
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        assert_eq!(app.status(), GameStatus::Won(Cell::Red));
        assert!(app.undo());
        assert_eq!(
            app.status(),
            GameStatus::Playing,
            "the win outlived the move"
        );
        assert_eq!(app.win_line(), None, "four cells are still ringed");
        assert_eq!(app.board.get(0, 3), Cell::Empty);
    }

    #[test]
    fn taking_back_the_winning_move_takes_back_the_point_it_scored() {
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        assert_eq!(app.human_wins, 1);
        app.undo();
        assert_eq!(
            app.human_wins, 0,
            "the tally kept a win that was taken back"
        );
    }

    #[test]
    fn taking_back_does_not_disturb_the_tally_of_earlier_games() {
        let mut app = game();
        app.human_wins = 4;
        app.ai_wins = 2;
        app.draws = 3;
        app.drop_at(0);
        app.undo();
        assert_eq!((app.human_wins, app.ai_wins, app.draws), (4, 2, 3));
    }

    #[test]
    fn taking_back_restores_the_position_exactly() {
        let mut app = shallow();
        app.drop_at(2);
        app.ai_turn();
        let before = app.board.clone();
        app.drop_at(4);
        app.ai_turn();
        app.undo();
        assert_eq!(app.board, before, "the position came back different");
    }

    #[test]
    fn the_history_is_trimmed_to_the_move_it_is_taken_back_to() {
        let mut app = shallow();
        app.drop_at(1);
        app.ai_turn();
        app.drop_at(5);
        app.ai_turn();
        app.undo();
        assert_eq!(
            app.moves().len(),
            2,
            "the taken-back moves are still listed"
        );
        assert_eq!(app.moves()[0], (1, Cell::Red));
    }

    // ── The cursor ──

    #[test]
    fn the_cursor_moves_one_column_at_a_time() {
        let mut app = game();
        assert_eq!(app.apply(Intent::CursorLeft), EventResult::Consumed);
        assert_eq!(app.cursor_col(), CENTER_COL - 1);
        assert_eq!(app.apply(Intent::CursorRight), EventResult::Consumed);
        assert_eq!(app.cursor_col(), CENTER_COL);
    }

    #[test]
    fn the_cursor_stops_at_the_left_wall_and_says_nothing_happened() {
        // A cursor already against the wall reports `Ignored`, because a redraw
        // would be a frame spent painting the same picture.
        let mut app = game();
        app.cursor_col = 0;
        assert_eq!(app.apply(Intent::CursorLeft), EventResult::Ignored);
        assert_eq!(app.cursor_col(), 0, "the cursor wrapped or went negative");
    }

    #[test]
    fn the_cursor_stops_at_the_right_wall_and_says_nothing_happened() {
        let mut app = game();
        app.cursor_col = COLS - 1;
        assert_eq!(app.apply(Intent::CursorRight), EventResult::Ignored);
        assert_eq!(app.cursor_col(), COLS - 1);
    }

    #[test]
    fn dropping_into_a_named_column_moves_the_cursor_there() {
        // So the keyboard picks up where the pointer left off.
        let mut app = game();
        app.apply(Intent::Drop(6));
        assert_eq!(app.cursor_col(), 6);
    }

    #[test]
    fn a_drop_into_a_column_that_is_not_on_the_board_leaves_the_cursor_alone() {
        let mut app = game();
        assert_eq!(app.apply(Intent::Drop(COLS)), EventResult::Ignored);
        assert_eq!(app.cursor_col(), CENTER_COL, "the cursor left the board");
    }

    #[test]
    fn dropping_at_the_cursor_drops_where_the_cursor_is() {
        let mut app = game();
        app.cursor_col = 5;
        assert_eq!(app.apply(Intent::DropAtCursor), EventResult::Consumed);
        assert_eq!(app.board.get(0, 5), Cell::Red);
    }

    #[test]
    fn dropping_at_a_cursor_over_a_full_column_is_refused() {
        let mut app = game();
        for _ in 0..ROWS {
            app.drop_at(0);
        }
        app.cursor_col = 0;
        assert_eq!(app.apply(Intent::DropAtCursor), EventResult::Ignored);
    }

    // ── The help sheet ──

    #[test]
    fn help_opens_and_shuts_on_the_same_intent() {
        let mut app = game();
        assert_eq!(app.apply(Intent::ToggleHelp), EventResult::Consumed);
        assert!(app.help_is_open());
        assert_eq!(app.apply(Intent::ToggleHelp), EventResult::Consumed);
        assert!(!app.help_is_open());
    }

    #[test]
    fn closing_a_sheet_that_is_already_shut_does_nothing() {
        let mut app = game();
        assert_eq!(app.apply(Intent::CloseHelp), EventResult::Ignored);
        assert!(!app.help_is_open());
    }

    #[test]
    fn an_open_sheet_swallows_the_move_a_key_would_have_made() {
        // The sheet is drawn over the board, so without this an arrow key would
        // move a cursor the player cannot see and Enter would drop a piece
        // behind the help text.
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::DropAtCursor), EventResult::Consumed);
        assert_eq!(
            app.board.pieces(),
            0,
            "a piece was dropped behind the sheet"
        );
        assert!(!app.help_is_open(), "the sheet stayed up");
    }

    #[test]
    fn an_open_sheet_swallows_a_cursor_move_and_shuts() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::CursorLeft), EventResult::Consumed);
        assert_eq!(app.cursor_col(), CENTER_COL, "the cursor moved behind it");
        assert!(!app.help_is_open());
    }

    #[test]
    fn an_open_sheet_swallows_a_new_game_and_an_undo() {
        let mut app = game();
        app.drop_at(0);
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::NewGame), EventResult::Consumed);
        assert_eq!(
            app.board.pieces(),
            1,
            "the board was cleared behind the sheet"
        );
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::Undo), EventResult::Consumed);
        assert_eq!(app.board.pieces(), 1, "the move was taken back behind it");
    }

    #[test]
    fn a_key_coming_back_up_is_not_a_second_move() {
        // The compositor sends a `KeyEvent` for the release as well as the
        // press, so a handler that reads only `key` plays every move twice: the
        // player's piece, and then the same column again -- which, because the
        // turn has passed, is a piece dropped on the AI's behalf. This file
        // shipped without the check that every other wired app in the tree
        // carries. The test is written against the whole event route rather
        // than against `key_intent`, because the route is what the compositor
        // uses and a guard placed in a function nothing calls is no guard.
        let mut app = game();
        assert_eq!(press(&mut app, Key::Num1), EventResult::Consumed);
        assert_eq!(app.board.pieces(), 1, "the press dropped one piece");

        let mut release = probe::press(Key::Num1);
        release.pressed = false;
        assert_eq!(
            handle_event(&mut app, &Event::Key(release)),
            EventResult::Ignored,
            "the key coming back up asked for something"
        );
        assert_eq!(app.board.pieces(), 1, "the release dropped a second piece");
    }

    #[test]
    fn the_two_help_controls_close_a_sheet_they_opened() {
        // The user-visible half of the modal rule: whatever opened the sheet
        // has to be able to shut it. This was once written as an exemption --
        // the two help intents were let through the modal guard to their own
        // arms -- and the exemption was worth nothing, because both arms ended
        // where the guard ends. The claim below is the part that mattered, and
        // it is stated in terms of what the player does rather than which
        // branch runs, so it survived the exemption's removal unchanged.
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::CloseHelp), EventResult::Consumed);
        assert!(!app.help_is_open());
        app.apply(Intent::ToggleHelp);
        assert_eq!(app.apply(Intent::ToggleHelp), EventResult::Consumed);
        assert!(!app.help_is_open());
    }

    #[test]
    fn an_undo_with_nothing_to_take_back_reports_that_nothing_happened() {
        let mut app = game();
        assert_eq!(app.apply(Intent::Undo), EventResult::Ignored);
    }

    #[test]
    fn a_new_game_always_reports_that_something_happened() {
        // Even on a board with nothing on it: the button is a promise that the
        // game in front of you is gone, and answering `Ignored` would leave the
        // frame unpainted after a control the player pressed.
        let mut app = game();
        assert_eq!(app.apply(Intent::NewGame), EventResult::Consumed);
    }

    // ── What a key asks for ──

    #[test]
    fn the_arrow_keys_move_the_cursor() {
        assert_eq!(
            key_intent(&probe::press(Key::Left)),
            Some(Intent::CursorLeft)
        );
        assert_eq!(
            key_intent(&probe::press(Key::Right)),
            Some(Intent::CursorRight)
        );
    }

    #[test]
    fn a_and_d_move_the_cursor_the_same_way_the_arrows_do() {
        assert_eq!(key_intent(&probe::press(Key::A)), Some(Intent::CursorLeft));
        assert_eq!(key_intent(&probe::press(Key::D)), Some(Intent::CursorRight));
    }

    #[test]
    fn enter_and_space_both_drop_at_the_cursor() {
        assert_eq!(
            key_intent(&probe::press(Key::Enter)),
            Some(Intent::DropAtCursor)
        );
        assert_eq!(
            key_intent(&probe::press(Key::Space)),
            Some(Intent::DropAtCursor)
        );
    }

    #[test]
    fn each_digit_key_names_its_own_column_counting_from_one() {
        // The keys are labelled 1-7 and the columns are numbered 0-6, so the
        // off-by-one is the whole content of this mapping.
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
        ];
        assert_eq!(keys.len(), COLS, "there is a column no digit key reaches");
        for (col, key) in keys.into_iter().enumerate() {
            assert_eq!(
                key_intent(&probe::press(key)),
                Some(Intent::Drop(col)),
                "the key for column {col}"
            );
        }
    }

    #[test]
    fn n_starts_a_new_game_and_u_takes_a_move_back_and_h_shows_the_sheet() {
        assert_eq!(key_intent(&probe::press(Key::N)), Some(Intent::NewGame));
        assert_eq!(key_intent(&probe::press(Key::U)), Some(Intent::Undo));
        assert_eq!(key_intent(&probe::press(Key::H)), Some(Intent::ToggleHelp));
    }

    #[test]
    fn escape_shuts_the_help_sheet() {
        // `Escape to quit` used to be in the module documentation and in no
        // handler at all. It is the sheet's dismissal now, which is the one
        // thing in this window it can mean.
        assert_eq!(
            key_intent(&probe::press(Key::Escape)),
            Some(Intent::CloseHelp)
        );
    }

    #[test]
    fn escape_on_a_game_with_no_sheet_up_does_nothing() {
        let mut app = game();
        assert_eq!(press(&mut app, Key::Escape), EventResult::Ignored);
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn ctrl_z_takes_a_move_back() {
        assert_eq!(key_intent(&probe::ctrl(Key::Z)), Some(Intent::Undo));
    }

    #[test]
    fn a_bare_z_asks_for_nothing() {
        // Only the chord undoes. A bare Z that undid would make the letter a
        // control the sheet does not list.
        assert_eq!(key_intent(&probe::press(Key::Z)), None);
    }

    #[test]
    fn ctrl_and_alt_chords_belong_to_the_window_and_not_to_the_board() {
        // A Ctrl+Left that moved the cursor is a Ctrl+Left the desktop cannot
        // have. Ctrl+Z is the one exception and is answered before this rule.
        for key in [Key::Left, Key::Right, Key::Enter, Key::N, Key::U, Key::H] {
            assert_eq!(key_intent(&probe::ctrl(key)), None, "ctrl+{key:?}");
            let alt = Modifiers {
                alt: true,
                ..Modifiers::NONE
            };
            assert_eq!(
                key_intent(&probe::press_with(key, alt)),
                None,
                "alt+{key:?}"
            );
        }
    }

    #[test]
    fn shift_does_not_stop_a_key_meaning_what_it_means() {
        // Shift is how a keyboard produces most of these letters in the first
        // place on some layouts, and it belongs to no window chord here.
        assert_eq!(
            key_intent(&probe::shift(Key::N)),
            Some(Intent::NewGame),
            "shift swallowed a plain letter"
        );
    }

    #[test]
    fn a_key_this_window_has_no_use_for_is_left_for_something_else() {
        for key in [Key::Tab, Key::Up, Key::Down, Key::Q, Key::Num0] {
            assert_eq!(key_intent(&probe::press(key)), None, "{key:?}");
        }
    }

    #[test]
    fn a_key_the_window_has_no_use_for_reports_that_it_did_nothing() {
        let mut app = game();
        assert_eq!(press(&mut app, Key::Q), EventResult::Ignored);
    }

    #[test]
    fn a_keystroke_reaches_the_board_it_names() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(press(&mut app, Key::Num7), EventResult::Consumed);
        assert_eq!(app.board.get(0, COLS - 1), Cell::Red);
    }

    // ── What clicking a control asks for ──

    #[test]
    fn every_control_asks_for_exactly_one_thing() {
        assert_eq!(target_intent(Target::Column(4)), Intent::Drop(4));
        assert_eq!(target_intent(Target::NewGame), Intent::NewGame);
        assert_eq!(target_intent(Target::Undo), Intent::Undo);
        assert_eq!(target_intent(Target::Help), Intent::ToggleHelp);
        assert_eq!(target_intent(Target::HelpSheet), Intent::CloseHelp);
    }

    #[test]
    fn a_column_asks_for_a_drop_into_itself_and_not_into_its_neighbour() {
        for col in 0..COLS {
            assert_eq!(target_intent(Target::Column(col)), Intent::Drop(col));
        }
    }

    #[test]
    fn clicking_a_column_drops_a_piece_into_it() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(click_on(&mut app, Target::Column(5)), EventResult::Consumed);
        assert_eq!(app.board.get(0, 5), Cell::Red, "the piece went elsewhere");
    }

    #[test]
    fn clicking_each_column_drops_into_that_column_and_no_other() {
        // The mapping from a place on the screen to a column is the interface;
        // an off-by-one in it would be invisible in every test that only ever
        // clicks one column.
        for col in 0..COLS {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            click_on(&mut app, Target::Column(col));
            assert_eq!(app.moves(), [(col, Cell::Red)], "clicking column {col}");
        }
    }

    #[test]
    fn a_click_low_in_a_column_and_a_click_high_in_it_are_the_same_move() {
        // The whole strip is one control: a column is played by pointing at it,
        // not by pointing at the hole the piece will land in.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let strip = l.column(2);
        for y in [strip.y + 1.0, strip.bottom() - 1.0] {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            let (cx, _) = strip.centre();
            assert_eq!(click(&mut app, cx, y), EventResult::Consumed, "at y={y}");
            assert_eq!(app.moves(), [(2, Cell::Red)]);
        }
    }

    #[test]
    fn a_click_on_a_piece_still_drops_into_the_column_that_piece_is_in() {
        // The column strips take clicks last of the board's parts, so the
        // pieces drawn over them do not swallow the move.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.drop_at(1);
        app.current_player = Cell::Red;
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.cell(0, 1).centre();
        assert_eq!(click(&mut app, cx, cy), EventResult::Consumed);
        assert_eq!(app.board.get(1, 1), Cell::Red, "the click hit the piece");
    }

    #[test]
    fn a_full_column_keeps_its_control_and_answers_no() {
        // A control that silently stops existing cannot say "no" -- and the
        // answer to clicking a full column is "no", not "nothing is here".
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        for _ in 0..ROWS {
            app.drop_at(0);
        }
        app.current_player = Cell::Red;
        assert!(
            probe::is_visible_sized(&app, Target::Column(0), (WINDOW_WIDTH, WINDOW_HEIGHT)),
            "the full column stopped being a control"
        );
        assert_eq!(click_on(&mut app, Target::Column(0)), EventResult::Ignored);
    }

    #[test]
    fn clicking_new_game_clears_the_board() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.drop_at(3);
        assert_eq!(click_on(&mut app, Target::NewGame), EventResult::Consumed);
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn clicking_undo_takes_the_last_move_back() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.drop_at(3);
        assert_eq!(click_on(&mut app, Target::Undo), EventResult::Consumed);
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn clicking_help_opens_the_sheet_and_clicking_it_again_shuts_it() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        click_on(&mut app, Target::Help);
        assert!(app.help_is_open());
        // The second click lands on the sheet, which covers the button -- and
        // shuts it, which is what the button would have done anyway.
        assert_eq!(click_on(&mut app, Target::HelpSheet), EventResult::Consumed);
        assert!(!app.help_is_open());
    }

    #[test]
    fn a_click_anywhere_at_all_shuts_an_open_sheet() {
        // The sheet's last line says "click anywhere to close", and anywhere
        // means anywhere -- including over the board it covers, where a live
        // column would otherwise drop a piece the player cannot see.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let places = [
            ("a column", l.column(0).centre()),
            ("the new game button", l.footer_button(0).centre()),
            ("the header", (l.header.x + 4.0, l.header.y + 4.0)),
            ("the corner", (WINDOW_WIDTH - 1.0, WINDOW_HEIGHT - 1.0)),
        ];
        for (name, (x, y)) in places {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.apply(Intent::ToggleHelp);
            assert_eq!(click(&mut app, x, y), EventResult::Consumed, "on {name}");
            assert!(!app.help_is_open(), "the sheet survived a click on {name}");
            assert_eq!(app.board.pieces(), 0, "a click on {name} played a move");
        }
    }

    #[test]
    fn a_click_that_lands_on_nothing_does_nothing() {
        // Not every pixel is a control: the gap between the footer buttons is
        // not, and a click there should not fall through to whatever is behind.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (x, y) = probe::bare_point(&app, (WINDOW_WIDTH, WINDOW_HEIGHT))
            .expect("the window is entirely covered by controls");
        assert_eq!(click(&mut app, x, y), EventResult::Ignored);
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn only_the_left_button_plays_a_move() {
        // The right button belongs to a context menu this program does not
        // have; taking it as a drop would make a stray right-click a move.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let out = probe::click_sized(&mut app, Target::Column(3), MouseButton::Right, size);
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(app.board.pieces(), 0, "a right-click dropped a piece");
    }

    #[test]
    fn a_click_is_read_against_the_window_the_last_frame_was_drawn_at() {
        // The two sizes put the same column in different places, so a click at
        // one window's coordinates means something else at the other's. Reading
        // it against a remembered size rather than a constant is the whole
        // reason the size is stored.
        let small = (300.0_f32, 400.0_f32);
        let column_at_small = Layout::new(small.0, small.1).column(0);
        let (x, y) = column_at_small.centre();

        let mut app = windowed(small.0, small.1);
        assert_eq!(click(&mut app, x, y), EventResult::Consumed);
        assert_eq!(app.moves(), [(0, Cell::Red)], "at the small size");

        // The same point in the big window is not column 0 -- it is not even on
        // the board, which is centred and much wider there.
        let big = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            !big.column(0).contains(x, y),
            "the fixture's two sizes put column 0 in the same place"
        );
    }

    // ── Events ──

    #[test]
    fn a_resize_is_taken_and_remembered() {
        let mut app = game();
        let out = handle_event(
            &mut app,
            &Event::Resize {
                width: 500,
                height: 320,
            },
        );
        assert_eq!(out, EventResult::Consumed);
        assert_eq!((app.width, app.height), (500.0, 320.0));
    }

    #[test]
    fn a_tick_plays_the_move_the_search_owes() {
        // The move is played on the tick rather than inside the handler for the
        // key that prompted it, so the frame that says the search is thinking
        // is painted before the search that makes it wait begins.
        let mut app = shallow();
        app.drop_at(3);
        assert_eq!(tick(&mut app), EventResult::Consumed);
        assert_eq!(app.moves().len(), 2, "the tick played nothing");
        assert_eq!(app.current_player, Cell::Red);
    }

    #[test]
    fn a_tick_on_the_players_turn_does_nothing() {
        let mut app = shallow();
        assert_eq!(tick(&mut app), EventResult::Ignored);
        assert_eq!(app.board.pieces(), 0, "the AI moved out of turn");
    }

    #[test]
    fn a_tick_on_a_finished_game_does_nothing() {
        let mut app = shallow();
        app.board = won_board(Cell::Red);
        app.status = GameStatus::Won(Cell::Red);
        app.current_player = Cell::Yellow;
        assert_eq!(tick(&mut app), EventResult::Ignored);
        assert_eq!(app.board.pieces(), RUN, "a piece was played after the win");
    }

    #[test]
    fn a_second_tick_does_not_play_a_second_reply() {
        let mut app = shallow();
        app.drop_at(3);
        tick(&mut app);
        assert_eq!(tick(&mut app), EventResult::Ignored);
        assert_eq!(app.moves().len(), 2, "the AI moved twice for one turn");
    }

    #[test]
    fn the_clock_runs_only_while_the_search_owes_a_move() {
        // `tick_interval` is consulted after every event, so this starts when
        // the player moves and stops when the reply lands. A game waiting on a
        // person holds no timer.
        let mut app = shallow();
        assert_eq!(app.tick_interval(), None, "a clock on the player's turn");
        app.drop_at(3);
        assert_eq!(app.tick_interval(), Some(AI_TICK));
        tick(&mut app);
        assert_eq!(app.tick_interval(), None, "the clock outlived the reply");
    }

    #[test]
    fn the_clock_stops_when_the_game_ends() {
        let mut app = shallow();
        app.current_player = Cell::Yellow;
        assert_eq!(app.tick_interval(), Some(AI_TICK));
        app.status = GameStatus::Draw;
        assert_eq!(app.tick_interval(), None);
    }

    #[test]
    fn a_close_request_ends_the_program() {
        let mut app = game();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn an_event_that_changed_something_asks_for_a_repaint_and_one_that_did_not_does_not() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Num1))),
            Response::Redraw
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle,
            "an ignored key asked for a frame"
        );
    }

    #[test]
    fn an_event_this_window_has_no_use_for_is_ignored() {
        let mut app = game();
        for event in [
            Event::FocusIn,
            Event::FocusOut,
            Event::Moved { x: 10, y: 20 },
            Event::ScaleChanged { scale: 2.0 },
        ] {
            assert_eq!(
                handle_event(&mut app, &event),
                EventResult::Ignored,
                "{event:?}"
            );
        }
        assert_eq!(app.board.pieces(), 0);
    }

    #[test]
    fn rendering_remembers_the_size_it_rendered_at() {
        // The size a frame is drawn at is the size the next click is read
        // against -- that is the whole point of storing it.
        let mut app = game();
        app.render(333.0, 222.0);
        assert_eq!((app.width, app.height), (333.0, 222.0));
    }

    #[test]
    fn the_window_names_itself_and_opens_at_a_size_the_board_fits_in() {
        let app = game();
        assert_eq!(app.title(), "Connect Four");
        assert_eq!(app.app_id(), "connect4");
        let (w, h) = app.initial_size();
        assert_eq!((w, h), (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32));
        let l = Layout::new(w as f32, h as f32);
        assert!(l.shows(l.header) && l.shows(l.info) && l.shows(l.chute) && l.shows(l.footer));
        assert!(
            !l.board.is_empty(),
            "the default window has no room for a board"
        );
    }

    // ── The drawing pass ──

    #[test]
    fn a_frame_is_drawn_at_the_size_it_was_asked_for() {
        for (w, h) in WINDOWS {
            let f = game().frame(w, h);
            assert_eq!((f.width, f.height), (w.max(1.0), h.max(1.0)), "at {w}x{h}");
        }
    }

    #[test]
    fn every_frame_closes_what_it_opens() {
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            assert!(
                app.frame(w, h).is_balanced(),
                "with the sheet up at {w}x{h}"
            );
            app.apply(Intent::ToggleHelp);
            assert!(app.frame(w, h).is_balanced(), "at {w}x{h}");
        }
    }

    #[test]
    fn a_frame_paints_something_at_every_size_it_is_asked_for() {
        // Including sizes with no room for a band or a word: the window is
        // never blank, because the background alone is a command.
        for (w, h) in WINDOWS {
            assert!(
                !game().frame(w, h).commands().is_empty(),
                "nothing was drawn at {w}x{h}"
            );
        }
    }

    #[test]
    fn nothing_is_ever_painted_outside_the_window() {
        for (w, h) in WINDOWS {
            let mut app = game();
            app.drop_at(3);
            app.apply(Intent::ToggleHelp);
            let f = app.frame(w, h);
            for c in f.commands() {
                if let RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } = c
                {
                    assert!(
                        *x >= -0.01 && *y >= -0.01 && x + width <= w.max(1.0) + 0.01,
                        "a box at ({x}, {y}) {width}x{height} leaves the {w}x{h} window"
                    );
                }
            }
        }
    }

    #[test]
    fn every_line_of_text_starts_inside_the_window() {
        // A line centred in a box narrower than the line would otherwise begin
        // at a negative offset -- off the left edge, and for a box at the top of
        // the window off the top as well.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.human_wins = 999;
            app.apply(Intent::ToggleHelp);
            for (body, r, _) in text_boxes(&app.frame(w, h)) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01,
                    "{body:?} starts at ({}, {}) in a {w}x{h} window",
                    r.x,
                    r.y
                );
            }
        }
    }

    #[test]
    fn no_line_is_ever_asked_for_at_a_size_the_renderer_would_round_up() {
        // The renderer clamps a requested size to a whole pixel, so a request
        // below one pixel is drawn *larger* than the band it was sized to fit
        // and every caller that shrinks its type to fit is silently overruled
        // (`known-issues.md` lesson 60). Refusing is the honest answer.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            for (body, _, size) in text_boxes(&app.frame(w, h)) {
                assert!(
                    size >= MIN_DRAWN_FONT,
                    "{body:?} asked for {size}px in a {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn a_window_with_no_room_for_a_legible_line_shows_its_boxes_and_no_words() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let f = app.frame(4.0, 4.0);
        assert!(
            texts(&f).is_empty(),
            "words were drawn at 4x4: {:?}",
            texts(&f)
        );
        assert!(
            f.commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { .. })),
            "the sheet was not drawn either"
        );
    }

    #[test]
    fn an_empty_string_is_never_pushed_as_a_line_of_text() {
        // Elided to fit in no space at all, a label would be an empty string
        // sitting in the frame: a text command that paints nothing and still
        // counts as text drawn.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            for body in texts(&app.frame(w, h)) {
                assert!(!body.is_empty(), "an empty line was drawn at {w}x{h}");
            }
        }
        // The sweep above is a claim about the callers: none of them passes an
        // empty body today, which is exactly why it could not tell whether
        // `label` refuses one. The refusal is the helper's own contract, so it
        // is asked directly.
        let mut f = Frame::new(100.0, 100.0);
        label(
            &mut f,
            10.0,
            10.0,
            "",
            12.0,
            COL_TEXT,
            FontWeightHint::Regular,
            Some(80.0),
        );
        assert!(
            f.commands().is_empty(),
            "an empty string was pushed as a line of text"
        );
    }

    #[test]
    fn a_label_with_no_width_to_fit_in_is_not_drawn() {
        let mut f = Frame::new(100.0, 100.0);
        label(
            &mut f,
            0.0,
            0.0,
            "hello",
            12.0,
            COL_TEXT,
            FontWeightHint::Regular,
            Some(0.0),
        );
        assert!(texts(&f).is_empty(), "a label was drawn into no width");
    }

    #[test]
    fn a_box_with_no_area_is_not_painted() {
        let mut f = Frame::new(100.0, 100.0);
        fill(&mut f, Rect::EMPTY, COL_RED, 0.0);
        fill(&mut f, Rect::new(10.0, 10.0, 0.0, 20.0), COL_RED, 0.0);
        assert!(f.commands().is_empty(), "an empty box was painted");
    }

    #[test]
    fn a_button_with_no_box_is_neither_painted_nor_clickable() {
        let mut f = Frame::new(100.0, 100.0);
        button(
            &mut f,
            Rect::EMPTY,
            Target::Help,
            "Help",
            10.0,
            COL_SURFACE1,
            COL_TEXT,
        );
        assert!(f.commands().is_empty());
        assert!(f.hits().is_empty(), "a control nobody can see took clicks");
    }

    #[test]
    fn a_line_is_centred_in_its_box_both_ways() {
        let mut f = Frame::new(200.0, 100.0);
        let r = Rect::new(20.0, 30.0, 160.0, 40.0);
        centred(&mut f, r, "hi", 12.0, COL_TEXT, FontWeightHint::Regular);
        let boxes = text_boxes(&f);
        assert_eq!(boxes.len(), 1);
        let drawn = boxes[0].1;
        assert!(
            (drawn.x - r.x - (r.w - drawn.w) / 2.0).abs() < 0.01,
            "not centred across: {drawn:?} in {r:?}"
        );
        assert!(
            (drawn.y - r.y - (r.h - drawn.h) / 2.0).abs() < 0.01,
            "not centred down: {drawn:?} in {r:?}"
        );
    }

    #[test]
    fn a_line_too_big_for_its_box_starts_at_the_boxs_corner_rather_than_before_it() {
        let mut f = Frame::new(200.0, 100.0);
        let r = Rect::new(20.0, 30.0, 4.0, 4.0);
        centred(
            &mut f,
            r,
            "a very long line indeed",
            12.0,
            COL_TEXT,
            FontWeightHint::Regular,
        );
        let drawn = text_boxes(&f)[0].1;
        assert!(drawn.x >= r.x - 0.01, "it began left of its box");
        assert!(drawn.y >= r.y - 0.01, "it began above its box");
    }

    // ── The header ──

    #[test]
    fn the_window_says_what_game_it_is() {
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(texts(&f).contains(&"Connect Four".to_string()));
    }

    #[test]
    fn the_three_readouts_are_labelled_and_show_their_counts() {
        // The counts are looked for *inside the readout they belong to*, not
        // anywhere in the frame. A tally of 3 and the number over column 3 are
        // the same string, so a frame that draws no counts at all still holds
        // "1" through "7" in the chute: this test passed with the count
        // replaced by an empty string until the box was part of the question
        // (`known-issues.md` lesson 57).
        let mut app = game();
        app.human_wins = 3;
        app.ai_wins = 5;
        app.draws = 2;
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (i, (label, count)) in [("You", "3"), ("AI", "5"), ("Draw", "2")]
            .into_iter()
            .enumerate()
        {
            // Index 0 is the box nearest the *right* edge, so the readouts
            // read left to right in the reverse of the order they are indexed.
            let box_ = l.score_box(2 - i);
            assert!(!box_.is_empty(), "readout {i} did not fit");
            for body in [label, count] {
                assert!(
                    drawn.iter().any(|(text, r, _)| text == body
                        && box_.contains(r.x + r.w / 2.0, r.y + r.h / 2.0)),
                    "{body} is not written in the {label} readout: {drawn:?}"
                );
            }
        }
    }

    #[test]
    fn the_readouts_read_left_to_right_as_you_then_ai_then_draw() {
        // They are laid out from the right edge inwards, so getting the order
        // right means reversing the list -- an easy thing to get backwards and
        // an invisible one without this.
        let mut app = game();
        app.human_wins = 1;
        app.ai_wins = 2;
        app.draws = 3;
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        let x_of = |name: &str| {
            boxes
                .iter()
                .find(|(body, _, _)| body == name)
                .unwrap_or_else(|| panic!("no {name} in the frame"))
                .1
                .x
        };
        assert!(x_of("You") < x_of("AI"), "the AI readout is left of yours");
        assert!(
            x_of("AI") < x_of("Draw"),
            "the draw readout is left of the AI's"
        );
    }

    #[test]
    fn a_readout_shows_the_tally_it_names_and_not_a_neighbours() {
        // Three different counts, so a readout wired to the wrong field shows a
        // number that is on screen but in the wrong box (`known-issues.md`
        // lesson 59 -- a fixture with no asymmetry cannot notice a swap).
        let mut app = game();
        app.human_wins = 7;
        app.ai_wins = 8;
        app.draws = 9;
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        let near = |name: &str, count: &str| {
            let label = boxes.iter().find(|(b, _, _)| b == name).expect("no label");
            let value = boxes.iter().find(|(b, _, _)| b == count).expect("no count");
            (label.1.centre().0 - value.1.centre().0).abs() < label.1.w.max(value.1.w)
        };
        assert!(near("You", "7"), "your wins are not under your label");
        assert!(near("AI", "8"), "the AI's wins are not under its label");
        assert!(near("Draw", "9"), "the draws are not under their label");
    }

    #[test]
    fn the_title_is_never_drawn_over_the_readouts() {
        // It takes whatever they left and is elided rather than written across
        // them.
        for (w, h) in [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (300.0, 400.0),
            (120.0, 800.0),
        ] {
            let mut app = game();
            app.human_wins = 88;
            let l = Layout::new(w, h);
            let f = app.frame(w, h);
            let title = text_boxes(&f)
                .into_iter()
                .find(|(b, _, _)| b == "Connect Four")
                .map(|(_, r, _)| r);
            let leftmost = (0..3)
                .map(|i| l.score_box(i))
                .filter(|r| !r.is_empty())
                .map(|r| r.x)
                .fold(f32::INFINITY, f32::min);
            if let Some(t) = title {
                assert!(
                    t.right() <= leftmost + 0.01,
                    "the title runs to {} and the readouts start at {leftmost} at {w}x{h}",
                    t.right()
                );
            }
        }
    }

    #[test]
    fn a_header_that_did_not_fit_is_not_drawn_at_all() {
        let f = game().frame(24.0, 24.0);
        let lines = texts(&f);
        assert!(!lines.contains(&"Connect Four".to_string()), "{lines:?}");
        assert!(!lines.contains(&"You".to_string()), "{lines:?}");
    }

    // ── The status line ──

    #[test]
    fn the_status_line_says_it_is_your_turn_when_it_is() {
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            texts(&f).iter().any(|t| t.contains("Your turn")),
            "the window does not say whose turn it is: {:?}",
            texts(&f)
        );
    }

    #[test]
    fn the_status_line_says_the_search_is_thinking_while_it_owes_a_move() {
        // The string existed before and could never be seen: the search ran
        // inside the key handler that started it, so the frame that would have
        // shown the message was never painted.
        let mut app = game();
        app.drop_at(3);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            texts(&f).iter().any(|t| t.contains("thinking")),
            "no word of the pause: {:?}",
            texts(&f)
        );
    }

    #[test]
    fn the_status_line_announces_a_win_a_loss_and_a_draw_differently() {
        let mut app = game();
        app.status = GameStatus::Won(Cell::Red);
        let won = app.status_line();
        app.status = GameStatus::Won(Cell::Yellow);
        let lost = app.status_line();
        app.status = GameStatus::Draw;
        let drawn = app.status_line();
        assert_ne!(won, lost, "winning and losing read the same");
        assert_ne!(won, drawn);
        assert_ne!(lost, drawn);
        assert!(won.contains("win"), "{won:?}");
        assert!(drawn.contains("draw"), "{drawn:?}");
    }

    #[test]
    fn the_status_line_is_coloured_by_what_it_says() {
        let mut app = game();
        assert_eq!(app.status_colour(), COL_BLUE, "a game in play");
        app.status = GameStatus::Won(Cell::Red);
        assert_eq!(app.status_colour(), COL_GREEN, "a win");
        app.status = GameStatus::Won(Cell::Yellow);
        assert_eq!(app.status_colour(), COL_RED, "a loss");
        app.status = GameStatus::Draw;
        assert_eq!(app.status_colour(), COL_PEACH, "a draw");
    }

    #[test]
    fn the_status_line_reaches_the_frame_in_the_colour_it_chose() {
        let mut app = game();
        app.status = GameStatus::Won(Cell::Red);
        let drawn = app
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text.contains("win") => Some(*color),
                _ => None,
            })
            .expect("the winning line was not drawn");
        assert_eq!(drawn, COL_GREEN);
    }

    // ── The chute ──

    #[test]
    fn every_column_is_numbered_above_it() {
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let lines = texts(&f);
        for col in 0..COLS {
            assert!(
                lines.contains(&(col + 1).to_string()),
                "column {col} is not numbered"
            );
        }
    }

    #[test]
    fn each_number_is_drawn_over_the_column_it_names() {
        // "Is it drawn?" is not "is it drawn *there*?" (`known-issues.md`
        // lesson 57): a column number in the wrong slot points at a column the
        // player did not mean.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (body, r, _) in text_boxes(&f) {
            let Ok(number) = body.parse::<usize>() else {
                continue;
            };
            if number == 0 || number > COLS {
                continue;
            }
            let slot = l.chute_slot(number - 1);
            let (cx, _) = r.centre();
            assert!(
                slot.x <= cx && cx <= slot.right(),
                "the number {number} is at {cx}, and its slot is {slot:?}"
            );
        }
    }

    #[test]
    fn the_piece_about_to_fall_is_shown_over_the_cursors_column() {
        let mut app = game();
        app.cursor_col = 1;
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let over = |col: usize| {
            let slot = l.chute_slot(col);
            f.commands().iter().any(|c| match c {
                RenderCommand::FillRect {
                    x, y, color, width, ..
                } => *color == Cell::Red.face() && slot.contains(x + width / 2.0, *y + 1.0),
                _ => false,
            })
        };
        assert!(over(1), "nothing is waiting over the cursor's column");
        // And over no other. "There is a piece over column 1" is also true of
        // a chute with a piece over every column, which is what the test said
        // before this line: the cursor is *where* the piece is, so the claim
        // has to be about the other six columns too (lesson 57).
        for col in (0..COLS).filter(|&c| c != 1) {
            assert!(!over(col), "a piece is also waiting over column {col}");
        }
    }

    #[test]
    fn the_piece_about_to_fall_moves_with_the_cursor() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let waiting_over = |app: &Connect4, col: usize| {
            let slot = l.chute_slot(col);
            app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands()
                .iter()
                .any(|c| match c {
                    RenderCommand::FillRect {
                        x, y, color, width, ..
                    } => *color == Cell::Red.face() && slot.contains(x + width / 2.0, *y + 1.0),
                    _ => false,
                })
        };
        let mut app = game();
        assert!(waiting_over(&app, CENTER_COL));
        app.apply(Intent::CursorLeft);
        assert!(waiting_over(&app, CENTER_COL - 1), "it did not follow");
        assert!(
            !waiting_over(&app, CENTER_COL),
            "it was left behind as well as moved"
        );
    }

    #[test]
    fn nothing_waits_over_a_column_that_cannot_take_a_piece() {
        // A disc over a full column would promise a move the program then
        // refuses.
        let mut app = game();
        for _ in 0..ROWS {
            app.drop_at(0);
        }
        app.current_player = Cell::Red;
        app.cursor_col = 0;
        let slot = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).chute_slot(0);
        let waiting = app
            .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
            .commands()
            .iter()
            .any(|c| match c {
                RenderCommand::FillRect {
                    x, y, color, width, ..
                } => *color == Cell::Red.face() && slot.contains(x + width / 2.0, *y + 1.0),
                _ => false,
            });
        assert!(!waiting, "a piece waits over a column that is full");
    }

    #[test]
    fn nothing_waits_over_any_column_once_the_game_is_over() {
        let mut app = game();
        app.board = won_board(Cell::Red);
        app.status = GameStatus::Won(Cell::Red);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for col in 0..COLS {
            let slot = l.chute_slot(col);
            let waiting = f.commands().iter().any(|c| match c {
                RenderCommand::FillRect {
                    x, y, color, width, ..
                } => *color == Cell::Red.face() && slot.contains(x + width / 2.0, *y + 1.0),
                _ => false,
            });
            assert!(!waiting, "a piece waits over column {col} after the game");
        }
    }

    // ── The board ──

    #[test]
    fn every_hole_of_the_board_is_painted() {
        let f = game().frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for row in 0..ROWS {
            for col in 0..COLS {
                assert_eq!(
                    fill_at(&f, l.cell(row, col)),
                    Some(Cell::Empty.face()),
                    "({row}, {col}) is not a hole"
                );
            }
        }
    }

    #[test]
    fn a_piece_is_painted_in_the_hole_it_landed_in() {
        let mut app = game();
        app.drop_at(2);
        app.drop_at(2);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(fill_at(&f, l.cell(0, 2)), Some(COL_RED), "the first piece");
        assert_eq!(fill_at(&f, l.cell(1, 2)), Some(COL_YELLOW), "the second");
        assert_eq!(fill_at(&f, l.cell(2, 2)), Some(Cell::Empty.face()), "above");
    }

    #[test]
    fn a_piece_is_painted_at_the_bottom_of_the_window_and_not_the_top() {
        // Row 0 is the floor. Drawn at the top the board would be upside down
        // and every game would read backwards.
        let mut app = game();
        app.drop_at(0);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let piece = f.commands().iter().find_map(|c| match c {
            RenderCommand::FillRect { x, y, color, .. } if *color == COL_RED => Some((*x, *y)),
            _ => None,
        });
        let (_, y) = piece.expect("the piece was not drawn");
        assert!(
            y > l.board.centre().1,
            "the piece was drawn in the top half of the board"
        );
    }

    #[test]
    fn the_winning_four_are_ringed_and_joined() {
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let rings = f
            .commands()
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == COL_GREEN))
            .count();
        assert_eq!(rings, RUN, "the ring is not around all four");
        assert!(
            f.commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Line { color, .. } if *color == COL_GREEN)),
            "the run's direction is not drawn"
        );
    }

    #[test]
    fn the_line_joining_the_winning_four_runs_between_the_ends_of_the_run() {
        let mut app = game();
        for col in 0..3 {
            app.drop_at(col);
            app.current_player = Cell::Red;
        }
        app.drop_at(3);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = f
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::Line { x1, y1, x2, y2, .. } => Some((*x1, *y1, *x2, *y2)),
                _ => None,
            })
            .expect("no line was drawn");
        let (ax, ay) = l.cell(0, 0).centre();
        let (bx, by) = l.cell(0, 3).centre();
        assert!(
            (drawn.0 - ax).abs() < 0.01 && (drawn.1 - ay).abs() < 0.01,
            "start"
        );
        assert!(
            (drawn.2 - bx).abs() < 0.01 && (drawn.3 - by).abs() < 0.01,
            "end"
        );
    }

    #[test]
    fn a_game_still_in_play_has_nothing_ringed() {
        let mut app = game();
        app.drop_at(0);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            !f.commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::StrokeRect { .. })),
            "four cells are ringed on a board with no win on it"
        );
    }

    // ── Hit boxes ──

    #[test]
    fn every_column_is_a_control_at_every_size_the_board_is_drawn_at() {
        for (w, h) in WINDOWS {
            let app = windowed(w, h);
            if Layout::new(w, h).board.is_empty() {
                continue;
            }
            for col in 0..COLS {
                assert!(
                    probe::is_visible_sized(&app, Target::Column(col), (w, h)),
                    "column {col} takes no clicks at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn a_columns_control_is_the_strip_the_layout_says_it_is() {
        // Recorded by the pass that paints it, so there is no second copy of
        // the geometry to disagree with the first.
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for col in 0..COLS {
            assert_eq!(
                hits_for(&f, Target::Column(col)),
                vec![l.column(col)],
                "column {col}"
            );
        }
    }

    #[test]
    fn the_three_footer_buttons_are_controls_and_are_labelled() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let lines = texts(&f);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (i, (target, name)) in [
            (Target::NewGame, "New game"),
            (Target::Undo, "Undo"),
            (Target::Help, "Help"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(hits_for(&f, target).len(), 1, "{name} is not one control");
            assert!(lines.contains(&name.to_string()), "{name} has no label");
            // The hit box is the button, not merely *a* box: counting controls
            // and reading labels cannot tell two buttons drawn in one place
            // from two buttons in two places, nor a hit box twice the width of
            // what it sits under.
            assert_eq!(
                hits_for(&f, target).first().copied(),
                Some(l.footer_button(i)),
                "{name}'s hit box is not the button it is drawn as"
            );
        }
    }

    #[test]
    fn a_footer_buttons_label_is_inside_the_button() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (i, name) in ["New game", "Undo", "Help"].into_iter().enumerate() {
            let r = boxes
                .iter()
                .find(|(b, _, _)| b == name)
                .unwrap_or_else(|| panic!("{name} is not drawn"))
                .1;
            let button = l.footer_button(i);
            assert!(
                r.x >= button.x - 0.01 && r.right() <= button.right() + 0.01,
                "{name} at {r:?} is outside its button {button:?}"
            );
        }
    }

    #[test]
    fn the_undo_button_is_greyed_when_there_is_nothing_to_take_back() {
        // Greyed rather than gone: the button keeps its hit box, so the answer
        // to pressing it is a refusal the player can see rather than a control
        // that moved out from under the pointer.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let cold = fill_at(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT), l.footer_button(1));
        app.drop_at(3);
        let warm = fill_at(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT), l.footer_button(1));
        assert_eq!(cold, Some(COL_SURFACE0), "the dead button is not greyed");
        assert_eq!(warm, Some(COL_SURFACE1), "the live button is greyed");
        assert_ne!(cold, warm);
    }

    #[test]
    fn the_undo_button_takes_clicks_even_when_there_is_nothing_to_undo() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!app.can_undo());
        assert!(
            probe::is_visible_sized(&app, Target::Undo, (WINDOW_WIDTH, WINDOW_HEIGHT)),
            "the greyed button stopped being a control"
        );
    }

    #[test]
    fn there_are_no_footer_controls_in_a_window_with_no_footer() {
        let app = windowed(24.0, 24.0);
        for target in [Target::NewGame, Target::Undo, Target::Help] {
            assert!(
                !probe::is_visible_sized(&app, target, (24.0, 24.0)),
                "{target:?} is clickable in a window that does not draw it"
            );
        }
    }

    #[test]
    fn the_window_lists_every_control_it_has_and_no_others() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut names = probe::control_names(&app);
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names,
            vec!["Column", "Help", "NewGame", "Undo"],
            "the controls on screen are not the ones the program means to have"
        );
    }

    #[test]
    fn the_help_sheet_appears_in_the_controls_only_when_it_is_up() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(!probe::control_names(&app).contains(&"HelpSheet".to_string()));
        app.apply(Intent::ToggleHelp);
        assert!(probe::control_names(&app).contains(&"HelpSheet".to_string()));
    }

    // ── The help sheet, drawn ──

    /// The sheet's title is the same string as the header's, so counting the
    /// title is the only way to tell "the sheet is up" from "the header is
    /// drawn": asserting the *absence* of `HELP_TITLE` here would fail on a
    /// closed sheet, because the header wrote it.
    #[test]
    fn the_sheet_is_not_drawn_until_it_is_asked_for() {
        let lines = texts(&game().frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (key, meaning) in HELP_ROWS {
            assert!(!lines.contains(&key.to_string()), "{key} without the sheet");
            assert!(
                !lines.contains(&meaning.to_string()),
                "{meaning} without the sheet"
            );
        }
        assert!(
            !lines.iter().any(|l| l.contains("Click anywhere")),
            "the closing line is drawn over a shut sheet"
        );
        assert_eq!(
            lines.iter().filter(|l| *l == HELP_TITLE).count(),
            1,
            "the title is drawn twice with the sheet shut: {lines:?}"
        );
    }

    #[test]
    fn the_sheet_lists_every_control_and_what_it_does() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let lines = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert_eq!(
            lines.iter().filter(|l| *l == HELP_TITLE).count(),
            2,
            "the sheet does not head itself: {lines:?}"
        );
        for (key, meaning) in HELP_ROWS {
            assert!(lines.contains(&key.to_string()), "{key} is not listed");
            assert!(lines.contains(&meaning.to_string()), "{meaning} is missing");
        }
        assert!(
            lines.iter().any(|l| l.contains("Click anywhere")),
            "the sheet does not say how to shut it"
        );
    }

    #[test]
    fn the_sheets_rows_do_not_overwrite_one_another() {
        // Each row is sized to the band it is written in, not to the sheet: a
        // row taller than its band is written across the row beneath it.
        //
        // Swept over every window, because the two sizes only part company in
        // a small one: at the sizes a desktop actually opens at, the type is
        // already smaller than the band and the `.min(step * 0.7)` that keeps
        // it that way cannot be told from its own absence.
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Intent::ToggleHelp);
            let f = app.frame(w, h);
            let mut rows: Vec<Rect> = text_boxes(&f)
                .into_iter()
                .filter(|(b, _, _)| HELP_ROWS.iter().any(|(k, _)| k == b))
                .map(|(_, r, _)| r)
                .collect();
            if (w, h) == (WINDOW_WIDTH, WINDOW_HEIGHT) {
                assert_eq!(rows.len(), HELP_ROWS.len(), "a row is missing");
            }
            rows.sort_by(|a, b| a.y.total_cmp(&b.y));
            for pair in rows.windows(2) {
                assert!(
                    pair[0].bottom() <= pair[1].y + 0.01,
                    "at {w}x{h} a row at {:?} runs into the one at {:?}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn the_closing_line_is_written_below_the_last_row_and_not_across_it() {
        // The ladder divides the sheet's body by `rows + 1` for exactly this
        // reason: measured back from the sheet's own bottom edge instead, this
        // line would sit inside the last row's band.
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        let closing = boxes
            .iter()
            .find(|(b, _, _)| b.contains("Click anywhere"))
            .expect("no closing line")
            .1;
        let last_key = HELP_ROWS[HELP_ROWS.len() - 1].0;
        let last = boxes
            .iter()
            .find(|(b, _, _)| b == last_key)
            .expect("no last row")
            .1;
        assert!(
            closing.y >= last.bottom() - 0.01,
            "the closing line at {closing:?} is written over the row at {last:?}"
        );
        // …and the band it is centred in is one of the sheet's own, not a
        // seventh hung off the bottom edge. Dividing the body by `rows`
        // instead of `rows + 1` still leaves this line below the last row —
        // the rows simply spread out — and puts it outside the sheet, so
        // "below the last row" is only half the claim.
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            closing.bottom() <= l.help.bottom() - l.pad + 0.01,
            "the closing line at {closing:?} hangs off the sheet at {:?}",
            l.help
        );
    }

    #[test]
    fn a_rows_meaning_is_written_to_the_right_of_the_key_it_explains() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let boxes = text_boxes(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (key, meaning) in HELP_ROWS {
            let k = boxes.iter().find(|(b, _, _)| b == key).expect("no key");
            let m = boxes
                .iter()
                .find(|(b, _, _)| b == meaning)
                .expect("no meaning");
            assert!(
                m.1.x >= k.1.right() - 0.01,
                "{meaning} is written over {key}"
            );
            assert!(
                (m.1.y - k.1.y).abs() < 0.01,
                "{meaning} is not on the same line as {key}"
            );
        }
    }

    #[test]
    fn every_line_of_the_sheet_stays_inside_the_sheet() {
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (body, r, _) in text_boxes(&app.frame(w, h)) {
                if !HELP_ROWS.iter().any(|(k, m)| *k == body || *m == body) {
                    continue;
                }
                assert!(
                    r.x >= l.help.x - 0.01 && r.bottom() <= l.help.bottom() + 0.01,
                    "{body:?} at {r:?} leaves the sheet {:?} at {w}x{h}",
                    l.help
                );
            }
        }
    }

    #[test]
    fn the_sheet_takes_the_whole_window_and_takes_it_last() {
        // Its own rectangle would leave the columns and the footer live
        // underneath a sheet that covers the board, and a click meant to shut
        // it would drop a piece the player could not see. Recorded last, so it
        // lies over every control drawn before it.
        let mut app = game();
        app.apply(Intent::ToggleHelp);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            hits_for(&f, Target::HelpSheet),
            vec![Rect::new(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT)],
            "the sheet claims less than the window"
        );
        assert_eq!(
            f.hits().last().map(|(t, _)| *t),
            Some(Target::HelpSheet),
            "a control was recorded over the sheet"
        );
    }

    #[test]
    fn the_sheet_can_be_shut_in_a_window_too_small_to_draw_the_button_that_opened_it() {
        let mut app = windowed(24.0, 24.0);
        app.apply(Intent::ToggleHelp);
        assert!(
            !probe::is_visible_sized(&app, Target::Help, (24.0, 24.0)),
            "the fixture window still draws the Help button"
        );
        assert_eq!(click(&mut app, 12.0, 12.0), EventResult::Consumed);
        assert!(!app.help_is_open(), "the sheet cannot be shut at 24x24");
    }
}
