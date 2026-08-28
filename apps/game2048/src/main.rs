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
    // A width of nothing is not a narrow label, it is no label: elided to fit
    // in no space at all it would be an empty string sitting in the frame,
    // a text command that paints nothing but still counts as text drawn.
    // The check lives here, once, rather than at each call site -- a guard
    // repeated in front of the guard is a guard no test can remove.
    if body.is_empty() || size <= 0.0 || max_width.is_some_and(|w| w <= 0.0) {
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
        // The left edge of whichever box is furthest left, or the right edge
        // of the header if neither box fitted. Reading it off `best` alone was
        // wrong for the one width where `best` is dropped and `score` is not:
        // the title would then have been given the whole header to run across,
        // and would have run underneath the box that was still there.
        let limit = [best, score]
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| r.x)
            .fold(l.header.right(), f32::min);
        let title = Rect::new(
            l.header.x + l.pad,
            l.header.y,
            (limit - l.pad * 2.0 - l.header.x).max(0.0),
            l.header.h,
        );
        // A title with no room left is not drawn at all -- `label` refuses a
        // maximum width of nothing, so this needs no guard of its own.
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
        // sheet. Dividing by the row count alone left that line sitting in the
        // last row's band, written across the last row and -- because it was
        // placed from the sheet's bottom edge rather than from the ladder --
        // hanging below the sheet, and in a short window below the window.
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

