//! 2048 — slide the tiles together until two of them make 2048.
//!
//! A 4x4 board. Every move slides every tile as far as it will go in one
//! direction; two touching tiles of the same value merge into one of twice
//! the value, and the merged value is added to the score. A new 2 (or, one
//! time in ten, a 4) appears in a free cell after every move that changed
//! something. Reach 2048 and you have won, and may keep going. Fill the board
//! with no two neighbours equal and you have lost.
//!
//! ## What wiring this up found
//!
//! The game logic underneath was sound — the slide, the merge, the scoring
//! and the end conditions all did what they claimed. Everything around it was
//! not:
//!
//! 1. **`main` was `let _app = Game2048App::new();`.** It built a board, dealt
//!    two tiles onto it, dropped the whole thing and exited. No window was
//!    ever opened, nothing was ever drawn, and no key or click ever arrived.
//! 2. **The layout was a picture of a window rather than a window.** The board
//!    lived at a constant `(40, 140)` with a constant 100-pixel cell, and
//!    `render(width, height)` used its two arguments for the background
//!    rectangle and nothing else. The board therefore always ran from x=40 to
//!    x=528 and y=140 to y=628 whatever window it was in: a window narrower
//!    than 530 cut the board in half, one shorter than 630 cut the bottom off,
//!    and a large one left the game in the top-left corner. `Layout` is
//!    derived from the live window size on every frame now and stored on
//!    nothing.
//! 3. **Nothing at all was clickable.** `handle_event` matched `Event::Key`
//!    and dropped everything else on the floor; there was no mouse code
//!    anywhere in the file. A pointer could not move a tile, start a game,
//!    undo, or open the help — which for a game whose whole interface is
//!    "push the pile that way" is not a limitation but an absence. There is a
//!    direction pad now, and a footer, and every control is a hit box
//!    recorded by the pass that paints it.
//! 4. **`Highest:` was a high-water mark over the session, not the highest
//!    tile on the board.** `update_highest` compared each cell against the
//!    stored value and kept the larger — it could only ever rise. It was
//!    never reset, and `UndoEntry` did not carry it, so undoing the merge
//!    that made a 512 left `Highest: 512` printed over a board whose largest
//!    tile was 256. It is computed from the grid now, so there is no second
//!    copy of it left to disagree.
//! 5. **An undo could not undo a win.** `UndoEntry` held the grid and the
//!    score and nothing else; `undo` put `Lost` back to `Playing` and left
//!    `Won` alone. Since `make_move` refuses to move while the status is
//!    `Won`, undoing the winning move left "You Win!" printed over a board
//!    with no 2048 anywhere on it and every direction refused — pressing `C`
//!    to continue was the only way out of a state the player had just asked
//!    to leave. The entry carries the status now, along with the move count,
//!    which had been patched up with a `saturating_sub(1)` that was only ever
//!    right by coincidence.
//! 6. **Winning on a dead board left a game that could never end.**
//!    `make_move` returned the moment it saw 2048, skipping the "can anything
//!    still move?" check, and `continue_after_win` did not check either. Reach
//!    2048 on the move that fills the last cell, choose to keep going, and the
//!    game sits at `WonContinuing` for ever: every direction refused, no
//!    "Game Over" ever shown, and nothing to say why. Continuing re-asks the
//!    question now.
//! 7. **The help sheet was drawn on top of the board and off the bottom of the
//!    window.** `render_help` put it at `BOARD_Y + 480` — y=620, where the
//!    board ends at y=628 — 420 wide and 200 tall, so it covered the last row
//!    of tiles and ran to y=820, well past the bottom of the 700-pixel window
//!    the rest of the file was drawn for. Its `_width` and `_height`
//!    parameters were ignored, which the underscores admit in writing.
//! 8. **Two pieces of text were centred by guessing.** The win and loss
//!    banners offset their titles by a hard-coded `-80.0`, which centres a
//!    string of one particular length: "You Win!" and "Game Over" are not the
//!    same width and both got the same offset. Tile numbers were centred with
//!    `text.len() as f32 * font_size * 0.3`, a guess at the width of a digit.
//!    Both measure the text now.
//! 9. **The crate did not pass the lane's clippy gate at all**, carrying
//!    `#![allow(dead_code)]` and nine more crate-wide allows. All ten are
//!    gone, and with them `spawn_tile_at` and `is_full`, which nothing called.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// ── Catppuccin Mocha, only the entries this program actually paints with ──
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);

