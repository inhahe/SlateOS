//! Lights Out — a puzzle where switching a light also switches its neighbours.
//!
//! Turn every light off. Clicking a cell flips it and the four cells
//! orthogonally beside it. Boards are 3x3, 5x5 or 7x7, and every board is
//! built by *applying* flips to a solved grid, so replaying those flips
//! always solves it.
//!
//! ## What wiring this up found
//!
//! The program had a complete game inside it and no way to reach it. `main`
//! built a `LightsOut`, dropped it and exited: no board ever reached a
//! screen and no key or click ever arrived. Underneath that, four faults:
//!
//! 1. **Every key fired twice.** The handler matched
//!    `Event::Key(KeyEvent { key, modifiers, .. })` and never read `pressed`,
//!    so each key ran a second time on release. The consequences were not
//!    cosmetic:
//!    - **Enter and Space could not play the game at all.** Flipping a cell
//!      twice restores the board exactly, so a keypress changed nothing —
//!      while charging two moves for it. The whole keyboard route to the one
//!      verb this game has was dead.
//!    - **The cursor moved two cells per press**, so on a 5x5 board starting
//!      at the centre only the nine even-indexed cells were reachable; the
//!      other sixteen could not be selected from the keyboard.
//!    - **`H` opened the help on press and closed it on release**, so the
//!      help panel could never be seen — and it was the only place the
//!      controls were written down.
//!    - **`N` and `3`/`5`/`7` dealt two boards per press**, throwing the
//!      first away.
//! 2. **Only the board was clickable.** No new game, no board size, no help
//!    — every one of those was keyboard-only, and the keyboard was broken.
//! 3. **The layout was a constant.** `render(width, height)` used its
//!    arguments for the background rectangle alone; the grid sat at a fixed
//!    (50, 90) and `cell_size()` was a lookup table keyed on board size. On a
//!    window narrower than 800 the best-scores panel hung off the right edge,
//!    and on a taller one the game stayed in the top-left corner. There is
//!    now one `Layout` derived from the live window on every frame, and the
//!    painter records each hit box as it draws, so there is no second copy of
//!    the geometry left to disagree with the first.
//! 4. **`size` was a second copy of `grid.len()`** that nothing kept in
//!    agreement. The board is now its own size.
//!
//! `level` counted the boards dealt and was never drawn, which the file's
//! blanket `#![allow(dead_code)]` is what let it sit there unnoticed. It is
//! drawn now.

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
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COL_OVERLAY: Color = Color::from_hex(0x6C7086);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

/// A lit cell, and an unlit one.
const LIGHT_ON: Color = COL_YELLOW;
const LIGHT_OFF: Color = COL_SURFACE0;
/// The keyboard cursor's ring. Distinct in hue from both light states, so it
/// reads as "you are here" rather than as a third kind of light.
const CURSOR_COLOR: Color = COL_BLUE;

/// Translucent fills. `Color` has no float constructor — the alpha is a byte.
const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
const COL_BANNER: Color = Color::rgba(0x11, 0x11, 0x1B, 224);
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

/// Every board size the game offers, in the order they are offered.
///
/// This is the single list the keys, the buttons, the flip counts and the
/// best-score slots all read. Splitting it into four hand-written `match`
/// arms is how a size ends up with a scoreboard slot and no way to select it.
const SIZES: [usize; 3] = [3, 5, 7];
/// One key per entry of [`SIZES`], in the same order.
const SIZE_KEYS: [Key; 3] = [Key::Num3, Key::Num5, Key::Num7];
/// How many random flips build a board of each [`SIZES`] entry.
///
/// Roughly half the cells: enough to look scrambled, few enough that the
/// shortest solution is not the whole board.
const SIZE_FLIPS: [usize; 3] = [4, 8, 14];

