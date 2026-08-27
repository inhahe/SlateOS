//! Flood It — a colour-flooding puzzle.
//!
//! Starting from the top-left corner, choose a colour. Every cell connected to
//! that corner takes it, and any neighbouring cell that already had it joins the
//! region. Cover the board before the moves run out.
//!
//! # What this file used to be
//!
//! `main` built a `FloodIt` and dropped it, so none of what follows ever ran.
//! Giving it a window is what made the rest visible, because every one of them
//! needs the game to be *played* to notice:
//!
//! * **Every key fired twice.** The handler matched on `Event::Key` without
//!   ever reading [`KeyEvent::pressed`], so each key ran again on release. `H`
//!   toggled the help panel on when pressed and off when let go, which means
//!   the help could not be seen at all — and it was the only place the size
//!   keys were written down. `N`, `S`, `M` and `L` each generated two boards
//!   per press, so the board the player got was never the one the first draw
//!   would have produced.
//! * **The colour buttons were clickable ten pixels below themselves.** The
//!   renderer drew the swatch row at y ∈ [56, 86]; the click handler tested
//!   y ∈ [60, 96]. The top four pixels of every swatch did nothing, and the
//!   strip *under* it — where the "this is your colour" underline is drawn —
//!   chose it. Two hand-copied constants that were never going to be checked
//!   against each other; there is now one [`Layout`] and the hit boxes are
//!   recorded by the code that paints them.
//! * **Nothing else was clickable.** Not the board — in a game whose whole
//!   surface is a grid of the six things you are choosing between — not a new
//!   game, and not a board size.
//! * **The layout was a constant.** `render` took a width and a height and used
//!   them for the background rectangle and nothing else; `cell_size` was a
//!   lookup table keyed on the board size. An 18-board with its help panel open
//!   needed 536x446 of the 800x600 that was assumed, and fell off the edge of
//!   anything smaller.
//! * **`size` was a second copy of `grid.len()`** that nothing kept in
//!   agreement, and it was what every draw and every flood indexed the grid by.
//!   The board is now its own length.
//! * **Board size 10 was unreachable.** `set_size` accepted it and
//!   `max_moves_for_size` had a number for it, and no key or button offered it.
//!
//! Under all of that, the file opened with `#![allow(dead_code)]` and
//! `#![allow(unused_imports)]`, which is how `selected_color` — written by
//! every move and read by nothing — survived. Both are gone, and so is it.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::similar_names)]

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;

// From `randrange`, not a local LCG. The local one coloured each cell with
// `state % NUM_COLORS`, and NUM_COLORS is 6. Six is even, `x % 6` preserves the
// parity of `x`, and the low bit of a modulus-2^64 LCG alternates 0,1,0,1 on
// every draw -- so consecutive cells always got colours of opposite parity.
// The grid is filled in row-major order and every board size is even (8, 10,
// 14, 18), which puts that parity on the column index.
//
// The palette is [R, O, Y, G, T, M], so the even colours are {R, Y, T} and the
// odd ones are {O, G, M}. Measured before the fix:
//
//   * within one board, the columns strictly alternated between those two
//     halves. Half the palette was missing from every column.
//   * **not one horizontally-adjacent pair of cells matched, ever** -- zero out
//     of 61 200 checked across four board sizes and 200 seeds.
//   * so the starting blob averaged 1.48 cells and could only ever grow
//     vertically, in a game whose entire subject is growing a blob.
use randrange::{RandomSource, SeededRng};

const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);

/// Laid over a swatch whose colour cannot move the game.
///
/// Translucent rather than a substitute colour, because the colour is the
/// whole of what the swatch says: a disabled swatch still has to be
/// identifiable as the one it is.
const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
/// Behind the win/loss banner, which sits over the board it reports on.
const COL_BANNER: Color = Color::rgba(0x11, 0x11, 0x1B, 224);
/// Behind the help sheet, dimming the window it covers.
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

const NUM_COLORS: usize = 6;
const PALETTE: [Color; NUM_COLORS] = [
    COL_RED, COL_PEACH, COL_YELLOW, COL_GREEN, COL_TEAL, COL_MAUVE,
];
const PALETTE_LABELS: [&str; NUM_COLORS] = ["R", "O", "Y", "G", "T", "M"];

/// The board sizes the game offers, smallest first.
///
/// This is the list, not one of several: [`max_moves_for_size`], the size
/// buttons and the size keys all read it, so a size cannot be offered by one
/// and unknown to another. That is what happened to 10, which `set_size`
/// accepted and nothing could ask for.
const SIZES: [usize; 4] = [8, 10, 14, 18];

/// One key per entry of [`SIZES`], in the same order.
///
/// The old scheme was S/M/L for 8/14/18 — three letters for four sizes, which
/// is exactly how 10 came to be a size the game knew about and could not be
/// asked for. M and L therefore mean one size larger than they used to; nobody
/// can have learnt otherwise, since the only place they were written down was a
/// help panel that could not be opened.
const SIZE_KEYS: [Key; 4] = [Key::S, Key::M, Key::L, Key::X];
const SIZE_KEY_LABELS: [&str; 4] = ["S", "M", "L", "X"];

const DEFAULT_SIZE: usize = 14;
const WINDOW_WIDTH: f32 = 720.0;
const WINDOW_HEIGHT: f32 = 640.0;

/// Beyond this the board is drawn as one flat region rather than as cells.
///
/// Not a limit on play — the largest board offered is 18 — but a floor on how
/// small a cell may get before the one-pixel gap between cells is the only
/// thing left of it. Below it the gap is dropped rather than the colour.
const MIN_GAPPED_CELL: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameState {
    Playing,
    Won,
    Lost,
}

/// Everything the player can ask for, from either the keyboard or the pointer.
///
/// Both go through [`FloodIt::apply`], so a button and its key cannot drift
/// apart the way the swatch row's two copies of its own geometry did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Flood the corner region with palette entry `n`.
    Choose(usize),
    /// Start a fresh board of `n` per side.
    SetSize(usize),
    NewGame,
    ToggleHelp,
}

/// Everything a click can land on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// A board cell, by row and column. Clicking one chooses its colour.
    Cell(usize, usize),
    /// A palette swatch, by index into [`PALETTE`].
    Swatch(usize),
    /// A board-size button, by index into [`SIZES`].
    Size(usize),
    NewGame,
    Help,
    /// The help sheet, which covers the window and swallows what it covers.
    HelpSheet,
}

pub type Frame = guitk::frame::Frame<Target>;