/// The side of the board, in cells. 2048 is a 4x4 game; the constant is here
/// so the arithmetic reads as arithmetic rather than as a sprinkling of 4s.
const GRID_SIZE: usize = 4;

/// The tile that wins the game.
const WIN_TILE: u32 = 2048;

/// How many moves can be taken back.
///
/// Bounded because the history is kept in memory and a long game is thousands
/// of moves; the oldest entry falls off the end rather than the newest, so
/// what you can always undo is what you just did.
const MAX_UNDO: usize = 50;

/// One time in ten a new tile is a 4 rather than a 2. The original game's
/// number, and the reason a board fills faster than doubling alone explains.
const FOUR_IN: u32 = 10;

const WINDOW_WIDTH: f32 = 560.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// The seed used when the kernel has no entropy to give.
///
/// A per-crate constant rather than a shared one, so two programs that lose
/// entropy on the same boot do not then produce correlated streams. Refusing
/// to start would be the worse failure — a predictable 2048 board costs the
/// player a game, and no game costs them the program. The bytes spell
/// `2048GAME`.
const FALLBACK_SEED: u64 = 0x3230_3438_4741_4D45;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates. Unlike most of them its reduction was *not* the broken one:
// `(self.next() >> 33) as usize % max` discards the low 31 bits before taking
// the remainder, so it never read the counter-like low bits of a
// power-of-two-modulus LCG -- which matters here, because a 4x4 board makes
// `empty.len()` a power of two on the very first move. It was left with an
// ordinary modulo bias and nothing worse.
//
// It is replaced anyway. The copy is the defect: this one happened to be a
// good copy, and the only way to know that was to read all sixteen and check.
// `randrange::below` is Lemire's method with its rejection step, so the draw
// is exactly uniform rather than merely unbiased in its high bits.

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 8] = [
    ("Arrows / WASD", "Slide every tile that way"),
    ("Click < ^ v >", "The same, with a pointer"),
    ("U / Ctrl+Z", "Take back the last move"),
    ("N / R", "Start a new game"),
    ("C / Enter", "Keep playing after winning"),
    ("H / Esc", "Show or hide this sheet"),
    ("", ""),
    ("Two tiles alike", "merge into one of twice the value"),
];

/// The share of the window height the board is guaranteed, before any band is
/// allowed to take a pixel. Bands are dropped whole until the rest fit.
const BOARD_SHARE: f32 = 0.42;

/// Which band is given up first when they do not all fit: footer, info,
/// header, and the direction pad last of all.
///
/// The pad goes last because it is the only route a pointer has to the one
/// verb this game has. A window with room for exactly one band should keep
/// the band you can play with, not the one with the title on it. Bands are
/// dropped whole rather than shrunk together, because a band scaled down to
/// four pixels costs the board four pixels and shows nothing.
const BAND_DROP_ORDER: [usize; 4] = [3, 1, 0, 2];

// ── Direction ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// The four, in the order the direction pad paints them: the reading
    /// order of a keyboard's arrow cluster flattened into a row.
    pub const ALL: [Direction; 4] = [
        Direction::Left,
        Direction::Up,
        Direction::Down,
        Direction::Right,
    ];

    /// The glyph painted on this direction's button.
    ///
    /// ASCII, not the Unicode arrows: a font without them draws a box, and a
    /// box is the difference between a playable window and an unreadable one.
    pub fn glyph(self) -> &'static str {
        match self {
            Direction::Up => "^",
            Direction::Down => "v",
            Direction::Left => "<",
            Direction::Right => ">",
        }
    }
}

// ── The board ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    Playing,
    /// 2048 has been reached and the player has not yet said whether to stop.
    Won,
    /// The board is full and no two neighbours are equal.
    Lost,
    /// 2048 was reached and the player chose to keep going.
    WonContinuing,
}

#[derive(Debug, Clone)]
pub struct Board {
    grid: [[u32; GRID_SIZE]; GRID_SIZE],
    score: u32,
    best_score: u32,
    status: GameStatus,
    moves: u32,
}

