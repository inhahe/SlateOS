//! Memory — a card-matching game.
//!
//! Turn two cards at a time and find the pairs. Boards are 4x4, 4x6 or 6x6.
//! A mismatched pair stays face up long enough to be read, then turns back.
//!
//! ## What wiring this up found
//!
//! `main` built a `MemoryGame`, dropped it and exited, so no board ever
//! reached a screen and no key or click ever arrived. Underneath that, five
//! faults:
//!
//! 1. **A mismatched pair was hidden before it could be seen — which is the
//!    whole game.** Two things caused it together. The `Showing` phase was
//!    commented "brief display before hiding mismatched cards" but nothing
//!    ever timed it: the cards stayed up until the *next* input dismissed
//!    them. And the key handler destructured
//!    `Event::Key(KeyEvent { key, modifiers, .. })` without reading
//!    `pressed`, so the release of the very keypress that turned the second
//!    card ran the handler again — landing in `Phase::Showing` and
//!    dismissing it. The second card was face up for the length of a key
//!    release. A memory game you cannot see the cards in is not a game.
//!    There is now a real clock: `Event::Tick` ages the display and turns the
//!    pair back after [`SHOW_MS`], and an input during that window dismisses
//!    it early for a player who has already looked.
//! 2. **The same double-fire broke the cursor**, which moved two cards per
//!    arrow press, so on a 4-wide board only every other column could be
//!    reached — and dealt two boards per `N` or size key, discarding the
//!    first.
//! 3. **There was no help panel at all.** `show_help` was toggled by `H`,
//!    never drawn, and the only thing it controlled was whether a "H for
//!    help" hint was *hidden*. Pressing the help key removed the only text
//!    on screen that mentioned help.
//! 4. **The layout was a constant.** `render(width, height)` used its
//!    arguments for the background rectangle alone; the grid sat at a fixed
//!    (50, 80) with 70x80 cards. A 6x6 board is 528px tall from y=80, so on
//!    an 800x600 window **the bottom row and the win message were off the
//!    screen entirely**, and the scoreboard was a second fixed rectangle that
//!    a narrower window pushed off the right edge. There is now one `Layout`
//!    derived from the live window on every frame, and the painter records
//!    each hit box as it draws.
//! 5. **The game's state was stored three times over.** `phase` was a copy of
//!    which of `first_pick`/`second_pick` were set, and `total_pairs` a copy
//!    of `cards.len() / 2`. `flip_card` read `self.first_pick.unwrap_or(0)`
//!    while acting on `Phase::SecondPick` — if those two ever disagreed it
//!    would silently compare against card 0. Both copies are gone; the phase
//!    is derived from the picks, so they cannot disagree.
//!
//! Only the grid was clickable: no new game, no board size, no help.

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
use std::time::Duration;

// ── Catppuccin Mocha, only the entries this program paints with ──
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
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
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
const COL_BANNER: Color = Color::rgba(0x11, 0x11, 0x1B, 224);
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

/// The card faces, one per pair. The largest board needs eighteen.
const SYMBOLS: [&str; 18] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
];
/// A colour per face. Only eight hues exist, so the ninth face onward repeats
/// one — which is why the letter, not the colour, is what identifies a card.
const SYMBOL_COLORS: [Color; 18] = [
    COL_RED,
    COL_BLUE,
    COL_GREEN,
    COL_YELLOW,
    COL_PEACH,
    COL_MAUVE,
    COL_TEAL,
    COL_LAVENDER,
    COL_RED,
    COL_BLUE,
    COL_GREEN,
    COL_YELLOW,
    COL_PEACH,
    COL_MAUVE,
    COL_TEAL,
    COL_LAVENDER,
    COL_RED,
    COL_BLUE,
];

/// Every board the game offers, as (rows, cols), in the order offered.
///
/// The single list the keys, the buttons and the best-score slots all read.
/// The old code had a `size_index` that mapped (6, 4) to a scoreboard slot
/// and a `set_size` that would never accept it — a row nothing could fill.
const SIZES: [(usize, usize); 3] = [(4, 4), (4, 6), (6, 6)];
/// One key per entry of [`SIZES`], in the same order.
const SIZE_KEYS: [Key; 3] = [Key::Num1, Key::Num2, Key::Num3];

const DEFAULT_SIZE: (usize, usize) = SIZES[0];
const WINDOW_WIDTH: f32 = 760.0;
const WINDOW_HEIGHT: f32 = 660.0;

/// How long a mismatched pair stays face up before turning back.
///
/// Long enough to read two letters and place them, short enough that a
/// player who has already looked is not left waiting. An input during the
/// window dismisses it early, so this is a ceiling rather than a delay.
const SHOW_MS: u64 = 1400;

/// How often the window is asked to tick while a pair is showing.
///
/// A hair under a 60Hz frame. The countdown is driven by the *interval the
/// compositor reports*, not by counting ticks, so a coarser or jittery clock
/// shortens or lengthens no display.
const TICK_MS: u64 = 16;

/// The seed a board falls back to when the kernel has no entropy to give.
///
/// A layout may be predictable: the worst outcome is a repeated board.
/// Refusing to start would be the worse failure; the rule is written out at
/// [`guitk::rng::seeded_from_system`]. "MEMORY!!" in ASCII.
const FALLBACK_SEED: u64 = 0x4D45_4D4F_5259_2121;

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 7] = [
    ("Arrows", "Move the cursor"),
    ("Enter / Space", "Turn the card under it"),
    ("Click", "Turn that card"),
    ("1 / 2 / 3", "Board 4x4 / 4x6 / 6x6"),
    ("N", "New deal, same board"),
    ("H", "Show or hide this sheet"),
    ("", "Two cards that do not match turn back over."),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the board keeps no matter what.
const BOARD_SHARE: f32 = 0.45;

/// Which band goes first when they do not all fit: footer, best scores,
/// header, info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the board four pixels and shows nothing. The live
/// readout goes last: it is the only chrome you need to keep playing.
const BAND_DROP_ORDER: [usize; 4] = [3, 2, 0, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    pub best: Rect,
    /// The whole grid, sized to the board's own aspect ratio.
    pub board: Rect,
    pub footer: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
}

impl Layout {
    /// The layout for a `rows`x`cols` board in a `width`x`height` window.
    ///
    /// The board shape is an argument because a 4x6 grid is not a 6x6 grid
    /// with blank rows: the cards have to be the same size in both, so the
    /// rectangle they fill has to take the grid's aspect ratio.
    pub fn new(width: f32, height: f32, rows: usize, cols: usize) -> Self {
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

        // Square cells, as many as fit, centred in what the bands left.
        let top = best.bottom();
        let bottom = if foot_h > 0.0 { footer.y } else { h };
        let avail_w = (w - pad * 2.0).max(0.0);
        let avail_h = (bottom - top - pad * 2.0).max(0.0);
        let cell = (avail_w / cols.max(1) as f32).min(avail_h / rows.max(1) as f32);
        let bw = cell * cols.max(1) as f32;
        let bh = cell * rows.max(1) as f32;
        let board = Rect::new((w - bw) / 2.0, top + (bottom - top - bh) / 2.0, bw, bh);

        let help_w = (w * 0.9).min(370.0);
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

    /// A size button, or — at `SIZES.len()` — the new-deal button.
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

    /// One cell of a `rows`x`cols` board, gutter included.
    pub fn cell(&self, rows: usize, cols: usize, row: usize, col: usize) -> Rect {
        let cw = self.board.w / cols.max(1) as f32;
        let ch = self.board.h / rows.max(1) as f32;
        Rect::new(
            self.board.x + col as f32 * cw,
            self.board.y + row as f32 * ch,
            cw,
            ch,
        )
    }

    /// Where the win message goes: across the foot of the board rather than
    /// below it. The old code put it below, where a 6x6 board pushed it off
    /// the bottom of the window.
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

/// One card on the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    /// Index into [`SYMBOLS`]. Exactly two cards on a board share one.
    pub face: usize,
    /// Found, and face up for good.
    pub matched: bool,
}

/// What the board is waiting for.
///
/// Derived from the picks on every call rather than stored beside them. The
/// old code kept a `phase` field *and* the two `Option`s, and `flip_card` read
/// `first_pick.unwrap_or(0)` while trusting the field — so a disagreement
/// between them would have silently compared the turned card against card 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Nothing is turned; the next card turned is the first of a pair.
    FirstPick,
    /// One card is turned and waiting for its partner.
    SecondPick,
    /// Two cards are turned and did not match. They are being read.
    Showing,
    /// Every pair is found.
    Won,
}

/// A direction for the keyboard cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Everything the game can be asked to do, from either input.
///
/// The pointer and the keyboard both build one of these and hand it to
/// [`MemoryGame::apply`], so there is one implementation of each rule rather
/// than one per input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Turn the card at this flat index.
    Turn(usize),
    /// Turn the card under the keyboard cursor.
    TurnCursor,
    /// Move the keyboard cursor one cell.
    Nudge(Dir),
    /// Switch to `SIZES[index]` and deal.
    SetSize(usize),
    /// Deal a new board of the same shape.
    NewGame,
    ToggleHelp,
}