fn max_moves_for_size(size: usize) -> u32 {
    match size {
        8 => 14,
        10 => 20,
        14 => 25,
        18 => 35,
        // Only reachable from a test that sets the grid directly. Two moves per
        // row is roughly what the four tuned numbers above come to.
        _ => (size as u32).saturating_mul(2),
    }
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, for one window size.
///
/// Derived on every frame and never stored. That is the whole of the third
/// fault: when the geometry lives in the app it is copied — once into the
/// renderer and once into the click handler — and the two copies disagreed by
/// ten pixels for as long as the file existed. There is one of these, the
/// renderer records its hit boxes as it paints them, and the click handler asks
/// the same frame what it hit.
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub palette: Rect,
    pub info: Rect,
    /// The board, always square.
    pub board: Rect,
    pub footer: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

/// The share of the window's height the board keeps no matter what.
///
/// Chrome is dropped to make room for it, rather than the board being squeezed
/// to make room for chrome. Every button has a key; no key has a board.
const BOARD_SHARE: f32 = 0.45;

/// Which band goes first when they do not all fit: footer, info, header,
/// palette.
///
/// Indices into the `wants` array in [`Layout::new`]. The footer leads because
/// every one of its buttons has a key; the palette survives longest of the four
/// because it is the only chrome that names the six things the game is about.
const BAND_DROP_ORDER: [usize; 4] = [3, 2, 0, 1];

impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 42.0).clamp(8.0, 14.0);
        let small = (font - 2.0).max(7.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, palette, info, footer] order.
        let mut wants = [
            (h * 0.09).clamp(20.0, 40.0),
            (h * 0.09).clamp(18.0, 42.0),
            (h * 0.05).clamp(14.0, 24.0),
            (h * 0.08).clamp(18.0, 32.0),
        ];
        // Dropped whole rather than shrunk together: a band scaled to four
        // pixels is a band that costs the board four pixels and shows nothing.
        let budget = (h - h * BOARD_SHARE).max(0.0);
        for &i in &BAND_DROP_ORDER {
            if wants.iter().sum::<f32>() <= budget {
                break;
            }
            if let Some(band) = wants.get_mut(i) {
                *band = 0.0;
            }
        }
        let [hdr_h, pal_h, inf_h, foot_h] = wants;

        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let palette = Rect::new(0.0, header.bottom(), w, pal_h);
        let info = Rect::new(0.0, palette.bottom(), w, inf_h);
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        // Square, and centred in what is left. A stretched board would put a
        // cell's colour somewhere other than the cell the player aimed at.
        let top = info.bottom();
        let bottom = if foot_h > 0.0 { footer.y } else { h };
        let avail_w = (w - pad * 2.0).max(0.0);
        let avail_h = (bottom - top - pad * 2.0).max(0.0);
        let side = avail_w.min(avail_h);
        let board = Rect::new(
            (w - side) / 2.0,
            top + (bottom - top - side) / 2.0,
            side,
            side,
        );

        let help_w = (w * 0.9).min(340.0);
        let help_h = (h * 0.9).min(260.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            palette,
            info,
            board,
            footer,
            help,
            font,
            small,
            pad,
        }
    }

    /// Split `row` into `count` buttons with even gaps.
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

    pub fn swatch(&self, index: usize) -> Rect {
        Self::nth_of(self.palette, NUM_COLORS, index)
    }

    /// The footer holds one button per board size, then New game.
    pub fn footer_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, SIZES.len().saturating_add(1), index)
    }

    /// The Help button, tucked into the right-hand end of the header.
    pub fn help_button(&self) -> Rect {
        let bw = (self.header.w * 0.22).clamp(0.0, 90.0);
        let bh = (self.header.h - 4.0).max(0.0);
        Rect::new(
            (self.header.right() - self.pad - bw).max(self.header.x),
            self.header.y + (self.header.h - bh) / 2.0,
            bw.min(self.header.w),
            bh,
        )
    }

    /// One cell of a `size`-per-side board.
    pub fn cell(&self, size: usize, row: usize, col: usize) -> Rect {
        let n = size.max(1) as f32;
        let cs = self.board.w / n;
        Rect::new(
            self.board.x + col as f32 * cs,
            self.board.y + row as f32 * cs,
            cs,
            cs,
        )
    }

    pub fn shows_palette(&self) -> bool {
        self.palette.h >= 10.0 && self.palette.w >= 60.0
    }

    pub fn shows_footer(&self) -> bool {
        self.footer.h >= 10.0 && self.footer.w >= 160.0
    }

    /// The banner that reports a win or a loss, laid across the board's foot.
    ///
    /// Over the board rather than under it, because a band below the board is a
    /// band that has to come from somewhere and there is no height to spare in
    /// a small window. A message that cannot be shown is worse than a message
    /// that covers two rows of a board the player has stopped playing.
    pub fn banner(&self) -> Rect {
        let bh = (self.board.h * 0.22).clamp(0.0, 56.0);
        Rect::new(self.board.x, self.board.bottom() - bh, self.board.w, bh)
    }
}

// ── The game ────────────────────────────────────────────────────────

pub struct FloodIt {
    /// The board, row-major. Its length *is* the board size — there is no
    /// second copy of that to disagree with it.
    grid: Vec<Vec<u8>>,
    moves: u32,
    max_moves: u32,
    state: GameState,
    rng: SeededRng,
    show_help: bool,
    width: f32,
    height: f32,
}

impl Default for FloodIt {
    fn default() -> Self {
        Self::new()
    }
}

impl FloodIt {
    pub fn new() -> Self {
        let mut rng = SeededRng::new(42);
        let grid = Self::generate_grid(DEFAULT_SIZE, &mut rng);
        Self {
            grid,
            moves: 0,
            max_moves: max_moves_for_size(DEFAULT_SIZE),
            state: GameState::Playing,
            rng,
            show_help: false,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    fn generate_grid(size: usize, rng: &mut SeededRng) -> Vec<Vec<u8>> {
        let mut grid = vec![vec![0u8; size]; size];
        for row in &mut grid {
            for cell in row.iter_mut() {
                *cell = rng.below(NUM_COLORS) as u8;
            }
        }
        grid
    }

    /// The board size. Read from the board, so it cannot be wrong.
    pub fn size(&self) -> usize {
        self.grid.len()
    }

    pub fn cell_count(&self) -> usize {
        self.grid.iter().map(Vec::len).sum()
    }

    fn at(&self, row: usize, col: usize) -> Option<u8> {
        self.grid.get(row).and_then(|r| r.get(col)).copied()
    }

    fn set_cell(&mut self, row: usize, col: usize, value: u8) {
        if let Some(cell) = self.grid.get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = value;
        }
    }

    /// The colour of the corner the flood grows from.
    ///
    /// An empty board has no corner. Nothing in the game can produce one, but
    /// the answer has to be *some* colour rather than a panic, and 0 is the one
    /// that makes "choose your own colour" a no-op rather than a move.
    pub fn head(&self) -> u8 {
        self.at(0, 0).unwrap_or(0)
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    pub fn max_moves(&self) -> u32 {
        self.max_moves
    }

    pub fn state(&self) -> GameState {
        self.state
    }

    pub fn show_help(&self) -> bool {
        self.show_help
    }

    /// The four orthogonal neighbours of (`row`, `col`) that are on the board.
    ///
    /// `wrapping_sub` on the way out of the top-left corner is deliberate: it
    /// produces `usize::MAX`, which fails the `< size` test below like any
    /// other off-board index, so there is one bounds check rather than two.
    fn neighbours(row: usize, col: usize) -> [(usize, usize); 4] {
        [
            (row.wrapping_sub(1), col),
            (row.saturating_add(1), col),
            (row, col.wrapping_sub(1)),
            (row, col.saturating_add(1)),
        ]
    }

    /// Every cell reachable from the corner through cells of colour `target`.
    fn region_of(&self, target: u8) -> Vec<(usize, usize)> {
        let size = self.size();
        if size == 0 || self.at(0, 0) != Some(target) {
            return Vec::new();
        }
        let mut visited = vec![vec![false; size]; size];
        let mut stack = vec![(0usize, 0usize)];
        let mut region = Vec::new();
        if let Some(seen) = visited.get_mut(0).and_then(|r| r.get_mut(0)) {
            *seen = true;
        }
        while let Some((r, c)) = stack.pop() {
            region.push((r, c));
            for (nr, nc) in Self::neighbours(r, c) {
                let on_board = nr < size && nc < size;
                let unseen = visited.get(nr).and_then(|row| row.get(nc)) == Some(&false);
                if on_board && unseen && self.at(nr, nc) == Some(target) {
                    if let Some(seen) = visited.get_mut(nr).and_then(|row| row.get_mut(nc)) {
                        *seen = true;
                    }
                    stack.push((nr, nc));
                }
            }
        }
        region
    }

    /// How many cells the player has claimed.
    pub fn filled_count(&self) -> usize {
        self.region_of(self.head()).len()
    }

    /// Would choosing `color` do anything?
    ///
    /// The answer is drawn as well as obeyed: a swatch that cannot move the
    /// game is dimmed, and it still takes its own click so the press stops
    /// there rather than falling through to whatever is behind it.
    pub fn can_choose(&self, color: usize) -> bool {
        self.state == GameState::Playing && color < NUM_COLORS && self.head() != color as u8
    }

    fn flood_fill(&mut self, new_color: u8) -> bool {
        if self.state != GameState::Playing {
            return false;
        }
        let old_color = self.head();
        if old_color == new_color {
            return false;
        }
        for (r, c) in self.region_of(old_color) {
            self.set_cell(r, c, new_color);
        }
        self.moves = self.moves.saturating_add(1);
        self.check_end();
        true
    }

    fn check_end(&mut self) {
        let first = self.head();
        let all_same = self.grid.iter().all(|row| row.iter().all(|&c| c == first));
        if all_same {
            self.state = GameState::Won;
        } else if self.moves >= self.max_moves {
            self.state = GameState::Lost;
        }
    }

    fn new_game(&mut self) {
        let size = self.size();
        self.grid = Self::generate_grid(size, &mut self.rng);
        self.max_moves = max_moves_for_size(size);
        self.moves = 0;
        self.state = GameState::Playing;
    }

    fn set_size(&mut self, size: usize) -> bool {
        if !SIZES.contains(&size) || size == self.size() {
            return false;
        }
        self.grid = Self::generate_grid(size, &mut self.rng);
        self.max_moves = max_moves_for_size(size);
        self.moves = 0;
        self.state = GameState::Playing;
        true
    }

    /// Can `action` change anything right now?
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Choose(c) => self.can_choose(c),
            Action::SetSize(n) => SIZES.contains(&n) && n != self.size(),
            Action::NewGame | Action::ToggleHelp => true,
        }
    }