impl Board {
    fn new() -> Self {
        Self {
            grid: [[0; GRID_SIZE]; GRID_SIZE],
            score: 0,
            best_score: 0,
            status: GameStatus::Playing,
            moves: 0,
        }
    }

    pub fn at(&self, row: usize, col: usize) -> u32 {
        self.grid
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(0)
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn best_score(&self) -> u32 {
        self.best_score
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// The largest tile **on the board**.
    ///
    /// Derived rather than stored. The field this replaced could only ever
    /// rise — it was compared against and never reset — so an undo left it
    /// naming a tile that was no longer anywhere on the board.
    pub fn highest_tile(&self) -> u32 {
        self.grid.iter().flatten().copied().max().unwrap_or(0)
    }

    fn empty_cells(&self) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        for (r, row) in self.grid.iter().enumerate() {
            for (c, &val) in row.iter().enumerate() {
                if val == 0 {
                    cells.push((r, c));
                }
            }
        }
        cells
    }

    fn spawn_tile(&mut self, rng: &mut SeededRng) {
        let empty = self.empty_cells();
        let Some(&(r, c)) = empty.get(rng.below(empty.len())) else {
            return;
        };
        let val = if rng.below(FOUR_IN as usize) == 0 {
            4
        } else {
            2
        };
        if let Some(cell) = self.grid.get_mut(r).and_then(|row| row.get_mut(c)) {
            *cell = val;
        }
    }

    /// Slide and merge one line towards index 0. Returns the new line, the
    /// points the merges earned, and whether anything moved.
    ///
    /// Every direction is this function: a column is a line, and a rightward
    /// slide is a reversed line slid leftwards and reversed back. There is one
    /// copy of the rule, so there is one place for it to be wrong.
    fn slide(line: &[u32; GRID_SIZE]) -> ([u32; GRID_SIZE], u32, bool) {
        let mut packed = [0u32; GRID_SIZE];
        let mut n = 0;
        for &val in line {
            if val != 0 {
                if let Some(slot) = packed.get_mut(n) {
                    *slot = val;
                }
                n = n.saturating_add(1);
            }
        }

        let mut out = [0u32; GRID_SIZE];
        let mut points = 0u32;
        let mut write = 0usize;
        let mut read = 0usize;
        while read < GRID_SIZE {
            let val = packed.get(read).copied().unwrap_or(0);
            if val == 0 {
                break;
            }
            // A tile merges with the one after it at most once per move, which
            // is why `read` advances by two: [2, 2, 2, 2] is [4, 4], never [8].
            let pair = packed.get(read.saturating_add(1)).copied().unwrap_or(0);
            let (value, step) = if pair == val {
                let merged = val.saturating_mul(2);
                points = points.saturating_add(merged);
                (merged, 2)
            } else {
                (val, 1)
            };
            if let Some(slot) = out.get_mut(write) {
                *slot = value;
            }
            write = write.saturating_add(1);
            read = read.saturating_add(step);
        }

        (out, points, out != *line)
    }

    /// One column of the board, top to bottom.
    fn column(&self, c: usize) -> [u32; GRID_SIZE] {
        let mut col = [0u32; GRID_SIZE];
        for (r, slot) in col.iter_mut().enumerate() {
            *slot = self.at(r, c);
        }
        col
    }

    fn set_column(&mut self, c: usize, col: [u32; GRID_SIZE]) {
        for (r, &val) in col.iter().enumerate() {
            if let Some(cell) = self.grid.get_mut(r).and_then(|row| row.get_mut(c)) {
                *cell = val;
            }
        }
    }

    /// Slide every tile one way. Returns whether the board changed.
    ///
    /// The score and the move count are only touched when something moved,
    /// because a move that changes nothing is not a move — pressing left
    /// against a wall costs neither a turn nor a spawned tile.
    pub fn apply_move(&mut self, dir: Direction) -> bool {
        let mut points = 0u32;
        let mut moved = false;
        let reversed = matches!(dir, Direction::Right | Direction::Down);
        let vertical = matches!(dir, Direction::Up | Direction::Down);

        for i in 0..GRID_SIZE {
            let mut line = if vertical {
                self.column(i)
            } else {
                self.grid.get(i).copied().unwrap_or([0; GRID_SIZE])
            };
            if reversed {
                line.reverse();
            }
            let (mut out, pts, line_moved) = Self::slide(&line);
            if reversed {
                out.reverse();
            }
            if vertical {
                self.set_column(i, out);
            } else if let Some(row) = self.grid.get_mut(i) {
                *row = out;
            }
            points = points.saturating_add(pts);
            moved |= line_moved;
        }

        if moved {
            self.score = self.score.saturating_add(points);
            self.best_score = self.best_score.max(self.score);
            self.moves = self.moves.saturating_add(1);
        }
        moved
    }

    /// Whether any direction would change the board.
    pub fn can_move(&self) -> bool {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let val = self.at(r, c);
                if val == 0 {
                    return true;
                }
                if c.saturating_add(1) < GRID_SIZE && self.at(r, c.saturating_add(1)) == val {
                    return true;
                }
                if r.saturating_add(1) < GRID_SIZE && self.at(r.saturating_add(1), c) == val {
                    return true;
                }
            }
        }
        false
    }

    pub fn has_won(&self) -> bool {
        self.grid.iter().flatten().any(|&v| v >= WIN_TILE)
    }
}