/// Everything a click can land on.
///
/// The painter records one of these for each thing it draws, so what is
/// clickable is exactly what is visible, in the place it was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A card, by flat index.
    Card(usize),
    /// A board-size button, by index into [`SIZES`].
    Size(usize),
    NewGame,
    Help,
    /// The help sheet itself: it swallows clicks meant for the board behind it.
    HelpSheet,
}

pub type Frame = guitk::frame::Frame<Target>;

/// The index into [`SIZES`] of a board shape, if the game offers it.
fn size_index_of(rows: usize, cols: usize) -> Option<usize> {
    SIZES.iter().position(|&(r, c)| r == rows && c == cols)
}

/// A game of Memory.
pub struct MemoryGame {
    /// The board, row-major. Its length *is* `rows * cols`; the old
    /// `total_pairs` field was a second copy of `cards.len() / 2`.
    cards: Vec<Card>,
    rows: usize,
    cols: usize,
    /// The first card of the pair being turned, if one is up.
    first_pick: Option<usize>,
    /// The second, set only while a mismatch is being read.
    second_pick: Option<usize>,
    /// Milliseconds left of that display. Zero when nothing is showing.
    showing_ms: u64,
    /// The keyboard cursor, as a flat index.
    cursor: usize,
    moves: u32,
    board_no: u32,
    /// Fewest moves ever taken to clear each entry of [`SIZES`].
    best_moves: [Option<u32>; SIZES.len()],
    show_help: bool,
    rng: SeededRng,
    /// The window the last frame was drawn for, so a click can be read
    /// against the same geometry it was aimed at.
    width: f32,
    height: f32,
}

impl Default for MemoryGame {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryGame {
    /// A game seeded from the system.
    pub fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A game whose deal is fixed by `seed`, for tests and for replaying a
    /// board.
    pub fn with_seed(seed: u64) -> Self {
        Self::with_rng(SeededRng::new(seed))
    }

    fn with_rng(rng: SeededRng) -> Self {
        let (rows, cols) = DEFAULT_SIZE;
        let mut game = Self {
            cards: Vec::new(),
            rows,
            cols,
            first_pick: None,
            second_pick: None,
            showing_ms: 0,
            cursor: 0,
            moves: 0,
            board_no: 0,
            best_moves: [None; SIZES.len()],
            show_help: false,
            rng,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        };
        game.deal();
        game
    }

    // ── What is true right now ─────────────────────────────────────────────

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn cards(&self) -> &[Card] {
        &self.cards
    }
    pub fn moves(&self) -> u32 {
        self.moves
    }
    pub fn board_no(&self) -> u32 {
        self.board_no
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn show_help(&self) -> bool {
        self.show_help
    }
    pub fn showing_ms(&self) -> u64 {
        self.showing_ms
    }
    pub fn best_moves(&self) -> &[Option<u32>; SIZES.len()] {
        &self.best_moves
    }

    /// The pairs on the board. Derived, not stored.
    pub fn total_pairs(&self) -> usize {
        self.cards.len() / 2
    }

    /// How many of them have been found.
    pub fn pairs_found(&self) -> usize {
        self.cards.iter().filter(|c| c.matched).count() / 2
    }

    pub fn won(&self) -> bool {
        !self.cards.is_empty() && self.cards.iter().all(|c| c.matched)
    }

    /// True while a mismatched pair is being read.
    pub fn showing(&self) -> bool {
        self.second_pick.is_some()
    }

    pub fn phase(&self) -> Phase {
        if self.won() {
            Phase::Won
        } else if self.second_pick.is_some() {
            Phase::Showing
        } else if self.first_pick.is_some() {
            Phase::SecondPick
        } else {
            Phase::FirstPick
        }
    }

    /// The face on the card at `index`, if there is a card there.
    pub fn face_of(&self, index: usize) -> Option<usize> {
        self.cards.get(index).map(|c| c.face)
    }

    /// Whether the card at `index` is showing its face.
    pub fn face_up(&self, index: usize) -> bool {
        self.cards.get(index).is_some_and(|c| c.matched)
            || self.first_pick == Some(index)
            || self.second_pick == Some(index)
    }

    /// The flat index of `(row, col)`, whether or not a card is there.
    pub fn index_of(&self, row: usize, col: usize) -> usize {
        row.saturating_mul(self.cols).saturating_add(col)
    }

    /// The cursor as (row, col).
    pub fn cursor_rc(&self) -> (usize, usize) {
        let cols = self.cols.max(1);
        // `checked_*` rather than `/` and `%` only because the lint cannot
        // see that `cols` was just clamped to at least one.
        (
            self.cursor.checked_div(cols).unwrap_or(0),
            self.cursor.checked_rem(cols).unwrap_or(0),
        )
    }

    // ── Changing it ────────────────────────────────────────────────────────

    /// Deal a fresh board of the current shape.
    fn deal(&mut self) {
        let pairs = self.rows.saturating_mul(self.cols) / 2;
        let mut faces: Vec<usize> = Vec::with_capacity(pairs.saturating_mul(2));
        for face in 0..pairs {
            faces.push(face);
            faces.push(face);
        }
        self.rng.shuffle(&mut faces);
        self.cards = faces
            .into_iter()
            .map(|face| Card {
                face,
                matched: false,
            })
            .collect();
        self.first_pick = None;
        self.second_pick = None;
        self.showing_ms = 0;
        self.cursor = 0;
        self.moves = 0;
        self.board_no = self.board_no.saturating_add(1);
    }

    /// Turn the mismatched pair back over.
    fn dismiss(&mut self) {
        self.first_pick = None;
        self.second_pick = None;
        self.showing_ms = 0;
    }

    /// Age a showing pair by the interval the compositor reported.
    ///
    /// Driven by the reported elapsed time rather than by counting ticks, so
    /// a coarse or jittery clock changes how *smoothly* the wait passes but
    /// never how long it lasts. Returns whether anything changed, which is
    /// what tells the window whether to redraw.
    pub fn advance(&mut self, elapsed_ms: u64) -> bool {
        if self.showing_ms == 0 {
            return false;
        }
        self.showing_ms = self.showing_ms.saturating_sub(elapsed_ms);
        if self.showing_ms == 0 {
            self.dismiss();
        }
        true
    }

    /// Turn the card at `index`, resolving a pair if it completes one.
    fn turn(&mut self, index: usize) -> bool {
        if self.won() {
            return false;
        }
        match self.cards.get(index) {
            Some(card) if !card.matched => {}
            _ => return false,
        }
        if self.first_pick == Some(index) {
            return false;
        }
        self.cursor = index;
        let Some(first) = self.first_pick else {
            self.first_pick = Some(index);
            return true;
        };
        self.moves = self.moves.saturating_add(1);
        let same = match (self.face_of(first), self.face_of(index)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if same {
            for i in [first, index] {
                if let Some(card) = self.cards.get_mut(i) {
                    card.matched = true;
                }
            }
            self.first_pick = None;
            if self.won() {
                self.record_best();
            }
        } else {
            self.second_pick = Some(index);
            self.showing_ms = SHOW_MS;
        }
        true
    }

    fn record_best(&mut self) {
        let moves = self.moves;
        let Some(slot) =
            size_index_of(self.rows, self.cols).and_then(|i| self.best_moves.get_mut(i))
        else {
            return;
        };
        let better = match *slot {
            Some(best) => moves < best,
            None => true,
        };
        if better {
            *slot = Some(moves);
        }
    }

    /// Where the cursor lands after a nudge. Clamped at the edges: a cursor
    /// that wrapped from the last column to the first would move the eye
    /// across the whole board for one key press.
    fn cursor_after(&self, dir: Dir) -> usize {
        let (row, col) = self.cursor_rc();
        let last_row = self.rows.saturating_sub(1);
        let last_col = self.cols.saturating_sub(1);
        let (r, c) = match dir {
            Dir::Up => (row.saturating_sub(1), col),
            Dir::Down => (row.saturating_add(1).min(last_row), col),
            Dir::Left => (row, col.saturating_sub(1)),
            Dir::Right => (row, col.saturating_add(1).min(last_col)),
        };
        self.index_of(r, c)
    }

    fn set_size(&mut self, index: usize) -> bool {
        let Some(&(rows, cols)) = SIZES.get(index) else {
            return false;
        };
        self.rows = rows;
        self.cols = cols;
        self.deal();
        true
    }

    /// Whether `action` would change anything, so a control can be drawn dim
    /// and a test can check that dim means what it looks like.
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::ToggleHelp | Action::NewGame => true,
            Action::SetSize(index) => index < SIZES.len(),
            // While a pair is being read, any board input turns it back — so
            // every board action is live, it just does that instead.
            Action::Turn(index) => {
                self.showing()
                    || (!self.won()
                        && self.first_pick != Some(index)
                        && self.cards.get(index).is_some_and(|c| !c.matched))
            }
            Action::TurnCursor => self.enabled(Action::Turn(self.cursor)),
            Action::Nudge(dir) => self.showing() || self.cursor_after(dir) != self.cursor,
        }
    }