    /// The one door every input goes through. Returns whether anything moved.
    pub fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::Choose(c) => c < NUM_COLORS && self.flood_fill(c as u8),
            Action::SetSize(n) => self.set_size(n),
            Action::NewGame => {
                self.new_game();
                true
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// What a click at (`x`, `y`) would land on, asked of the frame that would
    /// be drawn — not of a second copy of the geometry.
    pub fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.width, self.height).hit_test(x, y)
    }
}

// ── Drawing ─────────────────────────────────────────────────────────

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
        corner_radii: CornerRadii::all(radius.max(0.0)),
    });
}

fn stroke(f: &mut Frame, r: Rect, color: Color, line_width: f32, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii: CornerRadii::all(radius.max(0.0)),
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
    if size <= 0.0 || body.is_empty() {
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
        overflow: if max_width.is_some() {
            TextOverflow::Ellipsis
        } else {
            TextOverflow::Clip
        },
    });
}

/// Centre `body` within the horizontal span `[left, left + span)`.
///
/// The start is clamped to `left` because centring is a subtraction: a label
/// wider than its box centres to a negative offset and hangs half its overflow
/// off each side. Clamping, and handing the toolkit the width so it can elide
/// the tail, keeps an over-long label inside the thing it labels.
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

/// A button: its box, its label, and its hit target.
///
/// Recorded even when `enabled` is false. A dimmed button that lets the click
/// through is a button that does something else's job, and the board is
/// directly behind the footer.
fn button(
    f: &mut Frame,
    r: Rect,
    body: &str,
    size: f32,
    enabled: bool,
    active: bool,
    target: Target,
) {
    if r.is_empty() {
        return;
    }
    let bg = if active { COL_SURFACE1 } else { COL_SURFACE0 };
    fill(f, r, bg, 4.0);
    if active {
        stroke(f, r, COL_BLUE, 1.0, 4.0);
    }
    let fg = if enabled { COL_TEXT } else { COL_OVERLAY0 };
    centred_in(
        f,
        r.x,
        r.w,
        r.centre().1,
        body,
        size,
        fg,
        FontWeightHint::Regular,
    );
    f.hit(target, r);
}

impl FloodIt {
    /// The picture, and the hit boxes, for one window size.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);

        self.draw_header(&mut f, &l);
        if l.shows_palette() {
            self.draw_palette(&mut f, &l);
        }
        self.draw_info(&mut f, &l);
        self.draw_board(&mut f, &l);
        if self.state != GameState::Playing {
            self.draw_banner(&mut f, &l);
        }
        if l.shows_footer() {
            self.draw_footer(&mut f, &l);
        }
        // Last, so the reverse scan in `hit_test` finds it before anything it
        // covers, and so nothing it covers is painted over it.
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        if l.header.h < 8.0 {
            return;
        }
        fill(f, l.header, COL_MANTLE, 0.0);
        let btn = l.help_button();
        let title_span = (btn.x - l.header.x - l.pad * 2.0).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            l.header.centre().1 - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
            "Flood It",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_span),
        );
        button(
            f,
            btn,
            if self.show_help {
                "H  Close"
            } else {
                "H  Help"
            },
            l.small,
            true,
            self.show_help,
            Target::Help,
        );
    }

    fn draw_palette(&self, f: &mut Frame, l: &Layout) {
        for (i, color) in PALETTE.iter().enumerate() {
            let r = l.swatch(i);
            if r.is_empty() {
                continue;
            }
            fill(f, r, *color, 4.0);
            // A swatch that cannot move the game is greyed by a scrim rather
            // than by a different colour, because the colour is the thing it is
            // for: the player still has to be able to tell which one it is.
            if !self.can_choose(i) {
                fill(f, r, COL_SCRIM, 4.0);
            }
            if self.head() == i as u8 {
                stroke(f, r, COL_TEXT, 2.0, 4.0);
            }
            let body = format!(
                "{}  {}",
                i.saturating_add(1),
                PALETTE_LABELS.get(i).unwrap_or(&"?")
            );
            centred_in(
                f,
                r.x,
                r.w,
                r.centre().1,
                &body,
                l.small,
                COL_CRUST,
                FontWeightHint::Bold,
            );
            f.hit(Target::Swatch(i), r);
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        if l.info.h < 8.0 {
            return;
        }
        let body = format!(
            "Moves {}/{}     Filled {}/{}",
            self.moves,
            self.max_moves,
            self.filled_count(),
            self.cell_count()
        );
        centred_in(
            f,
            l.info.x + l.pad,
            (l.info.w - l.pad * 2.0).max(0.0),
            l.info.centre().1,
            &body,
            l.small,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.is_empty() {
            return;
        }
        let size = self.size();
        let inset = l.board.x - 3.0;
        fill(
            f,
            Rect::new(inset, l.board.y - 3.0, l.board.w + 6.0, l.board.h + 6.0),
            COL_CRUST,
            4.0,
        );
        let cs = l.board.w / (size.max(1) as f32);
        // Below a few pixels a cell is mostly the gap between cells, so the gap
        // goes rather than the colour.
        let gap = if cs >= MIN_GAPPED_CELL { 1.0 } else { 0.0 };
        let radius = if cs >= MIN_GAPPED_CELL { 2.0 } else { 0.0 };

        for (row, cells) in self.grid.iter().enumerate() {
            for (col, &value) in cells.iter().enumerate() {
                let cell = l.cell(size, row, col);
                let color = PALETTE.get(value as usize).copied().unwrap_or(COL_SURFACE0);
                fill(
                    f,
                    Rect::new(
                        cell.x + gap,
                        cell.y + gap,
                        (cell.w - gap * 2.0).max(0.0),
                        (cell.h - gap * 2.0).max(0.0),
                    ),
                    color,
                    radius,
                );
                f.hit(Target::Cell(row, col), cell);
            }
        }

        // The corner the flood grows from. Without it the rule "your region is
        // the one touching the top-left" is something the player has to be told
        // rather than something they can see.
        let corner = l.cell(size, 0, 0);
        if corner.w >= 6.0 {
            let d = corner.w * 0.34;
            fill(
                f,
                Rect::new(
                    corner.centre().0 - d / 2.0,
                    corner.centre().1 - d / 2.0,
                    d,
                    d,
                ),
                COL_CRUST,
                d / 2.0,
            );
        }
    }

    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let r = l.banner();
        if r.is_empty() {
            return;
        }
        fill(f, r, COL_BANNER, 4.0);
        let (body, color) = match self.state {
            GameState::Won => (format!("Flooded in {} moves", self.moves), COL_GREEN),
            GameState::Lost => (
                format!("Out of moves ({}/{})", self.moves, self.max_moves),
                COL_RED,
            ),
            GameState::Playing => return,
        };
        let line = text::line_height(l.font, FontWeightHint::Bold);
        centred_in(
            f,
            r.x + l.pad,
            (r.w - l.pad * 2.0).max(0.0),
            r.centre().1 - line * 0.4,
            &body,
            l.font,
            color,
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            r.x + l.pad,
            (r.w - l.pad * 2.0).max(0.0),
            r.centre().1 + line * 0.6,
            "N for a new board",
            l.small,
            COL_SUBTEXT0,
            FontWeightHint::Regular,
        );
        // It says to press N, so it may as well be N. Recorded after the cells
        // it covers, which is what puts it in front of them.
        f.hit(Target::NewGame, r);
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.footer, COL_MANTLE, 0.0);
        for (i, side) in SIZES.iter().enumerate() {
            let body = format!("{}  {side}", SIZE_KEY_LABELS.get(i).unwrap_or(&"?"));
            button(
                f,
                l.footer_button(i),
                &body,
                l.small,
                *side != self.size(),
                *side == self.size(),
                Target::Size(i),
            );
        }
        button(
            f,
            l.footer_button(SIZES.len()),
            "N  New",
            l.small,
            true,
            false,
            Target::NewGame,
        );
    }
}

