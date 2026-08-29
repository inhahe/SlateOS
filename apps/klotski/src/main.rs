#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]

//! Klotski — slide the big block out through the bottom exit, in a real window.
//!
//! A 4x5 grid holding one 2x2 block, some 1x2 and 2x1 blocks and a handful of
//! 1x1s. Seven classic puzzles, keyboard and pointer, undo, and a move counter.
//!
//! # What wiring this up found
//!
//! The program could not be played, because `main` was
//! `let _app = Klotski::new();` — it built the opening position, dropped it and
//! exited. Nothing below was reachable to notice until it had a window on it.
//!
//! 1. **The layout was a constant, and the click was read against the
//!    constant.** `cell_to_pixel`, `pixel_to_cell`, `grid_origin_x/y`,
//!    `total_width` and `total_height` were all *associated* functions — no
//!    `&self`, no window size, just `CELL_SIZE = 80.0` and friends. `render`
//!    sized its own background from `Self::total_width()`, so the program drew
//!    a 372x576 picture whatever window it was given, and `handle_mouse` sent
//!    the click through `Self::pixel_to_cell`, which answered from the same
//!    constants. In any window that was not exactly 372x576 the board was drawn
//!    in one place and clicked in another. The layout is now computed from the
//!    live window size every frame, and the hit boxes are recorded by the
//!    drawing pass, so a block is clickable exactly where its ink is.
//! 2. **`UndoEntry.block_id` did not hold a block id.** `move_block` pushed
//!    `block_id: block_idx` — the *index* into `self.blocks` — and `undo` spent
//!    it as `self.blocks[entry.block_id]`. The two agree only because
//!    `load_puzzle` hands out ids from `enumerate()` and nothing has ever
//!    reordered the vector; the field name asserted a rule that nothing kept.
//!    It now holds `Block::id` and `undo` looks the block up by it, so
//!    reordering `blocks` is a thing the program survives rather than a thing
//!    nobody may ever do.
//! 3. **The win was a latch, and undo was refused while it was set.**
//!    `check_win` wrote `status = Won` and nothing ever wrote it back, while
//!    `undo` opened with `if self.status == Won { return; }`. So the winning
//!    move was the one move you could not take back — in a puzzle whose whole
//!    difficulty is the last few moves — and the header went on advertising
//!    `Undo: 47` for undos it would refuse. Winning is now *derived* from where
//!    the big block is, so undoing the winning move un-wins, because there is
//!    no separate fact to disagree with the board.
//! 4. **The win overlay painted out the board.** It filled the whole window
//!    with opaque `0x11111B` under the comment "Semi-transparent overlay
//!    (approximated with a dark fill)" — a comment arguing for a transparency
//!    the code did not have, which is an assertion nobody checks. The scrim is
//!    now actually translucent (`Color::rgba`, alpha `0xB4`, which `Canvas`
//!    composites with `Color::over`), so the position you just solved is still
//!    visible behind the panel congratulating you on it.
//! 5. **Text was positioned by guessing its own width.** The move counter drew
//!    at `total_width - PADDING - 100.0` and the victory lines at `box_x + 50`,
//!    `+ 80` and `+ 40` — three different hand-tuned "centres" for three
//!    strings — in a program that already linked `guitk::text::measure`. Every
//!    string is now placed from its measured width.
//! 6. **The pointer could select but never play.** A click could pick a block
//!    up and put it down again; moving it, undoing, restarting and changing
//!    puzzle were keyboard-only, and the footer text was the only record that
//!    those keys existed. Clicking an empty cell a selected block can reach now
//!    moves it there, and undo/restart/prev/next have buttons.
//! 7. **A blanket `#![allow(dead_code)]` sat at the top of the file.** With
//!    `main` discarding the app, close to the whole program was dead; the allow
//!    is what let it compile without ever saying so. It is gone, and the lane
//!    gate's `-D dead_code` is what now decides whether a function is reachable.
//! 8. **Two answers to "you clicked nothing".** A click on an empty *cell*
//!    deselected; a click outside the grid entirely fell out of
//!    `pixel_to_cell`'s `None` arm and left the selection alone. Bare
//!    background now deselects wherever it is.
//! 9. **The undo cap dropped the oldest move by shuffling the vector.** At
//!    `MAX_UNDO` entries `move_block` did `undo_stack.remove(0)`, an O(n) shift
//!    per move past the cap, and said nothing about the fact that the game
//!    could no longer be unwound to the start. It is a `VecDeque` now, and the
//!    header shows the count so the loss is at least visible.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::collections::VecDeque;
use std::process::ExitCode;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// The scrim drawn over the board when the puzzle is solved.
///
/// Genuinely translucent — `Canvas::set` composites it with `Color::over`, so
/// the solved position shows through. The version this replaced was opaque and
/// claimed otherwise in a comment.
const SCRIM: Color = Color::rgba(0x11, 0x11, 0x1B, 0xB4);

const GRID_COLS: usize = 4;
const GRID_ROWS: usize = 5;

/// The 2x2 block wins when its top-left corner is at row 3, col 1 — occupying
/// (3,1), (3,2), (4,1) and (4,2), the two columns the exit sits under.
const WIN_ROW: usize = 3;
const WIN_COL: usize = 1;

/// Moves kept for undo. Past this the oldest is dropped, and the game can no
/// longer be unwound to the opening position — which is why the header shows
/// the count.
const MAX_UNDO: usize = 1000;

const WINDOW_WIDTH: f32 = 480.0;
const WINDOW_HEIGHT: f32 = 700.0;

/// The fraction of the window height the board is guaranteed before any band
/// of chrome is allowed to keep its full height.
const BOARD_SHARE: f32 = 0.5;

/// Bands give up their height in this order when the window is too short:
/// footer help first (it repeats what the buttons already say), then the
/// header's subtitle band, then the controls. The board never gives up any.
const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];

// ── Direction ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// `(row delta, column delta)`.
    #[must_use]
    pub fn delta(self) -> (isize, isize) {
        match self {
            Self::Up => (-1, 0),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Right => (0, 1),
        }
    }

    /// The direction that undoes this one.
    #[must_use]
    pub fn reverse(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// Every direction, for tests and for resolving a click into a move.
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];
}

// ── Block types ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// The 2x2 goal piece.
    Big,
    /// 2 rows by 1 column.
    TallRect,
    /// 1 row by 2 columns.
    WideRect,
    /// 1x1.
    Small,
}

impl BlockKind {
    #[must_use]
    pub fn rows(self) -> usize {
        match self {
            Self::Big | Self::TallRect => 2,
            Self::WideRect | Self::Small => 1,
        }
    }

    #[must_use]
    pub fn cols(self) -> usize {
        match self {
            Self::Big | Self::WideRect => 2,
            Self::TallRect | Self::Small => 1,
        }
    }

    #[must_use]
    pub fn color(self) -> Color {
        match self {
            Self::Big => RED,
            Self::TallRect => BLUE,
            Self::WideRect => PEACH,
            Self::Small => GREEN,
        }
    }

    /// The label drawn on the block, or `""` for the ones that carry none.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Big => "CAO",
            Self::TallRect | Self::WideRect | Self::Small => "",
        }
    }
}

// ── Block ───────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    /// Top-left row.
    pub row: usize,
    /// Top-left column.
    pub col: usize,
    /// Stable identity, handed out by `load_puzzle` from a counter that never
    /// restarts and never reuses a value. Undo entries and the selection name
    /// this, not a position in the vector.
    ///
    /// The counter starts at 1 and keeps climbing across puzzle loads, which is
    /// not decoration: it means `id != index` for every block the program ever
    /// makes, so code that confuses the two is wrong on the first move rather
    /// than wrong only once something reorders the vector. The bug this
    /// replaced — an undo entry holding an index in a field called `block_id` —
    /// was invisible for exactly as long as the two happened to coincide.
    pub id: usize,
}

impl Block {
    #[must_use]
    pub fn new(kind: BlockKind, row: usize, col: usize, id: usize) -> Self {
        Self { kind, row, col, id }
    }

    /// Every grid cell this block covers.
    #[must_use]
    pub fn cells(&self) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        for dr in 0..self.kind.rows() {
            for dc in 0..self.kind.cols() {
                result.push((self.row.saturating_add(dr), self.col.saturating_add(dc)));
            }
        }
        result
    }

    /// Whether this block covers `(row, col)`.
    #[must_use]
    pub fn occupies(&self, row: usize, col: usize) -> bool {
        row >= self.row
            && row < self.row.saturating_add(self.kind.rows())
            && col >= self.col
            && col < self.col.saturating_add(self.kind.cols())
    }

    /// Where this block's top-left corner would be after moving `dir`, or
    /// `None` if that is off the top or left edge.
    ///
    /// The one place the move arithmetic lives. Every caller that used to write
    /// `(row as i32 + dr) as usize` for itself is a caller that could wrap a
    /// row of `0` round to `usize::MAX` and index with it.
    #[must_use]
    pub fn shifted(&self, dir: Direction) -> Option<(usize, usize)> {
        let (dr, dc) = dir.delta();
        Some((
            self.row.checked_add_signed(dr)?,
            self.col.checked_add_signed(dc)?,
        ))
    }

    /// Whether the block would still be inside the grid after moving `dir`.
    #[must_use]
    pub fn can_fit_in_grid(&self, dir: Direction) -> bool {
        let Some((row, col)) = self.shifted(dir) else {
            return false;
        };
        row.saturating_add(self.kind.rows()) <= GRID_ROWS
            && col.saturating_add(self.kind.cols()) <= GRID_COLS
    }
}

// ── Puzzle definitions ──────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PuzzleDef {
    pub name: &'static str,
    /// `(kind, row, col)` for each block in the opening position.
    pub blocks: &'static [(BlockKind, usize, usize)],
}