    /// The one place any input changes the game.
    ///
    /// Returns whether anything changed, which is what the window is told
    /// when it asks whether to redraw.
    pub fn apply(&mut self, action: Action) -> bool {
        match action {
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
            Action::NewGame => {
                self.deal();
                true
            }
            Action::SetSize(index) => self.set_size(index),
            // A board input arriving while a mismatch is on screen turns the
            // pair back and does nothing else: the board it was aimed at is
            // about to change, and acting on it would turn a card the player
            // had not decided on.
            Action::Turn(index) => {
                if self.showing() {
                    self.dismiss();
                    return true;
                }
                self.turn(index)
            }
            Action::TurnCursor => {
                if self.showing() {
                    self.dismiss();
                    return true;
                }
                self.turn(self.cursor)
            }
            Action::Nudge(dir) => {
                if self.showing() {
                    self.dismiss();
                    return true;
                }
                let next = self.cursor_after(dir);
                if next == self.cursor {
                    return false;
                }
                self.cursor = next;
                true
            }
        }
    }
}
// ── Window ─────────────────────────────────────────────────────────────────

impl MemoryGame {
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    pub fn width(&self) -> f32 {
        self.width
    }
    pub fn height(&self) -> f32 {
        self.height
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

/// A ring drawn as four bars rather than a stroked rectangle, because a stroke
/// is centred on the edge and would bleed half its width into the gutter — and
/// so into the neighbouring card's hit box.
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

impl MemoryGame {
    /// The layout of this board in a window of the given size.
    ///
    /// The board shape goes in because the grid is not always square: a 4x6
    /// board and a 6x6 board want different rectangles out of the same window.
    pub fn layout(&self, width: f32, height: f32) -> Layout {
        Layout::new(width, height, self.rows, self.cols)
    }

    /// The whole window, and every hit box in it, in one pass.
    ///
    /// `Frame::hit_test` scans the recorded boxes in reverse, so anything
    /// drawn later wins the click over what it covers. That is why the help
    /// sheet is painted last.
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
        if l.shows_best() {
            self.draw_best(&mut f, &l);
        }
        self.draw_board(&mut f, &l);
        if self.won() {
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
        let btn = l.help_button();
        let title_span = (btn.x - l.pad * 2.0 - l.header.x).max(0.0);
        label(
            f,
            l.header.x + l.pad,
            cy - text::line_height(l.font, FontWeightHint::Bold) / 2.0,
            "Memory",
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
        let body = format!(
            "Deal {}   {}x{}   Moves {}   Pairs {}/{}",
            self.board_no,
            self.rows,
            self.cols,
            self.moves,
            self.pairs_found(),
            self.total_pairs(),
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
        for (i, (rows, cols)) in SIZES.iter().enumerate() {
            let score = match self.best_moves.get(i).copied().flatten() {
                Some(moves) => moves.to_string(),
                None => String::from("-"),
            };
            body.push_str(&format!("   {rows}x{cols} {score}"));
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
        let cell0 = l.cell(self.rows, self.cols, 0, 0);
        // The gutter is part of the cell, not part of the card: the card is
        // drawn inset, but the hit box is the whole cell, so a click a pixel
        // into the gap picks the card it is nearest instead of falling
        // through to the board behind.
        let gutter = (cell0.w.min(cell0.h) * 0.06).clamp(0.0, 6.0);
        let radius = (cell0.w.min(cell0.h) * 0.12).min(10.0);
        let face_size = (cell0.h * 0.42).clamp(6.0, 46.0);
        let (crow, ccol) = self.cursor_rc();

        for row in 0..self.rows {
            for col in 0..self.cols {
                let index = self.index_of(row, col);
                let cell = l.cell(self.rows, self.cols, row, col);
                let card = Rect::new(
                    cell.x + gutter,
                    cell.y + gutter,
                    (cell.w - gutter * 2.0).max(0.0),
                    (cell.h - gutter * 2.0).max(0.0),
                );
                let Some(&Card { face, matched }) = self.cards.get(index) else {
                    continue;
                };
                let up = self.face_up(index);
                let back = if matched {
                    COL_SURFACE0
                } else if up {
                    COL_MANTLE
                } else {
                    COL_SURFACE1
                };
                fill(f, card, back, radius);
                if up {
                    let colour = if matched {
                        COL_GREEN
                    } else {
                        SYMBOL_COLORS.get(face).copied().unwrap_or(COL_TEXT)
                    };
                    let glyph = SYMBOLS.get(face).copied().unwrap_or("?");
                    centred_in(
                        f,
                        card.x,
                        card.w,
                        card.y + card.h / 2.0,
                        glyph,
                        face_size,
                        colour,
                        FontWeightHint::Bold,
                    );
                } else {
                    // A face-down card is not blank: an empty rounded box on a
                    // dark board reads as a hole in the grid rather than as a
                    // card waiting to be turned.
                    let dot = (card.w.min(card.h) * 0.18).min(14.0);
                    fill(
                        f,
                        Rect::new(
                            card.x + (card.w - dot) / 2.0,
                            card.y + (card.h - dot) / 2.0,
                            dot,
                            dot,
                        ),
                        COL_CRUST,
                        dot / 2.0,
                    );
                }
                if row == crow && col == ccol {
                    ring(f, card, (gutter * 0.9).max(1.0), COL_YELLOW);
                }
                f.hit(Target::Card(index), cell);
            }
        }
    }

    fn draw_banner(&self, f: &mut Frame, l: &Layout) {
        let r = l.banner();
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        fill(f, r, COL_BANNER, (r.h * 0.2).min(10.0));
        let body = format!("Cleared in {} moves — click for a new deal", self.moves);
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h / 2.0,
            &body,
            l.small,
            COL_GREEN,
            FontWeightHint::Bold,
        );
        // The banner is the new-deal button while it is up: a player who has
        // just won is already looking here.
        f.hit(Target::NewGame, r);
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        for (i, (rows, cols)) in SIZES.iter().enumerate() {
            let r = l.footer_button(i);
            let current = self.rows == *rows && self.cols == *cols;
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
                &format!("{rows}x{cols}"),
                l.small,
                if current { COL_YELLOW } else { COL_TEXT },
                FontWeightHint::Bold,
            );
            // Recorded even for the board already showing: a click there
            // should stop at the button, not reach whatever is behind it.
            f.hit(Target::Size(i), r);
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
}

/// The help sheet the old code toggled a flag for and never drew.
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
// ── Input ──────────────────────────────────────────────────────────────────

impl MemoryGame {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The fault that broke every key in this file, in one line. A release
        // is not a second press. Acting on both meant the release of the key
        // that turned the second card immediately turned the pair back over —
        // so a mismatch was visible for the length of a key release, in a
        // game whose entire content is seeing the cards. It also moved the
        // cursor two cards per press and dealt two boards per `N`.
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
            Key::Enter | Key::Space => Some(Action::TurnCursor),
            Key::N => Some(Action::NewGame),
            Key::H => Some(Action::ToggleHelp),
            key => SIZE_KEYS
                .iter()
                .position(|k| *k == key)
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
            Target::Card(index) => {
                self.apply(Action::Turn(index));
            }
            Target::Size(index) => {
                self.apply(Action::SetSize(index));
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
pub fn handle_event(app: &mut MemoryGame, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The clock the old `Showing` phase never had. Ageing by the reported
        // interval rather than by counting ticks keeps the display the same
        // length whatever rate the compositor settles on.
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

impl App for MemoryGame {
    fn title(&self) -> String {
        "Memory".to_string()
    }

    fn app_id(&self) -> String {
        "memory".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Ticks are asked for only while a pair is being read.
    ///
    /// A board sitting still needs no frames, and a game that asks for 60 a
    /// second regardless is a game that keeps a laptop awake to draw the same
    /// pixels.
    fn tick_interval(&self) -> Option<Duration> {
        if self.showing() {
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

impl Probe for MemoryGame {
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
    let mut game = MemoryGame::new();
    app::launch("memory", &mut game)
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

    /// Windows the board has to survive, from a desktop down to a postage
    /// stamp. Every layout test walks all of them.
    const WINDOWS: [(f32, f32); 9] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (760.0, 660.0),
        (640.0, 480.0),
        (480.0, 320.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (80.0, 60.0),
        (24.0, 24.0),
    ];

    fn game() -> MemoryGame {
        MemoryGame::with_seed(0x5EED)
    }

    fn windowed(width: f32, height: f32) -> MemoryGame {
        let mut app = game();
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

    /// Dispatched straight at `handle_event`, not through `probe::key`, which
    /// resizes the app to `Probe::SIZE` first — that would hide any test about
    /// the window size surviving an action.
    fn press(app: &mut MemoryGame, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn lift(app: &mut MemoryGame, key: Key) -> EventResult {
        handle_event(app, &Event::Key(release(key)))
    }

    /// A whole keystroke: down and up, as a window delivers it.
    fn tap(app: &mut MemoryGame, key: Key) {
        press(app, key);
        lift(app, key);
    }

    fn click(app: &mut MemoryGame, x: f32, y: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    fn tick(app: &mut MemoryGame, elapsed_ms: u64) -> EventResult {
        handle_event(app, &Event::Tick { elapsed_ms })
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

    /// Two cards on the board that carry the same face.
    fn matching_pair(app: &MemoryGame) -> (usize, usize) {
        let cards = app.cards();
        for i in 0..cards.len() {
            for j in (i + 1)..cards.len() {
                if !cards[i].matched && cards[i].face == cards[j].face {
                    return (i, j);
                }
            }
        }
        panic!("a dealt board always holds at least one pair");
    }

    /// Two cards on the board that do not.
    fn mismatched_pair(app: &MemoryGame) -> (usize, usize) {
        let cards = app.cards();
        for i in 0..cards.len() {
            for j in (i + 1)..cards.len() {
                if !cards[i].matched && !cards[j].matched && cards[i].face != cards[j].face {
                    return (i, j);
                }
            }
        }
        panic!("a board of more than one pair always holds a mismatch");
    }

    /// Turn a mismatched pair and leave it showing.
    fn show_a_mismatch(app: &mut MemoryGame) -> (usize, usize) {
        let (a, b) = mismatched_pair(app);
        assert!(app.apply(Action::Turn(a)));
        assert!(app.apply(Action::Turn(b)));
        assert!(app.showing(), "two unlike cards should be left on show");
        (a, b)
    }

    // ── The faults ─────────────────────────────────────────────────────────

    #[test]
    fn a_release_is_not_a_second_press() {
        for key in [Key::Right, Key::Down, Key::N, Key::H, Key::Num2, Key::Enter] {
            let mut app = game();
            assert_eq!(
                lift(&mut app, key),
                EventResult::Ignored,
                "{key:?} acted on its release"
            );
            assert_eq!(app.moves(), 0);
            assert_eq!(app.board_no(), 1);
            assert_eq!(app.cursor(), 0);
            assert!(!app.show_help());
        }
    }

    #[test]
    fn turning_the_second_card_leaves_it_face_up() {
        // The fault this whole file exists for: the release of the keystroke
        // that turned the second card ran the handler again, landed in the
        // showing phase and dismissed it. The card was visible for the length
        // of a key release.
        let mut app = game();
        let (a, b) = mismatched_pair(&app);
        app.apply(Action::Turn(a));

        // Reach `b` with the cursor and turn it with a whole keystroke.
        while app.cursor() != b {
            let (row, col) = app.cursor_rc();
            let (brow, bcol) = (b / app.cols(), b % app.cols());
            if col < bcol {
                tap(&mut app, Key::Right);
            } else if col > bcol {
                tap(&mut app, Key::Left);
            } else if row < brow {
                tap(&mut app, Key::Down);
            } else {
                tap(&mut app, Key::Up);
            }
        }
        tap(&mut app, Key::Enter);

        assert!(
            app.face_up(a),
            "the first card turned back on a key release"
        );
        assert!(
            app.face_up(b),
            "the second card turned back on a key release"
        );
        assert_eq!(app.phase(), Phase::Showing);
        assert_eq!(app.showing_ms(), SHOW_MS);
    }

    #[test]
    fn a_mismatched_pair_stays_up_until_the_clock_says_otherwise() {
        let mut app = game();
        let (a, b) = show_a_mismatch(&mut app);
        // Most of the window gone, and the cards are still readable.
        assert_eq!(tick(&mut app, SHOW_MS - 1), EventResult::Consumed);
        assert!(app.face_up(a));
        assert!(app.face_up(b));
        assert_eq!(app.showing_ms(), 1);
    }

    #[test]
    fn the_pair_turns_back_when_the_window_runs_out() {
        let mut app = game();
        let (a, b) = show_a_mismatch(&mut app);
        tick(&mut app, SHOW_MS);
        assert!(!app.showing());
        assert!(!app.face_up(a), "a card left face up after its display");
        assert!(!app.face_up(b), "a card left face up after its display");
        assert_eq!(app.phase(), Phase::FirstPick);
        assert_eq!(app.showing_ms(), 0);
    }

    #[test]
    fn the_display_is_timed_by_the_clock_not_by_the_tick_count() {
        // Two clocks of very different rates must hide the pair at the same
        // point in time, which is what "driven by the reported interval"
        // means. A count of ticks would make a slow clock a long display.
        for step in [1_u64, 7, 16, 100, 350] {
            let mut app = game();
            show_a_mismatch(&mut app);
            let mut elapsed = 0;
            while app.showing() {
                tick(&mut app, step);
                elapsed += step;
                assert!(elapsed <= SHOW_MS + step, "display overran at {step}ms");
            }
            assert!(
                elapsed >= SHOW_MS,
                "display ended after {elapsed}ms at a {step}ms tick"
            );
        }
    }

    #[test]
    fn a_tick_with_nothing_showing_is_not_work() {
        let mut app = game();
        assert_eq!(tick(&mut app, 1000), EventResult::Ignored);
        assert_eq!(app.moves(), 0);
    }

    #[test]
    fn the_window_is_only_asked_for_ticks_while_a_pair_shows() {
        let mut app = game();
        assert!(app.tick_interval().is_none());
        show_a_mismatch(&mut app);
        assert_eq!(app.tick_interval(), Some(Duration::from_millis(TICK_MS)));
        tick(&mut app, SHOW_MS);
        assert!(app.tick_interval().is_none());
    }

    #[test]
    fn an_input_during_the_display_turns_the_pair_back_and_nothing_else() {
        // A player who has already read the cards should not have to wait —
        // but the board is about to change under them, so the input must not
        // also turn a card they had not decided on.
        for key in [Key::Enter, Key::Right, Key::Space] {
            let mut app = game();
            let (a, b) = show_a_mismatch(&mut app);
            let before_cursor = app.cursor();
            let before_moves = app.moves();
            tap(&mut app, key);
            assert!(!app.showing(), "{key:?} left the pair showing");
            assert!(!app.face_up(a));
            assert!(!app.face_up(b));
            assert_eq!(app.cursor(), before_cursor, "{key:?} also moved");
            assert_eq!(app.moves(), before_moves, "{key:?} also played");
            assert_eq!(app.phase(), Phase::FirstPick);
        }
    }

    #[test]
    fn a_click_during_the_display_turns_the_pair_back_and_nothing_else() {
        let mut app = game();
        show_a_mismatch(&mut app);
        let elsewhere = mismatched_pair(&app).0;
        let r = probe::rect_of(&app, Target::Card(elsewhere)).unwrap();
        app.resize(MemoryGame::SIZE.0, MemoryGame::SIZE.1);
        click(&mut app, r.centre().0, r.centre().1);
        assert!(!app.showing());
        assert_eq!(
            app.phase(),
            Phase::FirstPick,
            "the click also picked a card"
        );
    }

    #[test]
    fn an_arrow_key_moves_the_cursor_one_card() {
        let mut app = game();
        tap(&mut app, Key::Right);
        assert_eq!(app.cursor_rc(), (0, 1), "the cursor moved twice");
        tap(&mut app, Key::Down);
        assert_eq!(app.cursor_rc(), (1, 1));
        tap(&mut app, Key::Left);
        assert_eq!(app.cursor_rc(), (1, 0));
        tap(&mut app, Key::Up);
        assert_eq!(app.cursor_rc(), (0, 0));
    }

    #[test]
    fn the_keyboard_can_reach_every_card_of_the_board() {
        // The double-fire moved two cards per press, so on a four-wide board
        // half the columns could not be landed on at all.
        for index in 0..SIZES.len() {
            let mut app = game();
            app.apply(Action::SetSize(index));
            let (rows, cols) = (app.rows(), app.cols());
            let mut seen = vec![false; rows * cols];
            for row in 0..rows {
                for col in 0..cols {
                    while app.cursor_rc().1 > col {
                        tap(&mut app, Key::Left);
                    }
                    while app.cursor_rc().1 < col {
                        tap(&mut app, Key::Right);
                    }
                    while app.cursor_rc().0 > row {
                        tap(&mut app, Key::Up);
                    }
                    while app.cursor_rc().0 < row {
                        tap(&mut app, Key::Down);
                    }
                    assert_eq!(app.cursor_rc(), (row, col));
                    seen[app.cursor()] = true;
                }
            }
            assert!(
                seen.iter().all(|s| *s),
                "{rows}x{cols} has unreachable cards"
            );
        }
    }

    #[test]
    fn a_key_deals_one_board_not_two() {
        let mut app = game();
        tap(&mut app, Key::N);
        assert_eq!(app.board_no(), 2, "N dealt twice");
        tap(&mut app, Key::Num3);
        assert_eq!(app.board_no(), 3, "a size key dealt twice");
        assert_eq!((app.rows(), app.cols()), SIZES[2]);
    }

    #[test]
    fn the_help_sheet_can_actually_be_opened() {
        // `show_help` used to be a flag nothing drew: pressing H removed the
        // "H for help" hint and put nothing in its place.
        let mut app = game();
        assert!(!app.show_help());
        tap(&mut app, Key::H);
        assert!(app.show_help(), "H opened the sheet and closed it again");
        let drawn = texts(&app.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1));
        assert!(
            drawn.iter().any(|t| t == HELP_TITLE),
            "the sheet is set but nothing draws it: {drawn:?}"
        );
        tap(&mut app, Key::H);
        assert!(!app.show_help());
    }

    #[test]
    fn the_help_sheet_names_every_key_the_game_answers() {
        let mut app = game();
        app.apply(Action::ToggleHelp);
        let drawn = texts(&app.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1)).join("\n");
        for (k, v) in HELP_ROWS {
            if !k.is_empty() {
                assert!(drawn.contains(k), "the sheet does not mention {k}");
            }
            assert!(drawn.contains(v), "the sheet does not explain {v}");
        }
    }

    #[test]
    fn every_control_can_be_reached_with_the_pointer() {
        // Only the grid used to be clickable: no new deal, no board size, no
        // help.
        let app = game();
        for index in 0..SIZES.len() {
            assert!(
                probe::is_visible(&app, Target::Size(index)),
                "board size {index} has no hit box"
            );
        }
        assert!(probe::is_visible(&app, Target::NewGame));
        assert!(probe::is_visible(&app, Target::Help));
        for index in 0..app.cards().len() {
            assert!(
                probe::is_visible(&app, Target::Card(index)),
                "card {index} has no hit box"
            );
        }
    }

    #[test]
    fn what_is_drawn_at_a_card_is_what_a_click_there_turns() {
        let mut app = windowed(900.0, 700.0);
        for index in 0..app.cards().len() {
            let r = probe::rect_of_sized(&app, Target::Card(index), (900.0, 700.0)).unwrap();
            let (cx, cy) = r.centre();
            assert_eq!(app.target_at(cx, cy), Some(Target::Card(index)));
        }
        // And the click actually turns that card, not its neighbour.
        let r = probe::rect_of_sized(&app, Target::Card(5), (900.0, 700.0)).unwrap();
        click(&mut app, r.centre().0, r.centre().1);
        assert!(app.face_up(5));
    }

    #[test]
    fn the_phase_cannot_disagree_with_the_picks() {
        // `phase` used to be a stored field beside the two `Option`s, and
        // `flip_card` read `first_pick.unwrap_or(0)` while trusting it.
        let mut app = game();
        assert_eq!(app.phase(), Phase::FirstPick);
        let (a, b) = mismatched_pair(&app);
        app.apply(Action::Turn(a));
        assert_eq!(app.phase(), Phase::SecondPick);
        app.apply(Action::Turn(b));
        assert_eq!(app.phase(), Phase::Showing);
        tick(&mut app, SHOW_MS);
        assert_eq!(app.phase(), Phase::FirstPick);
    }

    #[test]
    fn the_board_is_its_own_pair_count() {
        // `total_pairs` used to be a second copy of `cards.len() / 2`.
        for index in 0..SIZES.len() {
            let mut app = game();
            app.apply(Action::SetSize(index));
            let (rows, cols) = SIZES[index];
            assert_eq!(app.cards().len(), rows * cols);
            assert_eq!(app.total_pairs(), rows * cols / 2);
        }
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        for (w, h) in WINDOWS {
            for index in 0..SIZES.len() {
                let mut app = game();
                app.apply(Action::SetSize(index));
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} board {index}");
                show_a_mismatch(&mut app);
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} showing");
                app.apply(Action::ToggleHelp);
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} help");
            }
        }
    }

    #[test]
    fn the_whole_window_is_painted() {
        // The old `render` used its arguments for one background rectangle and
        // then drew everything else at fixed coordinates.
        for (w, h) in WINDOWS {
            let f = game().frame(w, h);
            assert_eq!(f.width, w.max(1.0));
            assert_eq!(f.height, h.max(1.0));
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
            assert!(covered, "{w}x{h} leaves part of the window unpainted");
        }
    }

    #[test]
    fn the_board_stays_inside_its_window() {
        for (w, h) in WINDOWS {
            for index in 0..SIZES.len() {
                let mut app = game();
                app.apply(Action::SetSize(index));
                let l = app.layout(w, h);
                assert!(l.board.x >= -0.01, "{w}x{h} board off the left");
                assert!(l.board.y >= -0.01, "{w}x{h} board off the top");
                assert!(l.board.right() <= w + 0.01, "{w}x{h} board off the right");
                assert!(
                    l.board.bottom() <= h + 0.01,
                    "{w}x{h} board off the bottom (board {index})"
                );
            }
        }
    }

    #[test]
    fn every_card_is_drawn_inside_the_window_it_was_sized_for() {
        // A 6x6 board used to be 528px of fixed-size cards starting at y=80,
        // so on an 800x600 window the bottom row was simply off the screen.
        for (w, h) in WINDOWS {
            for index in 0..SIZES.len() {
                let mut app = game();
                app.apply(Action::SetSize(index));
                for card in 0..app.cards().len() {
                    let Some(r) = probe::rect_of_sized(&app, Target::Card(card), (w, h)) else {
                        panic!("{w}x{h} board {index}: card {card} was not drawn");
                    };
                    assert!(
                        r.x >= -0.01
                            && r.y >= -0.01
                            && r.right() <= w + 0.01
                            && r.bottom() <= h + 0.01,
                        "{w}x{h} board {index}: card {card} at {r:?} is outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn cards_are_the_same_size_whatever_shape_the_board_is() {
        // A 4x6 grid is not a 6x6 grid with two blank rows: the rectangle has
        // to take the grid's aspect ratio or the cards come out oblong.
        for (w, h) in WINDOWS {
            for index in 0..SIZES.len() {
                let (rows, cols) = SIZES[index];
                let l = Layout::new(w, h, rows, cols);
                let cell = l.cell(rows, cols, 0, 0);
                assert!(
                    (cell.w - cell.h).abs() <= 0.01,
                    "{w}x{h} {rows}x{cols}: cells are {cell:?}"
                );
            }
        }
    }

    #[test]
    fn no_band_is_laid_past_the_bottom_of_the_window() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h, 6, 6);
            for (name, r) in [
                ("header", l.header),
                ("info", l.info),
                ("best", l.best),
                ("footer", l.footer),
            ] {
                assert!(r.bottom() <= h + 0.01, "{w}x{h}: {name} runs past the foot");
            }
        }
    }

    #[test]
    fn a_window_too_short_for_the_chrome_drops_it_rather_than_the_board() {
        // Bands are dropped whole rather than squeezed: a footer scaled to
        // four pixels costs the board four pixels and shows nothing.
        let tall = Layout::new(760.0, 660.0, 4, 4);
        assert!(tall.shows_footer());
        assert!(tall.shows_best());
        let squat = Layout::new(760.0, 90.0, 4, 4);
        assert!(!squat.shows_footer(), "the footer survived a 90px window");
        assert!(squat.board.h > 0.0, "the board was squeezed to nothing");
        assert!(squat.board.h >= 90.0 * BOARD_SHARE - 0.01);
    }

    #[test]
    fn the_board_keeps_its_share_of_every_window() {
        for (w, h) in WINDOWS {
            for index in 0..SIZES.len() {
                let (rows, cols) = SIZES[index];
                let l = Layout::new(w, h, rows, cols);
                // Not the full share on very wide-and-short windows, where the
                // width is what binds — but never nothing.
                assert!(l.board.w > 0.0 && l.board.h > 0.0, "{w}x{h} {rows}x{cols}");
            }
        }
    }

    #[test]
    fn a_board_the_size_of_a_postage_stamp_still_draws_every_card() {
        let mut app = windowed(24.0, 24.0);
        app.apply(Action::SetSize(2));
        let f = app.frame(24.0, 24.0);
        for card in 0..app.cards().len() {
            assert!(
                f.rect_of(|t| *t == Target::Card(card)).is_some(),
                "card {card} vanished at 24x24"
            );
        }
    }

    #[test]
    fn the_banner_stays_inside_the_board_it_covers() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h, 4, 4);
            let b = l.banner();
            assert!(b.x >= l.board.x - 0.01);
            assert!(b.right() <= l.board.right() + 0.01);
            assert!(b.bottom() <= l.board.bottom() + 0.01, "{w}x{h}");
            assert!(b.y >= l.board.y - 0.01, "{w}x{h}: banner taller than board");
        }
    }

    #[test]
    fn the_help_sheet_never_writes_past_its_own_panel() {
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Action::ToggleHelp);
            let l = app.layout(w, h);
            let f = app.frame(w, h);
            for c in f.commands() {
                if let RenderCommand::Text { x, y, text, .. } = c {
                    if text == HELP_TITLE || HELP_ROWS.iter().any(|(k, v)| text == k || text == v) {
                        assert!(
                            *x >= l.help.x - 0.01 && *y >= l.help.y - 0.01,
                            "{w}x{h}: {text:?} at ({x}, {y}) is above/left of {:?}",
                            l.help
                        );
                        assert!(
                            *y <= l.help.bottom() + 0.01,
                            "{w}x{h}: {text:?} runs off the foot of the sheet"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_help_sheet_is_in_front_of_everything_it_covers() {
        let mut app = game();
        app.apply(Action::ToggleHelp);
        let f = app.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1);
        // `hit_test` scans in reverse, so the sheet wins only by being last.
        assert_eq!(f.hit_test(10.0, 10.0), Some(Target::HelpSheet));
        let (w, h) = MemoryGame::SIZE;
        assert_eq!(f.hit_test(w / 2.0, h / 2.0), Some(Target::HelpSheet));
        assert_eq!(f.hit_test(w - 2.0, h - 2.0), Some(Target::HelpSheet));
    }

    #[test]
    fn a_click_is_read_against_the_window_it_was_drawn_in() {
        // The whole reason the window size is stored on the model.
        let mut app = windowed(1200.0, 900.0);
        let r = probe::rect_of_sized(&app, Target::Card(0), (1200.0, 900.0)).unwrap();
        let (cx, cy) = r.centre();
        assert_eq!(app.target_at(cx, cy), Some(Target::Card(0)));
        app.resize(400.0, 300.0);
        let small = probe::rect_of_sized(&app, Target::Card(0), (400.0, 300.0)).unwrap();
        assert_ne!(
            (small.x, small.y),
            (r.x, r.y),
            "the layout did not follow the window"
        );
        assert_eq!(
            app.target_at(small.centre().0, small.centre().1),
            Some(Target::Card(0))
        );
    }

    #[test]
    fn a_resize_is_remembered_across_an_action() {
        let mut app = windowed(1000.0, 800.0);
        press(&mut app, Key::N);
        assert_eq!((app.width(), app.height()), (1000.0, 800.0));
        handle_event(
            &mut app,
            &Event::Resize {
                width: 500,
                height: 400,
            },
        );
        assert_eq!((app.width(), app.height()), (500.0, 400.0));
    }

    #[test]
    fn rendering_at_a_size_is_what_the_next_click_is_read_against() {
        let mut app = game();
        let _ = app.render(1100.0, 850.0);
        assert_eq!((app.width(), app.height()), (1100.0, 850.0));
        let r = probe::rect_of_sized(&app, Target::NewGame, (1100.0, 850.0)).unwrap();
        assert_eq!(
            app.target_at(r.centre().0, r.centre().1),
            Some(Target::NewGame)
        );
    }

    #[test]
    fn the_readout_counts_the_board_it_is_drawn_beside() {
        let mut app = game();
        let (a, b) = matching_pair(&app);
        app.apply(Action::Turn(a));
        app.apply(Action::Turn(b));
        let drawn = texts(&app.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1)).join("\n");
        assert!(drawn.contains("Moves 1"), "{drawn}");
        assert!(
            drawn.contains(&format!("Pairs 1/{}", app.total_pairs())),
            "{drawn}"
        );
        assert!(drawn.contains("4x4"));
    }

    #[test]
    fn the_cursor_ring_follows_the_card_it_is_on() {
        let mut app = windowed(900.0, 700.0);
        for target in [3_usize, 7, 12] {
            while app.cursor() != target {
                if app.cursor() < target {
                    if app.cursor_rc().1 + 1 < app.cols() {
                        tap(&mut app, Key::Right);
                    } else {
                        tap(&mut app, Key::Down);
                        while app.cursor_rc().1 > 0 {
                            tap(&mut app, Key::Left);
                        }
                    }
                } else {
                    tap(&mut app, Key::Left);
                }
            }
            let l = app.layout(900.0, 700.0);
            let (row, col) = app.cursor_rc();
            let cell = l.cell(app.rows(), app.cols(), row, col);
            let f = app.frame(900.0, 700.0);
            let ringed = f.commands().iter().any(|c| match c {
                RenderCommand::FillRect {
                    x, y, color, width, ..
                } => {
                    *color == COL_YELLOW
                        && *x >= cell.x - 0.01
                        && *y >= cell.y - 0.01
                        && *x + *width <= cell.right() + 0.01
                }
                _ => false,
            });
            assert!(ringed, "no cursor ring on card {target}");
        }
    }

    // ── The rules ──────────────────────────────────────────────────────────

    /// Play a board out perfectly.
    fn solve(app: &mut MemoryGame) {
        while !app.won() {
            let (a, b) = matching_pair(app);
            app.apply(Action::Turn(a));
            app.apply(Action::Turn(b));
            assert!(!app.showing(), "a matched pair should not need dismissing");
        }
    }

    #[test]
    fn two_like_cards_stay_up_and_count_a_move() {
        let mut app = game();
        let (a, b) = matching_pair(&app);
        assert!(app.apply(Action::Turn(a)));
        assert!(app.apply(Action::Turn(b)));
        assert_eq!(app.moves(), 1);
        assert_eq!(app.pairs_found(), 1);
        assert!(app.face_up(a) && app.face_up(b));
        assert!(app.cards()[a].matched && app.cards()[b].matched);
        assert_eq!(app.phase(), Phase::FirstPick);
    }

    #[test]
    fn two_unlike_cards_count_a_move_too() {
        let mut app = game();
        show_a_mismatch(&mut app);
        assert_eq!(app.moves(), 1);
        assert_eq!(app.pairs_found(), 0);
    }

    #[test]
    fn a_matched_pair_cannot_be_turned_back() {
        let mut app = game();
        let (a, b) = matching_pair(&app);
        app.apply(Action::Turn(a));
        app.apply(Action::Turn(b));
        assert!(!app.apply(Action::Turn(a)), "a found card was turned again");
        assert_eq!(app.moves(), 1);
        assert!(app.face_up(a) && app.face_up(b));
    }

    #[test]
    fn turning_the_card_already_up_is_not_a_move() {
        let mut app = game();
        let (a, _) = mismatched_pair(&app);
        app.apply(Action::Turn(a));
        assert!(!app.apply(Action::Turn(a)), "the first pick matched itself");
        assert_eq!(app.moves(), 0);
        assert_eq!(app.phase(), Phase::SecondPick);
    }

    #[test]
    fn turning_a_card_that_is_not_there_changes_nothing() {
        let mut app = game();
        let off = app.cards().len() + 3;
        assert!(!app.apply(Action::Turn(off)));
        assert_eq!(app.phase(), Phase::FirstPick);
    }

    #[test]
    fn a_deal_puts_every_face_on_the_board_exactly_twice() {
        for index in 0..SIZES.len() {
            let mut app = game();
            app.apply(Action::SetSize(index));
            let (rows, cols) = SIZES[index];
            assert_eq!(app.cards().len(), rows * cols);
            let mut counts = vec![0_usize; app.total_pairs()];
            for card in app.cards() {
                assert!(
                    card.face < counts.len(),
                    "face {} is off the deck",
                    card.face
                );
                assert!(
                    card.face < SYMBOLS.len(),
                    "no symbol for face {}",
                    card.face
                );
                counts[card.face] += 1;
                assert!(!card.matched, "a fresh deal already has a pair found");
            }
            assert!(
                counts.iter().all(|c| *c == 2),
                "{rows}x{cols} was not dealt in pairs: {counts:?}"
            );
        }
    }

    #[test]
    fn the_deal_follows_the_generator_it_was_given() {
        let faces = |app: &MemoryGame| app.cards().iter().map(|c| c.face).collect::<Vec<_>>();
        assert_eq!(
            faces(&MemoryGame::with_seed(7)),
            faces(&MemoryGame::with_seed(7)),
            "the same seed dealt two different boards"
        );
        let mut differs = false;
        for seed in 0..40_u64 {
            if faces(&MemoryGame::with_seed(seed)) != faces(&MemoryGame::with_seed(seed + 1)) {
                differs = true;
                break;
            }
        }
        assert!(differs, "every seed dealt the same board");
    }

    /// On a host build there is no Slate kernel to ask, so `seeded_from_system`
    /// takes its documented fallback and two fresh games *are* identical here.
    /// What distinguishes a seeded-from-the-system game from a hardcoded one is
    /// therefore *which* seed, so that is what is checked -- and on real
    /// hardware the same line reaches the kernel instead.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let faces = |app: &MemoryGame| app.cards().iter().map(|c| c.face).collect::<Vec<_>>();
        assert_eq!(
            faces(&MemoryGame::new()),
            faces(&MemoryGame::with_seed(FALLBACK_SEED)),
            "new() is not going through seeded_from_system"
        );
        assert_ne!(
            faces(&MemoryGame::new()),
            faces(&MemoryGame::with_seed(42)),
            "new() is back on the hardcoded seed the deal used to carry"
        );
    }

    /// A deal must be able to put any face in any position, not a band of
    /// them. A shuffle whose bound reads the low bits of an LCG concentrates
    /// them, which is the game-level shape of the bug `randrange::below`
    /// avoids.
    #[test]
    fn the_deal_can_put_the_first_face_anywhere_on_the_board() {
        let mut seen = vec![false; 16];
        for seed in 0..400_u64 {
            let app = MemoryGame::with_seed(seed);
            // Both of them: the *first* card carrying a face can never be the
            // last cell, so counting only that one would leave a slot nothing
            // could fill and blame the shuffle for it.
            for (at, card) in app.cards().iter().enumerate() {
                if card.face == 0 {
                    if let Some(slot) = seen.get_mut(at) {
                        *slot = true;
                    }
                }
            }
        }
        assert!(
            seen.iter().all(|hit| *hit),
            "face 0 never reached some cells over 400 deals: {seen:?}"
        );
    }

    #[test]
    fn finding_every_pair_wins() {
        let mut app = game();
        solve(&mut app);
        assert!(app.won());
        assert_eq!(app.phase(), Phase::Won);
        assert_eq!(app.pairs_found(), app.total_pairs());
        assert_eq!(app.moves(), app.total_pairs() as u32, "perfect play misc");
    }

    #[test]
    fn a_won_board_takes_no_more_turns() {
        let mut app = game();
        solve(&mut app);
        let moves = app.moves();
        assert!(!app.apply(Action::Turn(0)));
        assert!(!app.apply(Action::TurnCursor));
        assert_eq!(app.moves(), moves);
    }

    #[test]
    fn a_won_board_can_still_be_replaced() {
        let mut app = game();
        solve(&mut app);
        let deal = app.board_no();
        assert!(app.apply(Action::NewGame));
        assert!(!app.won());
        assert_eq!(app.board_no(), deal + 1);
        assert_eq!(app.moves(), 0);
    }

    #[test]
    fn the_best_score_keeps_the_lowest_and_is_kept_per_board() {
        let mut app = game();
        solve(&mut app);
        let perfect = app.moves();
        assert_eq!(app.best_moves()[0], Some(perfect));
        assert_eq!(
            app.best_moves()[1],
            None,
            "one win filled another board's slot"
        );

        // A worse round leaves the record alone.
        app.apply(Action::NewGame);
        show_a_mismatch(&mut app);
        tick(&mut app, SHOW_MS);
        solve(&mut app);
        assert!(app.moves() > perfect);
        assert_eq!(
            app.best_moves()[0],
            Some(perfect),
            "a worse round overwrote"
        );

        // A different board keeps its own record.
        app.apply(Action::SetSize(1));
        solve(&mut app);
        assert_eq!(app.best_moves()[0], Some(perfect));
        assert_eq!(app.best_moves()[1], Some(app.total_pairs() as u32));
    }

    #[test]
    fn the_scoreboard_shows_the_score_that_was_recorded() {
        let mut app = game();
        solve(&mut app);
        let drawn = texts(&app.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1)).join("\n");
        assert!(
            drawn.contains(&format!("4x4 {}", app.moves())),
            "the best score is not on screen: {drawn}"
        );
    }

    #[test]
    fn every_board_size_is_completely_wired() {
        // One list — `SIZES` — feeds the keys, the buttons and the score
        // slots. The old code had a size the scoreboard knew and `set_size`
        // refused.
        for (index, (rows, cols)) in SIZES.iter().enumerate() {
            let mut by_key = game();
            tap(&mut by_key, SIZE_KEYS[index]);
            assert_eq!((by_key.rows(), by_key.cols()), (*rows, *cols));

            let mut by_click = game();
            probe::click(&mut by_click, Target::Size(index));
            assert_eq!((by_click.rows(), by_click.cols()), (*rows, *cols));

            let drawn = texts(&by_click.frame(MemoryGame::SIZE.0, MemoryGame::SIZE.1));
            assert!(
                drawn.iter().any(|t| t == &format!("{rows}x{cols}")),
                "no button is labelled {rows}x{cols}: {drawn:?}"
            );
            assert!(by_click.best_moves().get(index).is_some());
        }
    }

    #[test]
    fn enabled_and_apply_agree() {
        let probes = |app: &MemoryGame| {
            let mut list = vec![
                Action::ToggleHelp,
                Action::NewGame,
                Action::SetSize(0),
                Action::SetSize(SIZES.len()),
                Action::TurnCursor,
                Action::Nudge(Dir::Up),
                Action::Nudge(Dir::Left),
                Action::Nudge(Dir::Down),
                Action::Turn(app.cards().len() + 1),
            ];
            for i in 0..app.cards().len().min(4) {
                list.push(Action::Turn(i));
            }
            list
        };

        for stage in 0..4 {
            let base = || {
                let mut app = game();
                match stage {
                    1 => {
                        let (a, _) = mismatched_pair(&app);
                        app.apply(Action::Turn(a));
                    }
                    2 => {
                        show_a_mismatch(&mut app);
                    }
                    3 => solve(&mut app),
                    _ => {}
                }
                app
            };
            for action in probes(&base()) {
                let mut app = base();
                let said = app.enabled(action);
                let did = app.apply(action);
                assert_eq!(
                    said, did,
                    "stage {stage}: {action:?} claimed {said}, did {did}"
                );
            }
        }
    }

    #[test]
    fn the_cursor_stays_on_the_board() {
        let mut app = game();
        for _ in 0..12 {
            tap(&mut app, Key::Up);
            tap(&mut app, Key::Left);
        }
        assert_eq!(app.cursor_rc(), (0, 0));
        for _ in 0..12 {
            tap(&mut app, Key::Down);
            tap(&mut app, Key::Right);
        }
        assert_eq!(app.cursor_rc(), (app.rows() - 1, app.cols() - 1));
        assert!(app.cursor() < app.cards().len());
    }

    #[test]
    fn a_smaller_board_leaves_the_cursor_on_it() {
        let mut app = game();
        app.apply(Action::SetSize(2));
        for _ in 0..12 {
            tap(&mut app, Key::Down);
            tap(&mut app, Key::Right);
        }
        assert_eq!(app.cursor(), 35);
        app.apply(Action::SetSize(0));
        assert!(
            app.cursor() < app.cards().len(),
            "the cursor was left off the smaller board"
        );
    }

    #[test]
    fn a_click_on_a_control_never_reaches_the_board_behind_it() {
        for target in [
            Target::Size(1),
            Target::Size(2),
            Target::NewGame,
            Target::Help,
        ] {
            let mut app = game();
            assert_eq!(probe::click(&mut app, target), EventResult::Consumed);
            assert!(
                app.cards().iter().enumerate().all(|(i, _)| !app.face_up(i)),
                "{target:?} turned a card behind it"
            );
        }
    }

    #[test]
    fn the_win_banner_deals_the_next_board_when_clicked() {
        let mut app = game();
        solve(&mut app);
        let deal = app.board_no();
        let banner = app.layout(MemoryGame::SIZE.0, MemoryGame::SIZE.1).banner();
        let (cx, cy) = banner.centre();
        app.resize(MemoryGame::SIZE.0, MemoryGame::SIZE.1);
        // The banner is drawn over the foot of the board, after the cards, so
        // it wins the click off the card it covers.
        assert_eq!(
            app.target_at(cx, cy),
            Some(Target::NewGame),
            "the click went through the banner to the card behind it"
        );
        click(&mut app, cx, cy);
        assert_eq!(app.board_no(), deal + 1);
        assert!(!app.won());
    }

    #[test]
    fn keys_do_nothing_behind_the_help_sheet() {
        // Enter, Space, Escape and H deliberately close the sheet; the rest
        // must not reach a board the player cannot see.
        let mut app = game();
        app.apply(Action::ToggleHelp);
        let deal = app.board_no();
        for key in [Key::Up, Key::Left, Key::N, Key::Num2, Key::Num3] {
            assert_eq!(press(&mut app, key), EventResult::Consumed);
            assert!(app.show_help(), "{key:?} closed the sheet");
            assert_eq!(app.board_no(), deal, "{key:?} dealt behind the sheet");
            assert_eq!(app.cursor(), 0, "{key:?} moved the cursor behind the sheet");
            assert_eq!((app.rows(), app.cols()), SIZES[0]);
        }
    }

    #[test]
    fn enter_dismisses_the_help_sheet_without_playing_a_move() {
        // A panel with no visible close button has to answer the keys anyone
        // would press to get rid of it.
        for key in [Key::Enter, Key::Space, Key::Escape, Key::H] {
            let mut app = game();
            app.apply(Action::ToggleHelp);
            tap(&mut app, key);
            assert!(!app.show_help(), "{key:?} left the sheet up");
            assert_eq!(app.moves(), 0, "{key:?} also played a move");
            assert_eq!(app.phase(), Phase::FirstPick);
        }
    }

    #[test]
    fn a_click_does_not_reach_the_board_through_the_help_sheet() {
        let mut app = game();
        let r = probe::rect_of(&app, Target::Card(0)).unwrap();
        app.apply(Action::ToggleHelp);
        app.resize(MemoryGame::SIZE.0, MemoryGame::SIZE.1);
        assert_eq!(
            click(&mut app, r.centre().0, r.centre().1),
            EventResult::Consumed
        );
        assert!(!app.show_help(), "the click did not dismiss the sheet");
        assert!(!app.face_up(0), "the click went through the sheet");
    }

    #[test]
    fn a_modified_key_is_left_for_someone_else() {
        let mut app = game();
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::ctrl(Key::N))),
            EventResult::Ignored
        );
        assert_eq!(app.board_no(), 1);
        assert_eq!(
            handle_event(
                &mut app,
                &Event::Key(probe::press_with(
                    Key::Right,
                    Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    }
                ))
            ),
            EventResult::Ignored
        );
        assert_eq!(app.cursor(), 0);
    }

    #[test]
    fn the_window_is_told_when_to_redraw_and_when_to_close() {
        let mut app = game();
        assert!(matches!(
            app.on_event(&Event::CloseRequested),
            Response::Exit
        ));
        assert!(matches!(
            app.on_event(&Event::Key(probe::press(Key::N))),
            Response::Redraw
        ));
        assert!(matches!(
            app.on_event(&Event::Key(release(Key::N))),
            Response::Idle
        ));
        assert!(matches!(app.on_event(&Event::FocusIn), Response::Idle));
    }

    #[test]
    fn a_new_deal_resets_everything_the_old_one_left_behind() {
        let mut app = game();
        show_a_mismatch(&mut app);
        tap(&mut app, Key::Right);
        let shape = (app.rows(), app.cols());
        app.apply(Action::NewGame);
        assert_eq!(app.moves(), 0);
        assert_eq!(app.pairs_found(), 0);
        assert_eq!(app.cursor(), 0);
        assert_eq!(app.showing_ms(), 0);
        assert!(!app.showing());
        assert_eq!(app.phase(), Phase::FirstPick);
        assert!(app.cards().iter().all(|c| !c.matched));
        assert_eq!(
            (app.rows(), app.cols()),
            shape,
            "a new deal changed the board shape"
        );
    }
}
