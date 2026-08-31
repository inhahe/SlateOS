//! Tic-tac-toe against a player that cannot be beaten.
//!
//! Three in a row on a 3x3 grid. The computer searches the whole game to the
//! end, so the best any opponent can manage is a draw.
//!
//! ## What wiring this up found
//!
//! `main` built a `TicTacToeApp`, dropped it and exited, so no grid ever
//! reached a screen and no key or click ever arrived. Underneath that, five
//! faults:
//!
//! 1. **Every key fired twice**, because the handler destructured
//!    `Event::Key(KeyEvent { key, .. })` and never read `pressed`. The arrow
//!    keys therefore moved the cursor *two* cells per press — from the bottom
//!    row, one press of Up landed on the top row — so the middle row of the
//!    board was unreachable by keyboard from either edge. Worse, `Enter` on a
//!    finished game reset the board on the press and then, finding a fresh
//!    game underneath, **played a move on the release**: asking for a new game
//!    started you one move down, with the computer's reply already on the
//!    board, in a square you never chose.
//! 2. **A click outside the grid placed a mark inside it.** The hit test was
//!    `((x - grid_x) / cell) as i32`, and a cast truncates *towards zero*: a
//!    click one cell to the left of the board gave `-0.4 as i32 == 0`, which
//!    `(0..3).contains(&0)` then accepted as column 0. The same held above the
//!    board — and what sits above the board is the title and the status line,
//!    so clicking the word "Tic-Tac-Toe" played a move in the top-left corner.
//!    Hit boxes are now recorded by the same pass that draws them, so a
//!    control's clickable area cannot drift from the pixels naming it.
//! 3. **The layout was a constant.** `render(width, height)` used `width` to
//!    centre the grid and `height` for the background rectangle and one line
//!    of help; everything else was hard-coded. Cells were 100px whatever the
//!    window, the grid always started 100px down, and the score line always
//!    sat at y=430 — so on a window shorter than 460px the score was drawn
//!    below the bottom edge, and on a window narrower than 300px the grid
//!    hung off both sides at once.
//! 4. **The status line lied and could not be caught at it.** `render`
//!    reported "AI thinking..." whenever it was the computer's turn, but the
//!    computer moved inside the same call as the human — `place_mark` ran the
//!    search and placed the reply before returning — so the turn flipped back
//!    before any frame was drawn and that branch was unreachable. The reply is
//!    now a state the window renders, held for `THINK_MS` and driven by
//!    `Event::Tick`, so the sentence describes something that is true.
//! 5. **`render` needed `&mut self` to hit-test.** It cached `win_width` into
//!    the model so the click handler could recompute the geometry by hand, and
//!    a `win_height` beside it that nothing ever read. Drawing a frame no
//!    longer mutates the game; the window's size lives on the model because
//!    the window sets it, and the frame is a pure function of it.
//!
//! The perfect-play claim is now tested against a brute-force solver — the
//! value of every reachable position, computed independently of the code being
//! tested — rather than against a handful of hand-set boards.

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

// ── Catppuccin Mocha, only the entries this program paints with ──
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

/// Drawn over the window behind the help sheet, and over a finished board.
const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

/// The board is 3x3. Named because `3` appears in a dozen expressions below
/// and half of them would still compile if one of them were a `4`.
const SIDE: usize = 3;
const CELLS: usize = SIDE * SIDE;

/// Every line of three, in index order: rows, then columns, then diagonals.
const LINES: [[usize; SIDE]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

const WINDOW_WIDTH: f32 = 620.0;
const WINDOW_HEIGHT: f32 = 660.0;

/// How long the computer is shown thinking before it plays.
///
/// Not a search cost — the whole game tree is a few thousand nodes and the
/// answer is ready immediately. It is there so a reply reads as a move made in
/// answer to yours rather than as two marks appearing at once.
const THINK_MS: u64 = 400;

/// The tick the pause is measured with, asked for only while it is running.
const TICK_MS: u64 = 16;

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 7] = [
    ("Goal", "Three of your marks in a row, column or diagonal"),
    ("Arrows", "Move the cursor"),
    ("Enter", "Play the square under the cursor"),
    ("Click", "Play a square directly"),
    ("S", "Swap sides; X always moves first"),
    ("N", "New game"),
    ("H", "Show or hide this sheet"),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the grid keeps no matter what.
const BOARD_SHARE: f32 = 0.55;

/// Which band goes first when they do not all fit: footer, header, info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the grid four pixels and shows nothing. The status
/// line goes last: whose turn it is and who won is the only chrome you cannot
/// play without.
const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in — which
/// is exactly how the old hand-written hit test came to accept clicks a cell's
/// width outside the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    /// The square the nine cells are cut from, centred in what is left.
    pub board: Rect,
    pub footer: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a window of the given size.
    ///
    /// The grid is a square — three cells by three, all the same size — so its
    /// side is whatever the shorter of the two free dimensions allows, and it
    /// is centred in the space that is left. The old code fixed the cell at
    /// 100px and the top edge at y=100 regardless, which is why a small window
    /// drew a board hanging off two edges at once.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 40.0).clamp(8.0, 17.0);
        let small = (font - 2.0).max(7.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, footer] order.
        let mut wants = [
            (h * 0.09).clamp(24.0, 46.0),
            (h * 0.06).clamp(16.0, 30.0),
            (h * 0.08).clamp(18.0, 36.0),
        ];
        let budget = (h - h * BOARD_SHARE).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, inf_h, foot_h] = wants;

        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let info = Rect::new(0.0, header.bottom(), w, inf_h);
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        let top = info.bottom();
        let bottom = if foot_h > 0.0 { footer.y } else { h };
        let free = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );
        // Rounded down to a whole multiple of three so the three cells are the
        // same width to the pixel; a side of 100.4 would give two cells of 33
        // and one of 34, and the odd one out is visible on a flat colour.
        let side = (free.w.min(free.h) / SIDE as f32).floor().max(0.0) * SIDE as f32;
        let board = Rect::new(
            free.x + (free.w - side) / 2.0,
            free.y + (free.h - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.9).min(430.0);
        let help_h = (h * 0.9).min(280.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            footer,
            help,
            font,
            small,
            pad,
        }
    }

    /// The `index`th of `count` evenly-spaced buttons filling `row`.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        let n = count.max(1) as f32;
        let gap = (row.w * 0.01).min(6.0);
        let bw = ((row.w - gap * (n + 1.0)) / n).max(0.0);
        Rect::new(
            row.x + gap + index as f32 * (bw + gap),
            row.y,
            bw,
            row.h.max(0.0),
        )
    }

    /// One square of the grid, `index` counting left-to-right then top-to-bottom.
    #[must_use]
    pub fn cell(&self, index: usize) -> Rect {
        let size = self.board.w / SIDE as f32;
        let col = index.checked_rem(SIDE).unwrap_or(0);
        let row = index.checked_div(SIDE).unwrap_or(0).min(SIDE - 1);
        Rect::new(
            self.board.x + col as f32 * size,
            self.board.y + row as f32 * size,
            size,
            size,
        )
    }

    /// The three header buttons — swap sides, new game, help — left to right.
    #[must_use]
    pub fn header_button(&self, index: usize) -> Rect {
        let group_w = (self.header.w * 0.58).min(280.0);
        let row = Rect::new(
            (self.header.right() - self.pad - group_w).max(self.header.x),
            self.header.y + self.header.h * 0.15,
            group_w,
            (self.header.h * 0.7).max(0.0),
        );
        Self::nth_of(row, 3, index)
    }

    /// One of the three score panels along the foot.
    #[must_use]
    pub fn score_panel(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, 3, index)
    }

    #[must_use]
    pub fn shows_header(&self) -> bool {
        self.header.h >= 14.0 && self.header.w >= 80.0
    }
    #[must_use]
    pub fn shows_info(&self) -> bool {
        self.info.h >= 10.0 && self.info.w >= 80.0
    }
    #[must_use]
    pub fn shows_footer(&self) -> bool {
        self.footer.h >= 10.0 && self.footer.w >= 180.0
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    X,
    O,
}

impl Mark {
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::X => Self::O,
            Self::O => Self::X,
        }
    }

    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::O => "O",
        }
    }

    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::X => COL_BLUE,
            Self::O => COL_RED,
        }
    }
}

/// A board: nine squares, each empty or holding a mark.
pub type Cells = [Option<Mark>; CELLS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    /// Somebody has three in a row, and this is where.
    Won(Mark, [usize; SIDE]),
    Draw,
}