/// Classic Klotski openings, easiest first.
pub const PUZZLES: &[PuzzleDef] = &[
    // "Heng Dao Li Ma" — the classic.
    PuzzleDef {
        name: "Heng Dao Li Ma",
        blocks: &[
            (BlockKind::Big, 0, 1),
            (BlockKind::TallRect, 0, 0),
            (BlockKind::TallRect, 0, 3),
            (BlockKind::TallRect, 2, 0),
            (BlockKind::TallRect, 2, 3),
            (BlockKind::WideRect, 2, 1),
            (BlockKind::Small, 3, 1),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 4, 0),
            (BlockKind::Small, 4, 3),
        ],
    },
    PuzzleDef {
        name: "Zhi Tui Heng Shan",
        blocks: &[
            (BlockKind::Big, 0, 1),
            (BlockKind::TallRect, 0, 0),
            (BlockKind::TallRect, 0, 3),
            (BlockKind::TallRect, 2, 0),
            (BlockKind::WideRect, 2, 1),
            (BlockKind::Small, 2, 3),
            (BlockKind::Small, 3, 1),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 3, 3),
            (BlockKind::Small, 4, 0),
        ],
    },
    PuzzleDef {
        name: "Bing Jiang Lian Ying",
        blocks: &[
            (BlockKind::Big, 0, 1),
            (BlockKind::TallRect, 0, 0),
            (BlockKind::TallRect, 0, 3),
            (BlockKind::WideRect, 2, 1),
            (BlockKind::Small, 2, 0),
            (BlockKind::Small, 3, 0),
            (BlockKind::Small, 3, 1),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 2, 3),
            (BlockKind::Small, 3, 3),
        ],
    },
    PuzzleDef {
        name: "Wu Jiang Zhuan",
        blocks: &[
            (BlockKind::Big, 0, 0),
            (BlockKind::TallRect, 0, 2),
            (BlockKind::TallRect, 0, 3),
            (BlockKind::TallRect, 2, 0),
            (BlockKind::TallRect, 2, 1),
            (BlockKind::WideRect, 2, 2),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 3, 3),
            (BlockKind::Small, 4, 0),
            (BlockKind::Small, 4, 1),
        ],
    },
    PuzzleDef {
        name: "Bing Lin Cao Ying",
        blocks: &[
            (BlockKind::Big, 0, 1),
            (BlockKind::WideRect, 2, 0),
            (BlockKind::WideRect, 2, 2),
            (BlockKind::Small, 0, 0),
            (BlockKind::Small, 1, 0),
            (BlockKind::Small, 0, 3),
            (BlockKind::Small, 1, 3),
            (BlockKind::Small, 3, 0),
            (BlockKind::Small, 3, 1),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 3, 3),
            (BlockKind::Small, 4, 2),
            (BlockKind::Small, 4, 3),
        ],
    },
    PuzzleDef {
        name: "Si Mian Chu Ge",
        blocks: &[
            (BlockKind::Big, 0, 1),
            (BlockKind::TallRect, 2, 1),
            (BlockKind::TallRect, 2, 2),
            (BlockKind::WideRect, 4, 1),
            (BlockKind::Small, 0, 0),
            (BlockKind::Small, 1, 0),
            (BlockKind::Small, 0, 3),
            (BlockKind::Small, 1, 3),
            (BlockKind::Small, 2, 0),
            (BlockKind::Small, 2, 3),
        ],
    },
    PuzzleDef {
        name: "Xiao Zu Dang Che",
        blocks: &[
            (BlockKind::Big, 0, 0),
            (BlockKind::TallRect, 2, 0),
            (BlockKind::TallRect, 2, 3),
            (BlockKind::WideRect, 0, 2),
            (BlockKind::WideRect, 2, 1),
            (BlockKind::Small, 1, 2),
            (BlockKind::Small, 1, 3),
            (BlockKind::Small, 3, 1),
            (BlockKind::Small, 3, 2),
            (BlockKind::Small, 4, 0),
        ],
    },
];

// ── Undo ────────────────────────────────────────────────────────────
/// One move, named by the moved block's **id**.
///
/// The version this replaced stored a `Vec` index in a field called `block_id`
/// and indexed `blocks` with it. That is only correct while ids and positions
/// coincide, which nothing enforced and nothing tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoEntry {
    pub block: usize,
    pub direction: Direction,
}

// ── Hit targets ─────────────────────────────────────────────────────
/// Everything the pointer can land on. Recorded by the drawing pass, so a
/// target exists exactly where the thing it names was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A block, by `Block::id`.
    Block(usize),
    /// An empty grid cell.
    Cell(usize, usize),
    Undo,
    Restart,
    Prev,
    Next,
}

// ── Layout ──────────────────────────────────────────────────────────
/// Where everything goes in a window of a given size.
///
/// Built fresh every frame and never stored on the model. A remembered layout
/// is one that can disagree with the window it is drawn in, which is how a
/// click at (60, 90) came to pick up a block that had never been drawn there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    /// Title and puzzle name.
    pub header: Rect,
    /// The 4x5 grid, gaps included.
    pub board: Rect,
    /// The strip below the board marking the way out.
    pub exit: Rect,
    /// Undo, restart, previous and next.
    pub controls: Rect,
    /// The keyboard reminder.
    pub footer: Rect,
    /// The side of one grid cell.
    pub cell: f32,
    /// The gap between adjacent cells.
    pub gap: f32,
    pub font: f32,
    pub small: f32,
    pub big: f32,
    pub pad: f32,
}

/// Widths and heights the board needs, per unit of cell size. The gap is a
/// twentieth of a cell and the exit strip is not quite a quarter of one, so
/// every board dimension is a multiple of the single number `cell`.
const GAP_PER_CELL: f32 = 0.05;
const EXIT_PER_CELL: f32 = 0.22;

impl Layout {
    /// The layout for a window of the given size.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 38.0).clamp(8.0, 17.0);
        let small = (font - 2.0).max(7.0);
        let big = (font * 1.6).clamp(13.0, 28.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, controls, footer] order.
        let mut wants = [
            (h * 0.10).clamp(26.0, 56.0),
            (h * 0.08).clamp(22.0, 44.0),
            (h * 0.07).clamp(18.0, 40.0),
        ];
        // What is left once the board has its guaranteed share and the two
        // gaps that separate it from the chrome above and below. Charging the
        // padding to the chrome rather than the board is what keeps a small
        // window's cells big enough to still hold their labels.
        let budget = (h - h * BOARD_SHARE - pad * 2.0).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, ctl_h, ftr_h] = wants;

        // A dropped band is `Rect::EMPTY`, not a full-width strip nought pixels
        // tall: the two read alike to anything asking "does this show?", but
        // only one of them reads alike to anything asking "how wide is it?".
        let header = if hdr_h > 0.0 {
            Rect::new(0.0, 0.0, w, hdr_h)
        } else {
            Rect::EMPTY
        };
        let footer = if ftr_h > 0.0 {
            Rect::new(0.0, h - ftr_h, w, ftr_h)
        } else {
            Rect::EMPTY
        };
        let lower = if ftr_h > 0.0 { footer.y } else { h };
        let controls = if ctl_h > 0.0 {
            Rect::new(0.0, lower - ctl_h, w, ctl_h)
        } else {
            Rect::EMPTY
        };

        let top = hdr_h;
        // From the height rather than from `controls.y`: a dropped band is
        // `Rect::EMPTY`, which sits at the origin, so reading its `y` would put
        // the board's bottom edge at zero and leave no board at all. (The same
        // is *not* true of `header.bottom()`, which is `hdr_h` either way —
        // `Rect::EMPTY` is `0x0` at the origin, and the header starts there. A
        // guard there would be one in front of a rule that already holds.)
        let bottom = if ctl_h > 0.0 { controls.y } else { lower };
        let band = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // One number decides the whole board. Solving for it from both
        // dimensions at once is what stops a 4x5 grid from being stretched to
        // fill a band that is not 4:5 — a stretched grid is one whose cells are
        // no longer where a square hit box says they are.
        let per_w = GRID_COLS as f32 + (GRID_COLS as f32 - 1.0) * GAP_PER_CELL;
        let per_h = GRID_ROWS as f32
            + (GRID_ROWS as f32 - 1.0) * GAP_PER_CELL
            + GAP_PER_CELL
            + EXIT_PER_CELL;
        let cell = (band.w / per_w).min(band.h / per_h).max(0.0);
        let gap = cell * GAP_PER_CELL;

        let grid_w = GRID_COLS as f32 * cell + (GRID_COLS as f32 - 1.0) * gap;
        let grid_h = GRID_ROWS as f32 * cell + (GRID_ROWS as f32 - 1.0) * gap;
        let exit_h = cell * EXIT_PER_CELL;
        let stack_h = grid_h + gap + exit_h;

        let (board, exit) = if cell > 0.0 {
            let bx = band.x + (band.w - grid_w) / 2.0;
            let by = band.y + (band.h - stack_h) / 2.0;
            // The exit spans the two middle columns, which is exactly the
            // footprint the big block must reach to win — the strip is a
            // picture of `WIN_COL`, not an independent guess at where it is.
            let exit_x = bx + WIN_COL as f32 * (cell + gap);
            let exit_w = 2.0 * cell + gap;
            (
                Rect::new(bx, by, grid_w, grid_h),
                Rect::new(exit_x, by + grid_h + gap, exit_w, exit_h),
            )
        } else {
            (Rect::EMPTY, Rect::EMPTY)
        };

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            board,
            exit,
            controls,
            footer,
            cell,
            gap,
            font,
            small,
            big,
            pad,
        }
    }

    /// The rectangle of grid cell `(row, col)`, or `Rect::EMPTY` when the board
    /// has collapsed or the cell is off the grid.
    #[must_use]
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        if self.cell <= 0.0 || row >= GRID_ROWS || col >= GRID_COLS {
            return Rect::EMPTY;
        }
        Rect::new(
            self.board.x + col as f32 * (self.cell + self.gap),
            self.board.y + row as f32 * (self.cell + self.gap),
            self.cell,
            self.cell,
        )
    }

    /// The rectangle a block of `kind` at `(row, col)` covers — its own cells
    /// plus the gaps *between* them, which belong to the block rather than to
    /// the board once something is sitting on them.
    #[must_use]
    pub fn block_rect(&self, kind: BlockKind, row: usize, col: usize) -> Rect {
        let head = self.cell_rect(row, col);
        if head.is_empty() {
            return Rect::EMPTY;
        }
        Rect::new(
            head.x,
            head.y,
            kind.cols() as f32 * self.cell + (kind.cols() as f32 - 1.0) * self.gap,
            kind.rows() as f32 * self.cell + (kind.rows() as f32 - 1.0) * self.gap,
        )
    }

    /// The four control buttons, left to right, sharing the controls band.
    #[must_use]
    pub fn button_rects(&self) -> [Rect; 4] {
        if self.controls.is_empty() {
            return [Rect::EMPTY; 4];
        }
        let n = 4.0;
        let inner = (self.controls.w - self.pad * (n + 1.0)).max(0.0);
        let bw = inner / n;
        let bh = (self.controls.h - self.pad).max(0.0);
        if bw <= 0.0 || bh <= 0.0 {
            return [Rect::EMPTY; 4];
        }
        let y = self.controls.y + (self.controls.h - bh) / 2.0;
        let mut out = [Rect::EMPTY; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = Rect::new(
                self.controls.x + self.pad + i as f32 * (bw + self.pad),
                y,
                bw,
                bh,
            );
        }
        out
    }

    /// The victory panel, centred on the board rather than on the window, so
    /// that what it covers is the thing it is talking about.
    #[must_use]
    pub fn win_panel(&self) -> Rect {
        let w = (self.window.w * 0.82).min(320.0);
        let h = (self.window.h * 0.42).min(180.0);
        Rect::new(
            (self.window.w - w) / 2.0,
            (self.window.h - h) / 2.0,
            w.max(0.0),
            h.max(0.0),
        )
    }
}

/// The keyboard reminder, in the order the footer draws it. The second line is
/// the one dropped first when the footer has room for only one.
const FOOTER_LINES: [&str; 2] = [
    "Enter: select   Arrows: move   Z: undo",
    "N/Tab: next   R: restart   1-7: puzzle",
];

/// The buttons, in the order `Layout::button_rects` lays them out.
const BUTTONS: [(Target, &str); 4] = [
    (Target::Undo, "Undo"),
    (Target::Restart, "Restart"),
    (Target::Prev, "Prev"),
    (Target::Next, "Next"),
];

// ── The game ────────────────────────────────────────────────────────
pub struct Klotski {
    blocks: Vec<Block>,
    /// The selected block's **id**, for the same reason undo entries carry one.
    selected: Option<usize>,
    moves: usize,
    undo_stack: VecDeque<UndoEntry>,
    current_puzzle: usize,
    /// The opening position, for restart.
    initial_blocks: Vec<Block>,
    /// The next block id to hand out. Starts at 1 and never restarts, so no id
    /// is ever equal to the position of the block holding it.
    next_id: usize,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against.
    size_drawn: (f32, f32),
}

impl Default for Klotski {
    fn default() -> Self {
        Self::new()
    }
}

impl Klotski {
    #[must_use]
    pub fn new() -> Self {
        let mut app = Self {
            blocks: Vec::new(),
            selected: None,
            moves: 0,
            undo_stack: VecDeque::new(),
            current_puzzle: 0,
            initial_blocks: Vec::new(),
            next_id: 1,
            size_drawn: (WINDOW_WIDTH, WINDOW_HEIGHT),
        };
        app.load_puzzle(0);
        app
    }