/// One key per palette entry, in palette order.
const COLOR_KEYS: [Key; NUM_COLORS] = [
    Key::Num1,
    Key::Num2,
    Key::Num3,
    Key::Num4,
    Key::Num5,
    Key::Num6,
];

/// Not "Flood It" — the header already says that, three bands above, and a
/// panel that repeats the window title tells the reader nothing they opened it
/// to find out.
const HELP_TITLE: &str = "How to play";

const HELP_ROWS: [(&str, &str); 7] = [
    ("1 - 6", "Choose that colour"),
    ("Click", "A swatch, or any cell of that colour"),
    ("N", "New board, same size"),
    ("S / M / L / X", "Board 8 / 10 / 14 / 18"),
    ("H", "Show or hide this sheet"),
    ("Esc", "Close this sheet"),
    ("", "Cover the board before the moves run out."),
];

impl FloodIt {
    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        fill(f, l.window, COL_VEIL, 0.0);
        let p = l.help;
        fill(f, p, COL_SURFACE0, 8.0);
        stroke(f, p, COL_SURFACE1, 1.0, 8.0);

        let pad = l.pad.max(6.0);
        let inner = (p.w - pad * 2.0).max(0.0);
        let line = text::line_height(l.small, FontWeightHint::Regular).max(1.0);
        let mut y = p.y + pad;

        centred_in(
            f,
            p.x + pad,
            inner,
            y + line / 2.0,
            HELP_TITLE,
            l.font,
            COL_YELLOW,
            FontWeightHint::Bold,
        );
        y += line * 1.6;

        // The key column is a fixed share of the panel rather than a fixed
        // number of pixels, so a narrow window narrows both columns instead of
        // pushing the descriptions off the right-hand edge.
        let key_w = (inner * 0.42).max(0.0);
        let desc_x = p.x + pad + key_w;
        let desc_w = (inner - key_w).max(0.0);
        for (key, desc) in HELP_ROWS {
            if y + line > p.bottom() - pad {
                break;
            }
            label(
                f,
                p.x + pad,
                y,
                key,
                l.small,
                COL_BLUE,
                FontWeightHint::Bold,
                Some(key_w),
            );
            label(
                f,
                desc_x,
                y,
                desc,
                l.small,
                COL_SUBTEXT0,
                FontWeightHint::Regular,
                Some(desc_w),
            );
            y += line * 1.35;
        }

        // Over the whole window, not just the panel: a sheet that lets clicks
        // through to the board behind it is a sheet the player can lose a move
        // to while reading how to play.
        f.hit(Target::HelpSheet, l.window);
    }
}

// ── Input ───────────────────────────────────────────────────────────

impl FloodIt {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The second fault, in one line. A release is not a second press, and
        // treating it as one is what made `H` a key that opened the help sheet
        // and closed it again before it could be drawn.
        if !ev.pressed {
            return EventResult::Ignored;
        }
        let m = ev.modifiers;
        if m.ctrl || m.alt || m.super_key {
            return EventResult::Ignored;
        }