// ── Tests ──────────────────────────────────────────────────────────────────

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
    use guitk::probe;

    /// Windows to check the layout against, from a desktop down to something
    /// no sane person would resize to.
    ///
    /// `(600, 150)` earns its place: it is short enough that the bands do
    /// *not* all fit, which is the only condition under which `BOARD_SHARE`
    /// has any effect at all. Every taller window in this list has room for
    /// the whole of the chrome, so the guarantee the constant makes is
    /// vacuous there and a test that only looked at those could not see the
    /// constant change.
    const WINDOWS: [(f32, f32); 10] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (480.0, 400.0),
        (400.0, 640.0),
        (600.0, 150.0),
        (640.0, 200.0),
        (300.0, 110.0),
        (120.0, 90.0),
        (24.0, 24.0),
    ];

    // ── Fixtures ──

    /// A game on a pinned generator, so the tiles it deals are the same every
    /// run and a failure is reproducible.
    fn game() -> Game2048 {
        Game2048::with_rng(SeededRng::new(7))
    }

    fn windowed(width: f32, height: f32) -> Game2048 {
        let mut app = game();
        app.resize(width, height);
        app
    }

    /// A bare board with exactly these tiles on it and nothing else.
    fn bare(rows: [[u32; GRID_SIZE]; GRID_SIZE]) -> Board {
        let mut b = Board::new();
        b.grid = rows;
        b
    }

    /// A game showing exactly this board, with an empty history and a clean
    /// score, so that what a test then does to it is the only thing in it.
    fn playing(rows: [[u32; GRID_SIZE]; GRID_SIZE]) -> Game2048 {
        let mut app = game();
        app.board = bare(rows);
        app.undo_stack.clear();
        app
    }

    fn press(app: &mut Game2048, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn click(app: &mut Game2048, x: f32, y: f32) -> EventResult {
        let size = (app.width, app.height);
        app.click_at(x, y, MouseButton::Left, size)
    }

    /// Click the control the predicate picks out, at the app's current size.
    fn click_on(app: &mut Game2048, target: Target) -> EventResult {
        let size = (app.width, app.height);
        probe::click_sized(app, target, MouseButton::Left, size)
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
    /// string measures to: a label carries a maximum width and is elided to
    /// fit it, so measuring the body would report an overflow that is never
    /// painted -- and would make every one of these tests a test of the
    /// string rather than of the layout.
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

    /// The colour a cell was painted, read out of the frame rather than out of
    /// the function that chose it.
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

    // ── The slide, which is the whole game ──

    #[test]
    fn a_line_slides_its_tiles_against_the_wall_with_no_gaps_left_between_them() {
        let (out, points, moved) = Board::slide(&[0, 2, 0, 4]);
        assert_eq!(out, [2, 4, 0, 0], "the tiles did not close up");
        assert_eq!(points, 0, "sliding without merging scored");
        assert!(moved, "a line that closed up said it had not moved");
    }

    #[test]
    fn two_tiles_alike_merge_into_one_of_twice_the_value() {
        let (out, points, moved) = Board::slide(&[2, 2, 0, 0]);
        assert_eq!(out, [4, 0, 0, 0]);
        assert_eq!(points, 4, "the merge did not score the tile it made");
        assert!(moved);
    }

    #[test]
    fn a_tile_merges_at_most_once_in_a_move() {
        // Four alike are two pairs, never one tile of four times the value:
        // [2,2,2,2] is [4,4], and a solver that merged the result again would
        // give [8]. The scores differ too -- 8 against 12 -- so this leg
        // cannot pass by the board alone.
        let (out, points, _) = Board::slide(&[2, 2, 2, 2]);
        assert_eq!(out, [4, 4, 0, 0], "a merged tile merged again");
        assert_eq!(points, 8, "a merged tile was scored twice");
    }

    #[test]
    fn a_merge_takes_the_leading_pair_and_not_the_trailing_one() {
        // Three alike make one pair and a single, and *which* pair is the
        // whole question. [2,2,2] -> [4,2] if the leading pair merges and
        // [2,4] if the trailing one does; both are two tiles summing the same,
        // so only their order can tell the two rules apart.
        let (out, _, _) = Board::slide(&[2, 2, 2, 0]);
        assert_eq!(out, [4, 2, 0, 0], "the trailing pair merged");
    }

    #[test]
    fn a_line_with_nothing_to_do_says_it_did_nothing() {
        let (out, points, moved) = Board::slide(&[2, 4, 8, 16]);
        assert_eq!(out, [2, 4, 8, 16]);
        assert_eq!(points, 0);
        assert!(!moved, "a line that could not move claimed it had");
    }

    #[test]
    fn an_empty_line_is_left_alone() {
        let (out, points, moved) = Board::slide(&[0, 0, 0, 0]);
        assert_eq!(out, [0, 0, 0, 0]);
        assert_eq!(points, 0);
        assert!(!moved);
    }

    #[test]
    fn unlike_tiles_never_merge() {
        let (out, points, _) = Board::slide(&[2, 4, 0, 0]);
        assert_eq!(out, [2, 4, 0, 0]);
        assert_eq!(points, 0, "two different tiles scored a merge");
    }

    #[test]
    fn a_merge_scores_what_it_made_and_not_what_it_was_made_from() {
        // 8 + 8 makes 16 and scores 16, not 8 and not 32. Three numbers that
        // a plausible wrong rule would each give.
        let (_, points, _) = Board::slide(&[8, 8, 0, 0]);
        assert_eq!(points, 16);
    }

    #[test]
    fn a_move_slides_every_row_at_once() {
        let mut b = bare([[2, 0, 0, 2], [0, 4, 4, 0], [0, 0, 0, 8], [1, 0, 0, 0]]);
        assert!(b.apply_move(Direction::Left));
        assert_eq!(b.grid[0], [4, 0, 0, 0], "row 0");
        assert_eq!(b.grid[1], [8, 0, 0, 0], "row 1");
        assert_eq!(b.grid[2], [8, 0, 0, 0], "row 2");
        assert_eq!(b.grid[3], [1, 0, 0, 0], "row 3");
        assert_eq!(b.score, 12, "the score is every row's merges added up");
    }

    #[test]
    fn sliding_right_is_sliding_left_read_backwards() {
        let rows = [[2, 0, 0, 2], [0, 4, 4, 0], [2, 2, 2, 0], [1, 0, 0, 0]];
        let mut left = bare(rows);
        let mut right = bare(rows);
        left.apply_move(Direction::Left);
        right.apply_move(Direction::Right);
        for r in 0..GRID_SIZE {
            let mut mirrored = right.grid[r];
            mirrored.reverse();
            // Row 2 is the asymmetric one: [2,2,2,0] goes to [4,2,0,0] left
            // and [0,0,2,4] right, which are mirror images. A row that was
            // symmetric under this check would tell us nothing.
            assert_eq!(
                left.grid[r], mirrored,
                "row {r} does not mirror between left and right"
            );
        }
        assert_eq!(left.score, right.score, "the two directions scored apart");
    }

    #[test]
    fn sliding_up_moves_columns_and_not_rows() {
        let mut b = bare([[2, 0, 0, 0], [2, 0, 0, 0], [0, 4, 0, 0], [0, 4, 0, 0]]);
        assert!(b.apply_move(Direction::Up));
        assert_eq!(b.grid[0], [4, 8, 0, 0], "the top row after sliding up");
        assert_eq!(b.grid[1], [0, 0, 0, 0]);
        assert_eq!(b.score, 12);
    }

    #[test]
    fn sliding_down_stacks_against_the_bottom() {
        let mut b = bare([[2, 0, 0, 0], [2, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);
        assert!(b.apply_move(Direction::Down));
        assert_eq!(b.grid[3], [4, 0, 0, 0], "the bottom row after sliding down");
        assert_eq!(b.grid[0], [0, 0, 0, 0], "a tile was left at the top");
    }

    #[test]
    fn a_move_that_changes_nothing_costs_neither_a_turn_nor_a_point() {
        let mut b = bare([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        b.score = 99;
        b.moves = 5;
        assert!(!b.apply_move(Direction::Left), "a wall was slid into");
        assert_eq!(b.score, 99, "a move that did nothing scored");
        assert_eq!(b.moves, 5, "a move that did nothing was counted");
    }

    #[test]
    fn the_best_score_is_the_high_water_mark_of_the_score() {
        let mut b = bare([[2, 2, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);
        b.apply_move(Direction::Left);
        assert_eq!(b.score, 4);
        assert_eq!(b.best_score, 4, "the best did not follow the score up");
        b.score = 1;
        b.apply_move(Direction::Right);
        assert!(
            b.best_score >= 4,
            "the best followed the score back down: {}",
            b.best_score
        );
    }

    // ── Reading the board ──

    #[test]
    fn a_square_off_the_board_reads_as_empty_rather_than_wrapping_round() {
        let b = bare([
            [1, 2, 3, 4],
            [5, 6, 7, 8],
            [9, 10, 11, 12],
            [13, 14, 15, 16],
        ]);
        // Column four of row nought is row one's first cell if the index is
        // computed row-major and then bounds-checked as one number. It is not
        // a cell of this board at all, and reads as empty.
        assert_eq!(b.at(0, GRID_SIZE), 0, "a column past the last wrapped");
        assert_eq!(b.at(GRID_SIZE, 0), 0, "a row past the last wrapped");
        assert_eq!(b.at(0, 0), 1, "an ordinary square stopped reading");
    }

    #[test]
    fn the_highest_tile_is_read_off_the_board_rather_than_remembered() {
        let mut app = playing([[512, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]]);
        assert_eq!(app.board.highest_tile(), 512);
        // The board loses its biggest tile. A high-water mark kept in a field
        // would still say 512 -- which is exactly what the old field did after
        // an undo, and what the readout printed over the board.
        app.board.grid[0][0] = 256;
        assert_eq!(
            app.board.highest_tile(),
            256,
            "the highest tile outlived the tile"
        );
    }

    #[test]
    fn an_empty_board_has_no_highest_tile_rather_than_a_wrong_one() {
        assert_eq!(Board::new().highest_tile(), 0);
    }

    #[test]
    fn a_board_with_a_free_cell_can_always_move() {
        let b = bare([
            [2, 4, 8, 16],
            [32, 64, 128, 256],
            [512, 1024, 2, 4],
            [8, 16, 32, 0],
        ]);
        assert!(b.can_move(), "a board with a hole in it said it was stuck");
    }

    #[test]
    fn a_full_board_with_two_alike_side_by_side_can_still_move() {
        // Full, and the only pair is horizontal. A checker that scanned
        // columns alone would call this stuck.
        let b = bare([
            [2, 2, 8, 16],
            [32, 64, 128, 256],
            [512, 1024, 2, 4],
            [8, 16, 32, 64],
        ]);
        assert!(b.can_move(), "a horizontal pair was not seen");
    }

    #[test]
    fn a_full_board_with_two_alike_one_above_the_other_can_still_move() {
        // The same board with the only pair turned on its side, so the two
        // legs cannot both be explained by one scan.
        let b = bare([
            [2, 4, 8, 16],
            [2, 64, 128, 256],
            [512, 1024, 2, 4],
            [8, 16, 32, 64],
        ]);
        assert!(b.can_move(), "a vertical pair was not seen");
    }

    #[test]
    fn a_full_board_with_no_two_neighbours_alike_cannot_move() {
        let b = bare([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        assert!(!b.can_move(), "a dead board said it could still move");
    }

    #[test]
    fn the_winning_tile_is_2048_and_not_merely_a_large_one() {
        assert!(!bare([[1024, 0, 0, 0], [0; 4], [0; 4], [0; 4]]).has_won());
        assert!(bare([[2048, 0, 0, 0], [0; 4], [0; 4], [0; 4]]).has_won());
        // Past the winning tile still counts: a player who kept going and
        // reached 4096 has not un-won.
        assert!(bare([[4096, 0, 0, 0], [0; 4], [0; 4], [0; 4]]).has_won());
    }

    // ── Spawning ──

    #[test]
    fn a_new_game_deals_exactly_two_tiles() {
        let app = game();
        let filled = app.board.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(filled, 2, "a game did not open with two tiles");
    }

    #[test]
    fn a_spawned_tile_is_a_two_or_a_four_and_never_anything_else() {
        let mut b = Board::new();
        let mut rng = SeededRng::new(3);
        for _ in 0..GRID_SIZE * GRID_SIZE {
            b.spawn_tile(&mut rng);
        }
        for (r, row) in b.grid.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                assert!(v == 2 || v == 4, "cell ({r}, {c}) was dealt a {v}");
            }
        }
    }

    #[test]
    fn a_four_turns_up_about_one_time_in_ten() {
        // Not a test of the generator -- that is `randrange`'s job -- but of
        // the ratio this game asks it for. A `FOUR_IN` of two would put the
        // count near five hundred and a `FOUR_IN` of a hundred near ten.
        let mut rng = SeededRng::new(1);
        let mut fours = 0;
        for _ in 0..1000 {
            let mut b = Board::new();
            b.spawn_tile(&mut rng);
            if b.grid.iter().flatten().any(|&v| v == 4) {
                fours += 1;
            }
        }
        assert!(
            (50..200).contains(&fours),
            "a four came up {fours} times in a thousand, not about a hundred"
        );
    }

    #[test]
    fn a_tile_is_only_ever_dealt_into_a_free_cell() {
        let mut b = bare([
            [2, 4, 8, 16],
            [32, 64, 128, 256],
            [512, 1024, 2, 4],
            [8, 16, 32, 0],
        ]);
        let mut rng = SeededRng::new(5);
        b.spawn_tile(&mut rng);
        assert_ne!(b.grid[3][3], 0, "the one free cell was not the one used");
        assert_eq!(b.grid[0][0], 2, "an occupied cell was written over");
        assert_eq!(b.grid[2][1], 1024, "an occupied cell was written over");
    }

    #[test]
    fn a_full_board_is_not_dealt_into_at_all() {
        let rows = [
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ];
        let mut b = bare(rows);
        let mut rng = SeededRng::new(9);
        b.spawn_tile(&mut rng);
        assert_eq!(b.grid, rows, "a full board was dealt into");
    }

    #[test]
    fn a_move_that_changed_the_board_deals_a_tile_and_one_that_did_not_does_not() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert!(app.make_move(Direction::Left));
        let after = app.board.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(after, 2, "the merge and the new tile do not add up");

        let mut stuck = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        let before = stuck.board.grid;
        assert!(!stuck.make_move(Direction::Left));
        assert_eq!(stuck.board.grid, before, "a refused move dealt a tile");
    }

    // ── Winning and losing ──

    #[test]
    fn reaching_the_winning_tile_wins_the_game() {
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert!(app.make_move(Direction::Left));
        assert_eq!(app.board.status(), GameStatus::Won);
        assert_eq!(app.board.score(), 2048, "the winning merge did not score");
    }

    #[test]
    fn a_won_game_refuses_to_move_until_the_player_says_to_keep_going() {
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        let frozen = app.board.grid;
        assert!(!app.make_move(Direction::Right), "a won board still moved");
        assert_eq!(app.board.grid, frozen, "a won board moved anyway");
    }

    #[test]
    fn keeping_going_after_a_win_lets_the_game_be_played_on() {
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        app.continue_after_win();
        assert_eq!(app.board.status(), GameStatus::WonContinuing);
        assert!(app.make_move(Direction::Right), "the game stayed frozen");
    }

    #[test]
    fn winning_a_second_time_is_not_offered_again() {
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        app.continue_after_win();
        // The board still holds a 2048, so a win check that only asked "is
        // there a 2048?" would freeze the game again on the very next move.
        app.make_move(Direction::Right);
        assert_eq!(
            app.board.status(),
            GameStatus::WonContinuing,
            "the game was won all over again"
        );
    }

    #[test]
    fn filling_the_board_with_nothing_left_to_do_loses_the_game() {
        // One hole, at the left end of the last row. Sliding left packs that
        // row against the wall and moves the hole to the far end, where the
        // dealt tile -- a 2 or a 4, never anything else -- meets a 64 above it
        // and a 128 beside it. Neither matches, so the board is dead whichever
        // of the two was dealt, and it is dead the moment the move ends.
        let mut app = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [0, 32, 64, 128],
        ]);
        assert!(app.make_move(Direction::Left));
        assert_eq!(
            app.board.status(),
            GameStatus::Lost,
            "a board that ran out of moves never said so"
        );
    }

    #[test]
    fn a_lost_game_refuses_every_direction() {
        let mut app = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        app.board.status = GameStatus::Lost;
        for dir in Direction::ALL {
            assert!(!app.make_move(dir), "a lost game accepted {dir:?}");
        }
    }

    #[test]
    fn winning_on_a_dead_board_ends_the_game_when_the_player_keeps_going() {
        // The fault this test was written for: `make_move` returned the moment
        // it saw the winning tile, skipping the can-anything-move check, and
        // `continue_after_win` did not check either -- so a player who won on
        // the move that filled the board and then chose to keep going sat at
        // `WonContinuing` for ever, every direction refused and nothing on
        // screen saying why.
        //
        // The pair of 1024s merges into the 2048 that wins, which packs the
        // top row and leaves its far end free. The tile dealt there is a 2 or
        // a 4; beside it stands a 16 and beneath it a 64, so neither of the
        // two possible deals leaves a move anywhere on the board.
        let mut app = playing([
            [1024, 1024, 8, 16],
            [4, 16, 32, 64],
            [8, 32, 64, 128],
            [16, 64, 128, 256],
        ]);
        assert!(app.make_move(Direction::Left));
        assert_eq!(app.board.status(), GameStatus::Won, "the win was missed");
        assert!(
            !app.board.can_move(),
            "the fixture is not the dead board this test is about"
        );
        app.continue_after_win();
        assert_eq!(
            app.board.status(),
            GameStatus::Lost,
            "a game with no move left called itself playable"
        );
    }

    #[test]
    fn keeping_going_on_a_board_that_still_has_moves_does_not_end_it() {
        // The other half of the check above: continuing must not lose a game
        // that is merely won. Without this, `check_stuck` could lose every
        // continued game and the test above would still pass.
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        app.continue_after_win();
        assert_eq!(app.board.status(), GameStatus::WonContinuing);
    }

    #[test]
    fn keeping_going_is_refused_by_a_game_that_has_not_won() {
        let mut app = playing([[2, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert_eq!(app.apply(Intent::Continue), EventResult::Ignored);
        assert_eq!(app.board.status(), GameStatus::Playing);
    }

    // ── Undo ──

    #[test]
    fn an_undo_puts_back_the_board_the_score_and_the_move_count() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        let before = app.board.grid;
        app.make_move(Direction::Left);
        assert_eq!(app.board.moves(), 1);
        assert_eq!(app.board.score(), 4);
        assert!(app.undo());
        assert_eq!(app.board.grid, before, "the board did not come back");
        assert_eq!(app.board.score(), 0, "the score did not come back");
        assert_eq!(app.board.moves(), 0, "the move count did not come back");
    }

    #[test]
    fn an_undo_puts_back_the_status_so_a_win_can_be_taken_back() {
        // The fault: the entry held the grid and the score alone, so undoing
        // the winning move left "You win!" printed over a board with no 2048
        // on it -- and since a won game refuses to move, every direction was
        // refused as well.
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        assert_eq!(app.board.status(), GameStatus::Won);
        assert!(app.undo());
        assert_eq!(
            app.board.status(),
            GameStatus::Playing,
            "the win outlived the move that won it"
        );
        assert!(!app.board.has_won(), "the fixture kept its winning tile");
        assert!(
            app.make_move(Direction::Down),
            "the board was still frozen after the win was undone"
        );
    }

    #[test]
    fn an_undo_puts_back_a_lost_game() {
        let mut app = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 0],
        ]);
        app.make_move(Direction::Right);
        for _ in 0..64 {
            if app.board.status() == GameStatus::Lost {
                break;
            }
            for dir in Direction::ALL {
                app.make_move(dir);
            }
        }
        assert_eq!(app.board.status(), GameStatus::Lost, "fixture never lost");
        assert!(app.undo());
        assert_ne!(
            app.board.status(),
            GameStatus::Lost,
            "the loss outlived the move that lost it"
        );
    }

    #[test]
    fn the_best_score_survives_an_undo() {
        // Deliberately not restored: the best score is the best across every
        // game in the session, and taking a move back does not un-happen
        // having once scored that much.
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        assert_eq!(app.board.best_score(), 4);
        app.undo();
        assert_eq!(app.board.score(), 0, "the score should have come back");
        assert_eq!(app.board.best_score(), 4, "the best score was rolled back");
    }

    #[test]
    fn a_move_that_changed_nothing_leaves_nothing_to_undo() {
        let mut app = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        assert!(!app.make_move(Direction::Left));
        assert_eq!(app.undo_depth(), 0, "a refused move went into the history");
    }

    #[test]
    fn an_undo_with_nothing_behind_it_is_refused_rather_than_pretended() {
        let mut app = playing([[2, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert_eq!(app.undo_depth(), 0);
        assert!(!app.undo(), "an empty history claimed to undo something");
        assert_eq!(app.apply(Intent::Undo), EventResult::Ignored);
    }

    #[test]
    fn the_history_forgets_its_oldest_move_rather_than_growing_for_ever() {
        // The two ends of the history have to be *different* moves, or
        // dropping either end leaves the same history behind and the test
        // cannot tell "forgets the oldest" from "forgets the newest"
        // (`known-issues.md` lesson 59). So: one move that is a merge, then a
        // full history's worth of moves that are not, and the merge is the
        // one that has to fall off the end.
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.board.grid = [[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]];
        app.make_move(Direction::Left);
        let after_merge = app.board.score();
        assert_eq!(after_merge, 4, "the fixture's first move did not merge");

        // Nudge a lone tile from wall to wall. Each is a real move and none
        // of them scores, so the score is the mark of the first move alone.
        for i in 0..MAX_UNDO {
            app.board.grid = [[0, 0, 0, 0], [0; 4], [0; 4], [0; 4]];
            app.board.grid[1][if i % 2 == 0 { 0 } else { 3 }] = 8;
            app.board.status = GameStatus::Playing;
            let dir = if i % 2 == 0 {
                Direction::Right
            } else {
                Direction::Left
            };
            assert!(app.make_move(dir), "nudge {i} did not move");
        }
        assert_eq!(
            app.undo_depth(),
            MAX_UNDO,
            "the history grew past the cap it is supposed to keep"
        );

        for _ in 0..MAX_UNDO {
            app.undo();
        }
        assert_eq!(app.undo_depth(), 0, "the history would not empty");
        assert_eq!(
            app.board.score(),
            after_merge,
            "the history forgot its newest move rather than its oldest"
        );
    }

    #[test]
    fn a_new_game_throws_the_history_away_and_keeps_the_best_score() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        assert_eq!(app.undo_depth(), 1);
        app.new_game();
        assert_eq!(app.undo_depth(), 0, "the old game could still be undone");
        assert_eq!(app.board.score(), 0, "the new game started with a score");
        assert_eq!(app.board.moves(), 0, "the new game started with moves");
        assert_eq!(app.board.status(), GameStatus::Playing);
        assert_eq!(app.board.best_score(), 4, "the best score was thrown away");
        let filled = app.board.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(filled, 2, "the new game was not dealt two tiles");
    }

    // ── Keys ──

    #[test]
    fn every_direction_key_and_its_letter_ask_for_the_same_move() {
        let pairs = [
            (Key::Up, Key::W, Direction::Up),
            (Key::Down, Key::S, Direction::Down),
            (Key::Left, Key::A, Direction::Left),
            (Key::Right, Key::D, Direction::Right),
        ];
        for (arrow, letter, dir) in pairs {
            assert_eq!(
                key_intent(&probe::press(arrow)),
                Some(Intent::Move(dir)),
                "{arrow:?} does not slide {dir:?}"
            );
            assert_eq!(
                key_intent(&probe::press(letter)),
                Some(Intent::Move(dir)),
                "{letter:?} does not slide {dir:?}"
            );
        }
    }

    #[test]
    fn the_keys_the_help_sheet_names_are_the_keys_the_program_reads() {
        let named = [
            (Key::U, Intent::Undo),
            (Key::N, Intent::NewGame),
            (Key::R, Intent::NewGame),
            (Key::C, Intent::Continue),
            (Key::Enter, Intent::Continue),
            (Key::H, Intent::ToggleHelp),
            (Key::Escape, Intent::CloseHelp),
        ];
        for (key, intent) in named {
            assert_eq!(
                key_intent(&probe::press(key)),
                Some(intent),
                "{key:?} does not do what the help sheet says"
            );
        }
    }

    #[test]
    fn ctrl_z_undoes_and_a_bare_z_does_nothing() {
        assert_eq!(key_intent(&probe::ctrl(Key::Z)), Some(Intent::Undo));
        assert_eq!(
            key_intent(&probe::press(Key::Z)),
            None,
            "a bare Z did something"
        );
    }

    #[test]
    fn a_ctrl_or_alt_arrow_belongs_to_the_window_and_not_to_the_board() {
        assert_eq!(key_intent(&probe::ctrl(Key::Left)), None);
        let mut alt = probe::press(Key::Left);
        alt.modifiers.alt = true;
        assert_eq!(key_intent(&alt), None, "Alt+Left slid the tiles");
        // And the plain one still works, or the two legs above would be
        // satisfied by a handler that reads no keys at all.
        assert_eq!(
            key_intent(&probe::press(Key::Left)),
            Some(Intent::Move(Direction::Left))
        );
    }

    #[test]
    fn a_key_release_is_not_a_second_press() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        let mut up = probe::press(Key::Left);
        up.pressed = false;
        assert_eq!(
            handle_event(&mut app, &Event::Key(up)),
            EventResult::Ignored
        );
        assert_eq!(app.board.moves(), 0, "letting a key go played a move");
    }

    #[test]
    fn a_key_the_game_has_no_use_for_is_left_for_someone_else() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert_eq!(press(&mut app, Key::Q), EventResult::Ignored);
        assert_eq!(app.board.moves(), 0);
    }

    #[test]
    fn a_direction_that_cannot_be_played_is_reported_as_unhandled() {
        // The distinction matters to the window: a move that changed nothing
        // must not ask for a repaint, or every key against a wall redraws the
        // screen for no reason.
        let mut app = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        assert_eq!(press(&mut app, Key::Left), EventResult::Ignored);
        let mut open = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert_eq!(press(&mut open, Key::Left), EventResult::Consumed);
    }

    #[test]
    fn escape_closes_the_help_and_otherwise_does_nothing() {
        let mut app = game();
        assert_eq!(press(&mut app, Key::Escape), EventResult::Ignored);
        press(&mut app, Key::H);
        assert!(app.help_is_open());
        assert_eq!(press(&mut app, Key::Escape), EventResult::Consumed);
        assert!(!app.help_is_open(), "escape did not close the help");
    }

    #[test]
    fn h_toggles_the_help_rather_than_only_opening_it() {
        let mut app = game();
        press(&mut app, Key::H);
        assert!(app.help_is_open());
        press(&mut app, Key::H);
        assert!(!app.help_is_open(), "H would not close what it opened");
    }

    #[test]
    fn undo_and_a_new_game_are_reachable_from_a_finished_game() {
        // Both are checked before the status guards, because the two things a
        // player wants from a game that has ended are to take the ending back
        // and to start again.
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.make_move(Direction::Left);
        app.board.status = GameStatus::Lost;
        assert_eq!(press(&mut app, Key::U), EventResult::Consumed);
        assert_ne!(app.board.status(), GameStatus::Lost);

        app.board.status = GameStatus::Lost;
        assert_eq!(press(&mut app, Key::N), EventResult::Consumed);
        assert_eq!(app.board.status(), GameStatus::Playing);
    }

    // ── Layout ──

    #[test]
    fn the_board_is_square_and_inside_the_window_at_every_size() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.01,
                "{w}x{h}: the board is not square: {:?}",
                l.board
            );
            assert!(l.board.x >= -0.01, "{w}x{h}: board off the left");
            assert!(l.board.y >= -0.01, "{w}x{h}: board off the top");
            assert!(l.board.right() <= w + 0.01, "{w}x{h}: board off the right");
            assert!(
                l.board.bottom() <= h + 0.01,
                "{w}x{h}: board off the bottom"
            );
        }
    }

    #[test]
    fn the_board_keeps_its_share_of_every_window() {
        // The share is written out here rather than taken from `BOARD_SHARE`:
        // a test that asserts against the constant it is testing moves with
        // it, and a mutation that zeroes the constant zeroes the assertion
        // too (`known-issues.md` lesson 52).
        //
        // The floor is the smaller of that share and half the width, because
        // the board is square: a window far wider than it is tall is limited
        // by its height, and one far taller than it is wide by its width.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let floor = (h * 0.42).min(w * 0.5);
            assert!(
                l.board.h >= floor - 0.01,
                "{w}x{h}: the board got {} of a window that owes it {floor}",
                l.board.h
            );
        }
    }

    #[test]
    fn the_bands_are_given_up_from_the_bottom_up_as_the_window_shrinks() {
        // Which bands survive, and in what order they go -- never where any of
        // them lands. Placement is
        // `the_bands_stack_down_the_window_in_the_order_they_are_named`, and
        // keeping the two apart is `known-issues.md` lesson 57: a band drawn
        // in the wrong place is still a band that is there.
        //
        // The heights are swept rather than named, so the test does not have
        // to know the arithmetic it is checking -- only the ladder.
        let names = ["header", "info", "dpad", "footer"];
        let shown = |h: f32| {
            let l = Layout::new(600.0, h);
            [
                l.shows(l.header),
                l.shows(l.info),
                l.shows(l.dpad),
                l.shows(l.footer),
            ]
        };
        assert_eq!(shown(900.0), [true; 4], "a tall window dropped a band");
        assert_eq!(shown(20.0), [false; 4], "a tiny window kept a band");

        // Walk down and record the order in which each band goes out, and
        // that none of them comes back.
        let mut order = Vec::new();
        let mut last = shown(900.0);
        let mut h = 900.0;
        while h > 20.0 {
            h -= 1.0;
            let now = shown(h);
            for i in 0..4 {
                // Read as: it was there before, or it is not there now --
                // which is exactly "a band that has gone stays gone".
                assert!(last[i] || !now[i], "{} came back at height {h}", names[i]);
                if last[i] && !now[i] {
                    order.push(i);
                }
            }
            last = now;
        }
        assert_eq!(
            order,
            vec![3, 1, 0, 2],
            "the bands went out as {:?}, not footer, info, header, pad",
            order.iter().map(|&i| names[i]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_bands_stack_down_the_window_in_the_order_they_are_named() {
        // Where they land, which the ladder test above deliberately does not
        // ask. Header at the top, then info, then the board, then the pad,
        // then the footer at the bottom.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let bands = [
                ("header", l.header),
                ("info", l.info),
                ("board", l.board),
                ("dpad", l.dpad),
                ("footer", l.footer),
            ];
            let live: Vec<_> = bands.iter().filter(|(_, r)| !r.is_empty()).collect();
            for pair in live.windows(2) {
                let (an, a) = pair[0];
                let (bn, b) = pair[1];
                assert!(
                    a.bottom() <= b.y + 0.01,
                    "{w}x{h}: {an} ({a:?}) is not above {bn} ({b:?})"
                );
            }
            if let Some((name, first)) = live.first() {
                assert!(first.y >= -0.01, "{w}x{h}: {name} starts off the top");
            }
            if let Some((name, last)) = live.last() {
                assert!(
                    last.bottom() <= h + 0.01,
                    "{w}x{h}: {name} ends past the bottom"
                );
            }
        }
    }

    #[test]
    fn a_band_that_did_not_fit_is_gone_rather_than_flat() {
        // A zero-high rectangle at the right y is still a rectangle, and a
        // click on its edge would still land in it. A band that did not fit
        // must be `Rect::EMPTY`, which is nowhere.
        let l = Layout::new(600.0, 130.0);
        for (name, band) in [
            ("header", l.header),
            ("info", l.info),
            ("dpad", l.dpad),
            ("footer", l.footer),
        ] {
            if !l.shows(band) {
                assert_eq!(band, Rect::EMPTY, "{name} was flattened, not dropped");
            }
        }
        assert!(
            [l.header, l.info, l.dpad, l.footer]
                .iter()
                .any(|b| b.is_empty()),
            "the fixture window kept every band, so it tests nothing"
        );
    }

    #[test]
    fn the_cells_tile_the_board_without_overlapping_or_running_off_it() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            let mut seen: Vec<Rect> = Vec::new();
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    let r = l.cell(row, col);
                    assert!(
                        r.x >= l.board.x - 0.01
                            && r.y >= l.board.y - 0.01
                            && r.right() <= l.board.right() + 0.01
                            && r.bottom() <= l.board.bottom() + 0.01,
                        "{w}x{h}: cell ({row}, {col}) {r:?} is outside {:?}",
                        l.board
                    );
                    for other in &seen {
                        assert!(
                            r.intersect(*other).is_none_or(|o| o.w < 0.01 || o.h < 0.01),
                            "{w}x{h}: cell ({row}, {col}) overlaps {other:?}"
                        );
                    }
                    seen.push(r);
                }
            }
        }
    }

    #[test]
    fn the_gap_is_taken_out_of_the_cell_rather_than_added_to_the_step() {
        // Sixteen cells stepped by "cell plus gap" run five gaps past the
        // right-hand edge of the board. The step here is the board divided by
        // four and the gap comes out of the cell, so the last cell ends where
        // the board does.
        let l = Layout::new(600.0, 800.0);
        let first = l.cell(0, 0);
        let last = l.cell(GRID_SIZE - 1, GRID_SIZE - 1);
        let step = l.board.w / GRID_SIZE as f32;
        assert!(
            (last.right() - (l.board.right() - (first.x - l.board.x))).abs() < 0.01,
            "the board is not padded evenly: first {first:?}, last {last:?}"
        );
        assert!(
            (l.cell(0, 1).x - l.cell(0, 0).x - step).abs() < 0.01,
            "the step between cells is not a quarter of the board"
        );
        assert!(first.w < step, "the gap was not taken out of the cell");
    }

    #[test]
    fn a_cell_off_the_board_has_no_rectangle_at_all() {
        let l = Layout::new(600.0, 800.0);
        assert_eq!(l.cell(GRID_SIZE, 0), Rect::EMPTY, "a row past the last");
        assert_eq!(l.cell(0, GRID_SIZE), Rect::EMPTY, "a column past the last");
        assert_ne!(l.cell(GRID_SIZE - 1, GRID_SIZE - 1), Rect::EMPTY);
    }

    #[test]
    fn the_direction_buttons_sit_in_a_row_inside_the_pad_and_do_not_overlap() {
        let l = Layout::new(600.0, 800.0);
        let mut prev: Option<Rect> = None;
        for i in 0..Direction::ALL.len() {
            let r = l.dpad_button(i);
            assert!(!r.is_empty(), "direction button {i} has no rectangle");
            assert!(
                r.y >= l.dpad.y - 0.01 && r.bottom() <= l.dpad.bottom() + 0.01,
                "button {i} {r:?} is taller than the pad {:?}",
                l.dpad
            );
            assert!(
                r.x >= l.dpad.x - 0.01 && r.right() <= l.dpad.right() + 0.01,
                "button {i} runs off the pad: {r:?}"
            );
            if let Some(p) = prev {
                assert!(
                    r.x >= p.right() - 0.01,
                    "button {i} overlaps the one before"
                );
            }
            prev = Some(r);
        }
        assert_eq!(
            l.dpad_button(Direction::ALL.len()),
            Rect::EMPTY,
            "a button past the last one still has a rectangle"
        );
    }

    #[test]
    fn the_footer_buttons_sit_in_a_row_inside_the_footer_and_do_not_overlap() {
        let l = Layout::new(600.0, 800.0);
        let mut prev: Option<Rect> = None;
        for i in 0..3 {
            let r = l.footer_button(i);
            assert!(!r.is_empty(), "footer button {i} has no rectangle");
            assert!(
                r.y >= l.footer.y - 0.01 && r.bottom() <= l.footer.bottom() + 0.01,
                "footer button {i} {r:?} is taller than {:?}",
                l.footer
            );
            if let Some(p) = prev {
                assert!(r.x >= p.right() - 0.01, "footer button {i} overlaps");
            }
            prev = Some(r);
        }
        assert_eq!(l.footer_button(3), Rect::EMPTY);
    }

    #[test]
    fn a_button_in_a_band_that_was_dropped_has_no_rectangle() {
        let l = Layout::new(600.0, 130.0);
        if !l.shows(l.footer) {
            assert_eq!(
                l.footer_button(0),
                Rect::EMPTY,
                "a dropped footer had a button"
            );
        }
        if !l.shows(l.dpad) {
            assert_eq!(l.dpad_button(0), Rect::EMPTY, "a dropped pad had a button");
        }
        assert!(
            !l.shows(l.footer) || !l.shows(l.dpad),
            "the fixture kept both bands, so it tests nothing"
        );
    }

    #[test]
    fn the_score_boxes_sit_side_by_side_inside_the_header() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let score = l.score_box(0);
        let best = l.score_box(1);
        assert!(!score.is_empty() && !best.is_empty());
        assert!(
            best.right() <= score.x + 0.01,
            "the two boxes overlap: {best:?} and {score:?}"
        );
        for (i, r) in [(0, score), (1, best)] {
            assert!(
                r.y >= l.header.y - 0.01 && r.bottom() <= l.header.bottom() + 0.01,
                "box {i} {r:?} is taller than the header {:?}",
                l.header
            );
            assert!(
                r.right() <= l.header.right() + 0.01,
                "box {i} runs off the right of the header: {r:?}"
            );
            assert!(
                r.x >= l.header.x - 0.01,
                "box {i} runs off the left of the header: {r:?}"
            );
        }
        assert_eq!(l.score_box(2), Rect::EMPTY, "a third box has a rectangle");
    }

    #[test]
    fn a_score_box_with_no_room_left_is_dropped_rather_than_drawn_off_the_edge() {
        // The left-hand clamp is unreachable at any comfortable width -- there
        // is always room for both boxes -- so it can only be tested in a header
        // too narrow to hold them. Narrow enough and the further box has
        // nowhere to start, and is given no rectangle at all rather than one
        // beginning off the window.
        let narrow = Layout::new(90.0, WINDOW_HEIGHT);
        assert!(
            narrow.shows(narrow.header),
            "the fixture dropped the header"
        );
        assert_eq!(
            narrow.score_box(1),
            Rect::EMPTY,
            "the second box was drawn in a header with no room for it"
        );
        for i in 0..2 {
            let r = narrow.score_box(i);
            if !r.is_empty() {
                assert!(r.x >= narrow.header.x - 0.01, "box {i} starts off the left");
            }
        }
    }

    #[test]
    fn the_title_makes_room_for_whichever_score_boxes_are_drawn() {
        for w in [WINDOW_WIDTH, 300.0, 200.0, 120.0, 90.0] {
            let l = Layout::new(w, WINDOW_HEIGHT);
            if !l.shows(l.header) {
                continue;
            }
            let app = windowed(w, WINDOW_HEIGHT);
            let f = app.frame(w, WINDOW_HEIGHT);
            let leftmost = [l.score_box(0), l.score_box(1)]
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| r.x)
                .fold(f32::INFINITY, f32::min);
            for (body, r, _) in text_boxes(&f) {
                if body == "2048" {
                    assert!(
                        r.right() <= leftmost + 0.01,
                        "at width {w} the title runs to {}, under a box at {leftmost}",
                        r.right()
                    );
                }
            }
        }
    }

    #[test]
    fn the_banner_and_the_button_on_it_stay_inside_the_board() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let b = l.banner();
            if b.is_empty() {
                continue;
            }
            assert!(
                b.x >= l.board.x - 0.01
                    && b.y >= l.board.y - 0.01
                    && b.right() <= l.board.right() + 0.01
                    && b.bottom() <= l.board.bottom() + 0.01,
                "{w}x{h}: the banner {b:?} is outside the board {:?}",
                l.board
            );
            let btn = l.banner_button();
            if !btn.is_empty() {
                assert!(
                    btn.x >= b.x - 0.01
                        && btn.y >= b.y - 0.01
                        && btn.right() <= b.right() + 0.01
                        && btn.bottom() <= b.bottom() + 0.01,
                    "{w}x{h}: the keep-going button {btn:?} is outside the banner {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_window_too_small_for_anything_still_draws_something() {
        let app = windowed(24.0, 24.0);
        let f = app.frame(24.0, 24.0);
        assert!(
            !f.commands().is_empty(),
            "a tiny window drew nothing at all"
        );
        assert!(
            f.is_balanced(),
            "a tiny window left a clip or a translate open"
        );
    }

    // ── Drawing ──

    #[test]
    fn every_frame_is_balanced_at_every_size() {
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            app.show_help = true;
            app.board.status = GameStatus::Won;
            assert!(app.frame(w, h).is_balanced(), "{w}x{h} left a state open");
        }
    }

    #[test]
    fn no_text_is_drawn_outside_the_window_it_belongs_to() {
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            app.show_help = true;
            let f = app.frame(w, h);
            for (body, r, _) in text_boxes(&f) {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01,
                    "{w}x{h}: {body:?} starts at ({}, {}), off the window",
                    r.x,
                    r.y
                );
                assert!(
                    r.right() <= w + 0.01,
                    "{w}x{h}: {body:?} runs off the right edge to {}",
                    r.right()
                );
                assert!(
                    r.bottom() <= h + 0.01,
                    "{w}x{h}: {body:?} runs off the bottom from y={}",
                    r.y
                );
            }
        }
    }

    #[test]
    fn the_numbers_on_screen_are_the_numbers_on_the_board() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[2, 4, 0, 0], [0, 8, 0, 0], [0, 0, 16, 0], [0, 0, 0, 32]]);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = texts(&f);
        for want in ["2", "4", "8", "16", "32"] {
            assert!(drawn.iter().any(|t| t == want), "no tile read {want}");
        }
        // Six tiles, one text each. The board holds five distinct values, one
        // of which -- 2 -- also appears in the title "2048" and in nothing
        // else, so counting is done on a value the chrome does not use.
        let sixteens = drawn.iter().filter(|t| t.as_str() == "16").count();
        assert_eq!(sixteens, 1, "the 16 was drawn {sixteens} times");
        let sixty_fours = drawn.iter().filter(|t| t.as_str() == "64").count();
        assert_eq!(sixty_fours, 0, "a tile that is not on the board was drawn");
    }

    #[test]
    fn an_empty_cell_has_no_number_written_on_it() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[0; 4]; 4]);
        app.board.score = 0;
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Counted inside the board only. A score of nothing is written "0" in
        // the header quite properly, so counting noughts over the whole frame
        // would be counting the chrome and not the cells.
        let on_the_board = text_boxes(&f)
            .into_iter()
            .filter(|(_, r, _)| l.board.contains(r.x, r.y))
            .filter(|(body, _, _)| body == "0")
            .count();
        assert_eq!(on_the_board, 0, "an empty cell was written with a nought");
    }

    #[test]
    fn a_tile_is_painted_in_its_own_colour_and_not_the_empty_one() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[2, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let filled = fill_at(&f, l.cell(0, 0)).expect("the filled cell was not painted");
        let empty = fill_at(&f, l.cell(0, 1)).expect("the empty cell was not painted");
        assert_ne!(
            filled, empty,
            "a tile and a hole were painted the same colour"
        );
        assert_eq!(filled, tile_face(2));
        assert_eq!(empty, tile_face(0));
    }

    #[test]
    fn the_moves_and_the_highest_tile_are_both_on_screen() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[128, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.board.moves = 7;
        let joined = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(joined.contains("Moves: 7"), "the move count is not shown");
        assert!(
            joined.contains("Highest: 128"),
            "the highest tile is not shown"
        );
    }

    #[test]
    fn the_score_and_the_best_score_are_both_on_screen_and_are_told_apart() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[2, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.board.score = 12;
        app.board.best_score = 340;
        let drawn = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(drawn.iter().any(|t| t == "SCORE"));
        assert!(drawn.iter().any(|t| t == "BEST"));
        assert!(drawn.iter().any(|t| t == "12"), "the score is not shown");
        assert!(drawn.iter().any(|t| t == "340"), "the best is not shown");
    }

    #[test]
    fn the_win_banner_offers_to_keep_going_and_the_loss_banner_does_not() {
        let mut won = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        won.board = bare([[2048, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        won.board.status = GameStatus::Won;
        assert!(
            probe::is_visible_sized(&won, Target::Continue, (WINDOW_WIDTH, WINDOW_HEIGHT)),
            "a won game did not offer to keep going"
        );

        let mut lost = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        lost.board.status = GameStatus::Lost;
        assert!(
            !probe::is_visible_sized(&lost, Target::Continue, (WINDOW_WIDTH, WINDOW_HEIGHT)),
            "a lost game offered to keep going"
        );
        let joined = texts(&lost.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(joined.contains("Game over"), "the loss was not announced");
    }

    #[test]
    fn the_banner_shows_the_score_the_game_ended_on() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[2048, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.board.status = GameStatus::Won;
        app.board.score = 20_484;
        let joined = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(
            joined.contains("Score 20484"),
            "the banner does not carry the score: {joined}"
        );
    }

    #[test]
    fn no_banner_is_drawn_over_a_game_still_being_played() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let joined = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(!joined.contains("You win!"), "a playing game claimed a win");
        assert!(
            !joined.contains("Game over"),
            "a playing game claimed a loss"
        );
    }

    // ── Pointer ──

    #[test]
    fn every_control_the_program_has_can_be_reached_with_a_mouse() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board = bare([[2048, 0, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.board.status = GameStatus::Won;
        app.show_help = true;
        let names = probe::control_names(&app);
        for want in ["Move", "NewGame", "Undo", "Help", "Continue", "HelpSheet"] {
            assert!(
                names.iter().any(|n| n == want),
                "{want} cannot be clicked; the window offers {names:?}"
            );
        }
    }

    #[test]
    fn all_four_directions_can_be_clicked() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        for dir in Direction::ALL {
            assert!(
                probe::is_visible_sized(&app, Target::Move(dir), (WINDOW_WIDTH, WINDOW_HEIGHT)),
                "{dir:?} has no button"
            );
        }
    }

    #[test]
    fn every_control_records_a_hit_box_where_it_was_painted() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.board.status = GameStatus::Won;
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (target, rect) in f.hits() {
            assert!(!rect.is_empty(), "{target:?} has an empty hit box");
            let (cx, cy) = rect.centre();
            assert_eq!(
                f.hit_test(cx, cy).as_ref(),
                Some(target).filter(|_| f.hit_test(cx, cy).is_some()),
                "{target:?} does not answer a click at its own centre"
            );
        }
        assert!(!f.hits().is_empty(), "nothing at all was clickable");
    }

    #[test]
    fn clicking_a_direction_button_slides_that_way() {
        let mut app = playing([[2, 0, 0, 2], [0; 4], [0; 4], [0; 4]]);
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            click_on(&mut app, Target::Move(Direction::Left)),
            EventResult::Consumed
        );
        assert_eq!(app.board.at(0, 0), 4, "clicking left did not slide left");
    }

    #[test]
    fn each_direction_button_slides_its_own_way_and_not_another() {
        for dir in Direction::ALL {
            // Four 4s in the middle. Whichever way they are pushed the two
            // pairs merge into two 8s against that wall -- four different
            // landing places, so no two directions can be swapped without
            // this noticing. The tiles are counted by their *value*: the deal
            // that follows every move can only ever make a 2 or a 4, so an 8
            // on the board is a tile the move made and not one dealt in.
            let mut app = playing([[0; 4], [0, 4, 4, 0], [0, 4, 4, 0], [0; 4]]);
            app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
            click_on(&mut app, Target::Move(dir));
            let landed: Vec<(usize, usize)> = (0..GRID_SIZE)
                .flat_map(|r| (0..GRID_SIZE).map(move |c| (r, c)))
                .filter(|&(r, c)| app.board.at(r, c) == 8)
                .collect();
            let want = match dir {
                Direction::Up => vec![(0, 1), (0, 2)],
                Direction::Down => vec![(GRID_SIZE - 1, 1), (GRID_SIZE - 1, 2)],
                Direction::Left => vec![(1, 0), (2, 0)],
                Direction::Right => vec![(1, GRID_SIZE - 1), (2, GRID_SIZE - 1)],
            };
            assert_eq!(landed, want, "{dir:?} put the tiles in the wrong place");
        }
    }

    #[test]
    fn clicking_the_footer_buttons_does_what_they_say() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.make_move(Direction::Left);
        assert_eq!(app.undo_depth(), 1);

        assert_eq!(click_on(&mut app, Target::Undo), EventResult::Consumed);
        assert_eq!(app.undo_depth(), 0, "the undo button did not undo");

        assert_eq!(click_on(&mut app, Target::Help), EventResult::Consumed);
        assert!(app.help_is_open(), "the help button did not open the help");
        // Close it again through the sheet, so the next click is not swallowed.
        click_on(&mut app, Target::HelpSheet);

        app.board.score = 40;
        assert_eq!(click_on(&mut app, Target::NewGame), EventResult::Consumed);
        assert_eq!(app.board.score(), 0, "the new-game button did not deal");
    }

    #[test]
    fn clicking_keep_going_unfreezes_a_won_board() {
        let mut app = playing([[1024, 1024, 0, 0], [0; 4], [0; 4], [0; 4]]);
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.make_move(Direction::Left);
        assert_eq!(app.board.status(), GameStatus::Won);
        assert_eq!(click_on(&mut app, Target::Continue), EventResult::Consumed);
        assert_eq!(app.board.status(), GameStatus::WonContinuing);
    }

    #[test]
    fn the_help_sheet_swallows_the_click_that_closes_it() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        press(&mut app, Key::H);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.help.centre();
        assert_eq!(click(&mut app, cx, cy), EventResult::Consumed);
        assert!(!app.help_is_open(), "the sheet would not close");
    }

    #[test]
    fn the_help_sheet_hides_what_it_covers_from_a_click() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // A point on the board, under the sheet. Without the sheet the board
        // takes no clicks anyway, so the meaningful check is that the sheet
        // answers there rather than nothing does.
        let (cx, cy) = l.help.centre();
        assert_eq!(app.target_at(cx, cy), None, "something under the sheet");
        app.show_help = true;
        assert_eq!(
            app.target_at(cx, cy),
            Some(Target::HelpSheet),
            "the sheet did not take the click"
        );
    }

    #[test]
    fn a_click_on_nothing_at_all_is_left_for_someone_else() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.board.centre();
        assert_eq!(click(&mut app, cx, cy), EventResult::Ignored);
    }

    #[test]
    fn a_right_click_is_not_a_left_click() {
        let mut app = playing([[2, 0, 0, 2], [0; 4], [0; 4], [0; 4]]);
        app.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        let r = probe::rect_of_sized(
            &app,
            Target::Move(Direction::Left),
            (WINDOW_WIDTH, WINDOW_HEIGHT),
        )
        .expect("no left button");
        let (cx, cy) = r.centre();
        let size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            app.click_at(cx, cy, MouseButton::Right, size),
            EventResult::Ignored,
            "the right button played a move"
        );
        assert_eq!(app.board.moves(), 0);
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        // Drawn through `render`, because that is the only way a window ever
        // gets a size into this program. Calling `resize` directly -- which
        // nothing outside the tests does -- would not see `render` failing to
        // remember the size it had just drawn at.
        let mut app = playing([[2, 0, 0, 2], [0; 4], [0; 4], [0; 4]]);
        let big = (1200.0, 1000.0);
        let tree = app.render(big.0, big.1);
        assert!(!tree.commands.is_empty(), "nothing was drawn");

        let l = Layout::new(big.0, big.1);
        let (cx, cy) = l.dpad_button(0).centre();
        assert_eq!(
            app.target_at(cx, cy),
            Some(Target::Move(Direction::Left)),
            "the click was read against a size the window is not"
        );
        // And the same point in the *default* window is not that button, so
        // the leg above cannot pass by the two layouts happening to agree.
        let small = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_ne!(
            small.dpad_button(0).centre(),
            (cx, cy),
            "the two window sizes lay the pad out identically"
        );
    }

    #[test]
    fn resizing_the_window_moves_the_board() {
        let mut app = game();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 400,
                height: 400,
            },
        );
        let small = Layout::new(400.0, 400.0).board;
        assert_eq!(app.width, 400.0);
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1000,
                height: 900,
            },
        );
        let large = Layout::new(1000.0, 900.0).board;
        assert_ne!(small, large, "the board did not move with the window");
        assert_eq!(app.width, 1000.0);
        assert_eq!(app.height, 900.0);
    }

    #[test]
    fn a_mouse_alone_can_play_a_game_to_the_end() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        // Bounded: a game that will not end must fail this test rather than
        // hang it (`known-issues.md` lesson 55). Two thousand clicks is far
        // more than a 4x4 board needs to fill with no merges left.
        let mut clicks = 0;
        for i in 0..2000 {
            if app.board.status() == GameStatus::Lost {
                break;
            }
            if app.board.status() == GameStatus::Won {
                click_on(&mut app, Target::Continue);
                continue;
            }
            let dir = Direction::ALL[i % Direction::ALL.len()];
            click_on(&mut app, Target::Move(dir));
            clicks += 1;
        }
        assert_eq!(
            app.board.status(),
            GameStatus::Lost,
            "the game never ended in {clicks} clicks"
        );
        assert!(app.board.moves() > 0, "no move was ever played");
        // And a new game is one more click away, from the same pointer.
        assert_eq!(click_on(&mut app, Target::NewGame), EventResult::Consumed);
        assert_eq!(app.board.status(), GameStatus::Playing);
    }

    #[test]
    fn the_direction_buttons_are_greyed_while_the_board_is_frozen() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let btn = l.dpad_button(0);

        let live = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let live_face =
            fill_at(&live.frame(WINDOW_WIDTH, WINDOW_HEIGHT), btn).expect("no button painted");

        let mut frozen = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        frozen.board.status = GameStatus::Won;
        let frozen_face =
            fill_at(&frozen.frame(WINDOW_WIDTH, WINDOW_HEIGHT), btn).expect("no button painted");

        assert_ne!(
            live_face, frozen_face,
            "a frozen board's direction buttons look playable"
        );
    }

    #[test]
    fn the_undo_button_is_greyed_while_there_is_nothing_to_undo() {
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let btn = l.footer_button(1);

        let empty = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let empty_face =
            fill_at(&empty.frame(WINDOW_WIDTH, WINDOW_HEIGHT), btn).expect("no button painted");

        let mut used = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        used.resize(WINDOW_WIDTH, WINDOW_HEIGHT);
        used.make_move(Direction::Left);
        let used_face =
            fill_at(&used.frame(WINDOW_WIDTH, WINDOW_HEIGHT), btn).expect("no button painted");

        assert_ne!(
            empty_face, used_face,
            "the undo button looks the same with and without a history"
        );
    }

    // ── The window ──

    #[test]
    fn what_the_window_draws_is_what_the_frame_drew() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let tree = app.render(WINDOW_WIDTH, WINDOW_HEIGHT);
        let frame = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            tree.commands.len(),
            frame.commands().len(),
            "the window and the frame disagree about what is on screen"
        );
    }

    #[test]
    fn the_window_closes_when_it_is_asked_to() {
        let mut app = game();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
    }

    #[test]
    fn a_move_asks_for_a_repaint_and_a_refused_one_does_not() {
        let mut app = playing([[2, 2, 0, 0], [0; 4], [0; 4], [0; 4]]);
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Left))),
            Response::Redraw
        ));
        let mut stuck = playing([
            [2, 4, 8, 16],
            [4, 8, 16, 32],
            [8, 16, 32, 64],
            [16, 32, 64, 128],
        ]);
        assert!(matches!(
            stuck.on_event(&Event::Key(probe::press(Key::Left))),
            Response::Idle
        ));
    }

    #[test]
    fn the_events_this_game_has_no_use_for_are_answered_anyway() {
        let mut app = game();
        assert_eq!(
            handle_event(&mut app, &Event::Tick { elapsed_ms: 16 }),
            EventResult::Ignored,
            "a tick was treated as something to do"
        );
    }

    #[test]
    fn the_program_names_itself_the_same_way_everywhere() {
        let app = game();
        assert_eq!(app.app_id(), "game2048");
        assert_eq!(app.title(), "2048");
        assert_eq!(
            app.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
    }

    #[test]
    fn a_window_opens_at_a_size_its_own_layout_can_use() {
        let (w, h) = Game2048::SIZE;
        let l = Layout::new(w, h);
        for (name, band) in [
            ("header", l.header),
            ("info", l.info),
            ("dpad", l.dpad),
            ("footer", l.footer),
        ] {
            assert!(
                l.shows(band),
                "the default window has no room for the {name}"
            );
        }
        assert!(!l.board.is_empty(), "the default window has no board");
    }

    #[test]
    fn every_key_and_every_button_go_through_the_same_intent() {
        // The two routes cannot drift apart if they are the same line of code,
        // and this is the assertion that says they are.
        let pairs = [
            (Target::NewGame, Key::N, Intent::NewGame),
            (Target::Undo, Key::U, Intent::Undo),
            (Target::Help, Key::H, Intent::ToggleHelp),
            (Target::Continue, Key::C, Intent::Continue),
        ];
        for (target, key, intent) in pairs {
            assert_eq!(target_intent(target), intent, "{target:?}");
            assert_eq!(key_intent(&probe::press(key)), Some(intent), "{key:?}");
        }
        for dir in Direction::ALL {
            assert_eq!(target_intent(Target::Move(dir)), Intent::Move(dir));
        }
        assert_eq!(target_intent(Target::HelpSheet), Intent::CloseHelp);
    }

    #[test]
    fn the_direction_buttons_are_labelled_with_four_different_glyphs() {
        let mut seen: Vec<&str> = Vec::new();
        for dir in Direction::ALL {
            let g = dir.glyph();
            assert!(!g.is_empty(), "{dir:?} has no glyph");
            assert!(g.is_ascii(), "{dir:?}'s glyph {g:?} is not ASCII");
            assert!(!seen.contains(&g), "{dir:?} shares a glyph with another");
            seen.push(g);
        }
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for dir in Direction::ALL {
            assert!(
                drawn.iter().any(|t| t == dir.glyph()),
                "{dir:?}'s glyph is not on screen"
            );
        }
    }

    #[test]
    fn the_help_sheet_names_every_control_the_game_has() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.show_help = true;
        let joined = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(joined.contains(HELP_TITLE));
        for (key, _) in HELP_ROWS {
            if !key.is_empty() {
                assert!(joined.contains(key), "the sheet does not mention {key:?}");
            }
        }
        assert!(
            joined.contains("Click anywhere to close"),
            "the sheet does not say how to shut it"
        );
    }

    #[test]
    fn the_help_sheet_is_only_drawn_when_it_is_open() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let joined = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).join(" ");
        assert!(
            !joined.contains(HELP_TITLE),
            "the help sheet is drawn over a game nobody asked it about"
        );
    }
}