/// Everything one move changes, kept so the move can be taken back.
///
/// The whole of it, deliberately: the previous entry held the grid and the
/// score, and left the status and the move count to be patched up afterwards
/// by hand — which is how "undo the winning move" ended up leaving the game
/// in a won state over a board that had not won.
///
/// `best_score` is the one thing *not* restored, and that is not an
/// oversight: the best score is the best across every game in the session,
/// and taking a move back does not un-happen having once scored that much.
#[derive(Debug, Clone)]
struct UndoEntry {
    grid: [[u32; GRID_SIZE]; GRID_SIZE],
    score: u32,
    moves: u32,
    status: GameStatus,
}

impl UndoEntry {
    fn of(board: &Board) -> Self {
        Self {
            grid: board.grid,
            score: board.score,
            moves: board.moves,
            status: board.status,
        }
    }

    fn restore(self, board: &mut Board) {
        board.grid = self.grid;
        board.score = self.score;
        board.moves = self.moves;
        board.status = self.status;
    }
}

// ── What a click can land on ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Move(Direction),
    NewGame,
    Undo,
    Help,
    /// The "keep going" button on the banner shown after a win.
    Continue,
    /// The help sheet itself. It swallows clicks meant for what it covers,
    /// and closes — a pointer that opened the sheet has to be able to shut it
    /// even in a window too small to still be drawing the button it used.
    HelpSheet,
}

/// The one thing a key or a click ultimately asks for.
///
/// Both routes go through here, so "what does clicking Undo do" and "what
/// does pressing U do" cannot drift apart: they are the same line of code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Move(Direction),
    NewGame,
    Undo,
    Continue,
    ToggleHelp,
    CloseHelp,
}