/// What the window can ask the game to do.
///
/// Every route in — a key, a click, the reply clock — goes through `apply`, so
/// there is one place that decides whether a move is legal and one place that
/// records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Move the cursor to a square without playing it.
    Select(usize),
    /// Play a square.
    Play(usize),
    NewGame,
    /// Swap which mark the human plays. X always moves first.
    SwapSides,
    ToggleHelp,
}

/// Everything in the window a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Cell(usize),
    NewGame,
    SwapSides,
    Help,
    /// The help sheet itself, over the whole window: while it is up, nothing
    /// behind it is clickable.
    HelpSheet,
}

/// The frame type this program records its hit boxes into.
pub type Frame = guitk::frame::Frame<Target>;

// ── Solver ─────────────────────────────────────────────────────────────────

/// Who has three in a row, and which three.
#[must_use]
pub fn winner_of(cells: &Cells) -> Option<(Mark, [usize; SIDE])> {
    for line in &LINES {
        let (Some(&Some(a)), Some(&b), Some(&c)) =
            (cells.get(line[0]), cells.get(line[1]), cells.get(line[2]))
        else {
            continue;
        };
        if b == Some(a) && c == Some(a) {
            return Some((a, *line));
        }
    }
    None
}

#[must_use]
pub fn is_full(cells: &Cells) -> bool {
    cells.iter().all(Option::is_some)
}

/// The empty squares, in index order.
#[must_use]
pub fn empty_cells(cells: &Cells) -> Vec<usize> {
    (0..CELLS)
        .filter(|&i| cells.get(i) == Some(&None))
        .collect()
}

/// The value of `cells` to `me`, with `to_move` to play and both sides perfect.
///
/// Positive is a win for `me`, zero a draw, negative a loss. The magnitude
/// carries the distance: a win in two scores higher than the same win in four,
/// and a loss in four scores higher than the same loss in two. Without that,
/// every winning move scores the same and the search is free to pick one that
/// wins eventually over one that wins now — which looks, from the other side
/// of the board, exactly like a program that has not noticed it can win.
fn value(cells: &Cells, to_move: Mark, me: Mark, depth: i32) -> i32 {
    if let Some((won, _)) = winner_of(cells) {
        return if won == me {
            10_i32.saturating_sub(depth)
        } else {
            depth.saturating_sub(10)
        };
    }
    let free = empty_cells(cells);
    if free.is_empty() {
        return 0;
    }

    let mut best = if to_move == me { i32::MIN } else { i32::MAX };
    for i in free {
        let mut next = *cells;
        if let Some(slot) = next.get_mut(i) {
            *slot = Some(to_move);
        }
        let score = value(&next, to_move.other(), me, depth.saturating_add(1));
        if to_move == me {
            best = best.max(score);
        } else {
            best = best.min(score);
        }
    }
    best
}

/// The best square for `me` to play, or `None` if the board is finished.
#[must_use]
pub fn best_move(cells: &Cells, me: Mark) -> Option<usize> {
    if winner_of(cells).is_some() {
        return None;
    }
    let mut best: Option<(i32, usize)> = None;
    for i in empty_cells(cells) {
        let mut next = *cells;
        if let Some(slot) = next.get_mut(i) {
            *slot = Some(me);
        }
        let score = value(&next, me.other(), me, 1);
        if best.is_none_or(|(top, _)| score > top) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i)
}

// ── Game ───────────────────────────────────────────────────────────────────

/// The whole game: a board, whose turn it is, the running score, and the size
/// of the window it was last drawn in.
#[derive(Clone)]
pub struct TicTacToe {
    cells: Cells,
    /// Which mark the human plays. The other one is the computer's.
    human: Mark,
    turn: Mark,
    cursor: usize,
    state: GameState,
    /// Wins for the human, wins for the computer, draws.
    scores: [u32; 3],
    /// Milliseconds left in the computer's pause; zero when it is not thinking.
    think_ms: u64,
    show_help: bool,
    width: f32,
    height: f32,
}

impl Default for TicTacToe {
    fn default() -> Self {
        Self::new()
    }
}

impl TicTacToe {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: [None; CELLS],
            human: Mark::X,
            turn: Mark::X,
            // The centre: the one square that is on four of the eight lines,
            // so a player who presses Enter without looking still opens well.
            cursor: 4,
            state: GameState::Playing,
            scores: [0; 3],
            think_ms: 0,
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    #[must_use]
    pub fn cells(&self) -> &Cells {
        &self.cells
    }
    #[must_use]
    pub fn cell(&self, index: usize) -> Option<Mark> {
        self.cells.get(index).copied().flatten()
    }
    #[must_use]
    pub fn human(&self) -> Mark {
        self.human
    }
    #[must_use]
    pub fn computer(&self) -> Mark {
        self.human.other()
    }
    #[must_use]
    pub fn turn(&self) -> Mark {
        self.turn
    }
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    #[must_use]
    pub fn state(&self) -> GameState {
        self.state
    }
    #[must_use]
    pub fn scores(&self) -> [u32; 3] {
        self.scores
    }
    #[must_use]
    pub fn think_ms(&self) -> u64 {
        self.think_ms
    }
    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    /// The winning line, once there is one.
    #[must_use]
    pub fn win_line(&self) -> Option<[usize; SIDE]> {
        match self.state {
            GameState::Won(_, line) => Some(line),
            _ => None,
        }
    }

    #[must_use]
    pub fn playing(&self) -> bool {
        self.state == GameState::Playing
    }

    /// True while the computer owes a reply it has not yet played.
    ///
    /// This is the state the old code had no room for: it searched and placed
    /// inside the human's own event handler, so "thinking" was never true in
    /// any frame that was drawn.
    #[must_use]
    pub fn thinking(&self) -> bool {
        self.think_ms > 0
    }

    /// True when the human may play a square right now.
    #[must_use]
    pub fn human_turn(&self) -> bool {
        self.playing() && self.turn == self.human && !self.thinking()
    }

    /// The sentence the status line shows.
    #[must_use]
    pub fn status(&self) -> String {
        match self.state {
            GameState::Playing => {
                if self.thinking() {
                    "Computer thinking...".to_string()
                } else if self.turn == self.human {
                    format!("Your turn ({})", self.human.symbol())
                } else {
                    format!("Computer's turn ({})", self.computer().symbol())
                }
            }
            GameState::Won(mark, _) if mark == self.human => "You win!".to_string(),
            GameState::Won(_, _) => "Computer wins".to_string(),
            GameState::Draw => "Draw".to_string(),
        }
    }

    /// The cursor moved by one square, stopping at the edge rather than
    /// wrapping — an off-board arrow key should feel like a wall, not like a
    /// jump to the far side.
    #[must_use]
    pub fn neighbour(&self, dx: i32, dy: i32) -> usize {
        let side = SIDE as i32;
        let last = side.saturating_sub(1);
        let here = self.cursor.min(CELLS.saturating_sub(1)) as i32;
        let col = here.rem_euclid(side).saturating_add(dx).clamp(0, last);
        let row = here.div_euclid(side).saturating_add(dy).clamp(0, last);
        let index = row.saturating_mul(side).saturating_add(col);
        index.clamp(0, CELLS.saturating_sub(1) as i32) as usize
    }

    /// Clear the board, keeping the running score.
    ///
    /// X always opens, so if the human plays O the computer owes the first
    /// move — and it takes its pause before making it, exactly as it would
    /// mid-game.
    pub fn new_game(&mut self) {
        self.cells = [None; CELLS];
        self.turn = Mark::X;
        self.state = GameState::Playing;
        self.think_ms = 0;
        if self.turn != self.human {
            self.begin_reply();
        }
    }

    /// Swap which mark the human plays, and start a fresh board.
    ///
    /// Swapping mid-game would hand one side a position the other built, so
    /// the board goes with it.
    pub fn swap_sides(&mut self) {
        self.human = self.human.other();
        self.new_game();
    }

    /// Play `index` for whoever is to move. Returns false if the square is
    /// taken, out of range, or the game is over.
    pub fn play(&mut self, index: usize) -> bool {
        if !self.playing() || self.cell(index).is_some() {
            return false;
        }
        let Some(slot) = self.cells.get_mut(index) else {
            return false;
        };
        *slot = Some(self.turn);
        self.cursor = index;
        self.settle();
        true
    }

    /// Score the board after a move and hand the turn on.
    fn settle(&mut self) {
        if let Some((mark, line)) = winner_of(&self.cells) {
            self.state = GameState::Won(mark, line);
            let slot = usize::from(mark != self.human);
            if let Some(count) = self.scores.get_mut(slot) {
                *count = count.saturating_add(1);
            }
            return;
        }
        if is_full(&self.cells) {
            self.state = GameState::Draw;
            if let Some(count) = self.scores.get_mut(2) {
                *count = count.saturating_add(1);
            }
            return;
        }
        self.turn = self.turn.other();
        if self.turn != self.human {
            self.begin_reply();
        }
    }