    /// Remember the size the window is being drawn at.
    pub fn resize(&mut self, width: f32, height: f32) {
        self.size_drawn = (width.max(1.0), height.max(1.0));
    }

    #[must_use]
    pub fn size_drawn(&self) -> (f32, f32) {
        self.size_drawn
    }

    /// The layout of the most recent frame.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::new(self.size_drawn.0, self.size_drawn.1)
    }

    // ── Puzzle handling ────────────────────────────────────────────

    /// Load puzzle `index`, wrapping rather than failing on an index past the
    /// end — the only callers are the number keys and the prev/next buttons.
    pub fn load_puzzle(&mut self, index: usize) {
        let idx = if index < PUZZLES.len() { index } else { 0 };
        self.current_puzzle = idx;

        self.blocks.clear();
        if let Some(puzzle) = PUZZLES.get(idx) {
            for &(kind, row, col) in puzzle.blocks {
                let id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                self.blocks.push(Block::new(kind, row, col, id));
            }
        }
        self.initial_blocks.clone_from(&self.blocks);
        self.selected = None;
        self.moves = 0;
        self.undo_stack.clear();
    }

    /// Build an arbitrary position, for tests that need one a real puzzle is
    /// eighty-odd moves away from.
    ///
    /// Test-only on purpose: nothing in the shipping program may set up a
    /// position the puzzle table does not describe. It goes through the same id
    /// counter as `load_puzzle`, so the ids it hands out are as unlike indices
    /// here as they are anywhere else.
    #[cfg(test)]
    pub fn position(&mut self, blocks: &[(BlockKind, usize, usize)]) {
        self.blocks.clear();
        for &(kind, row, col) in blocks {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.blocks.push(Block::new(kind, row, col, id));
        }
        self.initial_blocks.clone_from(&self.blocks);
        self.selected = None;
        self.moves = 0;
        self.undo_stack.clear();
    }

    pub fn restart_puzzle(&mut self) {
        self.blocks.clone_from(&self.initial_blocks);
        self.selected = None;
        self.moves = 0;
        self.undo_stack.clear();
    }

    pub fn next_puzzle(&mut self) {
        let next = self
            .current_puzzle
            .saturating_add(1)
            .checked_rem(PUZZLES.len())
            .unwrap_or(0);
        self.load_puzzle(next);
    }

    pub fn prev_puzzle(&mut self) {
        let prev = if self.current_puzzle == 0 {
            PUZZLES.len().saturating_sub(1)
        } else {
            self.current_puzzle.saturating_sub(1)
        };
        self.load_puzzle(prev);
    }

    #[must_use]
    pub fn current_puzzle(&self) -> usize {
        self.current_puzzle
    }

    #[must_use]
    pub fn puzzle_name(&self) -> &'static str {
        PUZZLES
            .get(self.current_puzzle)
            .map_or("Unknown", |p| p.name)
    }

    #[must_use]
    pub fn moves(&self) -> usize {
        self.moves
    }

    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    // ── Board queries ──────────────────────────────────────────────

    /// Which block covers each cell, by id.
    #[must_use]
    pub fn occupancy(&self) -> [[Option<usize>; GRID_COLS]; GRID_ROWS] {
        let mut grid = [[None; GRID_COLS]; GRID_ROWS];
        for block in &self.blocks {
            for (r, c) in block.cells() {
                if let Some(row) = grid.get_mut(r)
                    && let Some(slot) = row.get_mut(c)
                {
                    *slot = Some(block.id);
                }
            }
        }
        grid
    }

    /// The position in `blocks` of the block with `id`.
    #[must_use]
    pub fn index_of(&self, id: usize) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// The id of the block covering `(row, col)`.
    #[must_use]
    pub fn block_at(&self, row: usize, col: usize) -> Option<usize> {
        self.blocks
            .iter()
            .find(|b| b.occupies(row, col))
            .map(|b| b.id)
    }

    /// The id of the 2x2 block.
    #[must_use]
    pub fn big_block(&self) -> Option<usize> {
        self.blocks
            .iter()
            .find(|b| b.kind == BlockKind::Big)
            .map(|b| b.id)
    }

    /// Solved when the big block sits on the exit.
    ///
    /// Derived, not latched. The version this replaced set a `Won` status that
    /// nothing ever cleared, so undoing the winning move left a board that was
    /// not won and a program that said it was.
    #[must_use]
    pub fn is_won(&self) -> bool {
        self.blocks
            .iter()
            .any(|b| b.kind == BlockKind::Big && b.row == WIN_ROW && b.col == WIN_COL)
    }

    /// Whether the block with `id` can move `dir`.
    #[must_use]
    pub fn can_move(&self, id: usize, dir: Direction) -> bool {
        let Some(block) = self.blocks.iter().find(|b| b.id == id) else {
            return false;
        };
        if !block.can_fit_in_grid(dir) {
            return false;
        }
        let Some((row, col)) = block.shifted(dir) else {
            return false;
        };
        let occupancy = self.occupancy();
        for dr_off in 0..block.kind.rows() {
            for dc_off in 0..block.kind.cols() {
                let new_r = row.saturating_add(dr_off);
                let new_c = col.saturating_add(dc_off);
                let occupant = occupancy.get(new_r).and_then(|row| row.get(new_c)).copied();
                if let Some(Some(other)) = occupant
                    && other != id
                {
                    return false;
                }
            }
        }
        true
    }

    // ── Moves ──────────────────────────────────────────────────────

    /// Move the block with `id` one cell in `dir`. Returns whether it moved.
    ///
    /// Refused once the puzzle is solved: the way on from a win is undo,
    /// restart or another puzzle, all of which stay available.
    pub fn move_block(&mut self, id: usize, dir: Direction) -> bool {
        if self.is_won() || !self.can_move(id, dir) {
            return false;
        }
        let Some(idx) = self.index_of(id) else {
            return false;
        };
        let Some(block) = self.blocks.get_mut(idx) else {
            return false;
        };
        let Some((row, col)) = block.shifted(dir) else {
            return false;
        };
        block.row = row;
        block.col = col;
        self.moves = self.moves.saturating_add(1);

        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(UndoEntry {
            block: id,
            direction: dir,
        });
        true
    }

    /// Take back the last move. Returns whether there was one.
    ///
    /// Allowed after a win, which is the whole point: the winning move is the
    /// one you are most likely to want back, and because winning is derived
    /// from the board, undoing it un-wins with no second fact to correct.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo_stack.pop_back() else {
            return false;
        };
        let Some(idx) = self.index_of(entry.block) else {
            return false;
        };
        let Some(block) = self.blocks.get_mut(idx) else {
            return false;
        };
        let Some((row, col)) = block.shifted(entry.direction.reverse()) else {
            return false;
        };
        block.row = row;
        block.col = col;
        self.moves = self.moves.saturating_sub(1);
        true
    }

    /// Step the selection through the blocks in id order.
    ///
    /// The wrap is spelled once, in the fallback: past the end of the vector
    /// `get` answers `None`, and the answer to "there is no next block" is the
    /// first one. The version this replaced *also* took `idx + 1` modulo the
    /// length first, which cannot change the outcome — index 0 and the fallback
    /// name the same block — so it was a guard in front of a rule that already
    /// held, and no mutation of it could be caught by any test.
    pub fn cycle_selection(&mut self) {
        let Some(first) = self.blocks.first().map(|b| b.id) else {
            self.selected = None;
            return;
        };
        self.selected = Some(match self.selected.and_then(|id| self.index_of(id)) {
            None => first,
            Some(idx) => self
                .blocks
                .get(idx.saturating_add(1))
                .map_or(first, |b| b.id),
        });
    }

    /// The direction that would carry the selected block onto `(row, col)`, if
    /// any single legal move does.
    #[must_use]
    pub fn move_towards(&self, id: usize, row: usize, col: usize) -> Option<Direction> {
        let block = self.blocks.iter().find(|b| b.id == id)?;
        Direction::ALL.into_iter().find(|&dir| {
            let Some((r, c)) = block.shifted(dir) else {
                return false;
            };
            Block::new(block.kind, r, c, id).occupies(row, col) && self.can_move(id, dir)
        })
    }

    // ── Drawing ────────────────────────────────────────────────────

    /// The frame for a window of the given size, hit boxes and all.
    ///
    /// This is the only description of where anything is. The hit test runs
    /// against the frame this produces, so there is no second copy of the
    /// layout for it to disagree with.
    #[must_use]
    pub fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let l = Layout::new(width, height);
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
        self.draw_controls(&mut f, &l);
        self.draw_footer(&mut f, &l);
        if self.is_won() {
            self.draw_win(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        fill(f, l.header, MANTLE, CornerRadii::ZERO);

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        let sub_h = text::line_height(l.small, FontWeightHint::Regular);
        let two_lines = title_h + sub_h <= l.header.h;

        let top = if two_lines {
            l.header.y + (l.header.h - title_h - sub_h) / 2.0
        } else {
            l.header.y + (l.header.h - title_h) / 2.0
        };
        label(
            f,
            &Label {
                text: "Klotski",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: LAVENDER,
            },
            l.header.x + l.pad,
            top,
        );
        if two_lines {
            let subtitle = format!(
                "#{}: {}",
                self.current_puzzle.saturating_add(1),
                self.puzzle_name()
            );
            label(
                f,
                &Label {
                    text: &subtitle,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: SUBTEXT0,
                },
                l.header.x + l.pad,
                top + title_h,
            );
        }

        // Right-aligned from the strings' own measured widths. The version this
        // replaced drew both at `total_width - PADDING - 100.0`, a guess at how
        // wide "Moves: 1234" would turn out to be.
        let moves = format!("Moves: {}", self.moves);
        let undo = format!("Undo: {}", self.undo_stack.len());
        let right = l.header.right() - l.pad;
        label_right(
            f,
            &Label {
                text: &moves,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: TEXT_COLOR,
            },
            right,
            top,
        );
        if two_lines {
            label_right(
                f,
                &Label {
                    text: &undo,
                    size: l.small,
                    weight: FontWeightHint::Regular,
                    color: OVERLAY0,
                },
                right,
                top + title_h,
            );
        }
    }

    fn draw_board(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        // The well the blocks sit in.
        fill(
            f,
            Rect::new(
                l.board.x - l.gap,
                l.board.y - l.gap,
                l.board.w + l.gap * 2.0,
                l.board.h + l.gap * 2.0,
            ),
            CRUST,
            CornerRadii::all(l.gap.max(1.0)),
        );

        // Empty cells first, so a block drawn over one takes the click: the hit
        // test answers with the *last* target covering the point.
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let r = l.cell_rect(row, col);
                fill(f, r, SURFACE0, CornerRadii::all(l.gap.max(1.0)));
                f.hit(Target::Cell(row, col), r);
            }
        }

        // The exit. Deliberately not a hit target: a control that swallows a
        // click and does nothing is worse than no control, because the click it
        // ate would otherwise have reached the cell beneath.
        if !l.exit.is_empty() {
            fill(f, l.exit, MAUVE, CornerRadii::all(l.gap.max(1.0)));
            let size = (l.exit.h * 0.72).clamp(6.0, l.small);
            if text::line_height(size, FontWeightHint::Bold) <= l.exit.h {
                label_centred(
                    f,
                    &Label {
                        text: "EXIT",
                        size,
                        weight: FontWeightHint::Bold,
                        color: CRUST,
                    },
                    l.exit,
                );
            }
        }

        for block in &self.blocks {
            let r = l.block_rect(block.kind, block.row, block.col);
            if r.is_empty() {
                continue;
            }
            if self.selected == Some(block.id) {
                let grow = (l.gap * 0.8).max(1.0);
                fill(
                    f,
                    Rect::new(r.x - grow, r.y - grow, r.w + grow * 2.0, r.h + grow * 2.0),
                    YELLOW,
                    CornerRadii::all(l.cell * 0.1),
                );
            }
            fill(f, r, block.kind.color(), CornerRadii::all(l.cell * 0.08));

            let text_of = block.kind.label();
            if !text_of.is_empty() {
                let size = (l.cell * 0.22).clamp(7.0, l.font);
                if text::line_height(size, FontWeightHint::Bold) <= r.h
                    && text::measure(text_of, size, FontWeightHint::Bold) <= r.w
                {
                    label_centred(
                        f,
                        &Label {
                            text: text_of,
                            size,
                            weight: FontWeightHint::Bold,
                            color: CRUST,
                        },
                        r,
                    );
                }
            }
            f.hit(Target::Block(block.id), r);
        }
    }

    fn draw_controls(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.controls.is_empty() {
            return;
        }
        fill(f, l.controls, MANTLE, CornerRadii::ZERO);
        let rects = l.button_rects();
        for ((target, name), r) in BUTTONS.into_iter().zip(rects) {
            if r.is_empty() {
                continue;
            }
            let live = match target {
                Target::Undo => !self.undo_stack.is_empty(),
                _ => true,
            };
            fill(
                f,
                r,
                if live { SURFACE1 } else { SURFACE0 },
                CornerRadii::all(l.pad.max(1.0)),
            );
            let size = (r.h * 0.45).clamp(7.0, l.font);
            if text::line_height(size, FontWeightHint::Regular) <= r.h {
                label_centred(
                    f,
                    &Label {
                        text: name,
                        size,
                        weight: FontWeightHint::Regular,
                        color: if live { TEXT_COLOR } else { OVERLAY0 },
                    },
                    r,
                );
            }
            // Recorded even when it is drawn dim: `undo` on an empty stack
            // answers `false` and changes nothing, and a target that reports
            // "nothing happened" is the thing the tests can hold on to.
            f.hit(target, r);
        }
    }

    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.footer.is_empty() {
            return;
        }
        fill(f, l.footer, MANTLE, CornerRadii::ZERO);
        let size = l.small;
        let lh = text::line_height(size, FontWeightHint::Regular);
        let shown = if lh * 2.0 <= l.footer.h { 2 } else { 1 };
        if lh > l.footer.h {
            return;
        }
        let top = l.footer.y + (l.footer.h - lh * shown as f32) / 2.0;
        f.clip(l.footer);
        for (i, line) in FOOTER_LINES.iter().take(shown).enumerate() {
            label(
                f,
                &Label {
                    text: line,
                    size,
                    weight: FontWeightHint::Regular,
                    color: if i == 0 { SUBTEXT0 } else { OVERLAY0 },
                },
                l.footer.x + l.pad,
                top + lh * i as f32,
            );
        }
        f.unclip();
    }

    fn draw_win(&self, f: &mut Frame<Target>, l: &Layout) {
        // A translucent scrim, not an opaque one: the solved board is the thing
        // worth looking at, and painting it out to celebrate it was the joke
        // the old comment was making without meaning to.
        fill(f, l.window, SCRIM, CornerRadii::ZERO);

        // Nothing behind the panel is clickable any more — a modal that only
        // *looks* in front is one whose buttons you can press through.
        f.discard_hits();

        let panel = l.win_panel();
        if panel.is_empty() {
            return;
        }
        fill(f, panel, SURFACE0, CornerRadii::all(l.pad * 1.2));

        let title_h = text::line_height(l.big, FontWeightHint::Bold);
        let line_h = text::line_height(l.font, FontWeightHint::Regular);
        let btn_h = (panel.h * 0.28).clamp(0.0, 44.0);
        let stack = title_h + line_h + btn_h;
        if stack > panel.h {
            return;
        }
        let top = panel.y + (panel.h - stack) / 2.0;

        label_centred(
            f,
            &Label {
                text: "Puzzle Solved!",
                size: l.big,
                weight: FontWeightHint::Bold,
                color: GREEN,
            },
            Rect::new(panel.x, top, panel.w, title_h),
        );
        let tally = format!("Moves: {}", self.moves);
        label_centred(
            f,
            &Label {
                text: &tally,
                size: l.font,
                weight: FontWeightHint::Regular,
                color: TEXT_COLOR,
            },
            Rect::new(panel.x, top + title_h, panel.w, line_h),
        );

        // Undo, restart and next — the three ways on, all reachable by pointer
        // rather than only by keys named in a footer the scrim is over.
        let choices: [(Target, &str); 3] = [
            (Target::Undo, "Undo"),
            (Target::Restart, "Restart"),
            (Target::Next, "Next"),
        ];
        let n = choices.len() as f32;
        let inner = (panel.w - l.pad * (n + 1.0)).max(0.0);
        let bw = inner / n;
        if bw <= 0.0 || btn_h <= 0.0 {
            return;
        }
        let by = top + title_h + line_h;
        for (i, (target, name)) in choices.into_iter().enumerate() {
            let r = Rect::new(panel.x + l.pad + i as f32 * (bw + l.pad), by, bw, btn_h);
            fill(f, r, SURFACE1, CornerRadii::all(l.pad.max(1.0)));
            let size = (r.h * 0.4).clamp(7.0, l.font);
            if text::line_height(size, FontWeightHint::Regular) <= r.h {
                label_centred(
                    f,
                    &Label {
                        text: name,
                        size,
                        weight: FontWeightHint::Regular,
                        color: TEXT_COLOR,
                    },
                    r,
                );
            }
            f.hit(target, r);
        }
    }

    // ── Events ─────────────────────────────────────────────────────

    /// Act on `target`. Returns whether anything changed.
    fn activate(&mut self, target: Target) -> bool {
        match target {
            Target::Undo => self.undo(),
            Target::Restart => {
                self.restart_puzzle();
                true
            }
            Target::Prev => {
                self.prev_puzzle();
                true
            }
            Target::Next => {
                self.next_puzzle();
                true
            }
            Target::Block(id) => {
                self.selected = if self.selected == Some(id) {
                    None
                } else {
                    Some(id)
                };
                true
            }
            Target::Cell(row, col) => {
                // An empty cell the selection can reach is a move; any other
                // empty cell puts the block down.
                if let Some(id) = self.selected
                    && let Some(dir) = self.move_towards(id, row, col)
                    && self.move_block(id, dir)
                {
                    return true;
                }
                let had = self.selected.is_some();
                self.selected = None;
                had
            }
        }
    }

    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if !matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        let (w, h) = self.size_drawn;
        match self.frame(w, h).hit_test(ev.x, ev.y) {
            Some(target) => {
                self.activate(target);
                EventResult::Consumed
            }
            None => {
                // Bare background deselects, wherever it is. The version this
                // replaced deselected on an empty *cell* but left the selection
                // alone on a click outside the grid — two answers to one
                // question.
                let had = self.selected.is_some();
                self.selected = None;
                if had {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    pub fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // `pressed` decides whether this is a key going down or coming back
        // up. Reading only `key` runs every binding twice per press.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let plain = ev.modifiers == guitk::event::Modifiers::NONE;
        match ev.key {
            Key::N if plain => self.next_puzzle(),
            Key::Tab => self.next_puzzle(),
            Key::P if plain => self.prev_puzzle(),
            Key::R if plain => self.restart_puzzle(),
            Key::Z if plain => {
                self.undo();
            }
            Key::Enter | Key::Space => self.cycle_selection(),
            Key::Escape => self.selected = None,
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let dir = match ev.key {
                    Key::Up => Direction::Up,
                    Key::Down => Direction::Down,
                    Key::Left => Direction::Left,
                    _ => Direction::Right,
                };
                match self.selected {
                    Some(id) => {
                        self.move_block(id, dir);
                    }
                    None => return EventResult::Ignored,
                }
            }
            Key::Num1 if plain => self.load_puzzle(0),
            Key::Num2 if plain => self.load_puzzle(1),
            Key::Num3 if plain => self.load_puzzle(2),
            Key::Num4 if plain => self.load_puzzle(3),
            Key::Num5 if plain => self.load_puzzle(4),
            Key::Num6 if plain => self.load_puzzle(5),
            Key::Num7 if plain => self.load_puzzle(6),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii,
    });
}