// ── Layout ─────────────────────────────────────────────────────────────────

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in, which
/// is the class of fault this file was built out of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    pub board: Rect,
    /// The direction pad: four buttons, the only way a pointer can move.
    pub dpad: Rect,
    pub footer: Rect,
    pub help: Rect,
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

        // What each band would like, in [header, info, dpad, footer] order.
        let mut wants = [
            (h * 0.13).clamp(38.0, 96.0),
            (h * 0.045).clamp(14.0, 26.0),
            (h * 0.09).clamp(28.0, 62.0),
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
        let [hdr_h, inf_h, dpad_h, foot_h] = wants;

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
        let dpad = if dpad_h > 0.0 {
            Rect::new(0.0, h - foot_h - dpad_h, w, dpad_h)
        } else {
            Rect::EMPTY
        };

        // The board is square and centred in whatever the bands left behind.
        let top = hdr_h + inf_h;
        let bottom = h - foot_h - dpad_h;
        let side = (w - pad * 2.0)
            .max(0.0)
            .min((bottom - top - pad * 2.0).max(0.0));
        let board = Rect::new(
            (w - side) / 2.0,
            top + (bottom - top - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.92).min(420.0);
        let help_h = (h * 0.92).min(300.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            board,
            dpad,
            footer,
            help,
            font,
            small,
            pad,
        }
    }

    /// Whether a band survived the drop ladder and is worth drawing into.
    ///
    /// A band that did not fit is `Rect::EMPTY`, not a flat one — a zero-high
    /// rectangle at the right y would still take clicks aimed at its edge.
    pub fn shows(&self, band: Rect) -> bool {
        band.w > 0.0 && band.h > 0.0
    }

    /// The `index`th of `count` evenly-spaced buttons filling `row`.
    fn nth_of(row: Rect, count: usize, index: usize) -> Rect {
        if row.is_empty() || index >= count {
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

    /// The button for the `index`th of [`Direction::ALL`].
    pub fn dpad_button(&self, index: usize) -> Rect {
        Self::nth_of(self.dpad, Direction::ALL.len(), index)
    }

    /// The footer buttons: new game, undo, help.
    pub fn footer_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, 3, index)
    }

    /// One of the two readouts at the right of the header: 0 is the score, 1
    /// the best. Empty when the header did not survive, or when the header is
    /// too narrow to hold the title and both boxes.
    pub fn score_box(&self, index: usize) -> Rect {
        if !self.shows(self.header) || index >= 2 {
            return Rect::EMPTY;
        }
        let bw = (self.header.w * 0.22).clamp(48.0, 130.0);
        let bh = (self.header.h * 0.72).max(1.0);
        let gap = self.pad;
        let right = self.header.right() - self.pad;
        // Boxes are laid out from the right edge inwards, so the one nearest
        // the edge is index 0 and adding a third would not move the other two.
        let x = right - (bw + gap) * (index as f32 + 1.0) + gap;
        if x < self.header.x {
            return Rect::EMPTY;
        }
        Rect::new(x, self.header.y + (self.header.h - bh) / 2.0, bw, bh)
    }

    /// One cell of the board, gaps taken out of the cell rather than added to
    /// the step — so the last cell ends exactly at the board's edge.
    pub fn cell(&self, row: usize, col: usize) -> Rect {
        if self.board.is_empty() || row >= GRID_SIZE || col >= GRID_SIZE {
            return Rect::EMPTY;
        }
        let n = GRID_SIZE as f32;
        let step = self.board.w / n;
        let gap = (step * 0.09).min(12.0);
        Rect::new(
            self.board.x + col as f32 * step + gap / 2.0,
            self.board.y + row as f32 * step + gap / 2.0,
            (step - gap).max(0.0),
            (step - gap).max(0.0),
        )
    }

    /// The banner drawn over the board when the game is won or lost.
    pub fn banner(&self) -> Rect {
        if self.board.is_empty() {
            return Rect::EMPTY;
        }
        let bh = (self.board.h * 0.42).min(200.0);
        Rect::new(
            self.board.x,
            self.board.y + (self.board.h - bh) / 2.0,
            self.board.w,
            bh,
        )
    }

    /// The "keep going" button inside the win banner.
    pub fn banner_button(&self) -> Rect {
        let b = self.banner();
        if b.is_empty() {
            return Rect::EMPTY;
        }
        let bw = (b.w * 0.5).min(220.0);
        let bh = (b.h * 0.28).min(40.0);
        Rect::new(
            b.x + (b.w - bw) / 2.0,
            (b.bottom() - bh - b.h * 0.08).max(b.y),
            bw,
            bh,
        )
    }
}

// ── Drawing helpers ────────────────────────────────────────────────────────

pub type Frame = guitk::frame::Frame<Target>;

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
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    fill(f, r, face, (r.h * 0.22).min(8.0));
    // Recorded by the pass that paints it, so a button that moved took its
    // hit box with it and there is no second copy of the geometry to disagree.
    f.hit(target, r);
    centred(f, r, body, size, ink, FontWeightHint::Bold);
}

/// The face a tile of this value is painted with.
fn tile_face(value: u32) -> Color {
    match value {
        0 => COL_SURFACE0,
        2 => Color::from_hex(0xEEE4DA),
        4 => Color::from_hex(0xEDE0C8),
        8 => COL_PEACH,
        16 => Color::from_hex(0xF59563),
        32 => COL_RED,
        64 => Color::from_hex(0xF65E3B),
        128 => COL_YELLOW,
        256 => Color::from_hex(0xEDCC61),
        512 => COL_GREEN,
        1024 => COL_TEAL,
        2048 => COL_BLUE,
        4096 => COL_MAUVE,
        8192 => COL_LAVENDER,
        _ => COL_SURFACE2,
    }
}