    /// Start the computer's pause. Its move is chosen when the pause runs out,
    /// not now, so a board that is reset mid-pause is never played into.
    fn begin_reply(&mut self) {
        self.think_ms = THINK_MS;
    }

    /// Age the computer's pause by `elapsed_ms` and play its move when the
    /// pause is spent. Returns true if anything changed, which is what tells
    /// the window whether the frame is worth redrawing.
    ///
    /// Ageing by the reported interval rather than by counting ticks keeps the
    /// pause the same length whatever rate the compositor settles on.
    pub fn advance(&mut self, elapsed_ms: u64) -> bool {
        if !self.thinking() {
            return false;
        }
        self.think_ms = self.think_ms.saturating_sub(elapsed_ms.max(1));
        if self.think_ms > 0 {
            return true;
        }
        if self.playing()
            && self.turn != self.human
            && let Some(index) = best_move(&self.cells, self.turn)
        {
            self.play(index);
        }
        true
    }

    /// Whether `action` would do anything, so the window can grey out a
    /// control instead of offering one that silently refuses.
    #[must_use]
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Select(i) => i < CELLS && i != self.cursor,
            Action::Play(i) => self.human_turn() && i < CELLS && self.cell(i).is_none(),
            Action::NewGame => !self.cells.iter().all(Option::is_none) || !self.playing(),
            Action::SwapSides | Action::ToggleHelp => true,
        }
    }

    /// The one place a move is made. Returns whether the game changed.
    pub fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::Select(i) => {
                if i >= CELLS || i == self.cursor {
                    return false;
                }
                self.cursor = i;
                true
            }
            Action::Play(i) => {
                if !self.human_turn() {
                    return false;
                }
                // Aim first, so a refused square still shows where you asked:
                // a click on an occupied cell moves the cursor there and does
                // nothing else, which reads as "that one is taken".
                let moved = self.cursor != i && i < CELLS;
                if moved {
                    self.cursor = i;
                }
                self.play(i) || moved
            }
            Action::NewGame => {
                self.new_game();
                true
            }
            Action::SwapSides => {
                self.swap_sides();
                true
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
        }
    }
}

/// Test support: reaching a named position without playing into it.
///
/// The app itself only ever arrives at a board by making legal moves, so these
/// live behind `cfg(test)` rather than widening the public surface with a way
/// to set up a position that nothing in the program needs.
#[cfg(test)]
impl TicTacToe {
    /// Place `mark` on `index`, scoring the board exactly as a real move
    /// would, whichever side is nominally to move.
    fn force(&mut self, index: usize, mark: Mark) {
        self.think_ms = 0;
        self.turn = mark;
        assert!(self.play(index), "square {index} was not free");
        self.think_ms = 0;
    }

    /// Drop any pending reply, for a test that wants to script both sides.
    fn think_clear(&mut self) {
        self.think_ms = 0;
    }
}

// ── Window ─────────────────────────────────────────────────────────────────

impl TicTacToe {
    /// Record the size the window is now, which is the size the next click
    /// will be read against.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.width
    }
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// What a click at (`x`, `y`) would land on, read from the frame the
    /// window is actually showing.
    ///
    /// This replaces the arithmetic that made a click outside the board place
    /// a mark inside it: there is no second copy of the geometry to get wrong,
    /// because the boxes were recorded by the pass that drew them.
    #[must_use]
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }
}

// ── Drawing ────────────────────────────────────────────────────────────────

fn fill(f: &mut Frame, r: Rect, color: Color, radius: f32) {
    if r.w <= 0.0 || r.h <= 0.0 {
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
    if body.is_empty() || size <= 0.0 {
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

/// A label centred in a horizontal span, clamped so a string wider than the
/// span starts at the span's left edge instead of overhanging to its left.
fn centred_in(
    f: &mut Frame,
    left: f32,
    span: f32,
    cy: f32,
    body: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    label(
        f,
        text::center_x(body, left + span / 2.0, size, weight).max(left),
        cy - text::line_height(size, weight) / 2.0,
        body,
        size,
        color,
        weight,
        Some(span.max(0.0)),
    );
}

/// A ring drawn as four bars rather than a stroked rectangle, because a stroke
/// is centred on the edge and would bleed half its width into the neighbouring
/// cell — and so into its hit box.
fn ring(f: &mut Frame, r: Rect, thickness: f32, color: Color) {
    if r.w <= 0.0 || r.h <= 0.0 || thickness <= 0.0 {
        return;
    }
    let t = thickness.min(r.w / 2.0).min(r.h / 2.0);
    fill(f, Rect::new(r.x, r.y, r.w, t), color, 0.0);
    fill(f, Rect::new(r.x, r.bottom() - t, r.w, t), color, 0.0);
    fill(
        f,
        Rect::new(r.x, r.y + t, t, (r.h - t * 2.0).max(0.0)),
        color,
        0.0,
    );
    fill(
        f,
        Rect::new(r.right() - t, r.y + t, t, (r.h - t * 2.0).max(0.0)),
        color,
        0.0,
    );
}

/// One button: a filled pill with a centred caption, and its hit box.
fn button(f: &mut Frame, r: Rect, body: &str, size: f32, bg: Color, fg: Color, target: Target) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(f, r, bg, (r.h * 0.28).min(8.0));
    centred_in(
        f,
        r.x,
        r.w,
        r.y + r.h / 2.0,
        body,
        size,
        fg,
        FontWeightHint::Bold,
    );
    f.hit(target, r);
}

impl TicTacToe {
    /// The layout of this window. A pure function of its size — nothing about
    /// the game changes where anything goes, because the board is always the
    /// same nine squares.
    #[must_use]
    pub fn layout(&self, width: f32, height: f32) -> Layout {
        Layout::new(width, height)
    }

    /// The whole window, and every hit box in it, in one pass.
    ///
    /// `Frame::hit_test` scans the recorded boxes in reverse, so anything drawn
    /// later wins the click over what it covers. That is why the help sheet is
    /// painted last.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = self.layout(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);

        if l.shows_header() {
            self.draw_header(&mut f, &l);
        }
        if l.shows_info() {
            self.draw_info(&mut f, &l);
        }
        self.draw_board(&mut f, &l);
        if !self.playing() {
            self.draw_banner(&mut f, &l);
        }
        if l.shows_footer() {
            self.draw_footer(&mut f, &l);
        }
        if self.show_help {
            draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        let cy = l.header.y + l.header.h / 2.0;
        let first = l.header_button(0);
        let title_span = (first.x - l.pad * 2.0 - l.header.x).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            cy - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
            "Tic-tac-toe",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_span),
        );

        // The side button names the mark you would move *to*, not the one you
        // hold: a button that reads "X" while you are already X invites a
        // click that appears to do nothing.
        let swap = format!("Play {}", self.computer().symbol());
        let captions = [swap.as_str(), "New game", "Help"];
        let targets = [Target::SwapSides, Target::NewGame, Target::Help];
        for (i, (caption, target)) in captions.iter().zip(targets).enumerate() {
            let r = l.header_button(i);
            let live = match target {
                Target::NewGame => self.enabled(Action::NewGame),
                _ => true,
            };
            button(
                f,
                r,
                caption,
                l.small,
                if live { COL_SURFACE0 } else { COL_CRUST },
                if live { COL_TEXT } else { COL_OVERLAY },
                target,
            );
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        let colour = match self.state {
            GameState::Playing => {
                if self.human_turn() {
                    COL_TEXT
                } else {
                    COL_SUBTEXT
                }
            }
            GameState::Won(mark, _) if mark == self.human => COL_GREEN,
            GameState::Won(_, _) => COL_RED,
            GameState::Draw => COL_YELLOW,
        };
        centred_in(
            f,
            l.info.x + l.pad,
            (l.info.w - l.pad * 2.0).max(0.0),
            l.info.y + l.info.h / 2.0,
            &self.status(),
            l.font,
            colour,
            FontWeightHint::Bold,
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.w <= 0.0 || l.board.h <= 0.0 {
            return;
        }
        fill(f, l.board, COL_CRUST, (l.board.w * 0.03).min(14.0));

        let line = self.win_line();
        let gap = (l.board.w * 0.012).min(7.0);
        let radius = (l.board.w * 0.03).min(12.0);
        for i in 0..CELLS {
            let outer = l.cell(i);
            let r = Rect::new(
                outer.x + gap,
                outer.y + gap,
                (outer.w - gap * 2.0).max(0.0),
                (outer.h - gap * 2.0).max(0.0),
            );
            let winning = line.is_some_and(|three| three.contains(&i));
            let aimed = i == self.cursor && self.human_turn();
            fill(
                f,
                r,
                if winning {
                    COL_SURFACE1
                } else if aimed {
                    COL_SURFACE0
                } else {
                    COL_BASE
                },
                radius,
            );
            if aimed {
                ring(f, r, (r.w * 0.035).max(1.5), COL_LAVENDER);
            }
            if let Some(mark) = self.cell(i) {
                // Sized from the cell, not from a constant: the mark has to
                // shrink with the board or a small window draws a glyph wider
                // than the square holding it.
                let size = (r.h * 0.62).max(6.0);
                centred_in(
                    f,
                    r.x,
                    r.w,
                    r.y + r.h / 2.0,
                    mark.symbol(),
                    size,
                    if winning { COL_YELLOW } else { mark.color() },
                    FontWeightHint::Bold,
                );
            }
            // The whole cell, gap included, so the grid lines belong to the
            // square they border rather than falling through to the board.
            f.hit(Target::Cell(i), outer);
        }
    }

    /// The result, over the finished board.
    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let h = (l.board.h * 0.2).clamp(0.0, 48.0);
        let w = (l.board.w * 0.86).min(340.0);
        if h <= 0.0 || w <= 0.0 {
            return;
        }
        let r = Rect::new(
            l.board.x + (l.board.w - w) / 2.0,
            l.board.y + (l.board.h - h) / 2.0,
            w,
            h,
        );
        fill(f, r, COL_VEIL, (h * 0.3).min(12.0));
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.36,
            &self.status(),
            l.font,
            match self.state {
                GameState::Won(mark, _) if mark == self.human => COL_GREEN,
                GameState::Won(_, _) => COL_RED,
                _ => COL_YELLOW,
            },
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.74,
            "Enter or N for a new game",
            l.small,
            COL_SUBTEXT,
            FontWeightHint::Regular,
        );
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        let captions = [
            format!("You ({})", self.human.symbol()),
            format!("Computer ({})", self.computer().symbol()),
            "Draws".to_string(),
        ];
        let colours = [self.human.color(), self.computer().color(), COL_YELLOW];
        for i in 0..3 {
            let r = l.score_panel(i);
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            fill(f, r, COL_CRUST, (r.h * 0.22).min(7.0));
            let count = self.scores.get(i).copied().unwrap_or(0);
            let caption = captions.get(i).map_or("", String::as_str);
            let colour = colours.get(i).copied().unwrap_or(COL_TEXT);
            centred_in(
                f,
                r.x,
                r.w,
                r.y + r.h * 0.32,
                caption,
                (l.small - 1.0).max(6.0),
                COL_SUBTEXT,
                FontWeightHint::Regular,
            );
            centred_in(
                f,
                r.x,
                r.w,
                r.y + r.h * 0.7,
                &count.to_string(),
                l.small,
                colour,
                FontWeightHint::Bold,
            );
        }
    }
}