const DEFAULT_SIZE: usize = 5;
const WINDOW_WIDTH: f32 = 640.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// The seed a puzzle falls back to when the kernel has no entropy to give.
///
/// A Lights Out board may be predictable — the worst outcome is that today's
/// first puzzle is the same as yesterday's. Refusing to start the game would
/// be the worse failure; see [`guitk::rng::seeded_from_system`] for the rule.
/// "LIGHTSO!" in ASCII.
const FALLBACK_SEED: u64 = 0x4C49_4748_5453_4F21;

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 7] = [
    ("Arrows", "Move the cursor"),
    ("Enter / Space", "Flip the cell under it"),
    ("Click", "Flip that cell"),
    ("3 / 5 / 7", "Board 3x3 / 5x5 / 7x7"),
    ("N", "New puzzle, same size"),
    ("H", "Show or hide this sheet"),
    ("", "A flip switches the cell and its four neighbours."),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the board keeps no matter what.
///
/// Chrome that squeezed the board to nothing would leave a puzzle you cannot
/// see next to the buttons for a puzzle you cannot play.
const BOARD_SHARE: f32 = 0.45;

/// Which band goes first when they do not all fit: footer, best scores,
/// header, info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// down to four pixels costs the board four pixels and shows nothing. The
/// live readout — moves and lights remaining — is the last to go, because it
/// is the only chrome you need in order to keep playing.
const BAND_DROP_ORDER: [usize; 4] = [3, 2, 0, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in, which
/// is exactly the class of fault this file was full of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    pub best: Rect,
    pub board: Rect,
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
        let font = (h / 42.0).clamp(8.0, 15.0);
        let small = (font - 2.0).max(7.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, best, footer] order.
        let mut wants = [
            (h * 0.09).clamp(22.0, 42.0),
            (h * 0.05).clamp(14.0, 24.0),
            (h * 0.05).clamp(14.0, 24.0),
            (h * 0.08).clamp(18.0, 34.0),
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
        let [hdr_h, inf_h, best_h, foot_h] = wants;

        let header = Rect::new(0.0, 0.0, w, hdr_h);
        let info = Rect::new(0.0, header.bottom(), w, inf_h);
        let best = Rect::new(0.0, info.bottom(), w, best_h);
        let footer = if foot_h > 0.0 {
            Rect::new(0.0, h - foot_h, w, foot_h)
        } else {
            Rect::EMPTY
        };

        // The board is square and centred in whatever the bands left behind.
        let top = best.bottom();
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

        let help_w = (w * 0.9).min(360.0);
        let help_h = (h * 0.9).min(270.0);
        let help = Rect::new((w - help_w) / 2.0, (h - help_h) / 2.0, help_w, help_h);

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            info,
            best,
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

    /// A size button, or — at `SIZES.len()` — the new-puzzle button.
    pub fn footer_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, SIZES.len().saturating_add(1), index)
    }

    /// The help toggle, at the right-hand end of the header.
    pub fn help_button(&self) -> Rect {
        let side = (self.header.h * 0.7).min(self.header.w / 4.0).max(0.0);
        Rect::new(
            (self.header.right() - self.pad - side).max(self.header.x),
            self.header.y + (self.header.h - side) / 2.0,
            side,
            side,
        )
    }

    /// One cell of a `size`x`size` board. Derived from the board rectangle, so
    /// the cell a click lands in is by construction the cell that was drawn.
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

    /// Where the win message goes: across the foot of the board rather than
    /// below it, because a band below the board has to come out of the
    /// board's own height and there is none to spare in a small window.
    pub fn banner(&self) -> Rect {
        let bh = (self.board.h * 0.22).clamp(0.0, 58.0);
        Rect::new(self.board.x, self.board.bottom() - bh, self.board.w, bh)
    }

    pub fn shows_header(&self) -> bool {
        self.header.h >= 12.0 && self.header.w >= 60.0
    }
    pub fn shows_info(&self) -> bool {
        self.info.h >= 10.0 && self.info.w >= 60.0
    }
    pub fn shows_best(&self) -> bool {
        self.best.h >= 10.0 && self.best.w >= 120.0
    }
    pub fn shows_footer(&self) -> bool {
        self.footer.h >= 10.0 && self.footer.w >= 160.0
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameState {
    Playing,
    Won,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Everything the game can be asked to do, from either input.
///
/// Both the keyboard and the pointer turn into one of these and go through
/// [`LightsOut::apply`], so there is one place where a move is made and one
/// place — [`LightsOut::enabled`] — that decides whether it may be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Flip this cell and its four orthogonal neighbours.
    Flip(usize, usize),
    /// Flip whatever the keyboard cursor is on.
    FlipCursor,
    /// Move the keyboard cursor one cell.
    Nudge(Dir),
    /// Start a fresh board at this size.
    SetSize(usize),
    /// Start a fresh board at the current size.
    NewGame,
    ToggleHelp,
}

/// Every part of the window a click can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Cell(usize, usize),
    Size(usize),
    NewGame,
    Help,
    /// The help sheet itself, which swallows clicks meant for what it covers.
    HelpSheet,
}

pub type Frame = guitk::frame::Frame<Target>;

pub struct LightsOut {
    /// The board. Its length *is* the board size; there is no second copy of
    /// that number for anything to fall out of step with.
    grid: Vec<Vec<bool>>,
    cursor_row: usize,
    cursor_col: usize,
    moves: u32,
    state: GameState,
    /// How many boards have been dealt this session, counting the first.
    board_no: u32,
    /// Fewest moves ever taken to clear a board of each [`SIZES`] entry.
    best_moves: [Option<u32>; SIZES.len()],
    show_help: bool,
    rng: SeededRng,
    width: f32,
    height: f32,
}

impl Default for LightsOut {
    fn default() -> Self {
        Self::new()
    }
}

impl LightsOut {
    pub fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A game driven by `rng`, so a test can pin the board it is given.
    pub fn with_rng(mut rng: SeededRng) -> Self {
        let grid = Self::generate_solvable(DEFAULT_SIZE, &mut rng);
        let mid = DEFAULT_SIZE / 2;
        Self {
            grid,
            cursor_row: mid,
            cursor_col: mid,
            moves: 0,
            state: GameState::Playing,
            board_no: 1,
            best_moves: [None; SIZES.len()],
            show_help: false,
            rng,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    // ── Board shape ──

    /// The board's size, which is the board's own length.
    pub fn size(&self) -> usize {
        self.grid.len()
    }

    /// Where `size` sits in [`SIZES`], if it is one of them.
    fn size_index_of(size: usize) -> Option<usize> {
        SIZES.iter().position(|&s| s == size)
    }

    pub fn size_index(&self) -> Option<usize> {
        Self::size_index_of(self.size())
    }

    pub fn best_for(&self, size: usize) -> Option<u32> {
        Self::size_index_of(size)
            .and_then(|i| self.best_moves.get(i))
            .copied()
            .flatten()
    }

    pub fn at(&self, row: usize, col: usize) -> Option<bool> {
        self.grid.get(row).and_then(|r| r.get(col)).copied()
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }
    pub fn state(&self) -> GameState {
        self.state
    }
    pub fn board_no(&self) -> u32 {
        self.board_no
    }
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    pub fn lights_on_count(&self) -> usize {
        self.grid.iter().flatten().filter(|&&on| on).count()
    }

    // ── Board generation ──

    /// Build a board by flipping random cells of a solved grid.
    ///
    /// Solvability is guaranteed by construction rather than checked: replay
    /// the same flips and the board is off again. That is also why the count
    /// comes from [`SIZE_FLIPS`] rather than being passed in — a caller free
    /// to ask for zero flips would get an already-won board.
    fn generate_solvable(size: usize, rng: &mut SeededRng) -> Vec<Vec<bool>> {
        let flips = Self::size_index_of(size)
            .and_then(|i| SIZE_FLIPS.get(i))
            .copied()
            .unwrap_or(8);
        let mut grid = vec![vec![false; size]; size];
        for _ in 0..flips {
            let r = rng.below(size);
            let c = rng.below(size);
            Self::flip_on_grid(&mut grid, r, c);
        }
        // An even number of flips can cancel out exactly. A board that starts
        // won is not a puzzle, so nudge it off the solved state.
        if !grid.iter().flatten().any(|&on| on) {
            Self::flip_on_grid(&mut grid, size / 2, size / 2);
        }
        grid
    }

    /// The one rule of the game: a cell and its four orthogonal neighbours.
    ///
    /// `wrapping_sub` on row 0 gives `usize::MAX`, which no row index can be,
    /// so the edge falls out of the bounds check rather than needing a
    /// separate branch per side.
    fn flip_on_grid(grid: &mut [Vec<bool>], row: usize, col: usize) {
        if grid.get(row).and_then(|r| r.get(col)).is_none() {
            return;
        }
        let touched = [
            (row, col),
            (row.wrapping_sub(1), col),
            (row.saturating_add(1), col),
            (row, col.wrapping_sub(1)),
            (row, col.saturating_add(1)),
        ];
        for (r, c) in touched {
            if let Some(cell) = grid.get_mut(r).and_then(|row| row.get_mut(c)) {
                *cell = !*cell;
            }
        }
    }

    // ── Actions ──

    /// Whether `action` would do anything. The renderer asks this to decide
    /// what to dim, and [`Self::apply`] asks it to decide what to refuse, so
    /// a greyed-out control and a rejected one can never disagree.
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::Flip(row, col) => {
                self.state == GameState::Playing && self.at(row, col).is_some()
            }
            Action::FlipCursor => self.enabled(Action::Flip(self.cursor_row, self.cursor_col)),
            Action::Nudge(dir) => {
                let last = self.size().saturating_sub(1);
                match dir {
                    Dir::Up => self.cursor_row > 0,
                    Dir::Down => self.cursor_row < last,
                    Dir::Left => self.cursor_col > 0,
                    Dir::Right => self.cursor_col < last,
                }
            }
            // A size button is live even after a win: choosing a size is how
            // you start the next puzzle at that size.
            Action::SetSize(size) => Self::size_index_of(size).is_some(),
            Action::NewGame | Action::ToggleHelp => true,
        }
    }

    /// Perform `action` if it is allowed. Returns whether anything changed.
    pub fn apply(&mut self, action: Action) -> bool {
        if !self.enabled(action) {
            return false;
        }
        match action {
            Action::Flip(row, col) => {
                self.cursor_row = row;
                self.cursor_col = col;
                Self::flip_on_grid(&mut self.grid, row, col);
                self.moves = self.moves.saturating_add(1);
                self.check_win();
            }
            Action::FlipCursor => {
                return self.apply(Action::Flip(self.cursor_row, self.cursor_col));
            }
            Action::Nudge(dir) => match dir {
                Dir::Up => self.cursor_row = self.cursor_row.saturating_sub(1),
                Dir::Down => self.cursor_row = self.cursor_row.saturating_add(1),
                Dir::Left => self.cursor_col = self.cursor_col.saturating_sub(1),
                Dir::Right => self.cursor_col = self.cursor_col.saturating_add(1),
            },
            Action::SetSize(size) => self.deal(size),
            Action::NewGame => self.deal(self.size()),
            Action::ToggleHelp => self.show_help = !self.show_help,
        }
        true
    }

    /// Deal a fresh board at `size`.
    fn deal(&mut self, size: usize) {
        self.grid = Self::generate_solvable(size, &mut self.rng);
        let mid = size / 2;
        self.cursor_row = mid;
        self.cursor_col = mid;
        self.moves = 0;
        self.state = GameState::Playing;
        self.board_no = self.board_no.saturating_add(1);
    }

    fn check_win(&mut self) {
        if self.grid.iter().flatten().any(|&on| on) {
            return;
        }
        self.state = GameState::Won;
        let moves = self.moves;
        if let Some(slot) = self.size_index().and_then(|i| self.best_moves.get_mut(i)) {
            if slot.is_none_or(|best| moves < best) {
                *slot = Some(moves);
            }
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

impl LightsOut {
    /// The whole window, and every hit box in it, in one pass.
    ///
    /// `Frame::hit_test` scans the recorded boxes in reverse, so anything
    /// drawn later wins the click over what it covers. That is why the help
    /// sheet is painted last.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut f = Frame::new(l.window.w, l.window.h);
        fill(&mut f, l.window, COL_BASE, 0.0);

        if l.shows_header() {
            self.draw_header(&mut f, &l);
        }
        if l.shows_info() {
            self.draw_info(&mut f, &l);
        }
        if l.shows_best() {
            self.draw_best(&mut f, &l);
        }
        self.draw_board(&mut f, &l);
        if self.state == GameState::Won {
            self.draw_banner(&mut f, &l);
        }
        if l.shows_footer() {
            self.draw_footer(&mut f, &l);
        }
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    fn draw_header(&self, f: &mut Frame, l: &Layout) {
        let cy = l.header.y + l.header.h / 2.0;
        let btn = l.help_button();
        let title_span = (btn.x - l.pad * 2.0 - l.header.x).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            cy - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
            "Lights Out",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_span),
        );

        if btn.w > 0.0 && btn.h > 0.0 {
            fill(
                f,
                btn,
                if self.show_help {
                    COL_SURFACE1
                } else {
                    COL_SURFACE0
                },
                (btn.h * 0.25).min(6.0),
            );
            centred_in(
                f,
                btn.x,
                btn.w,
                btn.y + btn.h / 2.0,
                "?",
                l.small,
                COL_TEXT,
                FontWeightHint::Bold,
            );
            f.hit(Target::Help, btn);
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        let size = self.size();
        let body = format!(
            "Board {}   {}x{}   Moves {}   Lights {}",
            self.board_no,
            size,
            size,
            self.moves,
            self.lights_on_count()
        );
        centred_in(
            f,
            l.info.x + l.pad,
            (l.info.w - l.pad * 2.0).max(0.0),
            l.info.y + l.info.h / 2.0,
            &body,
            l.small,
            COL_SUBTEXT,
            FontWeightHint::Regular,
        );
    }

    fn draw_best(&self, f: &mut Frame, l: &Layout) {
        let mut body = String::from("Best");
        for size in SIZES {
            match self.best_for(size) {
                Some(m) => body.push_str(&format!("   {size}x{size} {m}")),
                None => body.push_str(&format!("   {size}x{size} -")),
            }
        }
        centred_in(
            f,
            l.best.x + l.pad,
            (l.best.w - l.pad * 2.0).max(0.0),
            l.best.y + l.best.h / 2.0,
            &body,
            l.small,
            COL_OVERLAY,
            FontWeightHint::Regular,
        );
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.w <= 0.0 || l.board.h <= 0.0 {
            return;
        }
        let size = self.size();
        fill(f, l.board, COL_CRUST, (l.board.w * 0.02).min(8.0));

        // The gutter between lights. Proportional, so a 7x7 board in a small
        // window still shows seven distinct cells instead of one yellow slab.
        let cs = l.board.w / size.max(1) as f32;
        let inset = (cs * 0.06).clamp(0.5, 4.0);
        let radius = (cs * 0.14).min(8.0);

        for row in 0..size {
            for col in 0..size {
                let outer = l.cell(size, row, col);
                let on = self.at(row, col).unwrap_or(false);
                let face = Rect::new(
                    outer.x + inset,
                    outer.y + inset,
                    (outer.w - inset * 2.0).max(0.0),
                    (outer.h - inset * 2.0).max(0.0),
                );
                fill(f, face, if on { LIGHT_ON } else { LIGHT_OFF }, radius);

                if (row, col) == (self.cursor_row, self.cursor_col) {
                    self.draw_cursor(f, face);
                }
                // The hit box is the whole cell, gutter included: a click one
                // pixel into the gap between two lights should pick one of
                // them, not fall through to the board behind.
                f.hit(Target::Cell(row, col), outer);
            }
        }
    }

    /// A ring around the cell the keyboard is on, drawn as four bars because
    /// a stroked rectangle would be centred on the edge and bleed outward
    /// into the gutter.
    fn draw_cursor(&self, f: &mut Frame, face: Rect) {
        let t = (face.w * 0.08).clamp(1.0, 4.0);
        if face.w <= t * 2.0 || face.h <= t * 2.0 {
            return;
        }
        fill(f, Rect::new(face.x, face.y, face.w, t), CURSOR_COLOR, 0.0);
        fill(
            f,
            Rect::new(face.x, face.bottom() - t, face.w, t),
            CURSOR_COLOR,
            0.0,
        );
        fill(f, Rect::new(face.x, face.y, t, face.h), CURSOR_COLOR, 0.0);
        fill(
            f,
            Rect::new(face.right() - t, face.y, t, face.h),
            CURSOR_COLOR,
            0.0,
        );
    }

    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let r = l.banner();
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        fill(f, r, COL_BANNER, (r.h * 0.15).min(8.0));
        let head = format!("All lights off in {} moves", self.moves);
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.34,
            &head,
            l.font,
            COL_GREEN,
            FontWeightHint::Bold,
        );
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h * 0.72,
            "N for the next puzzle",
            l.small,
            COL_SUBTEXT,
            FontWeightHint::Regular,
        );
        // The banner says what to press, so it may as well be the button.
        f.hit(Target::NewGame, r);
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        for (i, size) in SIZES.iter().enumerate() {
            let r = l.footer_button(i);
            let current = *size == self.size();
            fill(
                f,
                r,
                if current { COL_SURFACE1 } else { COL_SURFACE0 },
                (r.h * 0.2).min(6.0),
            );
            centred_in(
                f,
                r.x,
                r.w,
                r.y + r.h / 2.0,
                &format!("{size}x{size}"),
                l.small,
                if current { COL_YELLOW } else { COL_TEXT },
                FontWeightHint::Bold,
            );
            // Recorded even for the size already showing: a click there should
            // stop at the button, not reach whatever is behind it.
            f.hit(Target::Size(*size), r);
        }

        let r = l.footer_button(SIZES.len());
        fill(f, r, COL_SURFACE0, (r.h * 0.2).min(6.0));
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h / 2.0,
            "New",
            l.small,
            COL_BLUE,
            FontWeightHint::Bold,
        );
        f.hit(Target::NewGame, r);
    }

    fn draw_help(&self, f: &mut Frame, l: &Layout) {
        // Dim the whole window first, then the panel on top of it, so the
        // sheet reads as in front of the game rather than part of it.
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

        // Rows share whatever is left below the title, so the sheet cannot
        // write past its own foot however short the window is.
        let top = p.y + pad + title_h + pad / 2.0;
        let room = (p.bottom() - pad - top).max(0.0);
        let step = room / HELP_ROWS.len() as f32;
        let key_span = (inner * 0.38).min(120.0);
        for (i, (k, v)) in HELP_ROWS.iter().enumerate() {
            let y = top + i as f32 * step;
            if y + l.small > p.bottom() - pad {
                break;
            }
            if k.is_empty() {
                label(
                    f,
                    p.x + pad,
                    y,
                    v,
                    l.small,
                    COL_SUBTEXT,
                    FontWeightHint::Regular,
                    Some(inner),
                );
            } else {
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
        }

        // Over the whole window, not just the panel: while the sheet is up,
        // nothing behind it is clickable.
        f.hit(Target::HelpSheet, l.window);
    }
}