        if self.show_help {
            // The sheet has the keyboard for as long as it is up. Keys that do
            // not dismiss it are swallowed rather than acted on, so a player
            // reading the sheet cannot spend a move on the board behind it.
            return match ev.key {
                Key::H | Key::Escape | Key::Enter | Key::Space => {
                    self.apply(Action::ToggleHelp);
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }

        let action = match ev.key {
            Key::N => Some(Action::NewGame),
            Key::H => Some(Action::ToggleHelp),
            key => COLOR_KEYS
                .iter()
                .position(|k| *k == key)
                .map(Action::Choose)
                .or_else(|| {
                    SIZE_KEYS
                        .iter()
                        .position(|k| *k == key)
                        .and_then(|i| SIZES.get(i).copied())
                        .map(Action::SetSize)
                }),
        };

        match action {
            // A key the game knows is answered by the game even when the answer
            // is "not now" -- choosing the colour you already are is a legal
            // thing to ask for and a no-op, not an unhandled key.
            Some(a) => {
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
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        // Anywhere at all dismisses the sheet, including the sheet itself. It
        // is a thing you read and put down, and hunting for its close button is
        // not part of reading it.
        if self.show_help {
            self.apply(Action::ToggleHelp);
            return EventResult::Consumed;
        }
        match target {
            Target::Help | Target::HelpSheet => {
                self.apply(Action::ToggleHelp);
            }
            Target::NewGame => {
                self.apply(Action::NewGame);
            }
            Target::Size(i) => {
                if let Some(&side) = SIZES.get(i) {
                    self.apply(Action::SetSize(side));
                }
            }
            Target::Swatch(i) => {
                self.apply(Action::Choose(i));
            }
            // Clicking a cell picks that cell's colour. In a game whose entire
            // surface is a grid of the six things being chosen between, the
            // grid is the obvious place to choose them, and it was the one
            // place a click did nothing.
            Target::Cell(row, col) => {
                if let Some(value) = self.at(row, col) {
                    self.apply(Action::Choose(value as usize));
                }
            }
        }
        // Consumed either way. A disabled button that lets the click through is
        // a button that does the job of whatever is behind it, and behind the
        // footer is the board.
        EventResult::Consumed
    }
}

// ── Window wiring ───────────────────────────────────────────────────

/// One body for every event, whoever delivers it.
///
/// [`App::on_event`] and the [`Probe`] impl both call this, so a test that
/// clicks a swatch is a test of the shipped program rather than of a second
/// implementation written to make the test pass. The ten-pixel gap between the
/// old renderer and the old click handler is exactly the shape of bug two
/// bodies produce.
pub fn handle_event(app: &mut FloodIt, event: &Event) -> EventResult {
    match event {
        Event::Key(key) => app.handle_key(key),
        Event::Mouse(mouse) => app.handle_mouse(mouse),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for FloodIt {
    fn on_event(&mut self, event: &Event) -> Response {
        match handle_event(self, event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The size the compositor is actually asking for, recorded before it is
        // drawn against, so a click arriving before the next `Resize` is tested
        // against the picture the player is looking at.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }

    fn title(&self) -> String {
        String::from("Flood It")
    }

    fn app_id(&self) -> String {
        String::from("flood")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }
}

impl Probe for FloodIt {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
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

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.resize(size.0, size.1);
        handle_event(self, &Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = FloodIt::new();
    app::launch("flood", &mut app)
}

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

    /// Window sizes to sweep, from a large desktop down past anything a
    /// compositor would sensibly hand out. The small end is the interesting
    /// end: it is where a layout that was really a set of constants falls off
    /// its own window.
    const WINDOWS: [(f32, f32); 9] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (720.0, 640.0),
        (500.0, 400.0),
        (360.0, 300.0),
        (240.0, 200.0),
        (160.0, 140.0),
        (100.0, 90.0),
        (24.0, 24.0),
    ];

    fn windowed(width: f32, height: f32) -> FloodIt {
        let mut app = FloodIt::new();
        app.resize(width, height);
        app
    }

    fn release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// A press delivered straight to the shipped handler.
    ///
    /// Not `probe::key`, which resizes the app to `Probe::SIZE` first -- fine
    /// almost everywhere, and exactly wrong in a test whose subject is the size
    /// the app is currently at.
    fn press(app: &mut FloodIt, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn click(app: &mut FloodIt, x: f32, y: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    /// The strings a frame actually draws.
    fn texts(f: &Frame) -> Vec<String> {
        f.commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Put the app in a state where every colour but the current one is a legal
    /// move and no move can end the game, so a test of *input* is not also a
    /// test of the win condition.
    fn mid_game(app: &mut FloodIt) {
        app.grid = vec![
            vec![0, 1, 2, 3],
            vec![4, 5, 0, 1],
            vec![2, 3, 4, 5],
            vec![0, 1, 2, 3],
        ];
        app.max_moves = 50;
        app.moves = 0;
        app.state = GameState::Playing;
    }

    // ── The faults this file was rewritten for ──────────────────────

    #[test]
    fn a_release_is_not_a_second_press() {
        // The whole of fault two. The old handler matched `Event::Key` without
        // looking at `pressed`, so H opened the help sheet on the way down and
        // closed it on the way up: the sheet existed and could not be seen.
        let mut app = FloodIt::new();
        assert!(!app.show_help());
        press(&mut app, Key::H);
        assert!(app.show_help(), "H did not open the help sheet");
        handle_event(&mut app, &Event::Key(release(Key::H)));
        assert!(
            app.show_help(),
            "letting go of H closed the help sheet again"
        );
    }

    #[test]
    fn the_help_sheet_can_actually_be_opened() {
        let mut app = FloodIt::new();
        press(&mut app, Key::H);
        let drawn = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for (key, desc) in HELP_ROWS {
            if !key.is_empty() {
                assert!(drawn.iter().any(|t| t == key), "help row {key} missing");
            }
            assert!(
                drawn.iter().any(|t| t == desc),
                "help description {desc:?} missing"
            );
        }
    }

    #[test]
    fn a_key_makes_one_board_not_two() {
        // N ran on the press and again on the release, so the board the player
        // was shown was the second one generated and the generator ran at twice
        // the rate the game asked it to. One press, one board.
        let mut pressed_only = FloodIt::new();
        press(&mut pressed_only, Key::N);

        let mut pressed_and_released = FloodIt::new();
        press(&mut pressed_and_released, Key::N);
        handle_event(&mut pressed_and_released, &Event::Key(release(Key::N)));

        assert_eq!(
            pressed_only.grid, pressed_and_released.grid,
            "letting go of N dealt a second board"
        );
    }

    #[test]
    fn a_size_key_makes_one_board_not_two() {
        let mut pressed_only = FloodIt::new();
        press(&mut pressed_only, Key::S);

        let mut pressed_and_released = FloodIt::new();
        press(&mut pressed_and_released, Key::S);
        handle_event(&mut pressed_and_released, &Event::Key(release(Key::S)));

        assert_eq!(pressed_only.size(), 8);
        assert_eq!(pressed_only.grid, pressed_and_released.grid);
    }

    #[test]
    fn what_is_drawn_at_a_swatch_is_what_a_click_there_selects() {
        // Fault three. The renderer drew the row at y in [56, 86] and the click
        // handler tested y in [60, 96]: the top four pixels of every swatch did
        // nothing and the ten below it chose the colour. There is now one
        // rectangle per swatch and the renderer is what records it.
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..NUM_COLORS {
            let r = l.swatch(i);
            let drawn = f
                .hits()
                .iter()
                .find(|(t, _)| *t == Target::Swatch(i))
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("swatch {i} recorded no hit box"));
            assert_eq!((drawn.x, drawn.y, drawn.w, drawn.h), (r.x, r.y, r.w, r.h));
            // Every corner of the drawn box, inset by a hair, is the swatch.
            for (px, py) in [
                (r.x + 0.5, r.y + 0.5),
                (r.right() - 0.5, r.y + 0.5),
                (r.x + 0.5, r.bottom() - 0.5),
                (r.right() - 0.5, r.bottom() - 0.5),
            ] {
                assert_eq!(
                    app.target_at(px, py),
                    Some(Target::Swatch(i)),
                    "({px}, {py}) is drawn as swatch {i} and does not select it"
                );
            }
        }
    }

    #[test]
    fn the_strip_below_a_swatch_is_not_the_swatch() {
        // The old handler's band ran ten pixels past the drawn one, which put
        // the "this is your colour" underline inside the button it underlined.
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        for i in 0..NUM_COLORS {
            let r = l.swatch(i);
            for below in [1.0_f32, 4.0, 9.0] {
                let hit = app.target_at(r.centre().0, r.bottom() + below);
                assert_ne!(
                    hit,
                    Some(Target::Swatch(i)),
                    "{below} pixels under swatch {i} still chose it"
                );
            }
        }
    }

    #[test]
    fn every_board_size_is_reachable() {
        // Size 10 was accepted by `set_size`, had its own move budget, and had
        // no key and no button. The list is now the single source of both.
        assert_eq!(SIZES.len(), SIZE_KEYS.len());
        assert_eq!(SIZES.len(), SIZE_KEY_LABELS.len());
        for (i, &side) in SIZES.iter().enumerate() {
            let mut by_key = FloodIt::new();
            press(&mut by_key, SIZE_KEYS[i]);
            assert_eq!(by_key.size(), side, "key for size {side} did not set it");

            let mut by_click = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).footer_button(i);
            click(&mut by_click, r.centre().0, r.centre().1);
            assert_eq!(
                by_click.size(),
                side,
                "footer button {i} did not set size {side}"
            );
        }
        assert!(
            SIZES.contains(&10),
            "the size that had no key is still offered"
        );
    }

    #[test]
    fn the_board_is_its_own_size() {
        // `size` used to be a second field nothing kept in step with the board.
        let mut app = FloodIt::new();
        for &side in &SIZES {
            app.apply(Action::SetSize(side));
            assert_eq!(app.size(), side);
            assert_eq!(app.grid.len(), side);
            for row in &app.grid {
                assert_eq!(row.len(), side);
            }
            assert_eq!(app.cell_count(), side * side);
        }
    }

    #[test]
    fn the_board_is_where_a_click_on_it_lands() {
        // The grid was the one surface in the game that a click could not
        // reach, in a game that is nothing but a grid of the six choices.
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let size = app.size();
        for row in 0..size {
            for col in 0..size {
                let c = l.cell(size, row, col);
                assert_eq!(
                    app.target_at(c.centre().0, c.centre().1),
                    Some(Target::Cell(row, col)),
                    "cell ({row}, {col}) is drawn where nothing is clickable"
                );
            }
        }
    }

    #[test]
    fn clicking_a_cell_chooses_its_colour() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        // (1, 1) holds 5 and the corner holds 0, so this is a real move.
        let c = l.cell(app.size(), 1, 1);
        assert_eq!(app.at(1, 1), Some(5));
        click(&mut app, c.centre().0, c.centre().1);
        assert_eq!(app.head(), 5);
        assert_eq!(app.moves(), 1);
    }

    #[test]
    fn clicking_a_cell_of_your_own_colour_is_not_a_move() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let corner = l.cell(app.size(), 0, 0);
        assert_eq!(
            click(&mut app, corner.centre().0, corner.centre().1),
            EventResult::Consumed,
            "the click landed on the board and should be answered by it"
        );
        assert_eq!(
            app.moves(),
            0,
            "flooding to the colour you already are cost a move"
        );
    }

    // ── The window ──────────────────────────────────────────────────

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        // Fault four: `render` took a width and a height and used them for the
        // background rectangle. Everything else was a constant, so the only
        // window the program was correct in was the one it was written on.
        for (w, h) in WINDOWS {
            for &side in &SIZES {
                for help in [false, true] {
                    for state in [GameState::Playing, GameState::Won, GameState::Lost] {
                        let mut app = windowed(w, h);
                        app.apply(Action::SetSize(side));
                        app.state = state;
                        app.show_help = help;
                        let f = app.frame(w, h);
                        assert!(
                            f.is_balanced(),
                            "{w}x{h} board {side} help {help} {state:?} left a clip or \
                             translate open"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_whole_window_is_painted() {
        for (w, h) in WINDOWS {
            let app = windowed(w, h);
            let f = app.frame(w, h);
            let first = f.commands().first().expect("nothing was drawn");
            match first {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    assert!(
                        *x <= 0.0 && *y <= 0.0 && *width >= w && *height >= h,
                        "{w}x{h} left a corner of the window unpainted"
                    );
                }
                other => panic!("{w}x{h} did not start with a background: {other:?}"),
            }
        }
    }

    #[test]
    fn the_board_is_square_and_inside_its_window() {
        // A stretched board would put a cell's colour somewhere other than the
        // cell the player aimed at, and a board that overhangs its window is
        // rows the player can see the effect of and never click.
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                (l.board.w - l.board.h).abs() < 0.01,
                "{w}x{h} board is {}x{}",
                l.board.w,
                l.board.h
            );
            assert!(
                l.board.w >= 0.0 && l.board.h >= 0.0,
                "{w}x{h} board is negative"
            );
            assert!(
                l.board.x >= -0.01
                    && l.board.y >= -0.01
                    && l.board.right() <= w + 0.01
                    && l.board.bottom() <= h + 0.01,
                "{w}x{h} board {:?} hangs off the window",
                (l.board.x, l.board.y, l.board.right(), l.board.bottom())
            );
        }
    }

    #[test]
    fn no_band_is_laid_past_the_bottom_of_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("header", l.header),
                ("palette", l.palette),
                ("info", l.info),
                ("footer", l.footer),
            ] {
                assert!(
                    r.bottom() <= h + 0.01,
                    "{w}x{h} {name} ends at {} past the window",
                    r.bottom()
                );
            }
        }
    }

    #[test]
    fn a_window_too_short_for_the_footer_drops_it_rather_than_the_board() {
        // The board is the game and every footer button has a key; no key has a
        // board. When something has to go, it is the buttons.
        let cramped = Layout::new(300.0, 100.0);
        assert!(
            !cramped.shows_footer(),
            "a 100-pixel-tall window still claimed room for the footer"
        );
        assert!(
            cramped.board.w > 20.0,
            "the board was squeezed to {} instead",
            cramped.board.w
        );
    }

    #[test]
    fn a_click_is_read_against_the_window_it_was_drawn_in() {
        // The old click handler tested against numbers it carried itself, so it
        // answered for an 800x600 window whatever size the window was.
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            mid_game(&mut app);
            let l = Layout::new(w, h);
            if !l.shows_palette() {
                continue;
            }
            let r = l.swatch(1);
            if r.is_empty() {
                continue;
            }
            click(&mut app, r.centre().0, r.centre().1);
            assert_eq!(
                app.head(),
                1,
                "at {w}x{h} a click on the second swatch did not choose it"
            );
        }
    }

    #[test]
    fn a_resize_is_remembered_across_an_action() {
        let mut app = FloodIt::new();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 500,
                height: 400,
            },
        );
        // `probe::key` would resize to `Probe::SIZE` first, which is the very
        // thing being asserted.
        press(&mut app, Key::N);
        assert_eq!((app.width, app.height), (500.0, 400.0));
    }

    #[test]
    fn a_board_the_size_of_a_postage_stamp_still_draws_every_cell() {
        // At 24x24 a cell of an 18-board is barely a pixel. The gap between
        // cells goes before the colour does, because a cell that is all gap is
        // a cell that is not there.
        let mut app = windowed(24.0, 24.0);
        app.apply(Action::SetSize(18));
        let f = app.frame(24.0, 24.0);
        let cells = f
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::Cell(_, _)))
            .count();
        assert_eq!(cells, 18 * 18, "cells went missing in a tiny window");
        for (_, r) in f
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::Cell(_, _)))
        {
            assert!(r.w > 0.0 && r.h > 0.0, "a cell was laid out with no area");
        }
    }

    #[test]
    fn the_banner_stays_inside_the_board_it_covers() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            let b = l.banner();
            assert!(
                b.x >= l.board.x - 0.01
                    && b.right() <= l.board.right() + 0.01
                    && b.bottom() <= l.board.bottom() + 0.01
                    && b.y >= l.board.y - 0.01,
                "{w}x{h} banner {b:?} is outside the board {:?}",
                l.board
            );
        }
    }

    #[test]
    fn the_help_sheet_never_writes_past_its_own_panel() {
        // The sheet's own rows, identified by what they say rather than by
        // where they are -- a positional filter would also catch the footer
        // labels, which are outside the panel because that is where they go.
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            app.show_help = true;
            let l = Layout::new(w, h);
            let f = app.frame(w, h);
            for c in f.commands() {
                let RenderCommand::Text { x, y, text: t, .. } = c else {
                    continue;
                };
                let is_sheet = t == HELP_TITLE || HELP_ROWS.iter().any(|(k, d)| t == k || t == d);
                if !is_sheet {
                    continue;
                }
                assert!(
                    *x >= l.help.x - 0.01 && *y >= l.help.y - 0.01 && *y <= l.help.bottom(),
                    "{w}x{h}: {t:?} starts at ({x}, {y}), outside the panel {:?}",
                    (l.help.x, l.help.y, l.help.right(), l.help.bottom())
                );
            }
        }
    }

    #[test]
    fn the_help_sheet_is_in_front_of_everything_it_covers() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let swatch = l.swatch(0);
        let cell = l.cell(app.size(), 3, 3);
        assert_eq!(
            app.target_at(swatch.centre().0, swatch.centre().1),
            Some(Target::Swatch(0))
        );
        app.show_help = true;
        for (x, y) in [
            (swatch.centre().0, swatch.centre().1),
            (cell.centre().0, cell.centre().1),
            (l.help.centre().0, l.help.centre().1),
            (1.0, 1.0),
            (WINDOW_WIDTH - 1.0, WINDOW_HEIGHT - 1.0),
        ] {
            assert_eq!(
                app.target_at(x, y),
                Some(Target::HelpSheet),
                "({x}, {y}) reached past the help sheet"
            );
        }
    }

    // ── Input ───────────────────────────────────────────────────────

    #[test]
    fn every_swatch_is_the_colour_it_shows() {
        for i in 0..NUM_COLORS {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            mid_game(&mut app);
            let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).swatch(i);
            click(&mut app, r.centre().0, r.centre().1);
            assert_eq!(
                app.head(),
                i as u8,
                "clicking swatch {i} did not flood with colour {i}"
            );
        }
    }

    #[test]
    fn every_colour_key_is_the_swatch_it_names() {
        for i in 0..NUM_COLORS {
            let mut by_key = FloodIt::new();
            mid_game(&mut by_key);
            press(&mut by_key, COLOR_KEYS[i]);

            let mut by_click = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            mid_game(&mut by_click);
            let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).swatch(i);
            click(&mut by_click, r.centre().0, r.centre().1);

            assert_eq!(
                by_key.grid, by_click.grid,
                "key {i} and swatch {i} do not do the same thing"
            );
        }
    }

    #[test]
    fn a_swatch_you_already_are_is_dimmed_and_still_takes_its_click() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        let head = app.head() as usize;
        assert!(
            !app.can_choose(head),
            "your own colour is offered as a move"
        );
        let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).swatch(head);
        assert_eq!(
            app.target_at(r.centre().0, r.centre().1),
            Some(Target::Swatch(head)),
            "the dimmed swatch let the click through to whatever is behind it"
        );
        assert_eq!(
            click(&mut app, r.centre().0, r.centre().1),
            EventResult::Consumed
        );
        assert_eq!(app.moves(), 0);
    }

    #[test]
    fn the_new_game_button_deals_a_new_board() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.apply(Action::Choose(if app.head() == 0 { 1 } else { 0 }));
        assert_eq!(app.moves(), 1);
        let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).footer_button(SIZES.len());
        click(&mut app, r.centre().0, r.centre().1);
        assert_eq!(app.moves(), 0);
        assert_eq!(app.state(), GameState::Playing);
        assert_eq!(app.size(), DEFAULT_SIZE, "New game changed the board size");
    }

    #[test]
    fn the_help_button_opens_and_closes_the_sheet() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).help_button();
        click(&mut app, r.centre().0, r.centre().1);
        assert!(app.show_help());
        // Anywhere at all closes it again, including where the button was.
        click(&mut app, r.centre().0, r.centre().1);
        assert!(!app.show_help());
    }

    #[test]
    fn keys_do_nothing_behind_the_help_sheet() {
        let mut app = FloodIt::new();
        mid_game(&mut app);
        app.show_help = true;
        let before = app.grid.clone();
        for key in [Key::Num1, Key::Num4, Key::N, Key::S, Key::X] {
            assert_eq!(
                press(&mut app, key),
                EventResult::Consumed,
                "{key:?} was passed on past the help sheet"
            );
        }
        assert_eq!(
            app.grid, before,
            "a key reached the board through the sheet"
        );
        assert_eq!(app.moves(), 0);
        assert!(app.show_help(), "an ordinary key closed the sheet");
    }

    #[test]
    fn the_keys_that_dismiss_the_sheet_dismiss_it() {
        for key in [Key::H, Key::Escape, Key::Enter, Key::Space] {
            let mut app = FloodIt::new();
            app.show_help = true;
            press(&mut app, key);
            assert!(!app.show_help(), "{key:?} did not close the help sheet");
        }
    }

    #[test]
    fn a_click_does_not_reach_the_board_through_the_help_sheet() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        app.show_help = true;
        let before = app.grid.clone();
        let cell = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).cell(app.size(), 2, 2);
        click(&mut app, cell.centre().0, cell.centre().1);
        assert_eq!(
            app.grid, before,
            "the click flooded the board behind the sheet"
        );
        assert_eq!(app.moves(), 0);
        assert!(!app.show_help(), "the click did not put the sheet down");
    }

    #[test]
    fn a_key_the_game_does_not_know_is_left_for_someone_else() {
        let mut app = FloodIt::new();
        for key in [Key::Q, Key::Tab, Key::F1, Key::Up] {
            assert_eq!(
                press(&mut app, key),
                EventResult::Ignored,
                "{key:?} was claimed by a game that does nothing with it"
            );
        }
    }

    #[test]
    fn a_modified_key_is_left_for_someone_else() {
        // Ctrl-N is the window manager's, not a new board.
        let mut app = FloodIt::new();
        app.moves = 3;
        for m in [
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                super_key: true,
                ..Modifiers::NONE
            },
        ] {
            let ev = KeyEvent {
                key: Key::N,
                pressed: true,
                modifiers: m,
                text: String::new(),
            };
            assert_eq!(
                handle_event(&mut app, &Event::Key(ev)),
                EventResult::Ignored
            );
        }
        assert_eq!(app.moves(), 3, "a modified key started a new game");
    }

    #[test]
    fn a_click_that_is_not_a_left_press_is_ignored() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        mid_game(&mut app);
        let r = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).swatch(1);
        for kind in [
            MouseEventKind::Move,
            MouseEventKind::Release(MouseButton::Left),
            MouseEventKind::Press(MouseButton::Right),
        ] {
            let ev = Event::Mouse(MouseEvent {
                x: r.centre().0,
                y: r.centre().1,
                kind: kind.clone(),
            });
            assert_eq!(
                handle_event(&mut app, &ev),
                EventResult::Ignored,
                "{kind:?}"
            );
        }
        assert_eq!(app.moves(), 0);
    }

    #[test]
    fn a_click_on_nothing_is_left_for_someone_else() {
        // The gap between the board and the window edge belongs to nobody.
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(app.target_at(1.0, l.info.centre().1), None);
    }

    #[test]
    fn enabled_and_apply_agree() {
        // A button drawn live that does nothing, or drawn dead that does
        // something, is a button that lies about the game's state.
        let mut probe_actions = Vec::new();
        for i in 0..NUM_COLORS {
            probe_actions.push(Action::Choose(i));
        }
        for &side in &SIZES {
            probe_actions.push(Action::SetSize(side));
        }
        probe_actions.push(Action::NewGame);
        probe_actions.push(Action::ToggleHelp);

        for state in [GameState::Playing, GameState::Won, GameState::Lost] {
            for action in &probe_actions {
                let mut app = FloodIt::new();
                mid_game(&mut app);
                app.state = state;
                let claimed = app.enabled(*action);
                let happened = app.apply(*action);
                assert_eq!(
                    claimed, happened,
                    "{action:?} in {state:?}: enabled said {claimed}, apply did {happened}"
                );
            }
        }
    }

    // ── The rules ───────────────────────────────────────────────────

    #[test]
    fn a_new_game_starts_playable() {
        let app = FloodIt::new();
        assert_eq!(app.size(), DEFAULT_SIZE);
        assert_eq!(app.moves(), 0);
        assert_eq!(app.max_moves(), 25);
        assert_eq!(app.state(), GameState::Playing);
        assert!(!app.show_help());
    }

    #[test]
    fn every_cell_is_a_colour_the_palette_has() {
        let mut app = FloodIt::new();
        for &side in &SIZES {
            app.apply(Action::SetSize(side));
            for row in &app.grid {
                for &c in row {
                    assert!((c as usize) < NUM_COLORS, "cell holds colour {c}");
                }
            }
        }
    }

    #[test]
    fn each_size_has_its_own_move_budget() {
        assert_eq!(max_moves_for_size(8), 14);
        assert_eq!(max_moves_for_size(10), 20);
        assert_eq!(max_moves_for_size(14), 25);
        assert_eq!(max_moves_for_size(18), 35);
        let mut app = FloodIt::new();
        for &side in &SIZES {
            app.apply(Action::SetSize(side));
            assert_eq!(app.max_moves(), max_moves_for_size(side));
        }
    }

    #[test]
    fn changing_size_resets_the_game_and_staying_put_does_not() {
        let mut app = FloodIt::new();
        app.apply(Action::Choose(if app.head() == 0 { 1 } else { 0 }));
        assert_eq!(app.moves(), 1);
        let before = app.grid.clone();
        assert!(!app.apply(Action::SetSize(DEFAULT_SIZE)));
        assert_eq!(
            app.grid, before,
            "asking for the size you are on dealt a board"
        );
        assert_eq!(app.moves(), 1);
        assert!(app.apply(Action::SetSize(8)));
        assert_eq!(app.moves(), 0);
        assert_eq!(app.size(), 8);
    }

    #[test]
    fn a_size_the_game_does_not_offer_is_refused() {
        let mut app = FloodIt::new();
        assert!(!app.apply(Action::SetSize(5)));
        assert!(!app.apply(Action::SetSize(0)));
        assert_eq!(app.size(), DEFAULT_SIZE);
    }

    #[test]
    fn flooding_your_own_colour_is_free() {
        let mut app = FloodIt::new();
        let before = app.grid.clone();
        assert!(!app.apply(Action::Choose(app.head() as usize)));
        assert_eq!(app.moves(), 0);
        assert_eq!(app.grid, before);
    }

    #[test]
    fn a_flood_takes_the_whole_connected_region_and_nothing_else() {
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 0, 1], vec![0, 1, 1], vec![2, 2, 2]];
        app.max_moves = 10;
        app.moves = 0;
        assert!(app.apply(Action::Choose(1)));
        // (0,0), (0,1) and (1,0) were the region; (2,0) was never in it.
        assert_eq!(app.at(0, 0), Some(1));
        assert_eq!(app.at(0, 1), Some(1));
        assert_eq!(app.at(1, 0), Some(1));
        assert_eq!(app.at(2, 0), Some(2));
        assert_eq!(app.moves(), 1);
    }

    #[test]
    fn the_filled_count_is_the_region_touching_the_corner() {
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 0, 1], vec![0, 1, 1], vec![2, 2, 2]];
        assert_eq!(app.filled_count(), 3);
        // A cell of the same colour that the region cannot reach is not yours.
        app.grid = vec![vec![0, 1, 0], vec![1, 1, 1], vec![0, 1, 0]];
        assert_eq!(app.filled_count(), 1);
        app.grid = vec![vec![0; 3]; 3];
        assert_eq!(app.filled_count(), 9);
    }

    #[test]
    fn covering_the_board_wins_it() {
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 1], vec![0, 0]];
        app.max_moves = 10;
        app.moves = 0;
        app.state = GameState::Playing;
        app.apply(Action::Choose(1));
        assert_eq!(app.state(), GameState::Won);
        assert_eq!(app.filled_count(), app.cell_count());
    }

    #[test]
    fn running_out_of_moves_loses_it() {
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 1, 2], vec![3, 4, 5], vec![0, 1, 2]];
        app.max_moves = 1;
        app.moves = 0;
        app.state = GameState::Playing;
        app.apply(Action::Choose(1));
        assert_eq!(app.state(), GameState::Lost);
    }

    #[test]
    fn the_last_move_can_still_win() {
        // The win is checked before the move limit, so filling the board with
        // the final move is a win and not a loss on a technicality.
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 1], vec![1, 1]];
        app.max_moves = 1;
        app.moves = 0;
        app.state = GameState::Playing;
        app.apply(Action::Choose(1));
        assert_eq!(app.state(), GameState::Won);
        assert_eq!(app.moves(), app.max_moves());
    }

    #[test]
    fn a_finished_game_takes_no_more_moves() {
        for state in [GameState::Won, GameState::Lost] {
            let mut app = FloodIt::new();
            mid_game(&mut app);
            app.state = state;
            let before = app.grid.clone();
            for i in 0..NUM_COLORS {
                assert!(!app.apply(Action::Choose(i)));
            }
            assert_eq!(app.moves(), 0);
            assert_eq!(app.grid, before);
        }
    }

    #[test]
    fn a_colour_the_palette_does_not_have_is_refused() {
        let mut app = FloodIt::new();
        assert!(!app.apply(Action::Choose(NUM_COLORS)));
        assert!(!app.apply(Action::Choose(99)));
        assert_eq!(app.moves(), 0);
    }

    #[test]
    fn a_small_board_can_be_played_to_a_win() {
        let mut app = FloodIt::new();
        app.grid = vec![vec![0, 1], vec![2, 3]];
        app.max_moves = 10;
        app.moves = 0;
        app.state = GameState::Playing;
        app.apply(Action::Choose(1));
        app.apply(Action::Choose(2));
        app.apply(Action::Choose(3));
        assert_eq!(app.state(), GameState::Won);
        assert_eq!(app.moves(), 3);
    }

    #[test]
    fn a_finished_game_says_so_and_offers_a_way_out() {
        for (state, needle) in [
            (GameState::Won, "Flooded in"),
            (GameState::Lost, "Out of moves"),
        ] {
            let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
            app.state = state;
            let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
            assert!(
                texts(&f).iter().any(|t| t.starts_with(needle)),
                "{state:?} drew no banner"
            );
            let b = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).banner();
            assert_eq!(
                app.target_at(b.centre().0, b.centre().1),
                Some(Target::NewGame),
                "the banner says to press N and is not N"
            );
        }
    }

    #[test]
    fn a_game_in_play_draws_no_banner() {
        let app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(!drawn.iter().any(|t| t.starts_with("Flooded in")));
        assert!(!drawn.iter().any(|t| t.starts_with("Out of moves")));
    }

    #[test]
    fn the_readout_counts_the_board_it_is_drawn_beside() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        for &side in &SIZES {
            app.apply(Action::SetSize(side));
            let want = format!(
                "Moves {}/{}     Filled {}/{}",
                app.moves(),
                app.max_moves(),
                app.filled_count(),
                side * side
            );
            assert!(
                texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT)).contains(&want),
                "board {side} did not report {want:?}"
            );
        }
    }

    #[test]
    fn the_footer_marks_the_size_being_played() {
        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        for (i, &side) in SIZES.iter().enumerate() {
            app.apply(Action::SetSize(side));
            assert!(
                !app.enabled(Action::SetSize(side)),
                "size {side} offers itself"
            );
            for (j, &other) in SIZES.iter().enumerate() {
                if i != j {
                    assert!(app.enabled(Action::SetSize(other)), "size {other} refused");
                }
            }
        }
    }

    // ── The board deal ──────────────────────────────────────────────

    #[test]
    fn the_same_seed_deals_the_same_board() {
        let mut r1 = SeededRng::new(42);
        let g1 = FloodIt::generate_grid(5, &mut r1);
        let mut r2 = SeededRng::new(42);
        let g2 = FloodIt::generate_grid(5, &mut r2);
        assert_eq!(g1, g2);
    }

    #[test]
    fn horizontally_adjacent_cells_can_match() {
        // The old draw made this literally impossible: consecutive draws had
        // opposite parity, `x % 6` preserves parity, and the grid is filled in
        // row-major order, so no two cells in a row ever shared a colour. Zero
        // matches in 61 200 pairs.
        let mut matches = 0_u32;
        let mut pairs = 0_u32;
        for seed in 0..200_u64 {
            let mut rng = SeededRng::new(seed);
            let grid = FloodIt::generate_grid(14, &mut rng);
            for row in &grid {
                for pair in row.windows(2) {
                    pairs += 1;
                    if let [a, b] = pair
                        && a == b
                    {
                        matches += 1;
                    }
                }
            }
        }
        // One pair in six should match; anything above a twentieth rules out
        // the fixed-parity board without being brittle.
        assert!(
            u64::from(matches) * 20 > u64::from(pairs),
            "only {matches} of {pairs} horizontally-adjacent pairs matched"
        );
    }

    #[test]
    fn a_column_is_not_confined_to_half_the_palette() {
        // Under the old draw a column could only ever show three of the six
        // colours -- the even ones {R, Y, T} or the odd ones {O, G, M} -- and
        // adjacent columns took opposite halves. Asking for all six in a
        // single column would be flaky (an 18-cell column misses a given
        // colour 3.7% of the time by chance); asking that it is not confined
        // to one parity class is not. A fair 18-cell column lands entirely in
        // one class with probability 2^-17.
        const SIZE: usize = 18;
        for seed in 0..50_u64 {
            let mut rng = SeededRng::new(seed);
            let grid = FloodIt::generate_grid(SIZE, &mut rng);
            for col in 0..SIZE {
                let mut parities = std::collections::BTreeSet::new();
                for row in 0..SIZE {
                    if let Some(&cell) = grid.get(row).and_then(|r| r.get(col)) {
                        parities.insert(cell % 2);
                    }
                }
                assert_eq!(
                    parities.len(),
                    2,
                    "seed {seed} column {col} used only half the palette"
                );
            }
        }
    }

    #[test]
    fn the_starting_blob_is_worth_having() {
        // The parity bug left the corner region averaging 1.48 cells and able
        // to grow only downwards, in a game whose entire subject is growing it.
        let mut total = 0_usize;
        const DEALS: usize = 60;
        for seed in 0..DEALS as u64 {
            let mut app = FloodIt::new();
            app.rng = SeededRng::new(seed);
            app.apply(Action::NewGame);
            total += app.filled_count();
        }
        let mean = total as f32 / DEALS as f32;
        assert!(
            mean > 1.6,
            "the corner region averages {mean} cells over {DEALS} deals"
        );
    }
}