fn draw_help(f: &mut Frame, l: &Layout) {
    // Dim the whole window first, then the panel on top of it, so the sheet
    // reads as in front of the game rather than part of it.
    fill(f, l.window, COL_SCRIM, 0.0);
    let p = l.help;
    fill(f, p, COL_VEIL, 10.0);

    let pad = (p.w * 0.06).clamp(6.0, 18.0);
    let inner = (p.w - pad * 2.0).max(0.0);
    let title_h = text::line_height(l.font, FontWeightHint::Bold);
    label(
        f,
        p.x + pad,
        p.y + pad,
        HELP_TITLE,
        l.font,
        COL_YELLOW,
        FontWeightHint::Bold,
        Some(inner),
    );

    // Rows share whatever is left below the title, so the sheet cannot write
    // past its own foot however short the window is.
    let top = p.y + pad + title_h + pad / 2.0;
    let room = (p.bottom() - pad - top).max(0.0);
    let step = room / HELP_ROWS.len() as f32;
    let key_span = (inner * 0.28).min(90.0);
    for (i, (k, v)) in HELP_ROWS.iter().enumerate() {
        let y = top + i as f32 * step;
        if y + l.small > p.bottom() - pad {
            break;
        }
        label(
            f,
            p.x + pad,
            y,
            k,
            l.small,
            COL_BLUE,
            FontWeightHint::Bold,
            Some(key_span),
        );
        label(
            f,
            p.x + pad + key_span,
            y,
            v,
            l.small,
            COL_TEXT,
            FontWeightHint::Regular,
            Some((inner - key_span).max(0.0)),
        );
    }

    // Over the whole window, not just the panel: while the sheet is up,
    // nothing behind it is clickable.
    f.hit(Target::HelpSheet, l.window);
}

// ── Input ──────────────────────────────────────────────────────────────────

impl TicTacToe {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The fault that broke every key in this file, in one line. A release
        // is not a second press. Acting on both moved the cursor two squares
        // per arrow press, and made `Enter` on a finished game reset the board
        // and then immediately play a move on the fresh one.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let m = ev.modifiers;
        if m.ctrl || m.alt || m.super_key {
            return EventResult::Ignored;
        }

        if self.show_help {
            // The sheet is modal: it takes every key, and a few of them close
            // it. Letting the rest through would mean playing blind.
            if matches!(ev.key, Key::H | Key::Escape | Key::Enter | Key::Space) {
                self.apply(Action::ToggleHelp);
            }
            return EventResult::Consumed;
        }

        let action = match ev.key {
            Key::Left => Some(Action::Select(self.neighbour(-1, 0))),
            Key::Right => Some(Action::Select(self.neighbour(1, 0))),
            Key::Up => Some(Action::Select(self.neighbour(0, -1))),
            Key::Down => Some(Action::Select(self.neighbour(0, 1))),
            // On a finished board Enter starts the next game — and, because a
            // release no longer fires, it starts it without also playing in it.
            Key::Enter | Key::Space => Some(if self.playing() {
                Action::Play(self.cursor)
            } else {
                Action::NewGame
            }),
            Key::N => Some(Action::NewGame),
            Key::S => Some(Action::SwapSides),
            Key::H => Some(Action::ToggleHelp),
            _ => None,
        };