// ── Input ──────────────────────────────────────────────────────────────────

impl LightsOut {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The fault that broke every key in this file, in one line. A release
        // is not a second press: acting on both flipped each cell twice
        // (restoring the board while charging two moves), moved the cursor
        // two cells per press, dealt two boards per `N`, and opened the help
        // sheet and closed it again before it could be seen.
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
            Key::Up => Some(Action::Nudge(Dir::Up)),
            Key::Down => Some(Action::Nudge(Dir::Down)),
            Key::Left => Some(Action::Nudge(Dir::Left)),
            Key::Right => Some(Action::Nudge(Dir::Right)),
            Key::Enter | Key::Space => Some(Action::FlipCursor),
            Key::N => Some(Action::NewGame),
            Key::H => Some(Action::ToggleHelp),
            key => SIZE_KEYS
                .iter()
                .position(|k| *k == key)
                .and_then(|i| SIZES.get(i).copied())
                .map(Action::SetSize),
        };

        match action {
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
        if self.show_help {
            // Anywhere at all dismisses the sheet, including outside it.
            self.apply(Action::ToggleHelp);
            return EventResult::Consumed;
        }
        let Some(target) = self.target_at(ev.x, ev.y) else {
            return EventResult::Ignored;
        };
        match target {
            Target::Cell(row, col) => {
                self.apply(Action::Flip(row, col));
            }
            Target::Size(size) => {
                self.apply(Action::SetSize(size));
            }
            Target::NewGame => {
                self.apply(Action::NewGame);
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

/// The one body both the window and the test probe drive, so what a click
/// does in a test is what it does on a screen.
pub fn handle_event(app: &mut LightsOut, event: &Event) -> EventResult {
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

impl App for LightsOut {
    fn title(&self) -> String {
        "Lights Out".to_string()
    }

    fn app_id(&self) -> String {
        "lightsout".to_string()
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

impl Probe for LightsOut {
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
    let mut game = LightsOut::new();
    app::launch("lightsout", &mut game)
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
    /// no sane person would resize to. The small end is the point: a window
    /// too short for the chrome must still show a playable board.
    const WINDOWS: [(f32, f32); 9] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (480.0, 400.0),
        (400.0, 640.0),
        (640.0, 200.0),
        (300.0, 100.0),
        (120.0, 90.0),
        (24.0, 24.0),
    ];

    /// A game on a pinned generator, sized to a given window.
    fn windowed(width: f32, height: f32) -> LightsOut {
        let mut app = LightsOut::with_rng(SeededRng::new(11));
        app.resize(width, height);
        app
    }

    fn game() -> LightsOut {
        windowed(WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    fn release(key: Key) -> KeyEvent {
        let mut ev = probe::press(key);
        ev.pressed = false;
        ev
    }

    /// A keypress delivered straight to the handler.
    ///
    /// Not `probe::key`, which resizes the app to `Probe::SIZE` first — that
    /// would quietly undo any window size a test had set up.
    fn press(app: &mut LightsOut, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn lift(app: &mut LightsOut, key: Key) -> EventResult {
        handle_event(app, &Event::Key(release(key)))
    }

    /// A press and its release, which is what a real key produces.
    fn tap(app: &mut LightsOut, key: Key) {
        press(app, key);
        lift(app, key);
    }

    fn click(app: &mut LightsOut, x: f32, y: f32) -> EventResult {
        let size = (app.width, app.height);
        app.click_at(x, y, MouseButton::Left, size)
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

    /// A board with exactly one light on, at `(row, col)`, so a single flip
    /// there is guaranteed *not* to win and a known flip elsewhere is.
    fn board_with(size: usize, on: &[(usize, usize)]) -> Vec<Vec<bool>> {
        let mut grid = vec![vec![false; size]; size];
        for &(r, c) in on {
            grid[r][c] = true;
        }
        grid
    }

    // ── The faults this wiring exists to fix ──

    /// The fault that broke every key in the file: the old handler destructured
    /// `KeyEvent { key, modifiers, .. }` and never looked at `pressed`, so the
    /// release ran the whole action a second time.
    #[test]
    fn a_release_is_not_a_second_press() {
        let mut app = game();
        assert_eq!(
            lift(&mut app, Key::N),
            EventResult::Ignored,
            "a key release was treated as a keypress"
        );
    }

    /// The worst consequence of that fault. Flipping a cell twice restores the
    /// board exactly, so Enter changed nothing at all while charging two
    /// moves for it -- the keyboard could not play this game.
    #[test]
    fn enter_flips_the_cell_once_not_back_again() {
        let mut app = game();
        let before = app.grid.clone();
        tap(&mut app, Key::Enter);
        assert_ne!(app.grid, before, "Enter left the board exactly as it was");
        assert_eq!(app.moves(), 1, "one keypress, one move");
    }

    #[test]
    fn space_flips_the_cell_once_too() {
        let mut app = game();
        let before = app.grid.clone();
        tap(&mut app, Key::Space);
        assert_ne!(app.grid, before);
        assert_eq!(app.moves(), 1);
    }

    /// The second consequence: two cells of travel per press meant that on a
    /// 5x5 board starting at the centre, only the nine even-indexed cells
    /// could ever be selected from the keyboard.
    #[test]
    fn an_arrow_key_moves_the_cursor_one_cell() {
        let mut app = game();
        assert_eq!(app.cursor(), (2, 2));
        tap(&mut app, Key::Up);
        assert_eq!(app.cursor(), (1, 2), "Up moved more than one cell");
        tap(&mut app, Key::Left);
        assert_eq!(app.cursor(), (1, 1));
        tap(&mut app, Key::Down);
        assert_eq!(app.cursor(), (2, 1));
        tap(&mut app, Key::Right);
        assert_eq!(app.cursor(), (2, 2));
    }

    /// The shape of that fault stated directly: every cell has to be
    /// reachable, not every other one.
    #[test]
    fn the_keyboard_can_reach_every_cell_of_the_board() {
        for size in SIZES {
            let mut app = game();
            tap(
                &mut app,
                SIZE_KEYS[SIZES.iter().position(|&s| s == size).unwrap()],
            );
            let mut seen = vec![vec![false; size]; size];
            // Walk to the top-left corner, then serpentine the whole board.
            for _ in 0..size {
                tap(&mut app, Key::Up);
                tap(&mut app, Key::Left);
            }
            for row in 0..size {
                for _ in 0..size {
                    let (r, c) = app.cursor();
                    seen[r][c] = true;
                    tap(&mut app, Key::Right);
                }
                for _ in 0..size {
                    let (r, c) = app.cursor();
                    seen[r][c] = true;
                    tap(&mut app, Key::Left);
                }
                if row + 1 < size {
                    tap(&mut app, Key::Down);
                }
            }
            assert!(
                seen.iter().flatten().all(|&hit| hit),
                "{size}x{size}: some cells cannot be reached with the arrow keys"
            );
        }
    }

    /// The third consequence: `H` opened the sheet on press and closed it on
    /// release, so the help -- the only written record of the controls --
    /// could not be seen at all.
    #[test]
    fn the_help_sheet_can_actually_be_opened() {
        let mut app = game();
        assert!(!app.show_help());
        tap(&mut app, Key::H);
        assert!(app.show_help(), "H opened the help and closed it again");
        tap(&mut app, Key::H);
        assert!(!app.show_help(), "H would not close the help");
    }

    /// The fourth: `N` and the size keys each dealt two boards, throwing the
    /// first one away.
    #[test]
    fn a_key_deals_one_board_not_two() {
        let mut app = game();
        assert_eq!(app.board_no(), 1);
        tap(&mut app, Key::N);
        assert_eq!(app.board_no(), 2, "N dealt more than one board");
        tap(&mut app, Key::Num7);
        assert_eq!(app.board_no(), 3, "a size key dealt more than one board");
        assert_eq!(app.size(), 7);
    }

    /// Only the board was clickable. Every other control was keyboard-only,
    /// and the keyboard was broken.
    #[test]
    fn every_control_can_be_reached_with_the_pointer() {
        let mut app = game();
        for size in SIZES {
            assert!(
                probe::is_visible(&app, Target::Size(size)),
                "no button selects a {size}x{size} board"
            );
        }
        assert!(
            probe::is_visible(&app, Target::NewGame),
            "no new-game button"
        );
        assert!(probe::is_visible(&app, Target::Help), "no help button");
        probe::click(&mut app, Target::Help);
        assert!(app.show_help(), "the help button did not open the help");
    }

    /// The board is drawn from the live window, so a click has to be read
    /// against the window it was drawn in. The old code read every click
    /// against a grid nailed to (50, 90).
    #[test]
    fn what_is_drawn_at_a_cell_is_what_a_click_there_flips() {
        for (w, h) in WINDOWS {
            let app = windowed(w, h);
            let l = Layout::new(w, h);
            let size = app.size();
            for row in 0..size {
                for col in 0..size {
                    let r = l.cell(size, row, col);
                    if r.w < 2.0 || r.h < 2.0 {
                        continue;
                    }
                    let (cx, cy) = r.centre();
                    assert_eq!(
                        app.target_at(cx, cy),
                        Some(Target::Cell(row, col)),
                        "{w}x{h}: cell ({row}, {col}) is not clickable where it is drawn"
                    );
                }
            }
        }
    }

    #[test]
    fn clicking_a_cell_flips_it_and_its_neighbours() {
        let mut app = game();
        app.grid = board_with(5, &[]);
        let l = Layout::new(app.width, app.height);
        let (cx, cy) = l.cell(5, 1, 1).centre();
        click(&mut app, cx, cy);
        assert_eq!(app.moves(), 1);
        assert_eq!(app.at(1, 1), Some(true));
        assert_eq!(app.at(0, 1), Some(true));
        assert_eq!(app.at(2, 1), Some(true));
        assert_eq!(app.at(1, 0), Some(true));
        assert_eq!(app.at(1, 2), Some(true));
        assert_eq!(app.at(0, 0), Some(false), "a diagonal is not a neighbour");
    }

    /// The cursor follows the pointer, so the two inputs do not fight over
    /// where "here" is.
    #[test]
    fn clicking_a_cell_moves_the_cursor_to_it() {
        let mut app = game();
        let l = Layout::new(app.width, app.height);
        let (cx, cy) = l.cell(app.size(), 0, 4).centre();
        click(&mut app, cx, cy);
        assert_eq!(app.cursor(), (0, 4));
    }

    /// The board's size was a separate field. Nothing kept it in step with the
    /// board it described, and every bounds check trusted it over the data.
    #[test]
    fn the_board_is_its_own_size() {
        let mut app = game();
        for size in SIZES {
            app.apply(Action::SetSize(size));
            assert_eq!(app.size(), size);
            assert_eq!(app.grid.len(), size);
            assert!(app.grid.iter().all(|row| row.len() == size));
        }
    }

    // ── The layout comes from the window ──

    /// Every state, at every window, produces a frame whose clips and
    /// translations balance. An unbalanced frame is a compositor bug waiting
    /// for the state that reaches it.
    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        for (w, h) in WINDOWS {
            for size in SIZES {
                for help in [false, true] {
                    for won in [false, true] {
                        let mut app = windowed(w, h);
                        app.apply(Action::SetSize(size));
                        app.show_help = help;
                        if won {
                            app.grid = board_with(size, &[]);
                            app.state = GameState::Won;
                        }
                        let f = app.frame(w, h);
                        assert!(f.is_balanced(), "{w}x{h} size {size} help {help} won {won}");
                        assert!(!f.commands().is_empty());
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
            let covered = f.commands().iter().any(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => *x <= 0.0 && *y <= 0.0 && *width >= w && *height >= h,
                _ => false,
            });
            assert!(covered, "{w}x{h}: the window has an unpainted region");
        }
    }

    /// The board is the game. It has to be square (or the flips stop looking
    /// orthogonal) and it has to be on screen.
    #[test]
    fn the_board_is_square_and_inside_its_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            assert!(
                l.board.w > 0.0 && l.board.h > 0.0,
                "{w}x{h}: no board at all"
            );
            assert!(
                (l.board.w - l.board.h).abs() < 0.01,
                "{w}x{h}: the board is {}x{}",
                l.board.w,
                l.board.h
            );
            assert!(l.board.x >= -0.01 && l.board.y >= -0.01, "{w}x{h}");
            assert!(
                l.board.right() <= w + 0.01 && l.board.bottom() <= h + 0.01,
                "{w}x{h}: the board hangs off the window"
            );
        }
    }

    #[test]
    fn no_band_is_laid_past_the_bottom_of_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h);
            for (name, r) in [
                ("header", l.header),
                ("info", l.info),
                ("best", l.best),
                ("footer", l.footer),
                ("help", l.help),
            ] {
                if r.is_empty() {
                    continue;
                }
                assert!(
                    r.bottom() <= h + 0.01 && r.right() <= w + 0.01,
                    "{w}x{h}: the {name} band runs off the window"
                );
                assert!(r.x >= -0.01 && r.y >= -0.01, "{w}x{h}: {name}");
            }
        }
    }

    /// The rule the band allocator exists for: when the window is too short
    /// for everything, the buttons go and the game stays.
    #[test]
    fn a_window_too_short_for_the_footer_drops_it_rather_than_the_board() {
        let l = Layout::new(300.0, 100.0);
        assert!(!l.shows_footer(), "the footer survived a 100px-tall window");
        assert!(
            l.board.h > 20.0,
            "the board was squeezed to {} to keep the chrome",
            l.board.h
        );
    }

    /// A 24x24 window is absurd, and a board that silently stops drawing
    /// cells in one is a board that will silently stop drawing cells in a
    /// window someone actually uses.
    #[test]
    fn a_board_the_size_of_a_postage_stamp_still_draws_every_cell() {
        for size in SIZES {
            let mut app = windowed(24.0, 24.0);
            app.apply(Action::SetSize(size));
            let f = app.frame(24.0, 24.0);
            let lights = f
                .commands()
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        RenderCommand::FillRect { color, .. }
                            if *color == LIGHT_ON || *color == LIGHT_OFF
                    )
                })
                .count();
            assert_eq!(
                lights,
                size * size,
                "{size}x{size} lost cells in a tiny window"
            );
        }
    }

    /// The win message sits across the foot of the board rather than below
    /// it, because a band below the board has to come out of the board's own
    /// height and there is none to spare in a small window.
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
                "{w}x{h}: the banner is outside its board"
            );
        }
    }

    #[test]
    fn the_help_sheet_never_writes_past_its_own_panel() {
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            app.show_help = true;
            let l = Layout::new(w, h);
            let f = app.frame(w, h);
            for c in f.commands() {
                let RenderCommand::Text { x, y, text, .. } = c else {
                    continue;
                };
                let mine =
                    text == HELP_TITLE || HELP_ROWS.iter().any(|(k, v)| text == k || text == v);
                if !mine {
                    continue;
                }
                assert!(
                    *x >= l.help.x - 0.01
                        && *x <= l.help.right() + 0.01
                        && *y >= l.help.y - 0.01
                        && *y <= l.help.bottom() + 0.01,
                    "{w}x{h}: {text:?} starts at ({x}, {y}), outside the panel"
                );
            }
        }
    }

    /// `Frame::hit_test` scans in reverse, so the sheet has to be drawn last
    /// or the board would take clicks through it.
    #[test]
    fn the_help_sheet_is_in_front_of_everything_it_covers() {
        for (w, h) in WINDOWS {
            let mut app = windowed(w, h);
            app.show_help = true;
            let l = Layout::new(w, h);
            let (cx, cy) = l.board.centre();
            assert_eq!(
                app.target_at(cx, cy),
                Some(Target::HelpSheet),
                "{w}x{h}: the board is reachable through the help sheet"
            );
        }
    }

    /// The whole point of storing the window size on the model: a click is
    /// read against the frame the user is actually looking at.
    #[test]
    fn a_click_is_read_against_the_window_it_was_drawn_in() {
        let small = Layout::new(400.0, 640.0);
        let (cx, cy) = small.cell(DEFAULT_SIZE, 0, 0).centre();

        let mut narrow = windowed(400.0, 640.0);
        assert_eq!(narrow.target_at(cx, cy), Some(Target::Cell(0, 0)));

        let mut wide = windowed(1920.0, 1080.0);
        assert_ne!(
            wide.target_at(cx, cy),
            Some(Target::Cell(0, 0)),
            "the same point means the same thing in every window"
        );

        // And the clicks agree with the readings.
        narrow.grid = board_with(DEFAULT_SIZE, &[]);
        click(&mut narrow, cx, cy);
        assert_eq!(narrow.at(0, 0), Some(true));
        wide.grid = board_with(DEFAULT_SIZE, &[]);
        click(&mut wide, cx, cy);
        assert_eq!(wide.at(0, 0), Some(false));
    }

    #[test]
    fn a_resize_is_remembered_across_an_action() {
        let mut app = game();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 900,
                height: 700,
            },
        );
        assert_eq!((app.width, app.height), (900.0, 700.0));
        press(&mut app, Key::N);
        assert_eq!(
            (app.width, app.height),
            (900.0, 700.0),
            "an action reset the window size"
        );
    }

    /// The readout is drawn beside the board, so it has to describe that
    /// board and not an older one.
    #[test]
    fn the_readout_counts_the_board_it_is_drawn_beside() {
        let mut app = game();
        app.grid = board_with(5, &[(0, 0), (4, 4), (2, 3)]);
        let joined = texts(&app.frame(app.width, app.height)).join(" | ");
        assert!(joined.contains("Lights 3"), "readout says: {joined}");
        assert!(joined.contains("5x5"), "readout says: {joined}");
        assert!(joined.contains("Moves 0"), "readout says: {joined}");

        app.apply(Action::SetSize(7));
        let joined = texts(&app.frame(app.width, app.height)).join(" | ");
        assert!(joined.contains("7x7"), "readout says: {joined}");
        assert!(
            joined.contains(&format!("Lights {}", app.lights_on_count())),
            "readout says: {joined}"
        );
    }

    #[test]
    fn the_cursor_ring_follows_the_cell_it_is_on() {
        let mut app = game();
        let ring = |app: &LightsOut| {
            app.frame(app.width, app.height)
                .commands()
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { x, y, color, .. } if *color == CURSOR_COLOR => {
                        Some((*x, *y))
                    }
                    _ => None,
                })
                .fold((f32::MAX, f32::MAX), |acc, p| {
                    (acc.0.min(p.0), acc.1.min(p.1))
                })
        };
        let before = ring(&app);
        tap(&mut app, Key::Up);
        let after = ring(&app);
        assert!(
            after.1 < before.1,
            "the cursor ring did not move up with it"
        );
        assert_eq!(after.0, before.0, "the cursor ring drifted sideways");
    }

    /// The sheet is modal both ways: it takes the keyboard as well as the
    /// pointer, so you cannot play a board you cannot see.
    #[test]
    fn keys_do_nothing_behind_the_help_sheet() {
        let mut app = game();
        tap(&mut app, Key::H);
        let before = app.grid.clone();
        for key in [Key::Up, Key::Left, Key::N, Key::Num3, Key::Num7] {
            assert_eq!(
                press(&mut app, key),
                EventResult::Consumed,
                "{key:?} leaked"
            );
        }
        assert!(app.show_help(), "a play key closed the help sheet");
        assert_eq!(app.grid, before, "the board changed behind the help sheet");
        assert_eq!(app.cursor(), (2, 2), "an arrow key moved behind the sheet");
        assert_eq!(app.moves(), 0);
        assert_eq!(app.board_no(), 1);
    }

    /// Enter and Space *do* answer while the sheet is up -- but by closing it,
    /// not by playing. A panel with no visible close button has to answer the
    /// two keys everyone presses to get rid of one.
    #[test]
    fn enter_dismisses_the_help_sheet_without_playing_a_move() {
        for key in [Key::Enter, Key::Space] {
            let mut app = game();
            tap(&mut app, Key::H);
            let before = app.grid.clone();
            tap(&mut app, key);
            assert!(!app.show_help(), "{key:?} did not close the sheet");
            assert_eq!(app.grid, before, "{key:?} flipped a cell as well");
            assert_eq!(app.moves(), 0);
        }
    }

    #[test]
    fn a_click_does_not_reach_the_board_through_the_help_sheet() {
        let mut app = game();
        tap(&mut app, Key::H);
        let before = app.grid.clone();
        let l = Layout::new(app.width, app.height);
        let (cx, cy) = l.cell(app.size(), 2, 2).centre();
        click(&mut app, cx, cy);
        assert_eq!(app.grid, before, "the click went through the sheet");
        assert!(!app.show_help(), "the click did not dismiss the sheet");
        assert_eq!(app.moves(), 0);
    }

    /// Escape closes the sheet, which is the one key everybody tries.
    #[test]
    fn escape_closes_the_help_sheet() {
        let mut app = game();
        tap(&mut app, Key::H);
        tap(&mut app, Key::Escape);
        assert!(!app.show_help());
    }

    /// A shortcut belonging to the window manager is not ours to eat.
    #[test]
    fn a_modified_key_is_left_for_someone_else() {
        let mut app = game();
        let before = app.grid.clone();
        for ev in [probe::ctrl(Key::N), probe::ctrl(Key::Enter)] {
            assert_eq!(
                handle_event(&mut app, &Event::Key(ev)),
                EventResult::Ignored
            );
        }
        assert_eq!(app.grid, before);
        assert_eq!(app.board_no(), 1);
        // Shift is not a modifier this game reads, but it is also not one it
        // should swallow silently on a key it does not otherwise answer.
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::press(Key::Q))),
            EventResult::Ignored
        );
    }

    /// The help sheet is the only written record of the controls, so it has
    /// to name every key the game actually answers.
    #[test]
    fn the_help_sheet_names_every_key_the_game_answers() {
        let listed = HELP_ROWS
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" | ");
        for size in SIZES {
            assert!(
                listed.contains(&size.to_string()),
                "the help never mentions the {size}x{size} board"
            );
        }
        for word in ["Arrows", "Enter", "Click", "N", "H"] {
            assert!(listed.contains(word), "the help never mentions {word}");
        }
    }

    // ── The rules of the game ──

    #[test]
    fn a_flip_touches_the_cell_and_its_four_neighbours_and_nothing_else() {
        let mut grid = board_with(3, &[]);
        LightsOut::flip_on_grid(&mut grid, 1, 1);
        assert!(grid[1][1]);
        assert!(grid[0][1]);
        assert!(grid[2][1]);
        assert!(grid[1][0]);
        assert!(grid[1][2]);
        for (r, c) in [(0, 0), (0, 2), (2, 0), (2, 2)] {
            assert!(!grid[r][c], "the diagonal at ({r}, {c}) was flipped");
        }
    }

    /// The edge cases are the corners: `wrapping_sub` on row 0 has to fall out
    /// of the bounds check rather than wrapping round to the last row.
    #[test]
    fn a_flip_at_the_edge_does_not_wrap_to_the_far_side() {
        let mut grid = board_with(3, &[]);
        LightsOut::flip_on_grid(&mut grid, 0, 0);
        assert!(grid[0][0]);
        assert!(grid[0][1]);
        assert!(grid[1][0]);
        assert!(!grid[2][0], "the top row wrapped to the bottom");
        assert!(!grid[0][2], "the left column wrapped to the right");
        assert_eq!(grid.iter().flatten().filter(|&&on| on).count(), 3);
    }

    #[test]
    fn flipping_the_same_cell_twice_restores_the_board() {
        let mut grid = board_with(5, &[(0, 3), (4, 1)]);
        let before = grid.clone();
        LightsOut::flip_on_grid(&mut grid, 2, 2);
        assert_ne!(grid, before);
        LightsOut::flip_on_grid(&mut grid, 2, 2);
        assert_eq!(grid, before);
    }

    #[test]
    fn a_flip_off_the_board_changes_nothing() {
        let mut grid = board_with(3, &[]);
        LightsOut::flip_on_grid(&mut grid, 5, 5);
        LightsOut::flip_on_grid(&mut grid, 0, 9);
        LightsOut::flip_on_grid(&mut grid, usize::MAX, 0);
        assert!(grid.iter().flatten().all(|&on| !on));
    }

    /// The property the whole generator exists to guarantee: replaying the
    /// flips that built a board solves it.
    #[test]
    fn every_generated_board_can_be_solved_by_replaying_its_flips() {
        for seed in 0..40 {
            let mut rng = SeededRng::new(seed);
            for size in SIZES {
                // Rebuild the board exactly as `generate_solvable` does, but
                // keeping the flips, so they can be replayed.
                let mut check = SeededRng::new(seed.wrapping_mul(31).wrapping_add(size as u64));
                let flips = SIZE_FLIPS[SIZES.iter().position(|&s| s == size).unwrap()];
                let mut grid = vec![vec![false; size]; size];
                let mut used = Vec::new();
                for _ in 0..flips {
                    let (r, c) = (check.below(size), check.below(size));
                    used.push((r, c));
                    LightsOut::flip_on_grid(&mut grid, r, c);
                }
                for (r, c) in used {
                    LightsOut::flip_on_grid(&mut grid, r, c);
                }
                assert!(
                    grid.iter().flatten().all(|&on| !on),
                    "seed {seed} size {size}: replaying the flips did not clear the board"
                );

                // And the real generator never hands out a board that is
                // already won, which would not be a puzzle.
                let dealt = LightsOut::generate_solvable(size, &mut rng);
                assert_eq!(dealt.len(), size);
                assert!(dealt.iter().all(|row| row.len() == size));
                assert!(
                    dealt.iter().flatten().any(|&on| on),
                    "seed {seed} size {size}: dealt an already-solved board"
                );
            }
        }
    }

    /// The board must be a function of the generator it was given. The defect
    /// that shipped was that it wasn't observably one: `new()` seeded a
    /// literal `42`, so every launch dealt the identical puzzle for ever.
    #[test]
    fn the_board_follows_the_generator_it_was_given() {
        let a = LightsOut::with_rng(SeededRng::new(1)).grid;
        let b = LightsOut::with_rng(SeededRng::new(2)).grid;
        assert_ne!(a, b, "the board ignores its generator");
    }

    /// `new()` must route through [`seeded_from_system`] and nothing else.
    ///
    /// This cannot be tested by observing variety, because the host test
    /// toolchain has no SlateOS kernel to ask: `seeded_from_system` correctly
    /// takes its documented fallback there, so two fresh games *are* identical
    /// on the host and would be identical under the old hardcoded `42` too.
    /// What distinguishes the two is *which* seed, so that is what is checked
    /// -- and on real hardware the same line reaches the kernel instead.
    #[test]
    #[cfg(not(unix))]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let fresh = LightsOut::new().grid;
        let fallback = LightsOut::with_rng(SeededRng::new(FALLBACK_SEED)).grid;
        assert_eq!(
            fresh, fallback,
            "new() is not going through seeded_from_system"
        );
        let old_defect = LightsOut::with_rng(SeededRng::new(42)).grid;
        assert_ne!(fresh, old_defect, "new() is back on a hardcoded seed");
    }

    /// Flip positions must reach every cell, not just a band of them. A
    /// reduction that reads the low bits of an LCG would concentrate them;
    /// this is the game-level shape of the bug `randrange::below` avoids.
    #[test]
    fn generated_flips_reach_every_cell_of_the_board() {
        let mut rng = SeededRng::new(7);
        let mut seen = [[false; 7]; 7];
        for _ in 0..500 {
            seen[rng.below(7)][rng.below(7)] = true;
        }
        assert!(
            seen.iter().flatten().all(|&hit| hit),
            "some cells were never chosen over 500 draws"
        );
    }

    // ── Winning, and what it records ──

    #[test]
    fn clearing_the_last_light_wins() {
        let mut app = game();
        // One light at (1, 1) plus its cross is exactly one flip from solved.
        app.grid = board_with(5, &[(1, 1), (0, 1), (2, 1), (1, 0), (1, 2)]);
        app.state = GameState::Playing;
        app.moves = 6;
        assert!(app.apply(Action::Flip(1, 1)));
        assert_eq!(app.state(), GameState::Won);
        assert_eq!(app.lights_on_count(), 0);
        assert_eq!(app.best_for(5), Some(7));
    }

    #[test]
    fn a_won_board_takes_no_more_flips() {
        let mut app = game();
        app.grid = board_with(5, &[]);
        app.state = GameState::Won;
        assert!(!app.enabled(Action::Flip(0, 0)));
        assert!(!app.apply(Action::Flip(0, 0)));
        assert!(!app.apply(Action::FlipCursor));
        assert_eq!(app.moves(), 0, "a flip was counted after the win");
        assert!(app.grid.iter().flatten().all(|&on| !on));
    }

    /// A size button stays live after a win, because choosing a size is how
    /// you start the next puzzle at that size.
    #[test]
    fn a_won_board_can_still_be_replaced() {
        let mut app = game();
        app.grid = board_with(5, &[]);
        app.state = GameState::Won;
        assert!(app.apply(Action::SetSize(7)));
        assert_eq!(app.size(), 7);
        assert_eq!(app.state(), GameState::Playing);
        assert_eq!(app.moves(), 0);
        assert!(app.lights_on_count() > 0);
    }

    #[test]
    fn the_best_score_keeps_the_lowest_and_is_kept_per_size() {
        let mut app = game();
        let win_in = |app: &mut LightsOut, size: usize, moves: u32| {
            app.grid = board_with(size, &[]);
            app.state = GameState::Playing;
            app.moves = moves;
            app.check_win();
        };
        win_in(&mut app, 5, 10);
        assert_eq!(app.best_for(5), Some(10));
        win_in(&mut app, 5, 3);
        assert_eq!(app.best_for(5), Some(3), "a better score was not recorded");
        win_in(&mut app, 5, 8);
        assert_eq!(app.best_for(5), Some(3), "a worse score overwrote the best");
        assert_eq!(app.best_for(3), None, "3x3 borrowed the 5x5 score");
        assert_eq!(app.best_for(7), None, "7x7 borrowed the 5x5 score");
        win_in(&mut app, 7, 20);
        assert_eq!(app.best_for(7), Some(20));
        assert_eq!(app.best_for(5), Some(3));
    }

    /// The scoreboard is drawn from the same lookup the win records into, so
    /// a size cannot end up with a score no panel can show.
    #[test]
    fn the_scoreboard_shows_the_score_that_was_recorded() {
        let mut app = game();
        app.grid = board_with(5, &[]);
        app.moves = 4;
        app.check_win();
        let joined = texts(&app.frame(app.width, app.height)).join(" | ");
        assert!(joined.contains("5x5 4"), "scoreboard says: {joined}");
        assert!(joined.contains("3x3 -"), "scoreboard says: {joined}");
        assert!(joined.contains("7x7 -"), "scoreboard says: {joined}");
    }

    /// Every board size has a key, a button, a flip count and a score slot.
    /// A size with three of the four is how the old file ended up offering a
    /// scoreboard row for a board nothing could select.
    #[test]
    fn every_board_size_is_completely_wired() {
        assert_eq!(SIZES.len(), SIZE_KEYS.len());
        assert_eq!(SIZES.len(), SIZE_FLIPS.len());
        assert_eq!(SIZES.len(), LightsOut::new().best_moves.len());
        for (i, size) in SIZES.iter().enumerate() {
            let mut by_key = game();
            tap(&mut by_key, SIZE_KEYS[i]);
            assert_eq!(
                by_key.size(),
                *size,
                "the key for {size} deals another board"
            );

            let mut by_click = game();
            probe::click(&mut by_click, Target::Size(*size));
            assert_eq!(
                by_click.size(),
                *size,
                "the button for {size} does not work"
            );
        }
    }

    // ── The two inputs are one game ──

    /// The renderer dims what it will not do, and `apply` refuses what it
    /// dims. If those two ever disagree, a live-looking button does nothing.
    #[test]
    fn enabled_and_apply_agree() {
        let mut app = game();
        let cases = [
            Action::Flip(0, 0),
            Action::Flip(9, 9),
            Action::FlipCursor,
            Action::Nudge(Dir::Up),
            Action::Nudge(Dir::Down),
            Action::Nudge(Dir::Left),
            Action::Nudge(Dir::Right),
            Action::SetSize(3),
            Action::SetSize(4),
            Action::NewGame,
            Action::ToggleHelp,
        ];
        for state in [GameState::Playing, GameState::Won] {
            for action in cases {
                let mut probe_app = game();
                probe_app.state = state;
                let expected = probe_app.enabled(action);
                assert_eq!(
                    probe_app.apply(action),
                    expected,
                    "{action:?} in {state:?}: enabled() and apply() disagree"
                );
            }
        }
        // And a size that is not offered is refused rather than half-applied.
        assert!(!app.apply(Action::SetSize(4)));
        assert_eq!(app.size(), DEFAULT_SIZE);
        assert_eq!(app.board_no(), 1);
    }

    /// The cursor cannot walk off the board, at any size.
    #[test]
    fn the_cursor_stays_on_the_board() {
        for size in SIZES {
            let mut app = game();
            app.apply(Action::SetSize(size));
            for _ in 0..size * 2 {
                tap(&mut app, Key::Up);
                tap(&mut app, Key::Left);
            }
            assert_eq!(
                app.cursor(),
                (0, 0),
                "{size}x{size} walked off the top-left"
            );
            for _ in 0..size * 2 {
                tap(&mut app, Key::Down);
                tap(&mut app, Key::Right);
            }
            assert_eq!(
                app.cursor(),
                (size - 1, size - 1),
                "{size}x{size} walked off the bottom-right"
            );
            assert!(app.at(app.cursor().0, app.cursor().1).is_some());
        }
    }

    /// A new board of a different size has to bring the cursor with it, or
    /// the next Enter would flip a cell that is not there.
    #[test]
    fn a_smaller_board_brings_the_cursor_back_onto_it() {
        let mut app = game();
        app.apply(Action::SetSize(7));
        for _ in 0..7 {
            tap(&mut app, Key::Down);
            tap(&mut app, Key::Right);
        }
        assert_eq!(app.cursor(), (6, 6));
        app.apply(Action::SetSize(3));
        let (r, c) = app.cursor();
        assert!(
            r < 3 && c < 3,
            "the cursor stayed at ({r}, {c}) on a 3x3 board"
        );
        assert!(app.at(r, c).is_some());
    }

    /// Clicking a control the game is refusing must stop at that control. If
    /// it fell through, a click on the size button already selected would
    /// flip whatever cell happens to be behind it.
    #[test]
    fn a_click_on_a_control_never_reaches_the_board_behind_it() {
        let mut app = game();
        let before = app.grid.clone();
        probe::click(&mut app, Target::Size(DEFAULT_SIZE));
        assert_eq!(app.size(), DEFAULT_SIZE);
        assert_ne!(app.grid, before, "a size button dealt no new board");
        assert_eq!(app.moves(), 0, "a click on the footer flipped a cell");
    }

    /// The win banner says which key deals the next board, so it may as well
    /// be that button.
    #[test]
    fn the_win_banner_deals_the_next_board_when_clicked() {
        let mut app = game();
        app.grid = board_with(5, &[]);
        app.state = GameState::Won;
        let l = Layout::new(app.width, app.height);
        let (cx, cy) = l.banner().centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::NewGame));
        click(&mut app, cx, cy);
        assert_eq!(app.state(), GameState::Playing);
        assert_eq!(app.board_no(), 2);
        assert!(app.lights_on_count() > 0);
    }

    /// The window contract: a close request exits, a key that means something
    /// asks for a redraw, and one that does not leaves the app idle.
    #[test]
    fn the_window_is_told_when_to_redraw_and_when_to_close() {
        let mut app = game();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Up))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(release(Key::Up))),
            Response::Idle
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::Q))),
            Response::Idle
        ));
    }

    /// `render` is what tells the model how big the window is, so the frame
    /// it returns and the next click have to be talking about the same one.
    #[test]
    fn rendering_at_a_size_is_what_the_next_click_is_read_against() {
        let mut app = game();
        let tree = app.render(500.0, 460.0);
        assert!(!tree.commands.is_empty());
        assert_eq!((app.width, app.height), (500.0, 460.0));
        let l = Layout::new(500.0, 460.0);
        let (cx, cy) = l.cell(app.size(), 3, 1).centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Cell(3, 1)));
    }

    #[test]
    fn a_new_puzzle_resets_everything_the_old_one_left_behind() {
        let mut app = game();
        tap(&mut app, Key::Enter);
        tap(&mut app, Key::Up);
        assert_eq!(app.moves(), 1);
        tap(&mut app, Key::N);
        assert_eq!(app.moves(), 0);
        assert_eq!(app.state(), GameState::Playing);
        assert_eq!(app.cursor(), (2, 2), "the cursor did not return to centre");
        assert_eq!(app.board_no(), 2);
        assert!(app.lights_on_count() > 0, "the new board is already solved");
    }
}