/// The ink a tile's number is written in. The two palest faces need dark ink;
/// every other face is dark enough to take light ink.
fn tile_ink(value: u32) -> Color {
    match value {
        2 | 4 => COL_CRUST,
        _ => COL_TEXT,
    }
}

// ── The program ────────────────────────────────────────────────────────────

pub struct Game2048 {
    board: Board,
    rng: SeededRng,
    undo_stack: Vec<UndoEntry>,
    show_help: bool,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    width: f32,
    height: f32,
}

impl Game2048 {
    pub fn new() -> Self {
        // Was `with_seed(42)`: every player, on every machine, got the same
        // two opening tiles in the same two cells and the same spawns for the
        // rest of the game. Predicting a 2048 board costs the player the game.
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    pub fn with_rng(rng: SeededRng) -> Self {
        let mut app = Self {
            board: Board::new(),
            rng,
            undo_stack: Vec::new(),
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        app.deal();
        app
    }

    /// The two tiles every game opens with.
    fn deal(&mut self) {
        self.board.spawn_tile(&mut self.rng);
        self.board.spawn_tile(&mut self.rng);
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn help_is_open(&self) -> bool {
        self.show_help
    }

    pub fn new_game(&mut self) {
        let best = self.board.best_score;
        self.board = Board::new();
        self.board.best_score = best;
        self.undo_stack.clear();
        self.deal();
    }

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(entry) => {
                entry.restore(&mut self.board);
                true
            }
            None => false,
        }
    }

    /// Whether the player is allowed to slide right now.
    ///
    /// A won game is deliberately frozen until the player says whether to keep
    /// going, so that the board they won on is still on the screen while they
    /// decide.
    fn can_play(&self) -> bool {
        matches!(
            self.board.status,
            GameStatus::Playing | GameStatus::WonContinuing
        )
    }

    pub fn make_move(&mut self, dir: Direction) -> bool {
        if !self.can_play() {
            return false;
        }
        // The snapshot is taken before the move and kept only if the move
        // happened, rather than pushed and popped back off again. A move that
        // changes nothing leaves no trace of having been tried.
        let before = UndoEntry::of(&self.board);
        if !self.board.apply_move(dir) {
            return false;
        }
        self.push_undo(before);
        self.board.spawn_tile(&mut self.rng);
        if self.board.status == GameStatus::Playing && self.board.has_won() {
            self.board.status = GameStatus::Won;
        } else {
            self.check_stuck();
        }
        true
    }

    /// Called wherever the board might have become unplayable.
    fn check_stuck(&mut self) {
        if self.can_play() && !self.board.can_move() {
            self.board.status = GameStatus::Lost;
        }
    }

    pub fn continue_after_win(&mut self) {
        if self.board.status == GameStatus::Won {
            self.board.status = GameStatus::WonContinuing;
            // The winning move may also have been the one that filled the
            // board. Choosing to keep going has to re-ask whether there is
            // anything left to do, or the game sits at `WonContinuing` for
            // ever with every direction refused and nothing saying why.
            self.check_stuck();
        }
    }

    // ── Window ──

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// What a click at (`x`, `y`) would land on, read from the frame the
    /// window is actually showing.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }

    pub fn apply(&mut self, intent: Intent) -> EventResult {
        match intent {
            Intent::Move(dir) => {
                if self.make_move(dir) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
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
            Intent::Continue => {
                if self.board.status == GameStatus::Won {
                    self.continue_after_win();
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

    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // Only presses. A handler that ignores `pressed` runs everything
        // twice, once on the way down and once on the way up.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let Some(intent) = key_intent(ev) else {
            return EventResult::Ignored;
        };
        self.apply(intent)
    }

    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        self.apply(target_intent(target))
    }

    // ── Painting ──

    /// The whole window, drawn once.
    ///
    /// `Frame::hit_test` scans the recorded boxes in reverse, so anything
    /// drawn later wins the click over what it covers. That is why the banner
    /// comes after the board and the help sheet comes after everything.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);

        if l.shows(l.header) {
            self.draw_header(&mut f, &l);
        }
        if l.shows(l.info) {
            self.draw_info(&mut f, &l);
        }
        self.draw_board(&mut f, &l);
        match self.board.status {
            GameStatus::Won => self.draw_banner(&mut f, &l, "You win!", COL_GREEN, true),
            GameStatus::Lost => self.draw_banner(&mut f, &l, "Game over", COL_RED, false),
            GameStatus::Playing | GameStatus::WonContinuing => {}
        }
        if l.shows(l.dpad) {
            self.draw_dpad(&mut f, &l);
        }
        if l.shows(l.footer) {
            self.draw_footer(&mut f, &l);
        }
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        let score = l.score_box(0);
        let best = l.score_box(1);
        // The title gets whatever is left to the left of the boxes, and says
        // so with a max width rather than by running underneath them.
        let limit = if best.is_empty() {
            l.header.right()
        } else {
            best.x
        };
        let title = Rect::new(
            l.header.x + l.pad,
            l.header.y,
            (limit - l.pad * 2.0 - l.header.x).max(0.0),
            l.header.h,
        );
        let size = (l.font * 1.9).min(title.h * 0.8);
        label(
            f,
            title.x,
            title.y + (title.h - text::line_height(size, FontWeightHint::Bold)).max(0.0) / 2.0,
            "2048",
            size,
            COL_YELLOW,
            FontWeightHint::Bold,
            Some(title.w),
        );

        self.draw_score_box(f, l, best, "BEST", self.board.best_score);
        self.draw_score_box(f, l, score, "SCORE", self.board.score);
    }

    fn draw_score_box(&self, f: &mut Frame, l: &Layout, r: Rect, caption: &str, value: u32) {
        if r.is_empty() {
            return;
        }
        fill(f, r, COL_SURFACE0, (r.h * 0.2).min(8.0));
        let cap_h = (r.h * 0.4).max(0.0);
        centred(
            f,
            Rect::new(r.x, r.y, r.w, cap_h),
            caption,
            l.small.min(cap_h * 0.8),
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        centred(
            f,
            Rect::new(r.x, r.y + cap_h, r.w, r.h - cap_h),
            &value.to_string(),
            (l.font * 1.1).min((r.h - cap_h) * 0.8),
            COL_TEXT,
            FontWeightHint::Bold,
        );
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        let size = l.small.min(l.info.h * 0.8);
        label(
            f,
            l.info.x + l.pad,
            l.info.y + (l.info.h - text::line_height(size, FontWeightHint::Regular)).max(0.0) / 2.0,
            &format!(
                "Moves: {}   Highest: {}",
                self.board.moves,
                self.board.highest_tile()
            ),
            size,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
            Some((l.info.w - l.pad * 2.0).max(0.0)),
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        fill(f, l.board, COL_MANTLE, (l.board.w * 0.02).min(10.0));
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let r = l.cell(row, col);
                let val = self.board.at(row, col);
                fill(f, r, tile_face(val), (r.h * 0.12).min(8.0));
                if val == 0 {
                    continue;
                }
                let body = val.to_string();
                // The size is chosen so the number fits the cell it is in,
                // measured rather than guessed at from the digit count.
                let mut size = r.h * 0.44;
                let width = text::measure(&body, size, FontWeightHint::Bold);
                if width > r.w * 0.84 && width > 0.0 {
                    size *= r.w * 0.84 / width;
                }
                centred(f, r, &body, size, tile_ink(val), FontWeightHint::Bold);
            }
        }
    }

    fn draw_dpad(&self, f: &mut Frame, l: &Layout) {
        let playable = self.can_play();
        for (i, &dir) in Direction::ALL.iter().enumerate() {
            let r = l.dpad_button(i);
            button(
                f,
                r,
                Target::Move(dir),
                dir.glyph(),
                (r.h * 0.55).min(l.font * 1.6),
                if playable { COL_SURFACE1 } else { COL_SURFACE0 },
                if playable { COL_TEXT } else { COL_OVERLAY0 },
            );
        }
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        let entries = [
            (Target::NewGame, "New game", true),
            (Target::Undo, "Undo", !self.undo_stack.is_empty()),
            (Target::Help, "Help", true),
        ];
        for (i, &(target, body, live)) in entries.iter().enumerate() {
            let r = l.footer_button(i);
            button(
                f,
                r,
                target,
                body,
                (r.h * 0.42).min(l.font),
                if live { COL_SURFACE1 } else { COL_SURFACE0 },
                if live { COL_TEXT } else { COL_OVERLAY0 },
            );
        }
    }

    fn draw_banner(&self, f: &mut Frame, l: &Layout, title: &str, accent: Color, offer: bool) {
        let b = l.banner();
        if b.is_empty() {
            return;
        }
        fill(f, b, Color::rgba(30, 30, 46, 224), (b.h * 0.08).min(10.0));
        let head = Rect::new(b.x, b.y, b.w, b.h * 0.45);
        centred(
            f,
            head,
            title,
            (head.h * 0.62).min(l.font * 2.4),
            accent,
            FontWeightHint::Bold,
        );
        let sub = Rect::new(b.x, head.bottom(), b.w, b.h * 0.2);
        centred(
            f,
            sub,
            &format!("Score {}", self.board.score),
            (sub.h * 0.72).min(l.font),
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        if offer {
            let btn = l.banner_button();
            button(
                f,
                btn,
                Target::Continue,
                "Keep going",
                (btn.h * 0.5).min(l.font),
                COL_SURFACE1,
                COL_TEXT,
            );
        }
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        let h = l.help;
        if h.is_empty() {
            return;
        }
        fill(f, h, COL_SURFACE0, (h.h * 0.04).min(10.0));
        // The whole sheet takes the click, and closes. A pointer that opened
        // the sheet must be able to shut it even in a window too small to be
        // drawing the footer it used.
        f.hit(Target::HelpSheet, h);

        let size = l.small.min(h.h / 12.0);
        let head_h = (h.h * 0.16).max(0.0);
        centred(
            f,
            Rect::new(h.x, h.y, h.w, head_h),
            HELP_TITLE,
            (head_h * 0.6).min(l.font * 1.2),
            COL_LAVENDER,
            FontWeightHint::Bold,
        );

        let rows = HELP_ROWS.len() as f32;
        let body_h = (h.h - head_h - l.pad * 2.0).max(0.0);
        let step = body_h / rows;
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
            Rect::new(h.x, h.bottom() - step, h.w, step),
            "Click anywhere to close",
            (size * 0.9).max(7.0),
            COL_OVERLAY0,
            FontWeightHint::Regular,
        );
    }
}

impl Default for Game2048 {
    fn default() -> Self {
        Self::new()
    }
}

/// What a key asks for, if anything.
///
/// A free function rather than a method, so the mapping can be read and tested
/// without a game to read it against.
pub fn key_intent(ev: &KeyEvent) -> Option<Intent> {
    if ev.key == Key::Z && ev.modifiers.ctrl {
        return Some(Intent::Undo);
    }
    // Ctrl and Alt combinations belong to the window, not to the board: a
    // Ctrl+Left that slides the tiles is a Ctrl+Left the desktop cannot have.
    if ev.modifiers.ctrl || ev.modifiers.alt {
        return None;
    }
    match ev.key {
        Key::Up | Key::W => Some(Intent::Move(Direction::Up)),
        Key::Down | Key::S => Some(Intent::Move(Direction::Down)),
        Key::Left | Key::A => Some(Intent::Move(Direction::Left)),
        Key::Right | Key::D => Some(Intent::Move(Direction::Right)),
        Key::U => Some(Intent::Undo),
        Key::N | Key::R => Some(Intent::NewGame),
        Key::C | Key::Enter => Some(Intent::Continue),
        Key::H => Some(Intent::ToggleHelp),
        Key::Escape => Some(Intent::CloseHelp),
        _ => None,
    }
}

/// What clicking a control asks for.
pub fn target_intent(target: Target) -> Intent {
    match target {
        Target::Move(dir) => Intent::Move(dir),
        Target::NewGame => Intent::NewGame,
        Target::Undo => Intent::Undo,
        Target::Help => Intent::ToggleHelp,
        Target::Continue => Intent::Continue,
        Target::HelpSheet => Intent::CloseHelp,
    }
}

pub fn handle_event(app: &mut Game2048, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Game2048 {
    fn title(&self) -> String {
        "2048".to_string()
    }

    fn app_id(&self) -> String {
        "game2048".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
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
        // against — that is the whole point of storing it here.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Game2048 {
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
    let mut game = Game2048::new();
    app::launch("game2048", &mut game)
}