        match action {
            Some(a) => {
                // Consumed even when the game refuses it: the key belongs to
                // this window either way, and a refused `Enter` must not reach
                // whatever is behind it.
                self.apply(a);
                EventResult::Consumed
            }
            None => EventResult::Ignored,
        }
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        if self.show_help {
            // Anywhere at all dismisses the sheet, including outside it.
            self.apply(Action::ToggleHelp);
            return EventResult::Consumed;
        }
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Cell(index) => {
                if self.playing() {
                    self.apply(Action::Play(index));
                } else {
                    // A finished board is a scoreboard, not a grid: clicking a
                    // square on it starts the next game rather than doing
                    // nothing, which is what every player tries first.
                    self.apply(Action::NewGame);
                }
            }
            Target::NewGame => {
                self.apply(Action::NewGame);
            }
            Target::SwapSides => {
                self.apply(Action::SwapSides);
            }
            Target::Help => {
                self.apply(Action::ToggleHelp);
            }
            Target::HelpSheet => {}
        }
        // Consumed either way: a click that lands on a control the game is
        // refusing should stop there, not fall through to the board.
        EventResult::Consumed
    }
}

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(app: &mut TicTacToe, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The clock the computer's reply never had. Without it the search ran
        // inside the human's own event handler and both marks appeared at once.
        Event::Tick { elapsed_ms } => {
            if app.advance(*elapsed_ms) {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

impl App for TicTacToe {
    fn title(&self) -> String {
        "Tic-tac-toe".to_string()
    }

    fn app_id(&self) -> String {
        "tictactoe".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Ticks are asked for only while the computer owes a reply.
    ///
    /// A finished board needs no frames, and a game that asks for 60 a second
    /// regardless is a game that keeps a laptop awake to draw the same pixels.
    fn tick_interval(&self) -> Option<Duration> {
        if self.thinking() {
            Some(Duration::from_millis(TICK_MS))
        } else {
            None
        }
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

impl Probe for TicTacToe {
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
    let mut game = TicTacToe::new();
    app::launch("tictactoe", &mut game)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use guitk::event::Modifiers;
    use guitk::probe;

    // ── Helpers ────────────────────────────────────────────────────────────

    /// The window sizes every layout claim is checked at: tiny, short, narrow,
    /// square, wide, and larger than any of them.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (620.0, 660.0),
        (800.0, 600.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    fn game() -> TicTacToe {
        TicTacToe::new()
    }

    /// A game whose window is a given size, as the compositor would set it.
    fn windowed(w: f32, h: f32) -> TicTacToe {
        let mut g = game();
        g.resize(w, h);
        g
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            modifiers: Modifiers::default(),
            pressed: false,
            text: String::new(),
        }
    }

    /// A press followed by its release — what a real keyboard sends.
    fn tap(g: &mut TicTacToe, key: Key) {
        probe::key(g, &probe::press(key));
        probe::key(g, &release(key));
    }

    fn click(g: &mut TicTacToe, target: Target) -> EventResult {
        probe::click(g, target)
    }

    /// Click a raw point, as the compositor reports it.
    fn poke(g: &mut TicTacToe, x: f32, y: f32, size: (f32, f32)) -> EventResult {
        g.click_at(x, y, MouseButton::Left, size)
    }

    /// Advance the reply clock far enough to spend any pending pause.
    fn settle(g: &mut TicTacToe) {
        for _ in 0..200 {
            if !g.thinking() {
                return;
            }
            handle_event(g, &Event::Tick { elapsed_ms: 16 });
        }
        panic!("reply clock never ran out");
    }

    /// Every string the frame draws at a given size.
    fn texts(g: &TicTacToe, w: f32, h: f32) -> Vec<String> {
        g.frame(w, h)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn filled(g: &TicTacToe) -> usize {
        g.cells().iter().filter(|c| c.is_some()).count()
    }

    /// Play the game out with the human always taking the lowest free square.
    fn play_to_the_end(g: &mut TicTacToe) {
        settle(g);
        while g.playing() {
            let free = empty_cells(g.cells());
            let Some(&first) = free.first() else { break };
            assert!(g.apply(Action::Play(first)));
            settle(g);
        }
    }

    // ── Fault 1: a release is not a second press ──────────────────────────

    #[test]
    fn arrow_key_release_does_not_move_the_cursor_again() {
        // The old handler read `Event::Key(KeyEvent { key, .. })` and never
        // looked at `pressed`, so one press of Up from the bottom row landed
        // on the top row and the middle row could not be reached at all.
        let mut g = game();
        g.apply(Action::Select(7)); // bottom middle
        tap(&mut g, Key::Up);
        assert_eq!(g.cursor(), 4, "one press of Up should move exactly one row");
        tap(&mut g, Key::Up);
        assert_eq!(g.cursor(), 1);
    }

    #[test]
    fn every_square_is_reachable_by_arrow_keys() {
        // The concrete cost of the double-fire: with two-square steps, half
        // the board was unreachable.
        for target in 0..CELLS {
            let mut g = game();
            for _ in 0..SIDE {
                tap(&mut g, Key::Up);
                tap(&mut g, Key::Left);
            }
            assert_eq!(g.cursor(), 0, "walking to the corner");
            for _ in 0..target % SIDE {
                tap(&mut g, Key::Right);
            }
            for _ in 0..target / SIDE {
                tap(&mut g, Key::Down);
            }
            assert_eq!(g.cursor(), target, "could not walk to square {target}");
        }
    }

    #[test]
    fn enter_on_a_finished_game_starts_an_empty_one() {
        // The old code reset on the press and then, finding a fresh game
        // underneath, played a move on the release — so "new game" handed you
        // a board with marks already on it, in a square you never chose.
        let mut g = game();
        play_to_the_end(&mut g);
        assert!(!g.playing());
        tap(&mut g, Key::Enter);
        assert!(g.playing());
        assert_eq!(filled(&g), 0, "a new game must start empty");
        assert!(!g.thinking());
    }

    #[test]
    fn a_release_plays_no_move() {
        let mut g = game();
        assert_eq!(
            probe::key(&mut g, &release(Key::Enter)),
            EventResult::Ignored
        );
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn help_toggles_once_per_press() {
        // Both `H` and the sheet's dismiss keys are toggles, and a toggle
        // fired twice is a control that does nothing at all.
        let mut g = game();
        tap(&mut g, Key::H);
        assert!(g.show_help(), "H should open the sheet and leave it open");
        tap(&mut g, Key::H);
        assert!(!g.show_help());
    }

    #[test]
    fn swap_sides_toggles_once_per_press() {
        let mut g = game();
        assert_eq!(g.human(), Mark::X);
        tap(&mut g, Key::S);
        assert_eq!(g.human(), Mark::O, "S should swap sides, not swap twice");
    }

    #[test]
    fn one_press_of_enter_plays_one_move() {
        let mut g = game();
        tap(&mut g, Key::Enter);
        assert_eq!(filled(&g), 1);
        assert!(g.thinking());
    }

    // ── Fault 2: clicks outside the board ─────────────────────────────────

    #[test]
    fn a_click_left_of_the_board_plays_nothing() {
        // `((x - grid_x) / cell) as i32` truncates towards zero, so a click a
        // fraction of a cell to the left gave column 0 and placed a mark.
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let board = g.layout(WINDOW_WIDTH, WINDOW_HEIGHT).board;
        let cell = board.w / SIDE as f32;
        for share in [0.3_f32, 0.6, 0.9] {
            let x = board.x - cell * share;
            let y = board.y + cell * 0.5;
            assert_eq!(
                g.target_at(x, y),
                None,
                "a click at x={x} is outside the board and must hit nothing"
            );
            poke(&mut g, x, y, (WINDOW_WIDTH, WINDOW_HEIGHT));
        }
        assert_eq!(
            filled(&g),
            0,
            "no click outside the board may play a square"
        );
    }

    #[test]
    fn a_click_on_the_title_plays_nothing() {
        // What sits above the board is the title and the status line — under
        // the old arithmetic, clicking either played the top-left square.
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = g.layout(WINDOW_WIDTH, WINDOW_HEIGHT);
        let size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        poke(&mut g, l.pad, l.header.y + l.header.h / 2.0, size);
        assert_eq!(filled(&g), 0, "the title is not a square");
        poke(&mut g, l.pad, l.info.y + l.info.h / 2.0, size);
        assert_eq!(filled(&g), 0, "the status line is not a square");
    }

    #[test]
    fn every_square_is_clickable_and_lands_where_it_looks() {
        for index in 0..CELLS {
            let mut g = game();
            assert_eq!(click(&mut g, Target::Cell(index)), EventResult::Consumed);
            assert_eq!(
                g.cell(index),
                Some(Mark::X),
                "clicking square {index} should mark square {index}"
            );
        }
    }

    #[test]
    fn the_hit_box_of_a_square_is_the_square() {
        let g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = g.layout(WINDOW_WIDTH, WINDOW_HEIGHT);
        for index in 0..CELLS {
            let r = l.cell(index);
            assert_eq!(
                probe::rect_of(&g, Target::Cell(index)),
                Some(r),
                "square {index} draws and hit-tests different rectangles"
            );
            for (dx, dy) in [(0.6, 0.6), (r.w - 0.6, 0.6), (0.6, r.h - 0.6)] {
                assert_eq!(g.target_at(r.x + dx, r.y + dy), Some(Target::Cell(index)));
            }
        }
    }

    #[test]
    fn the_frame_is_balanced() {
        let g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(g.frame(WINDOW_WIDTH, WINDOW_HEIGHT).is_balanced());
    }

    #[test]
    fn the_help_sheet_takes_every_click_behind_it() {
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        g.apply(Action::ToggleHelp);
        let r = probe::rect_of(&g, Target::Cell(0)).unwrap();
        assert_eq!(
            g.target_at(r.x + r.w / 2.0, r.y + r.h / 2.0),
            Some(Target::HelpSheet),
            "the sheet must cover the board it hides"
        );
        poke(
            &mut g,
            r.x + r.w / 2.0,
            r.y + r.h / 2.0,
            (WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        assert!(!g.show_help(), "a click anywhere dismisses the sheet");
        assert_eq!(filled(&g), 0, "and plays nothing behind it");
    }

    // ── Fault 3: the layout is not a constant ─────────────────────────────

    #[test]
    fn the_board_is_square_and_inside_the_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.001,
                "board is {}x{} at {w}x{h}",
                l.board.w,
                l.board.h
            );
            assert!(
                l.board.x >= 0.0 && l.board.y >= 0.0,
                "board off-window at {w}x{h}"
            );
            assert!(
                l.board.right() <= w + 0.001 && l.board.bottom() <= h + 0.001,
                "board runs off {w}x{h}: {:?}",
                l.board
            );
        }
    }

    #[test]
    fn nothing_is_drawn_outside_the_window() {
        for &(w, h) in WINDOWS {
            for show_help in [false, true] {
                let mut g = windowed(w, h);
                g.apply(Action::Play(0));
                settle(&mut g);
                if show_help {
                    g.apply(Action::ToggleHelp);
                }
                for cmd in g.frame(w, h).commands() {
                    if let RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } = cmd
                    {
                        assert!(
                            *x >= -0.001
                                && *y >= -0.001
                                && x + width <= w + 0.001
                                && y + height <= h + 0.001,
                            "rect {x},{y} {width}x{height} escapes {w}x{h}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_squares_tile_the_board_without_gaps_or_overlap() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let size = l.board.w / SIDE as f32;
            for index in 0..CELLS {
                let r = l.cell(index);
                assert!((r.w - size).abs() < 0.001 && (r.h - size).abs() < 0.001);
                assert!(r.x >= l.board.x - 0.001 && r.right() <= l.board.right() + 0.001);
                assert!(r.y >= l.board.y - 0.001 && r.bottom() <= l.board.bottom() + 0.001);
            }
            for row in 0..SIDE {
                for col in 1..SIDE {
                    let left = l.cell(row * SIDE + col - 1);
                    let right = l.cell(row * SIDE + col);
                    assert!((left.right() - right.x).abs() < 0.001, "gap at {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_board_keeps_its_share_of_the_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let want = h.min(w) * BOARD_SHARE;
            assert!(
                l.board.h >= want - SIDE as f32 - 2.0 * l.pad,
                "board {} too small in {w}x{h} (wanted about {want})",
                l.board.h
            );
        }
    }

    #[test]
    fn chrome_is_dropped_whole_rather_than_squashed() {
        // A 90px-tall window has no room for three bands. Whichever survive
        // must still be tall enough to read; none may be a two-pixel sliver.
        let l = Layout::new(120.0, 90.0);
        for band in [l.header, l.info, l.footer] {
            assert!(band.h == 0.0 || band.h >= 10.0, "sliver band {band:?}");
        }
        assert!(l.shows_info(), "the status line is the last thing to go");
    }

    #[test]
    fn a_tiny_window_still_draws_a_playable_board() {
        let mut g = windowed(120.0, 90.0);
        let l = g.layout(120.0, 90.0);
        assert!(l.board.w > 0.0);
        // "Playable" is two claims, and a hit box only carries one of them
        // (lesson 81): the box is the whole cell including its gap, while the
        // square you can see is that box inset and pushed by a different
        // statement. So ask separately that every square was *painted* — by a
        // fill lying inside its own box, which is containment the way round
        // the window's full-bleed background cannot satisfy (lesson 83) — and
        // that the centre one takes a click.
        let f = g.frame(120.0, 90.0);
        let fills: Vec<Rect> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect();
        for i in 0..9 {
            let box_ = f
                .rect_of(|t| *t == Target::Cell(i))
                .unwrap_or_else(|| panic!("square {i} is not clickable at 120x90"));
            assert!(
                fills.iter().any(|r| {
                    r.w > 0.0
                        && r.h > 0.0
                        && r.x >= box_.x - 0.01
                        && r.y >= box_.y - 0.01
                        && r.right() <= box_.right() + 0.01
                        && r.bottom() <= box_.bottom() + 0.01
                }),
                "square {i} is clickable at 120x90 but nothing was drawn in it"
            );
        }
        probe::click_sized(&mut g, Target::Cell(4), MouseButton::Left, (120.0, 90.0));
        assert_eq!(g.cell(4), Some(Mark::X));
    }

    #[test]
    fn the_grid_follows_the_window_when_it_is_resized() {
        // The old geometry was hard-coded: a resize moved nothing.
        let mut g = windowed(800.0, 600.0);
        let before = probe::rect_of_sized(&g, Target::Cell(0), (800.0, 600.0)).unwrap();
        handle_event(
            &mut g,
            &Event::Resize {
                width: 1280,
                height: 720,
            },
        );
        let after = probe::rect_of_sized(&g, Target::Cell(0), (1280.0, 720.0)).unwrap();
        assert_ne!(before, after, "the grid must move with the window");
        assert!(after.w > before.w, "a bigger window gets bigger squares");
        assert!((g.width() - 1280.0).abs() < 0.001);
        assert!((g.height() - 720.0).abs() < 0.001);
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        // The bug this replaces: the click handler used a cached width that
        // could disagree with the frame on screen.
        let mut g = windowed(800.0, 600.0);
        handle_event(
            &mut g,
            &Event::Resize {
                width: 1280,
                height: 720,
            },
        );
        let r = probe::rect_of_sized(&g, Target::Cell(8), (1280.0, 720.0)).unwrap();
        poke(&mut g, r.x + r.w / 2.0, r.y + r.h / 2.0, (1280.0, 720.0));
        assert_eq!(g.cell(8), Some(Mark::X));
    }

    #[test]
    fn the_mark_shrinks_with_the_square() {
        let mut small = windowed(320.0, 240.0);
        let mut big = windowed(1920.0, 1080.0);
        small.apply(Action::Play(0));
        big.apply(Action::Play(0));
        let size_of = |g: &TicTacToe, w: f32, h: f32| {
            g.frame(w, h)
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Text {
                        text, font_size, ..
                    } if text == "X" => Some(*font_size),
                    _ => None,
                })
                .fold(0.0_f32, f32::max)
        };
        let a = size_of(&small, 320.0, 240.0);
        let b = size_of(&big, 1920.0, 1080.0);
        assert!(a > 0.0 && b > a, "mark did not scale: {a} vs {b}");
    }

    // ── Fault 4: the reply is a state, not a side effect ──────────────────

    #[test]
    fn the_computer_does_not_reply_in_the_same_breath() {
        let mut g = game();
        g.apply(Action::Play(4));
        assert_eq!(filled(&g), 1, "only the human's mark so far");
        assert!(g.thinking(), "the computer owes a reply");
        assert_eq!(g.turn(), Mark::O);
    }

    #[test]
    fn thinking_is_a_sentence_some_frame_actually_shows() {
        // In the old code this branch was unreachable: the search ran inside
        // the human's own handler, so the turn had flipped back before any
        // frame was drawn.
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        g.apply(Action::Play(4));
        let shown = texts(&g, WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            shown.iter().any(|t| t.contains("thinking")),
            "no frame said the computer was thinking: {shown:?}"
        );
    }

    #[test]
    fn the_reply_lands_when_the_pause_runs_out() {
        let mut g = game();
        g.apply(Action::Play(4));
        let mut spent = 0_u64;
        while g.thinking() {
            handle_event(&mut g, &Event::Tick { elapsed_ms: 16 });
            spent = spent.saturating_add(16);
            assert!(spent <= THINK_MS + 32, "the pause never ended");
        }
        assert!(spent >= THINK_MS, "the pause was cut short at {spent}ms");
        assert_eq!(filled(&g), 2, "the computer should have replied");
        assert_eq!(g.turn(), Mark::X);
    }

    #[test]
    fn the_pause_is_the_same_length_whatever_the_tick_rate() {
        // Ageing by the reported interval, not by counting ticks.
        let mut slow = game();
        let mut fast = game();
        slow.apply(Action::Play(0));
        fast.apply(Action::Play(0));
        handle_event(&mut slow, &Event::Tick { elapsed_ms: 100 });
        for _ in 0..10 {
            handle_event(&mut fast, &Event::Tick { elapsed_ms: 10 });
        }
        assert_eq!(slow.think_ms(), fast.think_ms());
    }

    #[test]
    fn ticks_are_asked_for_only_while_the_reply_is_owed() {
        let mut g = game();
        assert_eq!(g.tick_interval(), None, "an idle board needs no frames");
        g.apply(Action::Play(4));
        assert_eq!(g.tick_interval(), Some(Duration::from_millis(TICK_MS)));
        settle(&mut g);
        assert_eq!(g.tick_interval(), None);
    }

    #[test]
    fn a_tick_with_nothing_owed_is_ignored() {
        let mut g = game();
        assert_eq!(
            handle_event(&mut g, &Event::Tick { elapsed_ms: 16 }),
            EventResult::Ignored
        );
    }

    #[test]
    fn the_human_cannot_move_while_the_computer_is_thinking() {
        let mut g = game();
        g.apply(Action::Play(4));
        assert!(g.thinking());
        assert!(
            !g.apply(Action::Play(0)),
            "a move during the pause is refused"
        );
        assert!(g.cell(0).is_none());
    }

    #[test]
    fn a_new_game_started_mid_pause_is_never_played_into() {
        // The move is chosen when the pause ends, not when it starts, so a
        // board that is replaced meanwhile cannot receive a stale reply.
        let mut g = game();
        g.apply(Action::Play(4));
        assert!(g.thinking());
        g.apply(Action::NewGame);
        assert!(!g.thinking());
        settle(&mut g);
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn the_computer_opens_when_the_human_takes_o() {
        let mut g = game();
        g.apply(Action::SwapSides);
        assert_eq!(g.human(), Mark::O);
        assert!(g.thinking(), "X moves first and X is now the computer");
        assert_eq!(filled(&g), 0, "not until the pause runs out");
        settle(&mut g);
        assert_eq!(filled(&g), 1);
        let opened = g.cells().iter().position(Option::is_some).unwrap();
        assert_eq!(g.cell(opened), Some(Mark::X));
        assert!(g.human_turn());
    }

    // ── Fault 5: drawing does not mutate the game ─────────────────────────

    #[test]
    fn drawing_a_frame_needs_no_mutable_game() {
        // `frame` takes `&self`. This is the compile-time half of the claim;
        // the run-time half is that two draws of the same game agree.
        let g = windowed(800.0, 600.0);
        let first = texts(&g, 800.0, 600.0);
        let second = texts(&g, 800.0, 600.0);
        assert_eq!(first, second);
    }

    #[test]
    fn hit_testing_does_not_disturb_the_game() {
        let g = windowed(800.0, 600.0);
        let before = texts(&g, 800.0, 600.0);
        for i in 0..40 {
            let _ = g.target_at(i as f32 * 20.0, i as f32 * 15.0);
        }
        assert_eq!(texts(&g, 800.0, 600.0), before);
    }

    // ── The rules ─────────────────────────────────────────────────────────

    /// A board written out as nine characters: `X`, `O`, or anything else for
    /// an empty square.
    fn board_from(spec: &str) -> Cells {
        let mut cells: Cells = [None; CELLS];
        for (i, ch) in spec.chars().filter(|c| !c.is_whitespace()).enumerate() {
            if let Some(slot) = cells.get_mut(i) {
                *slot = match ch {
                    'X' => Some(Mark::X),
                    'O' => Some(Mark::O),
                    _ => None,
                };
            }
        }
        cells
    }

    #[test]
    fn every_line_of_three_wins() {
        // All eight, spelled out independently of the `LINES` table the code
        // uses, so a typo in that table is caught rather than agreed with.
        let wins = [
            "XXX......",
            "...XXX...",
            "......XXX",
            "X..X..X..",
            ".X..X..X.",
            "..X..X..X",
            "X...X...X",
            "..X.X.X..",
        ];
        for spec in wins {
            let cells = board_from(spec);
            let (mark, line) = winner_of(&cells).unwrap_or_else(|| panic!("no win in {spec}"));
            assert_eq!(mark, Mark::X);
            for i in line {
                assert_eq!(cells.get(i).copied().flatten(), Some(Mark::X));
            }
        }
        assert_eq!(winner_of(&board_from("XX.OO....")), None);
        assert_eq!(winner_of(&board_from(".........")), None);
        assert!(is_full(&board_from("XOXXOOOXX")));
        assert!(!is_full(&board_from("XOXXOOOX.")));
    }

    #[test]
    fn two_perfect_players_draw() {
        // Played out through the app, with the human side taking the same
        // search the computer uses: a drawn game, every time, whichever mark
        // the human holds.
        for swap in [false, true] {
            let mut g = game();
            if swap {
                g.apply(Action::SwapSides);
            }
            settle(&mut g);
            while g.playing() {
                let mine = g.human();
                let square = best_move(g.cells(), mine).expect("a live board has a move");
                assert!(g.apply(Action::Play(square)));
                settle(&mut g);
            }
            assert_eq!(g.state(), GameState::Draw, "perfect play is a draw");
            assert_eq!(g.scores(), [0, 0, 1]);
            assert_eq!(g.status(), "Draw");
            assert_eq!(filled(&g), CELLS);
        }
    }

    #[test]
    fn a_taken_square_cannot_be_played_again() {
        let mut g = game();
        g.apply(Action::Play(4));
        settle(&mut g);
        let before = *g.cells();
        // The move is refused, but the cursor still moves to the square you
        // named — a refused click should show you what you asked for.
        g.apply(Action::Play(4));
        assert_eq!(*g.cells(), before, "square 4 was overwritten");
        assert_eq!(g.cursor(), 4);
        // With the cursor already there, nothing at all changes.
        assert!(!g.apply(Action::Play(4)), "square 4 is taken");
        assert_eq!(*g.cells(), before);
    }

    #[test]
    fn a_click_on_a_taken_square_moves_the_cursor_and_nothing_else() {
        let mut g = game();
        g.apply(Action::Play(0));
        settle(&mut g);
        let before = *g.cells();
        click(&mut g, Target::Cell(0));
        assert_eq!(*g.cells(), before, "no mark changed");
        assert_eq!(g.cursor(), 0, "but the cursor shows where you asked");
    }

    #[test]
    fn a_square_out_of_range_is_refused() {
        let mut g = game();
        assert!(!g.apply(Action::Play(CELLS)));
        assert!(!g.apply(Action::Select(CELLS)));
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn a_finished_board_takes_no_more_moves() {
        let mut g = game();
        play_to_the_end(&mut g);
        let before = *g.cells();
        for i in 0..CELLS {
            g.apply(Action::Play(i));
        }
        assert_eq!(*g.cells(), before);
    }

    #[test]
    fn winning_records_the_line_and_the_score() {
        let mut g = game();
        g.force(0, Mark::X);
        g.force(3, Mark::O);
        g.force(1, Mark::X);
        g.force(4, Mark::O);
        g.force(2, Mark::X);
        assert_eq!(g.state(), GameState::Won(Mark::X, [0, 1, 2]));
        assert_eq!(g.win_line(), Some([0, 1, 2]));
        assert_eq!(g.scores(), [1, 0, 0], "the human plays X by default");
        assert_eq!(g.status(), "You win!");
    }

    #[test]
    fn the_score_follows_the_human_not_the_mark() {
        // After swapping sides, an X win belongs to the computer.
        let mut g = game();
        g.apply(Action::SwapSides);
        g.think_clear();
        assert_eq!(g.human(), Mark::O);
        g.force(0, Mark::X);
        g.force(3, Mark::O);
        g.force(1, Mark::X);
        g.force(4, Mark::O);
        g.force(2, Mark::X);
        assert_eq!(g.scores(), [0, 1, 0], "X is the computer now");
        assert_eq!(g.status(), "Computer wins");
    }

    #[test]
    fn a_full_board_with_no_line_is_a_draw() {
        let mut g = game();
        for (i, mark) in [
            (0, Mark::X),
            (1, Mark::O),
            (2, Mark::X),
            (4, Mark::O),
            (3, Mark::X),
            (5, Mark::O),
            (7, Mark::X),
            (6, Mark::O),
            (8, Mark::X),
        ] {
            assert!(g.playing(), "the game ended early at square {i}");
            g.force(i, mark);
        }
        assert_eq!(g.state(), GameState::Draw);
        assert_eq!(g.win_line(), None);
        assert_eq!(g.scores(), [0, 0, 1]);
    }

    #[test]
    fn the_score_survives_a_new_game() {
        let mut g = game();
        play_to_the_end(&mut g);
        let scores = g.scores();
        assert_ne!(scores, [0, 0, 0], "somebody must have won or drawn");
        g.apply(Action::NewGame);
        assert_eq!(g.scores(), scores);
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn swapping_sides_starts_a_fresh_board() {
        // Swapping mid-game would hand one side a position the other built.
        let mut g = game();
        g.apply(Action::Play(0));
        settle(&mut g);
        assert_eq!(filled(&g), 2);
        g.apply(Action::SwapSides);
        assert_eq!(filled(&g), 0);
        assert_eq!(g.human(), Mark::O);
    }

    #[test]
    fn x_always_moves_first() {
        for swaps in 0..4 {
            let mut g = game();
            for _ in 0..swaps {
                g.apply(Action::SwapSides);
            }
            assert_eq!(g.turn(), Mark::X, "after {swaps} swaps");
        }
    }

    #[test]
    fn new_game_is_offered_only_when_it_would_do_something() {
        let mut g = game();
        assert!(
            !g.enabled(Action::NewGame),
            "an untouched board is already new"
        );
        g.apply(Action::Play(4));
        assert!(g.enabled(Action::NewGame));
    }

    #[test]
    fn the_cursor_never_leaves_the_board() {
        let mut g = game();
        for key in [Key::Up, Key::Down, Key::Left, Key::Right] {
            for _ in 0..6 {
                tap(&mut g, key);
                assert!(g.cursor() < CELLS, "cursor escaped via {key:?}");
            }
        }
    }

    #[test]
    fn a_click_on_a_finished_board_starts_the_next_game() {
        // What every player tries first.
        let mut g = game();
        play_to_the_end(&mut g);
        assert!(!g.playing());
        click(&mut g, Target::Cell(0));
        assert!(g.playing());
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn the_header_buttons_do_what_they_say() {
        let mut g = game();
        click(&mut g, Target::Help);
        assert!(g.show_help());
        click(&mut g, Target::HelpSheet);
        assert!(!g.show_help(), "a click on the sheet dismisses it");
        click(&mut g, Target::SwapSides);
        assert_eq!(g.human(), Mark::O);
        settle(&mut g);
        assert_eq!(filled(&g), 1);
        click(&mut g, Target::NewGame);
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn the_side_button_names_the_mark_you_would_move_to() {
        // A button reading "X" while you are already X invites a click that
        // appears to do nothing.
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(texts(&g, WINDOW_WIDTH, WINDOW_HEIGHT).contains(&"Play O".to_string()));
        g.apply(Action::SwapSides);
        assert!(texts(&g, WINDOW_WIDTH, WINDOW_HEIGHT).contains(&"Play X".to_string()));
    }

    #[test]
    fn the_help_sheet_lists_every_key_the_window_answers() {
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        g.apply(Action::ToggleHelp);
        let shown = texts(&g, WINDOW_WIDTH, WINDOW_HEIGHT);
        for (k, v) in HELP_ROWS {
            assert!(shown.iter().any(|t| t == k), "help omits {k}");
            assert!(shown.iter().any(|t| t == v), "help omits {v}");
        }
    }

    #[test]
    fn keys_with_a_modifier_belong_to_the_window_manager() {
        let mut g = game();
        probe::key(&mut g, &probe::ctrl(Key::N));
        assert_eq!(filled(&g), 0);
        probe::key(&mut g, &probe::ctrl(Key::Enter));
        assert_eq!(filled(&g), 0);
    }

    #[test]
    fn the_sheet_swallows_the_keys_it_does_not_answer() {
        let mut g = game();
        g.apply(Action::ToggleHelp);
        assert_eq!(
            probe::key(&mut g, &probe::press(Key::N)),
            EventResult::Consumed
        );
        assert!(g.show_help(), "N does not reach the game through the sheet");
        assert_eq!(filled(&g), 0);
        tap(&mut g, Key::Escape);
        assert!(!g.show_help());
    }

    #[test]
    fn the_score_panels_show_the_running_score() {
        let mut g = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        play_to_the_end(&mut g);
        let shown = texts(&g, WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(shown.iter().any(|t| t == "You (X)"));
        assert!(shown.iter().any(|t| t == "Computer (O)"));
        assert!(shown.iter().any(|t| t == "Draws"));
        let total: u32 = g.scores().iter().sum();
        assert_eq!(total, 1, "one finished game");
    }

    // ── The opponent, against a solver that does not share its code ───────

    /// The value of `cells` to the side `to_move`, under perfect play by both:
    /// 1 for a win, 0 for a draw, -1 for a loss.
    ///
    /// Deliberately *not* the depth-aware score the app searches with — this
    /// is a plain three-valued negamax written from the rules, so agreeing
    /// with it is evidence rather than a restatement.
    fn solve(
        cells: &Cells,
        to_move: Mark,
        memo: &mut std::collections::HashMap<(Cells, Mark), i8>,
    ) -> i8 {
        if winner_of(cells).is_some() {
            // Whoever just moved made the line, so the side to move has lost.
            return -1;
        }
        let free = empty_cells(cells);
        if free.is_empty() {
            return 0;
        }
        let key = (*cells, to_move);
        if let Some(&known) = memo.get(&key) {
            return known;
        }
        let mut best = -1_i8;
        for i in free {
            let mut next = *cells;
            if let Some(slot) = next.get_mut(i) {
                *slot = Some(to_move);
            }
            // A move good for the mover is bad for the other side, so the
            // score flips with the turn.
            best = best.max(-solve(&next, to_move.other(), memo));
        }
        memo.insert(key, best);
        best
    }

    /// Every position reachable by legal play from the empty board, paired
    /// with whose turn it is.
    fn all_positions() -> Vec<(Cells, Mark)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut stack = vec![([None; CELLS], Mark::X)];
        while let Some((cells, turn)) = stack.pop() {
            if !seen.insert((cells, turn)) {
                continue;
            }
            out.push((cells, turn));
            if winner_of(&cells).is_some() {
                continue;
            }
            for i in empty_cells(&cells) {
                let mut next = cells;
                if let Some(slot) = next.get_mut(i) {
                    *slot = Some(turn);
                }
                stack.push((next, turn.other()));
            }
        }
        out
    }

    #[test]
    fn the_search_never_gives_away_a_position_it_could_hold() {
        // The claim this file's own doc comment makes — "cannot be beaten" —
        // over *every* position the game can reach, for both marks, checked
        // against a solver written independently of the code being tested.
        let mut memo = std::collections::HashMap::new();
        let positions = all_positions();
        assert!(
            positions.len() > 5000,
            "only {} positions — the walk is wrong",
            positions.len()
        );
        let mut checked = 0_usize;
        for (cells, turn) in positions {
            if winner_of(&cells).is_some() || empty_cells(&cells).is_empty() {
                continue;
            }
            let before = solve(&cells, turn, &mut memo);
            let chosen = best_move(&cells, turn).expect("a live position has a move");
            assert_eq!(cells.get(chosen).copied().flatten(), None, "square taken");
            let mut after = cells;
            if let Some(slot) = after.get_mut(chosen) {
                *slot = Some(turn);
            }
            let value = -solve(&after, turn.other(), &mut memo);
            assert_eq!(
                value, before,
                "the search threw away a {before} position by playing {chosen}: {cells:?}"
            );
            checked = checked.saturating_add(1);
        }
        assert!(checked > 4000, "only {checked} positions had a move");
    }

    #[test]
    fn the_computer_takes_a_win_it_has_rather_than_blocking() {
        let cells = board_from("OO.XX....");
        assert_eq!(best_move(&cells, Mark::O), Some(2), "take the win");
    }

    #[test]
    fn the_computer_blocks_a_win_it_cannot_beat_to() {
        let cells = board_from("XX...O...");
        assert_eq!(best_move(&cells, Mark::O), Some(2));
    }

    #[test]
    fn the_computer_prefers_the_win_it_can_take_now() {
        // O wins at once by taking 8, and also wins in three by taking 1 —
        // that fork cannot be blocked. Without depth-aware scoring both score
        // the same and the lower index is chosen, so the program stares past a
        // win it already has. This is the position that catches it.
        let cells = board_from("X.....OO.");
        assert_eq!(best_move(&cells, Mark::O), Some(8));
    }

    #[test]
    fn a_finished_board_has_no_best_move() {
        assert_eq!(best_move(&board_from("XXXOO...."), Mark::O), None);
        assert_eq!(best_move(&board_from("XOXXOOOXX"), Mark::X), None);
    }

    #[test]
    fn the_human_can_never_win_however_they_play() {
        // The whole tree of human choices against the app itself, driven
        // through `apply` and the reply clock rather than through the solver —
        // so a fault in the wiring between them is caught as well as one in
        // the search.
        fn explore(g: &TicTacToe, depth: usize, seen: &mut usize) {
            assert!(depth <= CELLS, "the game ran longer than the board");
            if !g.playing() {
                if let GameState::Won(mark, _) = g.state() {
                    assert_ne!(mark, g.human(), "the human won: {:?}", g.cells());
                }
                *seen = seen.saturating_add(1);
                return;
            }
            assert!(g.human_turn(), "it should be the human's move to make");
            for i in empty_cells(g.cells()) {
                let mut branch = g.clone();
                assert!(branch.apply(Action::Play(i)));
                settle(&mut branch);
                explore(&branch, depth.saturating_add(1), seen);
            }
        }
        for swap in [false, true] {
            let mut g = game();
            if swap {
                g.apply(Action::SwapSides);
            }
            settle(&mut g);
            let mut seen = 0;
            explore(&g, 0, &mut seen);
            assert!(seen > 40, "only {seen} games explored (swap={swap})");
        }
    }
}