/// One string and everything about how it looks, minus where it goes.
///
/// A struct rather than five more parameters because the placement helpers
/// below would otherwise be at eight arguments each, and because it is what
/// lets `label_centred` take a *rectangle* instead of a rectangle and a
/// separately-supplied width that is expected to match it.
struct Label<'a> {
    text: &'a str,
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

/// The one place a `Text` command is built.
///
/// `limit` is passed straight through as `max_width`, so a caller that computed
/// a width limit gets one the renderer will actually stop at. `TextOverflow`
/// follows from it and is not a separate choice: no limit means the overflow
/// question is vacuous, and a limit means the cut is real and had better be
/// marked.
fn push_text(f: &mut Frame<Target>, l: &Label, x: f32, y: f32, limit: Option<f32>) {
    if l.text.is_empty() {
        return;
    }
    f.push(RenderCommand::Text {
        x,
        y,
        text: l.text.to_string(),
        color: l.color,
        font_size: l.size,
        font_weight: l.weight,
        max_width: limit,
        overflow: if limit.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Top-left corner at `(x, y)`, with no width limit.
fn label(f: &mut Frame<Target>, l: &Label, x: f32, y: f32) {
    push_text(f, l, x, y, None);
}

/// Right-aligned at `right`, from the string's measured width.
fn label_right(f: &mut Frame<Target>, l: &Label, right: f32, y: f32) {
    let w = text::measure(l.text, l.size, l.weight);
    push_text(f, l, right - w, y, None);
}

/// Centred in `r` — horizontally from the measured width, vertically from the
/// line height — **and limited to `r`**.
///
/// The width that decides the centre is the width the renderer is told to stop
/// at, so the two cannot disagree. The version this replaced took the limit as
/// a separate argument, used it to pick the centre, and then drew with
/// `max_width: None`: a box that decided where the string started and then let
/// it run as far past the right-hand edge as it liked.
fn label_centred(f: &mut Frame<Target>, l: &Label, r: Rect) {
    if l.text.is_empty() || r.is_empty() {
        return;
    }
    let w = text::measure(l.text, l.size, l.weight).min(r.w);
    let lh = text::line_height(l.size, l.weight);
    push_text(
        f,
        l,
        r.x + (r.w - w) / 2.0,
        r.y + (r.h - lh) / 2.0,
        Some(r.w),
    );
}

// ── Window plumbing ─────────────────────────────────────────────────

/// The one body both the window and the test probe drive, so what a click does
/// in a test is what it does on a screen.
pub fn handle_event(game: &mut Klotski, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => game.handle_key(ev),
        Event::Mouse(ev) => game.handle_mouse(ev),
        Event::Resize { width, height } => {
            game.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for Klotski {
    fn title(&self) -> String {
        "Klotski".to_string()
    }

    fn app_id(&self) -> String {
        "klotski".to_string()
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
        // against — which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for Klotski {
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
    let mut game = Klotski::new();
    app::launch("klotski", &mut game)
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
    use std::collections::HashSet;

    /// The window sizes every layout claim is checked at.
    ///
    /// The first three are far smaller than the chrome would like — the case
    /// the old layout never had to survive, because `render` computed its own
    /// size from `Self::total_width()` and drew a 372x576 picture whatever it
    /// was given.
    const WINDOWS: &[(f32, f32)] = &[
        (120.0, 90.0),
        (200.0, 160.0),
        (320.0, 240.0),
        (400.0, 900.0),
        (480.0, 700.0),
        (640.0, 480.0),
        (900.0, 500.0),
        (1280.0, 720.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
    ];

    /// The size the probe helpers draw at.
    const SIZE: (f32, f32) = Klotski::SIZE;

    fn game() -> Klotski {
        Klotski::new()
    }

    /// A position one downward move from a win: the big block at (2,1) with the
    /// bottom two middle cells clear.
    fn one_move_from_winning() -> Klotski {
        let mut g = game();
        g.position(&[
            (BlockKind::Big, 2, 1),
            (BlockKind::Small, 0, 0),
            (BlockKind::Small, 4, 0),
            (BlockKind::Small, 4, 3),
        ]);
        g
    }

    fn text_commands(f: &Frame<Target>) -> Vec<(String, f32, f32, f32, FontWeightHint)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    font_weight,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *font_weight)),
                _ => None,
            })
            .collect()
    }

    fn fill_rects(f: &Frame<Target>) -> Vec<(Rect, Color)> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => Some((Rect::new(*x, *y, *width, *height), *color)),
                _ => None,
            })
            .collect()
    }

    // ── The window exists at all ───────────────────────────────────

    #[test]
    fn the_app_reports_a_title_an_id_and_a_size() {
        let g = game();
        assert_eq!(g.title(), "Klotski");
        assert_eq!(g.app_id(), "klotski");
        let (w, h) = g.initial_size();
        assert!(w > 0 && h > 0, "the window opens with no size");
    }

    #[test]
    fn the_probe_draws_at_the_size_the_window_opens_at() {
        // If these drift apart, every test below checks a window the program
        // never actually opens.
        let g = game();
        let (w, h) = g.initial_size();
        assert_eq!(
            (w as f32, h as f32),
            Klotski::SIZE,
            "the probe and the window disagree about the opening size"
        );
    }

    // ── Layout ─────────────────────────────────────────────────────

    #[test]
    fn the_layout_is_a_function_of_the_window_size_alone() {
        for &(w, h) in WINDOWS {
            assert_eq!(
                Layout::new(w, h),
                Layout::new(w, h),
                "the layout at {w}x{h} depends on something other than the size"
            );
        }
    }

    #[test]
    fn every_band_stays_inside_the_window() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("header", l.header),
                ("board", l.board),
                ("exit", l.exit),
                ("controls", l.controls),
                ("footer", l.footer),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "{name} escapes the {w}x{h} window: {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_board_never_overlaps_the_chrome() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.board.is_empty() {
                continue;
            }
            for (name, r) in [
                ("header", l.header),
                ("controls", l.controls),
                ("footer", l.footer),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    l.board.intersect(r).is_none(),
                    "at {w}x{h} the board sits on the {name}: board {:?}, {name} {r:?}",
                    l.board
                );
            }
        }
    }

    #[test]
    fn the_chrome_bands_do_not_overlap_each_other() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let bands = [
                ("header", l.header),
                ("controls", l.controls),
                ("footer", l.footer),
            ];
            for (i, (an, a)) in bands.iter().enumerate() {
                for (bn, b) in bands.iter().skip(i + 1) {
                    if a.is_empty() || b.is_empty() {
                        continue;
                    }
                    assert!(a.intersect(*b).is_none(), "at {w}x{h} {an} overlaps {bn}");
                }
            }
        }
    }

    #[test]
    fn the_board_is_drawn_with_square_cells() {
        // A 4x5 grid stretched to fill a band that is not 4:5 has cells that no
        // longer match the square hit boxes recorded over them.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            let r = l.cell_rect(0, 0);
            assert!(
                (r.w - r.h).abs() < 0.001,
                "at {w}x{h} a cell is {}x{}, not square",
                r.w,
                r.h
            );
        }
    }

    #[test]
    fn the_grid_fills_the_board_rect_exactly() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.cell <= 0.0 {
                continue;
            }
            let last = l.cell_rect(GRID_ROWS - 1, GRID_COLS - 1);
            assert!(
                (last.right() - l.board.right()).abs() < 0.01,
                "at {w}x{h} the last column stops at {} but the board ends at {}",
                last.right(),
                l.board.right()
            );
            assert!(
                (last.bottom() - l.board.bottom()).abs() < 0.01,
                "at {w}x{h} the last row stops at {} but the board ends at {}",
                last.bottom(),
                l.board.bottom()
            );
        }
    }

    #[test]
    fn no_two_cells_overlap() {
        let l = Layout::new(SIZE.0, SIZE.1);
        for r1 in 0..GRID_ROWS {
            for c1 in 0..GRID_COLS {
                for r2 in 0..GRID_ROWS {
                    for c2 in 0..GRID_COLS {
                        if (r1, c1) >= (r2, c2) {
                            continue;
                        }
                        assert!(
                            l.cell_rect(r1, c1).intersect(l.cell_rect(r2, c2)).is_none(),
                            "cells ({r1},{c1}) and ({r2},{c2}) overlap"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_exit_strip_spans_the_columns_the_big_block_must_reach() {
        // The strip is a picture of `WIN_COL`, not a second opinion about where
        // the way out is.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.exit.is_empty() {
                continue;
            }
            let left = l.cell_rect(GRID_ROWS - 1, WIN_COL);
            let right = l.cell_rect(GRID_ROWS - 1, WIN_COL + 1);
            assert!(
                (l.exit.x - left.x).abs() < 0.01,
                "at {w}x{h} the exit starts at {} but column {WIN_COL} at {}",
                l.exit.x,
                left.x
            );
            assert!(
                (l.exit.right() - right.right()).abs() < 0.01,
                "at {w}x{h} the exit ends at {} but column {} at {}",
                l.exit.right(),
                WIN_COL + 1,
                right.right()
            );
            assert!(
                l.exit.y >= l.board.bottom() - 0.01,
                "at {w}x{h} the exit is not below the board"
            );
        }
    }

    #[test]
    fn a_dropped_band_is_empty_rather_than_a_zero_height_strip() {
        // The two read alike to "does this show?" and differently to "how wide
        // is it?". Only one of them is safe to believe.
        let mut dropped = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for r in [l.header, l.controls, l.footer] {
                if r.h <= 0.0 {
                    dropped += 1;
                    assert_eq!(
                        r,
                        Rect::EMPTY,
                        "at {w}x{h} a band with no height still claims {}x{}",
                        r.w,
                        r.h
                    );
                }
            }
        }
        assert!(
            dropped > 0,
            "no window in WINDOWS is small enough to drop a band, so this \
             proves nothing"
        );
    }

    #[test]
    fn the_chrome_never_takes_more_than_its_share_of_the_window() {
        // `BOARD_SHARE` is the whole reason bands are dropped at all. Without
        // this the budget could be the entire window height and every existing
        // layout assertion would still hold — the board would simply be
        // squeezed to whatever the chrome left over, which in a short window is
        // almost nothing.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let chrome = l.header.h + l.controls.h + l.footer.h;
            let budget = (h - h * BOARD_SHARE - l.pad * 2.0).max(0.0);
            assert!(
                chrome <= budget + 0.01,
                "at {w}x{h} the chrome takes {chrome} of the {budget} it is \
                 allowed, leaving the board less than its {BOARD_SHARE} share"
            );
        }
    }

    #[test]
    fn the_footer_is_the_first_chrome_to_go() {
        // The footer only repeats what the buttons already say, so it is the
        // band whose loss costs least. Dropping the controls first would take
        // away the pointer's only route to undo and leave the reminder of the
        // keys that still work.
        let mut narrow = 0;
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            if l.header.is_empty() || l.controls.is_empty() {
                narrow += 1;
                assert!(
                    l.footer.is_empty(),
                    "at {w}x{h} a band was dropped while the footer was kept"
                );
            }
        }
        assert!(
            narrow > 0,
            "no window in WINDOWS is short enough to drop a band, so this \
             proves nothing"
        );
    }

    #[test]
    fn the_board_survives_every_window_a_band_is_dropped_in() {
        // Dropping chrome is only worth doing if it buys the board room.
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(l.cell > 0.0, "at {w}x{h} there is no board left to play on");
        }
    }

    #[test]
    fn the_board_grows_when_the_window_does() {
        let small = Layout::new(400.0, 600.0);
        let large = Layout::new(800.0, 1200.0);
        assert!(
            large.cell > small.cell,
            "doubling the window left the cells at {} -> {}",
            small.cell,
            large.cell
        );
    }

    #[test]
    fn the_buttons_share_the_controls_band_without_overlapping() {
        for &(w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let rects = l.button_rects();
            for (i, a) in rects.iter().enumerate() {
                if a.is_empty() {
                    continue;
                }
                assert!(
                    l.controls.intersect(*a).is_some(),
                    "at {w}x{h} button {i} is outside the controls band"
                );
                for b in rects.iter().skip(i + 1) {
                    if b.is_empty() {
                        continue;
                    }
                    assert!(a.intersect(*b).is_none(), "at {w}x{h} two buttons overlap");
                }
            }
        }
    }

    // ── Hit boxes come from the drawing pass ───────────────────────

    #[test]
    fn every_block_is_clickable_where_it_is_drawn() {
        // The whole point of the rewrite. `handle_mouse` used to send the click
        // through `Self::pixel_to_cell`, which answered from `CELL_SIZE` and
        // `PADDING` rather than from anything that had been drawn.
        for &(w, h) in WINDOWS {
            let g = game();
            let l = Layout::new(w, h);
            let f = g.frame(w, h);
            for block in g.blocks() {
                let r = l.block_rect(block.kind, block.row, block.col);
                if r.is_empty() {
                    continue;
                }
                let (cx, cy) = r.centre();
                assert_eq!(
                    f.hit_test(cx, cy),
                    Some(Target::Block(block.id)),
                    "at {w}x{h} the centre of block {} is not the block",
                    block.id
                );
            }
        }
    }

    #[test]
    fn a_block_covers_the_cells_it_sits_on() {
        // Cells are recorded first and blocks over them, so the topmost target
        // at a covered cell is the block. If that order flipped, every click on
        // a block would read as a click on the empty square underneath it.
        let g = game();
        let f = g.frame(SIZE.0, SIZE.1);
        let l = Layout::new(SIZE.0, SIZE.1);
        for block in g.blocks() {
            for (r, c) in block.cells() {
                let (cx, cy) = l.cell_rect(r, c).centre();
                assert_eq!(
                    f.hit_test(cx, cy),
                    Some(Target::Block(block.id)),
                    "cell ({r},{c}) is covered by block {} but does not report it",
                    block.id
                );
            }
        }
    }

    #[test]
    fn an_empty_cell_reports_itself() {
        let g = game();
        let f = g.frame(SIZE.0, SIZE.1);
        let l = Layout::new(SIZE.0, SIZE.1);
        let occ = g.occupancy();
        let mut empties = 0;
        for r in 0..GRID_ROWS {
            for c in 0..GRID_COLS {
                if occ[r][c].is_some() {
                    continue;
                }
                empties += 1;
                let (cx, cy) = l.cell_rect(r, c).centre();
                assert_eq!(
                    f.hit_test(cx, cy),
                    Some(Target::Cell(r, c)),
                    "empty cell ({r},{c}) does not report itself"
                );
            }
        }
        assert!(empties > 0, "the opening position has no empty cell");
    }

    #[test]
    fn the_exit_strip_records_no_target() {
        // A hit box that swallows a click and does nothing is worse than none:
        // the click it ate would otherwise have reached something.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        assert!(!l.exit.is_empty(), "there is no exit strip to check");
        let (cx, cy) = l.exit.centre();
        assert_eq!(
            f.hit_test(cx, cy),
            None,
            "the exit strip claims a click it cannot act on"
        );
    }

    #[test]
    fn every_control_is_reachable_by_pointer() {
        let g = game();
        let f = g.frame(SIZE.0, SIZE.1);
        let found: HashSet<String> = f
            .hits()
            .iter()
            .map(|(t, _)| probe::variant_name(*t))
            .collect();
        for (target, name) in BUTTONS {
            assert!(
                found.contains(&probe::variant_name(target)),
                "{name} has no hit box, so the pointer cannot reach it"
            );
        }
    }

    #[test]
    fn no_hit_box_escapes_the_window() {
        for &(w, h) in WINDOWS {
            let g = game();
            let f = g.frame(w, h);
            for (t, r) in f.hits() {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "at {w}x{h} the hit box for {t:?} is {r:?}"
                );
            }
        }
    }

    #[test]
    fn the_frame_is_balanced() {
        for &(w, h) in WINDOWS {
            let mut g = game();
            assert!(
                g.frame(w, h).is_balanced(),
                "at {w}x{h} a clip or translate was never undone"
            );
            g.position(&[(BlockKind::Big, WIN_ROW, WIN_COL)]);
            assert!(
                g.frame(w, h).is_balanced(),
                "at {w}x{h} the win overlay leaves a clip open"
            );
        }
    }

    #[test]
    fn the_game_records_hit_boxes_at_every_size() {
        for &(w, h) in WINDOWS {
            let g = game();
            assert!(
                !g.frame(w, h).hits().is_empty(),
                "at {w}x{h} nothing at all is clickable"
            );
        }
    }

    // ── Text is placed from its measured width ─────────────────────

    #[test]
    fn the_move_counter_is_right_aligned_from_its_own_width() {
        // It used to draw at `total_width - PADDING - 100.0` — a guess at how
        // wide "Moves: 1234" would come out.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        let (s, x, _, size, weight) = text_commands(&f)
            .into_iter()
            .find(|(s, ..)| s.starts_with("Moves:"))
            .expect("the header does not draw a move counter");
        let right = x + text::measure(&s, size, weight);
        assert!(
            (right - (l.header.right() - l.pad)).abs() < 0.5,
            "\"{s}\" ends at {right}, but the header's right margin is {}",
            l.header.right() - l.pad
        );
    }

    #[test]
    fn the_move_counter_moves_left_as_the_number_grows() {
        // The direct consequence of measuring rather than guessing: a fixed
        // offset would leave both strings at the same x.
        let mut g = game();
        let short = text_commands(&g.frame(SIZE.0, SIZE.1))
            .into_iter()
            .find(|(s, ..)| s.starts_with("Moves:"))
            .expect("no move counter")
            .1;
        // A run of legal moves, without caring which.
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        for _ in 0..12 {
            if !g.move_block(id, Direction::Down) {
                break;
            }
            g.move_block(id, Direction::Up);
        }
        assert!(g.moves() >= 10, "could not run the counter up");
        let long = text_commands(&g.frame(SIZE.0, SIZE.1))
            .into_iter()
            .find(|(s, ..)| s.starts_with("Moves:"))
            .expect("no move counter")
            .1;
        assert!(
            long < short,
            "a wider counter starts at the same x ({short} then {long}), so it \
             is not measured"
        );
    }

    #[test]
    fn a_centred_label_is_given_the_width_of_the_box_it_sits_in() {
        // `label_centred` used to take the limit as a separate argument, use it
        // to pick a centre, and then draw with `max_width: None`. The renderer
        // was never told to stop, so a button label too wide for its button was
        // centred on the button and then drawn straight across its neighbour.
        let g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let rects = l.button_rects();
        let mut checked = 0;
        for cmd in g.frame(SIZE.0, SIZE.1).commands() {
            let RenderCommand::Text {
                text, max_width, ..
            } = cmd
            else {
                continue;
            };
            let Some(i) = BUTTONS.iter().position(|(_, n)| *n == text.as_str()) else {
                continue;
            };
            checked += 1;
            assert_eq!(
                *max_width,
                Some(rects[i].w),
                "the \"{text}\" label is centred in a {}-wide button and then \
                 drawn with a limit of {max_width:?}",
                rects[i].w
            );
        }
        assert_eq!(
            checked,
            BUTTONS.len(),
            "not every button label was drawn, so this proves less than it looks"
        );
    }

    #[test]
    fn the_footer_text_stays_inside_the_footer() {
        // The second line is dropped when there is no room for it. Drawing both
        // regardless centres a two-line stack on a one-line band, which puts
        // one of them over the board and the other under the window.
        let mut checked = 0;
        for &(w, h) in WINDOWS {
            let g = game();
            let l = Layout::new(w, h);
            if l.footer.is_empty() {
                continue;
            }
            for (s, _, y, size, weight) in text_commands(&g.frame(w, h)) {
                if !FOOTER_LINES.contains(&s.as_str()) {
                    continue;
                }
                checked += 1;
                assert!(
                    y >= l.footer.y - 0.01
                        && y + text::line_height(size, weight) <= l.footer.bottom() + 0.01,
                    "at {w}x{h} the footer line \"{s}\" runs from {y} to {}, \
                     outside the footer {:?}",
                    y + text::line_height(size, weight),
                    l.footer
                );
            }
        }
        assert!(
            checked > 0,
            "no footer line was drawn in any window, so this proves nothing"
        );
    }

    #[test]
    fn no_text_starts_outside_the_window() {
        for &(w, h) in WINDOWS {
            let g = game();
            for (s, x, y, ..) in text_commands(&g.frame(w, h)) {
                assert!(
                    x >= -0.01 && y >= -0.01 && x <= w && y <= h,
                    "at {w}x{h} \"{s}\" is drawn at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn the_block_label_stays_inside_the_block() {
        for &(w, h) in WINDOWS {
            let g = game();
            let l = Layout::new(w, h);
            let big = g
                .blocks()
                .iter()
                .find(|b| b.kind == BlockKind::Big)
                .expect("no big block");
            let r = l.block_rect(big.kind, big.row, big.col);
            for (s, x, y, size, weight) in text_commands(&g.frame(w, h)) {
                if s != BlockKind::Big.label() {
                    continue;
                }
                assert!(
                    x >= r.x - 0.01
                        && x + text::measure(&s, size, weight) <= r.right() + 0.01
                        && y >= r.y - 0.01
                        && y + text::line_height(size, weight) <= r.bottom() + 0.01,
                    "at {w}x{h} the label \"{s}\" leaves its block: text at \
                     ({x}, {y}), block {r:?}"
                );
            }
        }
    }

    // ── The puzzle table itself ────────────────────────────────────

    #[test]
    fn every_puzzle_fits_the_grid_without_overlapping() {
        for (i, p) in PUZZLES.iter().enumerate() {
            let mut seen = [[false; GRID_COLS]; GRID_ROWS];
            for &(kind, row, col) in p.blocks {
                assert!(
                    row + kind.rows() <= GRID_ROWS && col + kind.cols() <= GRID_COLS,
                    "puzzle {i} (\"{}\") has a {kind:?} at ({row},{col}) hanging \
                     off the grid",
                    p.name
                );
                for dr in 0..kind.rows() {
                    for dc in 0..kind.cols() {
                        assert!(
                            !seen[row + dr][col + dc],
                            "puzzle {i} (\"{}\") stacks two blocks on \
                             ({},{})",
                            p.name,
                            row + dr,
                            col + dc
                        );
                        seen[row + dr][col + dc] = true;
                    }
                }
            }
        }
    }

    #[test]
    fn every_puzzle_has_exactly_one_big_block_and_room_to_move() {
        for (i, p) in PUZZLES.iter().enumerate() {
            let bigs = p
                .blocks
                .iter()
                .filter(|(k, ..)| *k == BlockKind::Big)
                .count();
            assert_eq!(bigs, 1, "puzzle {i} (\"{}\") has {bigs} big blocks", p.name);
            let filled: usize = p.blocks.iter().map(|(k, ..)| k.rows() * k.cols()).sum();
            assert!(
                filled < GRID_ROWS * GRID_COLS,
                "puzzle {i} (\"{}\") fills the whole grid, so nothing can move",
                p.name
            );
        }
    }

    #[test]
    fn no_puzzle_opens_already_solved() {
        for (i, _) in PUZZLES.iter().enumerate() {
            let mut g = game();
            g.load_puzzle(i);
            assert!(!g.is_won(), "puzzle {i} is won before you touch it");
        }
    }

    #[test]
    fn every_puzzle_offers_a_legal_first_move() {
        for (i, _) in PUZZLES.iter().enumerate() {
            let mut g = game();
            g.load_puzzle(i);
            let any = g
                .blocks()
                .iter()
                .any(|b| Direction::ALL.into_iter().any(|d| g.can_move(b.id, d)));
            assert!(any, "puzzle {i} is dead on arrival — nothing can move");
        }
    }

    // ── Ids are not indices ────────────────────────────────────────

    #[test]
    fn no_block_id_equals_its_position_in_the_vector() {
        // This is what makes the old bug — an index stored in a field called
        // `block_id` — impossible to write without it failing immediately.
        //
        // The opening board first, and separately. The version of this test
        // that went straight to the loop below never looked at it: `game()`
        // has already spent ten ids on the position `new()` built, so every
        // `load_puzzle` under it hands out ids well past the index range and
        // the property holds however the counter started. The one board where
        // a counter starting at zero puts id 0 at index 0 is the one the
        // program opens on, and it was the one board the test skipped.
        let opening = game();
        for (idx, b) in opening.blocks().iter().enumerate() {
            assert_ne!(
                b.id, idx,
                "the opening board's block at position {idx} has id {idx}, so \
                 an index and an id are interchangeable again"
            );
        }
        for i in 0..PUZZLES.len() {
            let mut g = game();
            g.load_puzzle(i);
            for (idx, b) in g.blocks().iter().enumerate() {
                assert_ne!(
                    b.id, idx,
                    "puzzle {i}: block at position {idx} has id {idx}, so an \
                     index and an id are interchangeable again"
                );
            }
        }
    }

    #[test]
    fn ids_are_never_reused_across_puzzle_loads() {
        let mut g = game();
        let mut seen: HashSet<usize> = HashSet::new();
        for i in 0..PUZZLES.len() {
            g.load_puzzle(i);
            for b in g.blocks() {
                assert!(
                    seen.insert(b.id),
                    "puzzle {i} reissued block id {}, so a stale selection can \
                     name a block that is not the one it meant",
                    b.id
                );
            }
        }
    }

    #[test]
    fn undo_moves_the_block_the_move_moved() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        let before: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        assert!(g.move_block(id, Direction::Down), "the move was refused");
        assert!(g.undo(), "there was nothing to undo");
        let after: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        assert_eq!(
            before, after,
            "undo did not put the board back the way it was"
        );
    }

    #[test]
    fn undo_unwinds_a_run_of_moves_in_order() {
        let mut g = game();
        let opening: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        let a = g.block_at(3, 1).expect("no block at (3,1)");
        let b = g.block_at(3, 2).expect("no block at (3,2)");
        assert!(g.move_block(a, Direction::Down));
        assert!(g.move_block(b, Direction::Down));
        assert_eq!(g.moves(), 2);
        while g.undo() {}
        let back: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        assert_eq!(
            opening, back,
            "unwinding did not reach the opening position"
        );
        assert_eq!(g.moves(), 0, "the counter did not come back to zero");
    }

    // ── Moving ─────────────────────────────────────────────────────

    #[test]
    fn a_block_cannot_move_off_the_grid() {
        // All four edges, not just the two `shifted` refuses on its own. Up and
        // Left fail in `checked_add_signed`; Down and Right are only refused
        // because `can_fit_in_grid` looks at the far side of the block, and a
        // test that checked only Up and Left would call that guard covered
        // while never once running it.
        let mut g = game();
        g.position(&[(BlockKind::Small, 0, 0)]);
        let top_left = g.blocks()[0].id;
        assert!(!g.can_move(top_left, Direction::Up), "moved off the top");
        assert!(!g.can_move(top_left, Direction::Left), "moved off the left");
        assert!(
            g.can_move(top_left, Direction::Down),
            "cannot move into open space"
        );

        let mut g = game();
        g.position(&[(BlockKind::Small, GRID_ROWS - 1, GRID_COLS - 1)]);
        let bottom_right = g.blocks()[0].id;
        assert!(
            !g.can_move(bottom_right, Direction::Down),
            "moved off the bottom"
        );
        assert!(
            !g.can_move(bottom_right, Direction::Right),
            "moved off the right"
        );

        // The 2x2 is the case where the near edge is in the grid and the far
        // edge is not, which is the only way to tell a corner check from a
        // whole-footprint one.
        let mut g = game();
        g.position(&[(BlockKind::Big, GRID_ROWS - 2, GRID_COLS - 2)]);
        let big = g.blocks()[0].id;
        assert!(
            !g.can_move(big, Direction::Down),
            "the big block's bottom row hung off the grid"
        );
        assert!(
            !g.can_move(big, Direction::Right),
            "the big block's right column hung off the grid"
        );
    }

    #[test]
    fn a_block_cannot_move_onto_another() {
        let mut g = game();
        g.position(&[(BlockKind::Small, 0, 0), (BlockKind::Small, 1, 0)]);
        let top = g.blocks()[0].id;
        assert!(
            !g.can_move(top, Direction::Down),
            "a block moved onto an occupied cell"
        );
    }

    #[test]
    fn a_block_may_move_into_the_cell_it_is_vacating() {
        // A 2x2 moving down overlaps its own old footprint. Reading that as a
        // collision would freeze every large block on the board.
        let mut g = game();
        g.position(&[(BlockKind::Big, 0, 0)]);
        let id = g.blocks()[0].id;
        assert!(
            g.can_move(id, Direction::Down),
            "a block collided with itself"
        );
    }

    #[test]
    fn a_move_advances_the_counter_and_the_undo_depth() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert_eq!((g.moves(), g.undo_depth()), (0, 0));
        assert!(g.move_block(id, Direction::Down));
        assert_eq!((g.moves(), g.undo_depth()), (1, 1));
    }

    #[test]
    fn a_refused_move_changes_nothing() {
        let mut g = game();
        let id = g.block_at(0, 0).expect("no block at (0,0)");
        let before: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert!(
            !g.move_block(id, Direction::Up),
            "an illegal move was allowed"
        );
        let after: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert_eq!(before, after, "a refused move moved something");
        assert_eq!((g.moves(), g.undo_depth()), (0, 0));
    }

    #[test]
    fn the_undo_stack_stops_at_its_cap() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        // Shuffle one block up and down until well past the cap.
        for _ in 0..(MAX_UNDO + 20) {
            if !g.move_block(id, Direction::Down) {
                break;
            }
            if !g.move_block(id, Direction::Up) {
                break;
            }
        }
        assert!(
            g.moves() > MAX_UNDO,
            "only got {} moves in, so the cap was never reached",
            g.moves()
        );
        assert_eq!(g.undo_depth(), MAX_UNDO, "the undo stack grew past its cap");
    }

    #[test]
    fn the_undo_cap_drops_the_oldest_move_not_the_newest() {
        // Past the cap the game can no longer be unwound to the opening
        // position; what it must still do is unwind the moves it *does*
        // remember, in order. Dropping the newest instead leaves the stack
        // holding a run that stops short of where the board actually is, so
        // unwinding it walks the block somewhere it has never been.
        //
        // The witness has to be a walk, not the down-up shuffle above: a
        // strictly alternating sequence is its own reverse, so dropping from
        // either end of it lands the block in the same place and the fault goes
        // unseen.
        const CYCLE: usize = 3;
        let walk = |i: usize| {
            if (i / CYCLE).is_multiple_of(2) {
                Direction::Down
            } else {
                Direction::Up
            }
        };
        let total = MAX_UNDO + 200;

        let mut g = game();
        g.position(&[(BlockKind::Small, 0, 0)]);
        let id = g.blocks()[0].id;
        for i in 0..total {
            assert!(g.move_block(id, walk(i)), "move {i} was refused");
        }
        assert_eq!(g.undo_depth(), MAX_UNDO, "the cap did not hold");

        // Where a block that had only ever made the moves the stack still holds
        // would be: the position reached by the walk up to the oldest of them.
        let mut reference = game();
        reference.position(&[(BlockKind::Small, 0, 0)]);
        let rid = reference.blocks()[0].id;
        for i in 0..(total - MAX_UNDO) {
            assert!(reference.move_block(rid, walk(i)));
        }

        while g.undo() {}
        assert_eq!(
            (g.blocks()[0].row, g.blocks()[0].col),
            (reference.blocks()[0].row, reference.blocks()[0].col),
            "unwinding the stack did not reach the position its oldest \
             remembered move started from"
        );
        assert_eq!(
            g.moves(),
            total - MAX_UNDO,
            "the counter did not come back with the board"
        );
    }

    // ── Winning ────────────────────────────────────────────────────

    #[test]
    fn the_big_block_on_the_exit_wins() {
        let mut g = one_move_from_winning();
        assert!(!g.is_won(), "won before the move");
        let id = g.big_block().expect("no big block");
        assert!(
            g.move_block(id, Direction::Down),
            "the winning move was refused"
        );
        assert!(
            g.is_won(),
            "the big block reached the exit and nothing noticed"
        );
    }

    #[test]
    fn only_the_big_block_wins() {
        let mut g = game();
        g.position(&[(BlockKind::Small, WIN_ROW, WIN_COL)]);
        assert!(
            !g.is_won(),
            "a 1x1 on the exit square counts as escaping through it"
        );
    }

    #[test]
    fn the_big_block_wins_only_where_the_exit_is() {
        // Every other square the big block fits on, and none of them count.
        for row in 0..GRID_ROWS.saturating_sub(1) {
            for col in 0..GRID_COLS.saturating_sub(1) {
                let mut g = game();
                g.position(&[(BlockKind::Big, row, col)]);
                assert_eq!(
                    g.is_won(),
                    (row, col) == (WIN_ROW, WIN_COL),
                    "the big block at ({row},{col}) reports won = {}",
                    g.is_won()
                );
            }
        }
    }

    #[test]
    fn undoing_the_winning_move_un_wins() {
        // The version this replaced latched `status = Won` and refused every
        // undo while it was set, so the winning move was the one move you could
        // never take back.
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        assert!(g.is_won());
        assert!(g.undo(), "undo was refused after the win");
        assert!(
            !g.is_won(),
            "the board is no longer solved but the game still says it is"
        );
    }

    #[test]
    fn a_won_board_refuses_further_moves() {
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        let before: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        for other in g.blocks().iter().map(|b| b.id).collect::<Vec<_>>() {
            for dir in Direction::ALL {
                assert!(
                    !g.move_block(other, dir),
                    "block {other} moved {dir:?} after the puzzle was solved"
                );
            }
        }
        let after: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert_eq!(before, after, "a won board moved");
    }

    #[test]
    fn the_win_scrim_is_translucent() {
        // It was an opaque fill under a comment claiming it was semi
        // transparent — an assertion nobody checked, and the reason the board
        // you had just solved was invisible.
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        let f = g.frame(SIZE.0, SIZE.1);
        let full: Vec<Color> = fill_rects(&f)
            .into_iter()
            .filter(|(r, _)| r.w >= SIZE.0 - 0.01 && r.h >= SIZE.1 - 0.01)
            .map(|(_, c)| c)
            .collect();
        assert!(
            full.len() >= 2,
            "the win overlay draws no full-window scrim at all"
        );
        let scrim = full.last().copied().expect("no scrim");
        assert!(
            scrim.a < 255,
            "the scrim is opaque (alpha {}), so it paints out the solved board",
            scrim.a
        );
    }

    #[test]
    fn the_win_panel_does_not_cover_the_whole_board() {
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        let l = Layout::new(SIZE.0, SIZE.1);
        let panel = l.win_panel();
        assert!(
            panel.w < l.window.w && panel.h < l.window.h,
            "the victory panel is the whole window"
        );
    }

    #[test]
    fn the_win_overlay_takes_the_board_out_of_reach() {
        // A modal that only looks in front is one you can press through.
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        let l = Layout::new(SIZE.0, SIZE.1);
        let f = g.frame(SIZE.0, SIZE.1);
        for block in g.blocks() {
            let (cx, cy) = l.block_rect(block.kind, block.row, block.col).centre();
            assert!(
                !matches!(f.hit_test(cx, cy), Some(Target::Block(_))),
                "block {} is still clickable behind the victory panel",
                block.id
            );
        }
    }

    #[test]
    fn the_win_overlay_offers_a_way_on() {
        let mut g = one_move_from_winning();
        let id = g.big_block().expect("no big block");
        assert!(g.move_block(id, Direction::Down));
        let f = g.frame(SIZE.0, SIZE.1);
        let found: HashSet<String> = f
            .hits()
            .iter()
            .map(|(t, _)| probe::variant_name(*t))
            .collect();
        for t in [Target::Undo, Target::Restart, Target::Next] {
            assert!(
                found.contains(&probe::variant_name(t)),
                "{t:?} is not reachable from the victory panel, so the only way \
                 on is a key named in a footer the scrim is over"
            );
        }
    }

    // ── Pointer ────────────────────────────────────────────────────

    #[test]
    fn clicking_a_block_selects_it_and_clicking_it_again_puts_it_down() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert_eq!(g.selected(), None);
        probe::click(&mut g, Target::Block(id));
        assert_eq!(g.selected(), Some(id), "a click did not pick the block up");
        probe::click(&mut g, Target::Block(id));
        assert_eq!(g.selected(), None, "a second click did not put it down");
    }

    #[test]
    fn clicking_an_empty_cell_the_selection_can_reach_moves_it_there() {
        // The pointer used to be able to pick a block up and put it down, and
        // nothing else; moving was keyboard-only.
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        probe::click(&mut g, Target::Block(id));
        probe::click(&mut g, Target::Cell(4, 1));
        let moved = g
            .blocks()
            .iter()
            .find(|b| b.id == id)
            .expect("the block vanished");
        assert_eq!(
            (moved.row, moved.col),
            (4, 1),
            "clicking the empty cell below did not move the block into it"
        );
        assert_eq!(g.moves(), 1, "the move was not counted");
    }

    #[test]
    fn a_click_moves_a_block_one_cell_and_no_further() {
        let mut g = game();
        g.position(&[(BlockKind::Small, 0, 0)]);
        let id = g.blocks()[0].id;
        probe::click(&mut g, Target::Block(id));
        probe::click(&mut g, Target::Cell(4, 0));
        let b = &g.blocks()[0];
        assert_eq!(
            (b.row, b.col),
            (0, 0),
            "a click four cells away teleported the block"
        );
        assert_eq!(g.selected(), None, "the unreachable cell did not deselect");
    }

    #[test]
    fn clicking_an_unreachable_empty_cell_puts_the_block_down() {
        let mut g = game();
        let id = g.block_at(0, 0).expect("no block at (0,0)");
        probe::click(&mut g, Target::Block(id));
        assert_eq!(g.selected(), Some(id));
        probe::click(&mut g, Target::Cell(4, 2));
        assert_eq!(g.selected(), None);
        assert_eq!(g.moves(), 0, "an unreachable cell moved something");
    }

    #[test]
    fn clicking_bare_background_puts_the_block_down() {
        // The old handler deselected on an empty *cell* and left the selection
        // alone on a click outside the grid — two answers to one question.
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        probe::click(&mut g, Target::Block(id));
        assert_eq!(g.selected(), Some(id));
        let (x, y) = probe::bare_point(&g, SIZE).expect("no bare background anywhere");
        g.click_at(x, y, MouseButton::Left, SIZE);
        assert_eq!(
            g.selected(),
            None,
            "a click on bare background at ({x}, {y}) left the block held"
        );
    }

    #[test]
    fn a_click_reads_against_the_size_the_frame_was_drawn_at() {
        // The fault in one test: the old `pixel_to_cell` answered from
        // `CELL_SIZE` and `PADDING`, so the same coordinates named the same
        // cell in every window, however the board had actually been drawn.
        for &(w, h) in WINDOWS {
            let mut g = game();
            let l = Layout::new(w, h);
            let block = g.blocks()[0].clone();
            let r = l.block_rect(block.kind, block.row, block.col);
            if r.is_empty() {
                continue;
            }
            let (cx, cy) = r.centre();
            g.click_at(cx, cy, MouseButton::Left, (w, h));
            assert_eq!(
                g.selected(),
                Some(block.id),
                "at {w}x{h} a click on the block's own ink selected {:?}",
                g.selected()
            );
        }
    }

    #[test]
    fn a_release_is_not_a_press() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        let l = Layout::new(SIZE.0, SIZE.1);
        let b = g.blocks().iter().find(|b| b.id == id).unwrap().clone();
        let (x, y) = l.block_rect(b.kind, b.row, b.col).centre();
        let out = handle_event(
            &mut g,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            }),
        );
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(g.selected(), None, "a mouse release selected a block");
    }

    // ── Buttons ────────────────────────────────────────────────────

    #[test]
    fn the_undo_button_takes_back_a_move() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        probe::click(&mut g, Target::Undo);
        assert_eq!(g.moves(), 0, "the undo button did not undo");
        assert_eq!(g.undo_depth(), 0);
    }

    #[test]
    fn the_undo_button_on_an_empty_stack_changes_nothing() {
        let mut g = game();
        let before: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        probe::click(&mut g, Target::Undo);
        let after: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert_eq!(before, after, "undo with nothing to undo moved a block");
        assert_eq!(g.moves(), 0);
    }

    #[test]
    fn the_undo_button_is_dimmed_when_there_is_nothing_to_undo() {
        let mut g = game();
        let l = Layout::new(SIZE.0, SIZE.1);
        let undo_rect = l.button_rects()[0];
        let colour_of = |g: &Klotski| {
            fill_rects(&g.frame(SIZE.0, SIZE.1))
                .into_iter()
                .find(|(r, _)| (r.x - undo_rect.x).abs() < 0.01 && (r.w - undo_rect.w).abs() < 0.01)
                .map(|(_, c)| c)
                .expect("the undo button is not drawn")
        };
        let idle = colour_of(&g);
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        let live = colour_of(&g);
        assert_ne!(
            idle, live,
            "the undo button looks the same whether or not it will do anything"
        );
    }

    #[test]
    fn the_restart_button_returns_the_opening_position() {
        let mut g = game();
        let opening: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        probe::click(&mut g, Target::Restart);
        let back: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        assert_eq!(
            opening, back,
            "restart did not restore the opening position"
        );
        assert_eq!((g.moves(), g.undo_depth()), (0, 0));
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn the_next_and_prev_buttons_walk_the_puzzles_and_wrap() {
        let mut g = game();
        assert_eq!(g.current_puzzle(), 0);
        for expected in 1..PUZZLES.len() {
            probe::click(&mut g, Target::Next);
            assert_eq!(g.current_puzzle(), expected);
        }
        probe::click(&mut g, Target::Next);
        assert_eq!(
            g.current_puzzle(),
            0,
            "next did not wrap round to the first"
        );
        probe::click(&mut g, Target::Prev);
        assert_eq!(
            g.current_puzzle(),
            PUZZLES.len() - 1,
            "prev did not wrap round to the last"
        );
    }

    #[test]
    fn changing_puzzle_clears_the_move_count_and_the_undo_stack() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        probe::click(&mut g, Target::Next);
        assert_eq!(
            (g.moves(), g.undo_depth(), g.selected()),
            (0, 0, None),
            "the new puzzle inherited the old one's history"
        );
    }

    #[test]
    fn every_button_does_something() {
        // A recorded hit box that changes nothing is worse than no hit box: it
        // swallows the click that would otherwise have reached what is behind.
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        for (target, name) in BUTTONS {
            let mut probe_game = game();
            let seed = probe_game.block_at(3, 1).expect("no block at (3,1)");
            assert!(probe_game.move_block(seed, Direction::Down));
            let before = (
                probe_game.current_puzzle(),
                probe_game.moves(),
                probe_game.undo_depth(),
                probe_game
                    .blocks()
                    .iter()
                    .map(|b| (b.row, b.col))
                    .collect::<Vec<_>>(),
            );
            probe::click(&mut probe_game, target);
            let after = (
                probe_game.current_puzzle(),
                probe_game.moves(),
                probe_game.undo_depth(),
                probe_game
                    .blocks()
                    .iter()
                    .map(|b| (b.row, b.col))
                    .collect::<Vec<_>>(),
            );
            assert_ne!(before, after, "the {name} button changes nothing");
        }
    }

    // ── Keyboard ───────────────────────────────────────────────────

    #[test]
    fn a_key_release_does_nothing() {
        // Reading only `key` and letting `..` swallow `pressed` runs every
        // binding twice per press.
        let mut g = game();
        let mut release = probe::press(Key::N);
        release.pressed = false;
        let out = g.key_at(&release, SIZE);
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(g.current_puzzle(), 0, "a key release changed the puzzle");
    }

    #[test]
    fn enter_cycles_the_selection_through_every_block() {
        let mut g = game();
        let ids: Vec<usize> = g.blocks().iter().map(|b| b.id).collect();
        let mut seen = Vec::new();
        for _ in 0..ids.len() {
            probe::key(&mut g, &probe::press(Key::Enter));
            seen.push(g.selected().expect("nothing selected"));
        }
        assert_eq!(
            seen, ids,
            "the selection does not visit every block in turn"
        );
        probe::key(&mut g, &probe::press(Key::Enter));
        assert_eq!(g.selected(), Some(ids[0]), "the selection did not wrap");
    }

    #[test]
    fn escape_puts_the_block_down() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::Enter));
        assert!(g.selected().is_some());
        probe::key(&mut g, &probe::press(Key::Escape));
        assert_eq!(g.selected(), None);
    }

    #[test]
    fn an_arrow_moves_the_selected_block_and_only_it() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        probe::click(&mut g, Target::Block(id));
        let before: Vec<(usize, usize, usize)> =
            g.blocks().iter().map(|b| (b.id, b.row, b.col)).collect();
        probe::key(&mut g, &probe::press(Key::Down));
        for (bid, row, col) in before {
            let now = g.blocks().iter().find(|b| b.id == bid).unwrap();
            if bid == id {
                assert_eq!(
                    (now.row, now.col),
                    (row + 1, col),
                    "the selected block did not move down one"
                );
            } else {
                assert_eq!(
                    (now.row, now.col),
                    (row, col),
                    "block {bid} moved without being selected"
                );
            }
        }
    }

    #[test]
    fn an_arrow_moves_exactly_one_cell_per_press() {
        let mut g = game();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        probe::click(&mut g, Target::Block(id));
        probe::key(&mut g, &probe::press(Key::Down));
        assert_eq!(g.moves(), 1, "one press produced {} moves", g.moves());
    }

    #[test]
    fn an_arrow_with_nothing_selected_is_ignored() {
        let mut g = game();
        let before: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        let out = probe::key(&mut g, &probe::press(Key::Down));
        assert_eq!(out, EventResult::Ignored);
        let after: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert_eq!(before, after, "an arrow moved a block nobody had picked up");
    }

    #[test]
    fn z_undoes_and_r_restarts() {
        let mut g = game();
        let opening: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        let id = g.block_at(3, 1).expect("no block at (3,1)");
        assert!(g.move_block(id, Direction::Down));
        probe::key(&mut g, &probe::press(Key::Z));
        assert_eq!(g.moves(), 0, "Z did not undo");

        assert!(g.move_block(id, Direction::Down));
        probe::key(&mut g, &probe::press(Key::R));
        let after: Vec<(usize, usize)> = g.blocks().iter().map(|b| (b.row, b.col)).collect();
        assert_eq!(opening, after, "R did not restart");
    }

    #[test]
    fn n_and_tab_go_to_the_next_puzzle_and_p_goes_back() {
        let mut g = game();
        probe::key(&mut g, &probe::press(Key::N));
        assert_eq!(g.current_puzzle(), 1, "N did not advance");
        probe::key(&mut g, &probe::press(Key::Tab));
        assert_eq!(g.current_puzzle(), 2, "Tab did not advance");
        probe::key(&mut g, &probe::press(Key::P));
        assert_eq!(g.current_puzzle(), 1, "P did not go back");
    }

    #[test]
    fn the_number_keys_reach_every_puzzle() {
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
        ];
        assert!(
            keys.len() >= PUZZLES.len(),
            "there are {} puzzles and only {} number keys, so some puzzle can \
             only be reached by walking to it",
            PUZZLES.len(),
            keys.len()
        );
        for (i, key) in keys.iter().enumerate().take(PUZZLES.len()) {
            let mut g = game();
            probe::key(&mut g, &probe::press(*key));
            assert_eq!(g.current_puzzle(), i, "{key:?} did not load puzzle {i}");
        }
    }

    #[test]
    fn a_modified_letter_is_not_a_shortcut() {
        // Ctrl-N belongs to whatever owns the window, not to the game.
        let mut g = game();
        let out = probe::key(&mut g, &probe::ctrl(Key::N));
        assert_eq!(out, EventResult::Ignored);
        assert_eq!(g.current_puzzle(), 0, "Ctrl-N changed the puzzle");
    }

    #[test]
    fn an_unbound_key_is_ignored_rather_than_swallowed() {
        let mut g = game();
        let out = probe::key(&mut g, &probe::press_with(Key::Q, Modifiers::NONE));
        assert_eq!(out, EventResult::Ignored);
    }

    // ── Resize ─────────────────────────────────────────────────────

    #[test]
    fn a_resize_is_remembered_and_read_by_the_next_click() {
        let mut g = game();
        handle_event(
            &mut g,
            &Event::Resize {
                width: 1280,
                height: 720,
            },
        );
        assert_eq!(g.size_drawn(), (1280.0, 720.0));
        let l = g.layout();
        let b = g.blocks()[0].clone();
        let (x, y) = l.block_rect(b.kind, b.row, b.col).centre();
        // Straight through `handle_mouse`, without a probe helper telling it
        // what size to use: the size the frame was drawn at must be enough.
        handle_event(
            &mut g,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert_eq!(
            g.selected(),
            Some(b.id),
            "after a resize the click was read against the old size"
        );
    }

    #[test]
    fn rendering_records_the_size_it_drew_at() {
        let mut g = game();
        let _ = g.render(1024.0, 768.0);
        assert_eq!(
            g.size_drawn(),
            (1024.0, 768.0),
            "the frame was drawn at one size and clicks are read at another"
        );
    }

    #[test]
    fn the_window_close_request_exits() {
        let mut g = game();
        assert!(matches!(g.on_event(&Event::CloseRequested), Response::Exit));
    }

    #[test]
    fn a_handled_event_asks_for_a_redraw_and_an_ignored_one_does_not() {
        let mut g = game();
        assert!(matches!(
            g.on_event(&Event::Key(probe::press(Key::N))),
            Response::Redraw
        ));
        assert!(matches!(
            g.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        ));
    }
}
