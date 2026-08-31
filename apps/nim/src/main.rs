//! Nim — the heap-taking game, against a perfect opponent.
//!
//! Players alternately take any number of tokens from one heap. In *misère*
//! play the player who takes the last token loses; in *normal* play they win.
//!
//! ## What wiring this up found
//!
//! `main` built a `Nim`, dropped it and exited, so no heap ever reached a
//! screen and no key or click ever arrived. Underneath that, five faults:
//!
//! 1. **Every key fired twice**, because the handler destructured
//!    `Event::Key(KeyEvent { key, modifiers, .. })` and never read `pressed`.
//!    Two of the game's controls are *toggles*, and a toggle fired twice is a
//!    control that does nothing at all: **`V` could not change the variant**
//!    — misère versus normal, which is the whole point of the game — and
//!    **`H` could not show the help**, since it opened on the press and closed
//!    on the release. Worse, `Enter` played *two* human moves and drew *two*
//!    computer replies: the press took the aimed-at tokens and let the
//!    computer answer, and the release then took one more token from the same
//!    heap and let it answer again. A player pressing Enter once lost a turn
//!    they never made.
//! 2. **The misère endgame was played backwards** — in the one place where
//!    misère differs from normal play at all. The rule is that when a move
//!    would leave every heap at one token or none, you must leave an *odd*
//!    number of single-token heaps, so that the opponent, forced to move, is
//!    the one who runs out. The adjustment tested that parity inverted: it
//!    played the move that left an *even* number (a loss), and when the
//!    normal-play move already left an odd number (the winning leave) it took
//!    one *more* token to spoil it. So the "perfect AI" advertised in this
//!    file's own doc comment threw every misère endgame it was winning. It is
//!    now the standard rule — and it is tested against a brute-force solver
//!    over every position of every preset, both variants, rather than against
//!    a restatement of itself.
//! 3. **Nothing was clickable.** `MouseButton`, `MouseEvent` and
//!    `MouseEventKind` were imported and never used, hidden by a file-level
//!    `#![allow(unused_imports)]`.
//! 4. **The layout was a constant.** `render(width, height)` used its
//!    arguments for the background rectangle alone. Heaps sat at
//!    `x = 60 + i * 150` with 24px tokens, so the fourth heap of a four-heap
//!    preset was drawn under the score line, and the help panel — a fixed
//!    200x220 box at (500, 100) — was painted directly on top of that heap's
//!    tokens. Everything below y=600 simply did not exist on a shorter
//!    window.
//! 5. **The presets were spelled out twice.** `PRESETS` held the heap sizes
//!    and `PRESET_NAMES` held strings that *also* spelled them
//!    (`"Classic (1,3,5,7)"`), with nothing keeping the two in step. The
//!    label is now built from the heaps themselves, so it cannot drift from
//!    the board it names.
//!
//! Also: the status line could read "Computer thinking…" in no frame that was
//! ever drawn, because the computer's reply ran inside the same event handler
//! as the human's move. The reply is now a state the window renders before it
//! is answered.

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
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);

const COL_SCRIM: Color = Color::rgba(0x1E, 0x1E, 0x2E, 158);
const COL_VEIL: Color = Color::rgba(0x11, 0x11, 0x1B, 214);

/// One colour per heap position, so the eye can tell the columns apart.
const HEAP_COLORS: [Color; 5] = [COL_RED, COL_PEACH, COL_YELLOW, COL_GREEN, COL_TEAL];

/// The boards on offer, as (name, heaps).
///
/// One list, not two. The old code had `PRESETS` holding the heap sizes and a
/// parallel `PRESET_NAMES` holding strings that *also* spelled them out —
/// `"Classic (1,3,5,7)"` beside `[1, 3, 5, 7]` — with nothing to keep them in
/// step. The sizes in the label are now built from the heaps at
/// [`Nim::preset_label`].
const PRESETS: [(&str, &[u32]); 5] = [
    ("Classic", &[1, 3, 5, 7]),
    ("Three", &[3, 4, 5]),
    ("Four", &[2, 3, 4, 5]),
    ("Simple", &[1, 2, 3]),
    ("Large", &[5, 7, 9]),
];
/// One key per entry of [`PRESETS`], in the same order.
const PRESET_KEYS: [Key; 5] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];

const WINDOW_WIDTH: f32 = 780.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// How long the computer's turn is left on screen before it answers.
///
/// The old code ran the reply inside the same event handler as the human's
/// move, so the "Computer thinking…" state it drew a label for could never
/// appear in a frame. A reply that lands in the same instant as the move that
/// provoked it also reads as though the board changed by itself.
const THINK_MS: u64 = 450;

/// A hair under a 60Hz frame; ticks are asked for only while the computer owes
/// a reply.
const TICK_MS: u64 = 16;

const HELP_TITLE: &str = "How to play";
const HELP_ROWS: [(&str, &str); 8] = [
    ("Left / Right", "Choose a heap"),
    ("Up / Down", "Change how many to take"),
    ("Enter / Space", "Take them"),
    ("Click", "Aim at a token; click it again to take"),
    ("1 - 5", "Choose a board"),
    ("V", "Misere or normal play"),
    ("N", "New game"),
    ("H", "Show or hide this sheet"),
];

// ── Layout ─────────────────────────────────────────────────────────────────

/// The share of the window's height the heaps keep no matter what.
const BOARD_SHARE: f32 = 0.5;

/// Which band goes first when they do not all fit: footer, header, info.
///
/// Bands are dropped whole rather than shrunk together, because a band scaled
/// to four pixels costs the heaps four pixels and shows nothing. The status
/// line goes last: whose turn it is and who won is the only chrome you cannot
/// play without.
const BAND_DROP_ORDER: [usize; 3] = [2, 0, 1];

/// Every rectangle in the window, derived from the window's own size.
///
/// Built fresh on every frame and never remembered. A layout stored on the
/// model is a layout that can disagree with the window it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub header: Rect,
    pub info: Rect,
    /// Where the heaps go, all of them.
    pub board: Rect,
    pub footer: Rect,
    pub help: Rect,
    pub font: f32,
    pub small: f32,
    pub pad: f32,
    /// One token's height, shared by every heap so that two heaps of the same
    /// size look the same size.
    pub token_h: f32,
    /// The strip at the top of every column that holds the heap's caption.
    pub caption_h: f32,
}

impl Layout {
    /// The layout for `heaps` columns whose tallest holds `tallest` tokens.
    ///
    /// The token size is an output rather than a constant: a nine-token heap
    /// in a short window needs smaller tokens than a three-token heap in a
    /// tall one, and the old fixed 24px simply ran off the bottom.
    pub fn new(width: f32, height: f32, heaps: usize, tallest: u32) -> Self {
        let w = width.max(1.0);
        let h = height.max(1.0);
        let font = (h / 40.0).clamp(8.0, 16.0);
        let small = (font - 2.0).max(7.0);
        let pad = (w.min(h) * 0.02).clamp(2.0, 10.0);

        // What each band would like, in [header, info, footer] order.
        let mut wants = [
            (h * 0.09).clamp(24.0, 44.0),
            (h * 0.06).clamp(16.0, 28.0),
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
        let board = Rect::new(
            pad,
            top + pad,
            (w - pad * 2.0).max(0.0),
            (bottom - top - pad * 2.0).max(0.0),
        );

        // Room for the heap's caption above its stack.
        let caption = (small * 1.6).min(board.h * 0.25);
        let stack_h = (board.h - caption).max(0.0);
        let column_w = board.w / (heaps.max(1) as f32);
        let token_h = (stack_h / (tallest.max(1) as f32)).min(column_w * 0.42);

        let help_w = (w * 0.9).min(420.0);
        let help_h = (h * 0.9).min(300.0);
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
            token_h,
            caption_h: caption,
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

    /// One preset button.
    pub fn preset_button(&self, index: usize) -> Rect {
        Self::nth_of(self.footer, PRESETS.len(), index)
    }

    /// The three header buttons — variant, new game, help — from the left of
    /// the group to the right of it.
    pub fn header_button(&self, index: usize) -> Rect {
        let group_w = (self.header.w * 0.55).min(270.0);
        let row = Rect::new(
            (self.header.right() - self.pad - group_w).max(self.header.x),
            self.header.y + self.header.h * 0.15,
            group_w,
            (self.header.h * 0.7).max(0.0),
        );
        Self::nth_of(row, 3, index)
    }

    /// The commit button, at the right-hand end of the status line.
    pub fn take_button(&self) -> Rect {
        let bw = (self.info.w * 0.22).min(120.0);
        Rect::new(
            (self.info.right() - self.pad - bw).max(self.info.x),
            self.info.y,
            bw,
            self.info.h.max(0.0),
        )
    }

    /// The column a heap's caption and stack share.
    pub fn column(&self, heaps: usize, index: usize) -> Rect {
        let cw = self.board.w / (heaps.max(1) as f32);
        Rect::new(
            self.board.x + index as f32 * cw,
            self.board.y,
            cw,
            self.board.h,
        )
    }

    /// The caption strip above one heap's stack.
    ///
    /// The same `caption_h` the token size was computed against, so the
    /// tallest heap's top token cannot reach into the words naming it.
    pub fn caption(&self, heaps: usize, index: usize) -> Rect {
        let col = self.column(heaps, index);
        Rect::new(col.x, col.y, col.w, self.caption_h.max(0.0))
    }

    /// One token, counting from the bottom of its heap.
    ///
    /// Bottom-up because a heap loses tokens from the top: a stack that grew
    /// downwards would move every remaining token each time one was taken.
    pub fn token(&self, heaps: usize, index: usize, from_bottom: u32) -> Rect {
        let slot = self.token_slot(heaps, index, from_bottom);
        let gap = (self.token_h * 0.16).min(5.0);
        let tw = (slot.w * 0.62).max(0.0);
        Rect::new(
            slot.x + (slot.w - tw) / 2.0,
            slot.y + gap / 2.0,
            tw,
            (slot.h - gap).max(0.0),
        )
    }

    /// The full-width strip a token is drawn inside.
    ///
    /// The click box, so that the gap between two tokens belongs to the one it
    /// is nearest rather than falling through to the board behind.
    pub fn token_slot(&self, heaps: usize, index: usize, from_bottom: u32) -> Rect {
        let col = self.column(heaps, index);
        Rect::new(
            col.x,
            col.bottom() - (from_bottom as f32 + 1.0) * self.token_h,
            col.w,
            self.token_h.max(0.0),
        )
    }

    pub fn shows_header(&self) -> bool {
        self.header.h >= 14.0 && self.header.w >= 80.0
    }
    pub fn shows_info(&self) -> bool {
        self.info.h >= 10.0 && self.info.w >= 80.0
    }
    pub fn shows_footer(&self) -> bool {
        self.footer.h >= 10.0 && self.footer.w >= 200.0
    }
}

// ── Model ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    Human,
    Computer,
}

impl Player {
    fn other(self) -> Self {
        match self {
            Self::Human => Self::Computer,
            Self::Computer => Self::Human,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Won(Player),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Taking the last token loses.
    Misere,
    /// Taking the last token wins.
    Normal,
}

impl Variant {
    fn other(self) -> Self {
        match self {
            Self::Misere => Self::Normal,
            Self::Normal => Self::Misere,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Misere => "Misere",
            Self::Normal => "Normal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Aim at a heap, keeping the count at one.
    Select(usize),
    /// Aim at a heap *and* a count, as a click on a token does.
    Aim(usize, u32),
    /// Take one more / one fewer token in the pending move.
    More,
    Fewer,
    /// Commit the pending move.
    Take,
    SetPreset(usize),
    ToggleVariant,
    NewGame,
    ToggleHelp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A heap's caption; a click selects it.
    Heap(usize),
    /// One token, as (heap, how many tokens are at or above it).
    ///
    /// The count rather than the position, because that is what a click there
    /// means: take this token and everything on top of it.
    Token(usize, u32),
    Preset(usize),
    Variant,
    NewGame,
    Take,
    Help,
    /// The help sheet itself: it swallows clicks meant for the board behind it.
    HelpSheet,
}

pub type Frame = guitk::frame::Frame<Target>;

// ── Perfect play ───────────────────────────────────────────────────────────

/// The move an optimal player makes from `heaps`, as (heap, tokens).
///
/// `None` only when there is nothing left to take.
///
/// Normal play is the textbook nim-sum rule: leave the XOR of the heap sizes
/// at zero. Misère play is the same *except* when the move would leave every
/// heap at one token or none, and that exception is the entire difference
/// between the two games — which is why the old code getting its parity
/// backwards threw every misère endgame it was winning.
///
/// The misère rule, stated as it is applied here:
///
/// * If every heap already holds at most one token, take one. That leaves one
///   fewer single-token heap, which wins exactly when the number of them was
///   even — and when it was odd every move loses, so any move will do.
/// * If exactly one heap holds two or more, reduce it to nothing or to one,
///   whichever leaves an *odd* number of single-token heaps for the opponent.
/// * Otherwise two heaps still hold two or more, no endgame is in reach this
///   move, and normal play is correct.
#[must_use]
pub fn best_move(heaps: &[u32], variant: Variant) -> Option<(usize, u32)> {
    let ones = heaps.iter().filter(|&&h| h == 1).count();
    let big: Vec<usize> = heaps
        .iter()
        .enumerate()
        .filter(|&(_, &h)| h >= 2)
        .map(|(i, _)| i)
        .collect();

    if variant == Variant::Misere {
        if big.is_empty() {
            // Every heap is 0 or 1: take one from whichever still has it.
            return heaps.iter().position(|&h| h == 1).map(|i| (i, 1));
        }
        if let (1, Some(&i)) = (big.len(), big.first()) {
            let size = heaps.get(i).copied().unwrap_or(0);
            // Leave that heap at 1 when the count of single-token heaps is
            // even, so the opponent faces an odd number of them and loses.
            let leave = u32::from(ones % 2 == 0);
            return Some((i, size.saturating_sub(leave)));
        }
    }

    let nim_sum = heaps.iter().fold(0_u32, |acc, &h| acc ^ h);
    if nim_sum != 0 {
        for (i, &h) in heaps.iter().enumerate() {
            let target = h ^ nim_sum;
            if target < h {
                return Some((i, h.saturating_sub(target)));
            }
        }
    }

    // A lost position: no move is better than any other, so take one from the
    // largest heap and give the opponent the most chances to err.
    heaps
        .iter()
        .enumerate()
        .max_by_key(|&(_, &h)| h)
        .filter(|&(_, &h)| h > 0)
        .map(|(i, _)| (i, 1))
}
// ── Model ──────────────────────────────────────────────────────────────────

/// A game of Nim, and the window it is drawn in.
///
/// The window size lives here because a click is read against the frame the
/// window is actually showing, and that frame is built from this size.
pub struct Nim {
    heaps: Vec<u32>,
    selected_heap: usize,
    take_count: u32,
    current_player: Player,
    state: GameState,
    variant: Variant,
    preset: usize,
    show_help: bool,
    /// [human, computer], across games.
    scores: [u32; 2],
    /// Milliseconds still owed before the computer answers. Zero when no
    /// reply is pending, which is also what "it is the human's move" means.
    think_ms: u64,
    width: f32,
    height: f32,
}

impl Default for Nim {
    fn default() -> Self {
        Self::new()
    }
}

impl Nim {
    #[must_use]
    pub fn new() -> Self {
        let preset = 0;
        Self {
            heaps: Self::heaps_from_preset(preset),
            selected_heap: 0,
            take_count: 1,
            current_player: Player::Human,
            state: GameState::Playing,
            variant: Variant::Misere,
            preset,
            show_help: false,
            scores: [0, 0],
            think_ms: 0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        }
    }

    /// The heaps a preset starts from.
    #[must_use]
    pub fn heaps_from_preset(preset: usize) -> Vec<u32> {
        PRESETS
            .get(preset)
            .map_or_else(Vec::new, |(_, heaps)| heaps.to_vec())
    }

    /// A preset's name with its heaps spelled out — built from the heaps, so
    /// the two cannot disagree the way `PRESETS` and the old `PRESET_NAMES`
    /// could.
    #[must_use]
    pub fn preset_label(index: usize) -> String {
        let Some((name, heaps)) = PRESETS.get(index) else {
            return String::new();
        };
        let sizes: Vec<String> = heaps.iter().map(u32::to_string).collect();
        format!("{name} ({})", sizes.join(","))
    }

    // ── What is true right now ─────────────────────────────────────────────

    #[must_use]
    pub fn heaps(&self) -> &[u32] {
        &self.heaps
    }
    #[must_use]
    pub fn selected_heap(&self) -> usize {
        self.selected_heap
    }
    #[must_use]
    pub fn take_count(&self) -> u32 {
        self.take_count
    }
    #[must_use]
    pub fn current_player(&self) -> Player {
        self.current_player
    }
    #[must_use]
    pub fn state(&self) -> GameState {
        self.state
    }
    #[must_use]
    pub fn variant(&self) -> Variant {
        self.variant
    }
    #[must_use]
    pub fn preset(&self) -> usize {
        self.preset
    }
    #[must_use]
    pub fn show_help(&self) -> bool {
        self.show_help
    }
    #[must_use]
    pub fn scores(&self) -> [u32; 2] {
        self.scores
    }
    #[must_use]
    pub fn think_ms(&self) -> u64 {
        self.think_ms
    }

    /// True while the computer's reply is on screen but not yet played.
    #[must_use]
    pub fn thinking(&self) -> bool {
        self.think_ms > 0
    }

    #[must_use]
    pub fn playing(&self) -> bool {
        self.state == GameState::Playing
    }

    /// True when the human may move: the game is live, it is their turn, and
    /// the computer is not mid-reply.
    #[must_use]
    pub fn human_turn(&self) -> bool {
        self.playing() && self.current_player == Player::Human && !self.thinking()
    }

    #[must_use]
    pub fn total_remaining(&self) -> u32 {
        self.heaps
            .iter()
            .fold(0_u32, |acc, &h| acc.saturating_add(h))
    }

    /// The XOR of the heap sizes — zero exactly in the positions normal play
    /// hands to the opponent as losing.
    #[must_use]
    pub fn nim_sum(&self) -> u32 {
        self.heaps.iter().fold(0_u32, |acc, &h| acc ^ h)
    }

    /// The tallest heap, which is what sets the token size for *every* heap:
    /// two heaps of the same size must look the same size.
    #[must_use]
    pub fn tallest(&self) -> u32 {
        self.heaps.iter().copied().max().unwrap_or(0)
    }

    /// How many tokens the aimed-at heap still holds.
    #[must_use]
    pub fn selected_size(&self) -> u32 {
        self.heaps.get(self.selected_heap).copied().unwrap_or(0)
    }

    /// The next heap in a direction that still holds tokens, or the current
    /// one when there is none.
    ///
    /// Empty heaps are stepped over rather than landed on: a cursor parked on
    /// an empty heap can neither take nor be taken from. It stops at the edge
    /// rather than wrapping, so one key press never moves the eye across the
    /// whole board.
    #[must_use]
    pub fn neighbour(&self, forward: bool) -> usize {
        let n = self.heaps.len();
        let mut i = self.selected_heap;
        for _ in 0..n {
            if forward {
                let next = i.saturating_add(1);
                if next >= n {
                    return self.selected_heap;
                }
                i = next;
            } else {
                if i == 0 {
                    return self.selected_heap;
                }
                i = i.saturating_sub(1);
            }
            if self.heaps.get(i).is_some_and(|&h| h > 0) {
                return i;
            }
        }
        self.selected_heap
    }

    /// The status line, as one string, so the window and a test read the same
    /// words.
    #[must_use]
    pub fn status(&self) -> String {
        match self.state {
            GameState::Won(Player::Human) => "You win".to_string(),
            GameState::Won(Player::Computer) => "Computer wins".to_string(),
            GameState::Playing if self.thinking() => "Computer thinking...".to_string(),
            GameState::Playing => format!(
                "Your turn: take {} from heap {}",
                self.take_count,
                self.selected_heap.saturating_add(1)
            ),
        }
    }

    // ── Changing it ────────────────────────────────────────────────────────

    /// Put the aim somewhere legal after the heaps have changed under it.
    ///
    /// A count of zero is not a move, and a count past the end of a heap is
    /// not one either; both are reachable by taking tokens out from under an
    /// aim that was legal when it was made.
    fn settle_aim(&mut self) {
        if self.heaps.is_empty() {
            self.selected_heap = 0;
            self.take_count = 1;
            return;
        }
        if self.selected_heap >= self.heaps.len() || self.selected_size() == 0 {
            // Prefer a heap that still has something in it; a cursor parked on
            // an empty heap can neither move nor take.
            if let Some(i) = self.heaps.iter().position(|&h| h > 0) {
                self.selected_heap = i;
            } else {
                self.selected_heap = self.selected_heap.min(self.heaps.len().saturating_sub(1));
            }
        }
        self.take_count = self.take_count.clamp(1, self.selected_size().max(1));
    }

    pub fn new_game(&mut self) {
        self.heaps = Self::heaps_from_preset(self.preset);
        self.selected_heap = 0;
        self.take_count = 1;
        self.current_player = Player::Human;
        self.state = GameState::Playing;
        self.think_ms = 0;
    }

    fn set_preset(&mut self, index: usize) -> bool {
        if index >= PRESETS.len() {
            return false;
        }
        self.preset = index;
        self.new_game();
        true
    }

    fn toggle_variant(&mut self) {
        self.variant = self.variant.other();
        self.new_game();
    }

    /// Take `count` from heap `heap` on behalf of whoever is to move.
    ///
    /// The one place the heaps shrink. Returns whether the move was legal.
    pub fn take(&mut self, heap: usize, count: u32) -> bool {
        if self.state != GameState::Playing {
            return false;
        }
        let Some(size) = self.heaps.get_mut(heap) else {
            return false;
        };
        if count == 0 || count > *size {
            return false;
        }
        *size = size.saturating_sub(count);

        if self.total_remaining() == 0 {
            // Misère: whoever took the last token loses. Normal: they win.
            let winner = match self.variant {
                Variant::Misere => self.current_player.other(),
                Variant::Normal => self.current_player,
            };
            self.state = GameState::Won(winner);
            let slot = match winner {
                Player::Human => 0,
                Player::Computer => 1,
            };
            if let Some(score) = self.scores.get_mut(slot) {
                *score = score.saturating_add(1);
            }
        } else {
            self.current_player = self.current_player.other();
        }
        self.settle_aim();
        true
    }

    /// Put the computer's reply on the clock instead of playing it now.
    ///
    /// The old code answered inside the same event handler as the human's
    /// move, so the "Computer thinking..." status it drew a label for appeared
    /// in no frame that was ever shown, and the board changed twice between
    /// one key press and the next frame.
    fn begin_reply(&mut self) {
        if self.playing() && self.current_player == Player::Computer {
            self.think_ms = THINK_MS;
        }
    }

    /// Age a pending reply by `elapsed_ms`, playing it when the clock runs
    /// out. Returns whether anything changed.
    ///
    /// Ageing by the reported interval rather than counting ticks keeps the
    /// pause the same length whatever rate the compositor settles on.
    pub fn advance(&mut self, elapsed_ms: u64) -> bool {
        if !self.thinking() {
            return false;
        }
        self.think_ms = self.think_ms.saturating_sub(elapsed_ms.max(1));
        if self.think_ms > 0 {
            return true;
        }
        if let Some((heap, count)) = best_move(&self.heaps, self.variant) {
            self.take(heap, count);
        }
        true
    }

    /// Whether `action` would change anything, so a control can be drawn dim
    /// and a test can check that dim means what it looks like.
    #[must_use]
    pub fn enabled(&self, action: Action) -> bool {
        match action {
            Action::ToggleHelp | Action::NewGame | Action::ToggleVariant => true,
            Action::SetPreset(index) => index < PRESETS.len(),
            Action::Select(index) => {
                self.human_turn()
                    && index != self.selected_heap
                    && self.heaps.get(index).is_some_and(|&h| h > 0)
            }
            Action::Aim(index, count) => {
                self.human_turn()
                    && count > 0
                    && self.heaps.get(index).is_some_and(|&h| count <= h)
                    && (index != self.selected_heap || count != self.take_count)
            }
            Action::More => self.human_turn() && self.take_count < self.selected_size(),
            Action::Fewer => self.human_turn() && self.take_count > 1,
            Action::Take => {
                self.human_turn() && self.take_count > 0 && self.take_count <= self.selected_size()
            }
        }
    }

    /// The one place any input changes the game.
    ///
    /// Returns whether anything changed, which is what the window is told when
    /// it asks whether to redraw.
    pub fn apply(&mut self, action: Action) -> bool {
        if !self.enabled(action) {
            return false;
        }
        match action {
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                true
            }
            Action::NewGame => {
                self.new_game();
                true
            }
            Action::ToggleVariant => {
                self.toggle_variant();
                true
            }
            Action::SetPreset(index) => self.set_preset(index),
            Action::Select(index) => {
                self.selected_heap = index;
                self.take_count = 1;
                true
            }
            Action::Aim(index, count) => {
                self.selected_heap = index;
                self.take_count = count;
                true
            }
            Action::More => {
                self.take_count = self.take_count.saturating_add(1);
                true
            }
            Action::Fewer => {
                self.take_count = self.take_count.saturating_sub(1).max(1);
                true
            }
            Action::Take => {
                let (heap, count) = (self.selected_heap, self.take_count);
                if !self.take(heap, count) {
                    return false;
                }
                // One move, and then the computer's answer on a clock — not a
                // second move from the same key.
                self.begin_reply();
                true
            }
        }
    }
}
// ── Window ─────────────────────────────────────────────────────────────────

impl Nim {
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
/// token's slot — and so into its hit box.
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

impl Nim {
    /// The layout of this board in a window of the given size.
    ///
    /// The board goes in because the heaps set the shape: three heaps of nine
    /// tokens and four heaps of three want different rectangles out of the
    /// same window, and the old fixed 24px token simply ran off the bottom.
    #[must_use]
    pub fn layout(&self, width: f32, height: f32) -> Layout {
        Layout::new(width, height, self.heaps.len(), self.tallest())
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
        if let GameState::Won(winner) = self.state {
            self.draw_banner(&mut f, &l, winner);
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
            "Nim",
            l.font,
            COL_LAVENDER,
            FontWeightHint::Bold,
            Some(title_span),
        );

        // Variant, New, Help — left to right, in the order a player reaches
        // for them.
        let buttons = [
            (self.variant.label(), COL_TEAL, Target::Variant, false),
            ("New", COL_BLUE, Target::NewGame, false),
            ("?", COL_TEXT, Target::Help, self.show_help),
        ];
        for (i, (body, colour, target, lit)) in buttons.into_iter().enumerate() {
            let r = l.header_button(i);
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            fill(
                f,
                r,
                if lit { COL_SURFACE1 } else { COL_SURFACE0 },
                (r.h * 0.25).min(6.0),
            );
            centred_in(
                f,
                r.x,
                r.w,
                r.y + r.h / 2.0,
                body,
                l.small,
                colour,
                FontWeightHint::Bold,
            );
            f.hit(target, r);
        }
    }

    fn draw_info(&self, f: &mut Frame, l: &Layout) {
        let cy = l.info.y + l.info.h / 2.0;
        let btn = l.take_button();
        let left = l.info.x + l.pad;
        let body = format!(
            "{}   You {} - CPU {}",
            self.status(),
            self.scores.first().copied().unwrap_or(0),
            self.scores.get(1).copied().unwrap_or(0),
        );
        let colour = match self.state {
            GameState::Won(Player::Human) => COL_GREEN,
            GameState::Won(Player::Computer) => COL_RED,
            GameState::Playing if self.thinking() => COL_YELLOW,
            GameState::Playing => COL_SUBTEXT,
        };
        label(
            f,
            left,
            cy - text::line_height(l.small, FontWeightHint::Regular) / 2.0,
            &body,
            l.small,
            colour,
            FontWeightHint::Regular,
            Some((btn.x - l.pad - left).max(0.0)),
        );

        if btn.w > 0.0 && btn.h > 0.0 {
            let live = self.enabled(Action::Take);
            fill(
                f,
                btn,
                if live { COL_SURFACE1 } else { COL_SURFACE0 },
                (btn.h * 0.25).min(6.0),
            );
            centred_in(
                f,
                btn.x,
                btn.w,
                btn.y + btn.h / 2.0,
                "Take",
                l.small,
                if live { COL_GREEN } else { COL_OVERLAY },
                FontWeightHint::Bold,
            );
            // Recorded even when refused, so a click there stops at the button
            // rather than falling through to a heap behind it.
            f.hit(Target::Take, btn);
        }
    }

    fn draw_board(&self, f: &mut Frame, l: &Layout) {
        if l.board.w <= 0.0 || l.board.h <= 0.0 || l.token_h <= 0.0 {
            return;
        }
        let n = self.heaps.len();
        let radius = (l.token_h * 0.28).min(8.0);
        for (i, &size) in self.heaps.iter().enumerate() {
            let colour = HEAP_COLORS
                .get(i.checked_rem(HEAP_COLORS.len()).unwrap_or(0))
                .copied()
                .unwrap_or(COL_TEXT);
            let aimed = i == self.selected_heap;
            // The top `take_count` tokens of the aimed-at heap are the pending
            // move: what "Take" would remove, shown before it is committed.
            let marked_from = size.saturating_sub(self.take_count);

            let caption = l.caption(n, i);
            if caption.h > 0.0 {
                centred_in(
                    f,
                    caption.x,
                    caption.w,
                    caption.y + caption.h / 2.0,
                    &size.to_string(),
                    l.small,
                    if aimed { COL_TEXT } else { COL_OVERLAY },
                    if aimed {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                );
                f.hit(Target::Heap(i), caption);
            }

            for j in 0..size {
                let slot = l.token_slot(n, i, j);
                let token = l.token(n, i, j);
                let taking = aimed && self.human_turn() && j >= marked_from;
                fill(f, token, if taking { colour } else { COL_SURFACE1 }, radius);
                if taking {
                    ring(f, token, (l.token_h * 0.1).max(1.0), COL_YELLOW);
                } else {
                    // A dot of the heap's colour: the columns must still be
                    // tellable apart when nothing in them is aimed at.
                    let dot = (token.w.min(token.h) * 0.34).min(10.0);
                    fill(
                        f,
                        Rect::new(
                            token.x + (token.w - dot) / 2.0,
                            token.y + (token.h - dot) / 2.0,
                            dot,
                            dot,
                        ),
                        colour,
                        dot / 2.0,
                    );
                }
                // Clicking a token aims at everything from it upwards, which is
                // exactly the move taking it would make.
                f.hit(Target::Token(i, size.saturating_sub(j)), slot);
            }
        }
    }

    fn draw_banner(&self, f: &mut Frame, l: &Layout, winner: Player) {
        let h = (l.board.h * 0.22).clamp(0.0, 44.0);
        let w = (l.board.w * 0.8).min(360.0);
        if h <= 0.0 || w <= 0.0 {
            return;
        }
        let r = Rect::new(
            l.board.x + (l.board.w - w) / 2.0,
            l.board.y + (l.board.h - h) / 2.0,
            w,
            h,
        );
        fill(f, r, COL_CRUST, (r.h * 0.2).min(10.0));
        let body = match winner {
            Player::Human => "You win - click for a new game",
            Player::Computer => "Computer wins - click for a new game",
        };
        centred_in(
            f,
            r.x,
            r.w,
            r.y + r.h / 2.0,
            body,
            l.small,
            match winner {
                Player::Human => COL_GREEN,
                Player::Computer => COL_RED,
            },
            FontWeightHint::Bold,
        );
        // The banner is the new-game button while it is up: a player who has
        // just finished a game is already looking here.
        f.hit(Target::NewGame, r);
    }

    fn draw_footer(&self, f: &mut Frame, l: &Layout) {
        for i in 0..PRESETS.len() {
            let r = l.preset_button(i);
            let current = i == self.preset;
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
                &Nim::preset_label(i),
                l.small,
                if current { COL_YELLOW } else { COL_TEXT },
                FontWeightHint::Bold,
            );
            f.hit(Target::Preset(i), r);
        }
    }
}

/// The help sheet the old code toggled a flag for and drew as a fixed 200x220
/// box at (500, 100) — on top of the fourth heap's tokens.
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

impl Nim {
    fn handle_key(&mut self, ev: &KeyEvent) -> EventResult {
        // The fault that broke every key in this file, in one line. A release
        // is not a second press. Acting on both made `V` and `H` — both
        // toggles — do nothing at all, and made one press of Enter play two
        // human moves and draw two computer replies.
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
            Key::Left => Some(Action::Select(self.neighbour(false))),
            Key::Right => Some(Action::Select(self.neighbour(true))),
            Key::Up => Some(Action::More),
            Key::Down => Some(Action::Fewer),
            Key::Enter | Key::Space => Some(Action::Take),
            Key::N => Some(Action::NewGame),
            Key::V => Some(Action::ToggleVariant),
            Key::H => Some(Action::ToggleHelp),
            key => PRESET_KEYS
                .iter()
                .position(|k| *k == key)
                .map(Action::SetPreset),
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
            // A token already aimed at is the commit: click it once to aim,
            // again to take. Anything else only moves the aim, so a misclick
            // costs a look rather than a turn.
            Target::Token(heap, count) => {
                if self.selected_heap == heap && self.take_count == count {
                    self.apply(Action::Take);
                } else {
                    self.apply(Action::Aim(heap, count));
                }
            }
            Target::Heap(heap) => {
                self.apply(Action::Select(heap));
            }
            Target::Preset(index) => {
                self.apply(Action::SetPreset(index));
            }
            Target::Variant => {
                self.apply(Action::ToggleVariant);
            }
            Target::NewGame => {
                self.apply(Action::NewGame);
            }
            Target::Take => {
                self.apply(Action::Take);
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
pub fn handle_event(app: &mut Nim, event: &Event) -> EventResult {
    match event {
        Event::Key(ev) => app.handle_key(ev),
        Event::Mouse(ev) => app.handle_mouse(ev),
        Event::Resize { width, height } => {
            app.resize(*width as f32, *height as f32);
            EventResult::Consumed
        }
        // The clock the computer's reply never had. Ageing by the reported
        // interval rather than by counting ticks keeps the pause the same
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

impl App for Nim {
    fn title(&self) -> String {
        "Nim".to_string()
    }

    fn app_id(&self) -> String {
        "nim".to_string()
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// Ticks are asked for only while the computer owes a reply.
    ///
    /// A board sitting still needs no frames, and a game that asks for 60 a
    /// second regardless is a game that keeps a laptop awake to draw the same
    /// pixels.
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

impl Probe for Nim {
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
    let mut game = Nim::new();
    app::launch("nim", &mut game)
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
    use std::collections::HashMap;

    /// Windows the board has to survive, from a desktop down to a postage
    /// stamp. Every layout test walks all of them.
    const WINDOWS: [(f32, f32); 9] = [
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (780.0, 620.0),
        (640.0, 480.0),
        (480.0, 320.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (80.0, 60.0),
        (24.0, 24.0),
    ];

    fn game() -> Nim {
        Nim::new()
    }

    fn windowed(width: f32, height: f32) -> Nim {
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
    fn press(app: &mut Nim, key: Key) -> EventResult {
        handle_event(app, &Event::Key(probe::press(key)))
    }

    fn lift(app: &mut Nim, key: Key) -> EventResult {
        handle_event(app, &Event::Key(release(key)))
    }

    /// A whole keystroke: down and up, as a window delivers it.
    fn tap(app: &mut Nim, key: Key) {
        press(app, key);
        lift(app, key);
    }

    fn click(app: &mut Nim, x: f32, y: f32) -> EventResult {
        handle_event(
            app,
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        )
    }

    fn tick(app: &mut Nim, elapsed_ms: u64) -> EventResult {
        handle_event(app, &Event::Tick { elapsed_ms })
    }

    /// Run the clock until the computer has answered.
    fn settle(app: &mut Nim) {
        for _ in 0..1000 {
            if !app.thinking() {
                return;
            }
            tick(app, TICK_MS);
        }
        panic!("the computer never finished its reply");
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

    // ── The faults ─────────────────────────────────────────────────────────

    #[test]
    fn a_release_is_not_a_second_press() {
        // The handler destructured `Event::Key(KeyEvent { key, modifiers, .. })`
        // and never read `pressed`, so every key in this file fired twice.
        let mut app = game();
        assert_eq!(app.selected_heap(), 0);
        press(&mut app, Key::Right);
        assert_eq!(app.selected_heap(), 1, "the press did not move the aim");
        lift(&mut app, Key::Right);
        assert_eq!(
            app.selected_heap(),
            1,
            "the release moved the aim a second time"
        );

        let before = app.take_count();
        press(&mut app, Key::Up);
        assert_eq!(app.take_count(), before + 1);
        lift(&mut app, Key::Up);
        assert_eq!(app.take_count(), before + 1, "the release took one more");
    }

    #[test]
    fn a_release_is_ignored_rather_than_consumed() {
        // Ignored, not consumed: a window that reported every release as
        // handled would ask for a redraw of a frame nothing changed in.
        let mut app = game();
        assert_eq!(press(&mut app, Key::Right), EventResult::Consumed);
        assert_eq!(lift(&mut app, Key::Right), EventResult::Ignored);
    }

    #[test]
    fn v_can_actually_change_the_variant() {
        // Misère versus normal is the whole point of the game, and `V` was the
        // only way to choose: a toggle fired on both edges of one keystroke is
        // a control that does nothing at all.
        let mut app = game();
        assert_eq!(app.variant(), Variant::Misere);
        tap(&mut app, Key::V);
        assert_eq!(app.variant(), Variant::Normal, "V did not change anything");
        tap(&mut app, Key::V);
        assert_eq!(app.variant(), Variant::Misere);
    }

    #[test]
    fn the_help_sheet_can_actually_be_opened() {
        // `H` opened the sheet on the press and closed it on the release, so
        // the help existed only for the length of a keystroke.
        let mut app = game();
        assert!(!app.show_help());
        tap(&mut app, Key::H);
        assert!(app.show_help(), "H did not open the sheet");

        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let drawn = texts(&f);
        assert!(drawn.iter().any(|t| t == HELP_TITLE));
        for (k, v) in HELP_ROWS {
            assert!(drawn.iter().any(|t| t == k), "the sheet omits {k:?}");
            assert!(drawn.iter().any(|t| t == v), "the sheet omits {v:?}");
        }

        tap(&mut app, Key::H);
        assert!(!app.show_help());
    }

    #[test]
    fn the_help_sheet_swallows_what_is_behind_it() {
        // A click that reached the board through the sheet would take tokens
        // the player cannot see.
        let aim = game();
        let spot = probe::rect_of(&aim, Target::Token(3, 1)).expect("heap 3 has tokens");
        let (x, y) = spot.centre();

        let mut app = windowed(WINDOW_WIDTH, WINDOW_HEIGHT);
        app.apply(Action::ToggleHelp);
        assert_eq!(app.target_at(x, y), Some(Target::HelpSheet));
        let before = app.heaps().to_vec();
        click(&mut app, x, y);
        assert_eq!(app.heaps(), &before[..], "the click reached the board");
        assert!(!app.show_help(), "the click did not dismiss the sheet");
    }

    #[test]
    fn enter_plays_one_move_not_two() {
        // The press took the aimed-at tokens and let the computer answer; the
        // release then took one more from the same heap and let it answer
        // again. A player pressing Enter once lost a turn they never made.
        let mut app = game();
        tap(&mut app, Key::Right);
        assert_eq!(app.selected_heap(), 1);
        tap(&mut app, Key::Enter);
        assert_eq!(
            app.heaps(),
            &[1, 2, 5, 7],
            "one press of Enter played more than one move"
        );
        assert_eq!(app.current_player(), Player::Computer);
        assert!(app.thinking(), "the computer's reply was played instantly");
    }

    #[test]
    fn the_computer_answers_on_a_clock_rather_than_inside_the_keystroke() {
        // "Computer thinking…" appeared in no frame that was ever drawn,
        // because the reply ran in the same event handler as the human's move.
        let mut app = game();
        tap(&mut app, Key::Enter);
        let after_human = app.heaps().to_vec();
        assert_eq!(after_human, vec![0, 3, 5, 7]);
        assert!(app.status().contains("thinking"), "{}", app.status());
        let drawn = texts(&app.frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(
            drawn.iter().any(|t| t.contains("thinking")),
            "the status the window draws does not say the computer is thinking"
        );

        tick(&mut app, THINK_MS - 1);
        assert_eq!(app.heaps(), &after_human[..], "the reply came early");
        assert!(app.thinking());

        tick(&mut app, 1);
        assert!(!app.thinking(), "the clock ran out without a reply");
        assert_ne!(app.heaps(), &after_human[..], "the computer did not move");
        assert_eq!(app.current_player(), Player::Human);
    }

    #[test]
    fn one_reply_is_played_however_the_ticks_are_chopped_up() {
        // Ageing by the reported interval rather than counting ticks keeps the
        // pause the same length whatever rate the compositor settles on.
        let mut coarse = game();
        tap(&mut coarse, Key::Enter);
        tick(&mut coarse, THINK_MS * 4);

        let mut fine = game();
        tap(&mut fine, Key::Enter);
        settle(&mut fine);

        assert_eq!(coarse.heaps(), fine.heaps());
        assert_eq!(coarse.current_player(), Player::Human);
    }

    #[test]
    fn ticks_are_asked_for_only_while_a_reply_is_owed() {
        let mut app = game();
        assert!(app.tick_interval().is_none(), "an idle board wants frames");
        tap(&mut app, Key::Enter);
        assert_eq!(app.tick_interval(), Some(Duration::from_millis(TICK_MS)));
        settle(&mut app);
        assert!(app.tick_interval().is_none());
        assert_eq!(tick(&mut app, TICK_MS), EventResult::Ignored);
    }

    #[test]
    fn the_board_does_not_move_while_the_computer_is_thinking() {
        // The human's second move must not race the reply to their first.
        let mut app = game();
        tap(&mut app, Key::Enter);
        let mid = app.heaps().to_vec();
        let aim = app.selected_heap();
        tap(&mut app, Key::Right);
        tap(&mut app, Key::Up);
        tap(&mut app, Key::Enter);
        assert_eq!(app.heaps(), &mid[..], "the human moved out of turn");
        assert_eq!(app.selected_heap(), aim, "the aim moved out of turn");
        settle(&mut app);
        assert_eq!(app.current_player(), Player::Human);
    }

    #[test]
    fn nothing_was_clickable_and_now_everything_is() {
        // `MouseButton`, `MouseEvent` and `MouseEventKind` were imported and
        // never used, hidden by a file-level `#![allow(unused_imports)]`.
        let mut app = game();

        probe::click(&mut app, Target::Token(2, 3));
        assert_eq!(app.selected_heap(), 2);
        assert_eq!(app.take_count(), 3, "clicking a token did not aim at it");

        // The same token again is the commit: click to aim, click to take.
        probe::click(&mut app, Target::Token(2, 3));
        assert_eq!(app.heaps()[2], 2, "clicking the aimed-at token did nothing");
        assert!(app.thinking());
        settle(&mut app);

        probe::click(&mut app, Target::Preset(3));
        assert_eq!(app.preset(), 3);
        assert_eq!(app.heaps(), &[1, 2, 3]);

        probe::click(&mut app, Target::Variant);
        assert_eq!(app.variant(), Variant::Normal);

        probe::click(&mut app, Target::Help);
        assert!(app.show_help());
        probe::click(&mut app, Target::Help);
        assert!(!app.show_help());
    }

    #[test]
    fn the_take_button_takes_what_the_board_says_it_will() {
        let mut app = game();
        probe::click(&mut app, Target::Heap(3));
        assert_eq!(app.selected_heap(), 3);
        press(&mut app, Key::Up);
        press(&mut app, Key::Up);
        assert_eq!(app.take_count(), 3);
        probe::click(&mut app, Target::Take);
        assert_eq!(app.heaps(), &[1, 3, 5, 4]);
    }

    #[test]
    fn a_refused_click_stops_at_the_control_it_landed_on() {
        // Consumed either way: a click on a control the game is refusing must
        // not fall through to the board behind it.
        let mut app = game();
        tap(&mut app, Key::Enter);
        let before = app.heaps().to_vec();
        assert_eq!(probe::click(&mut app, Target::Take), EventResult::Consumed);
        assert_eq!(app.heaps(), &before[..]);
    }

    #[test]
    fn the_banner_is_the_new_game_button_once_a_game_is_over() {
        let mut app = game();
        app.apply(Action::SetPreset(3));
        // [1, 2, 3] — take the lot and lose the misère game outright.
        for _ in 0..3 {
            while app.enabled(Action::More) {
                app.apply(Action::More);
            }
            app.apply(Action::Take);
            settle(&mut app);
        }
        assert!(
            matches!(app.state(), GameState::Won(_)),
            "{:?}",
            app.heaps()
        );
        assert!(probe::is_visible(&app, Target::NewGame));
        probe::click(&mut app, Target::NewGame);
        assert_eq!(app.state(), GameState::Playing);
        assert_eq!(app.heaps(), &[1, 2, 3]);
    }

    #[test]
    fn a_finished_game_keeps_the_score() {
        let mut app = game();
        app.apply(Action::SetPreset(3));
        app.apply(Action::ToggleVariant);
        assert_eq!(app.scores(), [0, 0]);
        for _ in 0..3 {
            while app.enabled(Action::More) {
                app.apply(Action::More);
            }
            app.apply(Action::Take);
            settle(&mut app);
        }
        let GameState::Won(winner) = app.state() else {
            panic!("the game did not end: {:?}", app.heaps());
        };
        let scores = app.scores();
        assert_eq!(scores[0] + scores[1], 1);
        match winner {
            Player::Human => assert_eq!(scores, [1, 0]),
            Player::Computer => assert_eq!(scores, [0, 1]),
        }
        // A new game keeps the score and resets the board.
        app.apply(Action::NewGame);
        assert_eq!(app.scores(), scores);
        assert_eq!(app.heaps(), &[1, 2, 3]);
    }

    // ── Layout ─────────────────────────────────────────────────────────────

    /// Every board this program can be asked to draw: each preset, in each
    /// variant.
    fn boards() -> Vec<Nim> {
        let mut out = Vec::new();
        for preset in 0..PRESETS.len() {
            for variant in [Variant::Misere, Variant::Normal] {
                let mut app = game();
                app.apply(Action::SetPreset(preset));
                if app.variant() != variant {
                    app.apply(Action::ToggleVariant);
                }
                out.push(app);
            }
        }
        out
    }

    #[test]
    fn every_state_draws_a_balanced_frame_at_every_size() {
        for (w, h) in WINDOWS {
            for mut app in boards() {
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} board");
                app.apply(Action::Take);
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} thinking");
                settle(&mut app);
                assert!(app.frame(w, h).is_balanced(), "{w}x{h} answered");
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
    fn no_band_is_laid_past_the_edge_of_the_window() {
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                for (name, r) in [
                    ("header", l.header),
                    ("info", l.info),
                    ("board", l.board),
                    ("footer", l.footer),
                    ("help", l.help),
                ] {
                    assert!(r.x >= -0.01, "{w}x{h}: {name} off the left");
                    assert!(r.y >= -0.01, "{w}x{h}: {name} off the top");
                    assert!(r.right() <= w + 0.01, "{w}x{h}: {name} off the right");
                    assert!(r.bottom() <= h + 0.01, "{w}x{h}: {name} off the bottom");
                }
            }
        }
    }

    #[test]
    fn every_token_is_drawn_inside_the_board_it_belongs_to() {
        // Heaps sat at `x = 60 + i * 150` with 24px tokens, so the fourth heap
        // of a four-heap preset was drawn under the score line and everything
        // below y=600 simply did not exist on a shorter window.
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                let n = app.heaps().len();
                for (i, &size) in app.heaps().iter().enumerate() {
                    for j in 0..size {
                        let r = l.token(n, i, j);
                        assert!(
                            r.x >= l.board.x - 0.01
                                && r.right() <= l.board.right() + 0.01
                                && r.y >= l.board.y - 0.01
                                && r.bottom() <= l.board.bottom() + 0.01,
                            "{w}x{h} heap {i} token {j} at {r:?} is outside {:?}",
                            l.board
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_heaps_tallest_token_never_reaches_into_its_caption() {
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                let n = app.heaps().len();
                for (i, &size) in app.heaps().iter().enumerate() {
                    if size == 0 {
                        continue;
                    }
                    let top = l.token_slot(n, i, size - 1);
                    let caption = l.caption(n, i);
                    assert!(
                        top.y >= caption.bottom() - 0.01,
                        "{w}x{h} heap {i}: token top {top:?} overlaps caption {caption:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn two_heaps_of_the_same_height_are_drawn_the_same_height() {
        // One token size for the whole board, set by the tallest heap: a heap
        // whose tokens were sized to its own column would make three heaps of
        // nine look like three heaps of three.
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                let n = app.heaps().len();
                let first = l.token(n, 0, 0);
                for i in 0..n {
                    let r = l.token(n, i, 0);
                    assert!(
                        (r.h - first.h).abs() <= 0.01 && (r.w - first.w).abs() <= 0.01,
                        "{w}x{h} heap {i}: {r:?} against {first:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_columns_tile_the_board_without_overlapping() {
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                let n = app.heaps().len();
                for i in 1..n {
                    let prev = l.column(n, i - 1);
                    let this = l.column(n, i);
                    assert!(
                        this.x >= prev.right() - 0.01,
                        "{w}x{h} columns {} and {i} overlap",
                        i - 1
                    );
                }
                let last = l.column(n, n - 1);
                assert!(
                    last.right() <= l.board.right() + 0.01,
                    "{w}x{h}: the last column runs off the board"
                );
            }
        }
    }

    #[test]
    fn the_buttons_in_a_row_do_not_overlap_each_other() {
        for (w, h) in WINDOWS {
            let l = Layout::new(w, h, 4, 7);
            for i in 1..PRESETS.len() {
                assert!(
                    l.preset_button(i).x >= l.preset_button(i - 1).right() - 0.01,
                    "{w}x{h}: preset buttons {} and {i} overlap",
                    i - 1
                );
            }
            for i in 1..3 {
                assert!(
                    l.header_button(i).x >= l.header_button(i - 1).right() - 0.01,
                    "{w}x{h}: header buttons {} and {i} overlap",
                    i - 1
                );
            }
            assert!(
                l.take_button().right() <= l.info.right() + 0.01,
                "{w}x{h}: the Take button runs off the status line"
            );
        }
    }

    #[test]
    fn a_window_too_short_for_the_chrome_drops_it_rather_than_the_board() {
        // Bands are dropped whole rather than squeezed: a footer scaled to
        // four pixels costs the heaps four pixels and shows nothing.
        let tall = Layout::new(780.0, 620.0, 4, 7);
        assert!(tall.shows_header() && tall.shows_info() && tall.shows_footer());
        let squat = Layout::new(780.0, 90.0, 4, 7);
        assert!(!squat.shows_footer(), "the footer survived a 90px window");
        assert!(squat.board.h > 0.0, "the board was squeezed to nothing");
        assert!(squat.board.h >= 90.0 * BOARD_SHARE - 2.0 * squat.pad - 0.01);
        // The status line is the last thing to go: whose turn it is and who
        // won is the only chrome the game cannot be played without.
        let sliver = Layout::new(780.0, 40.0, 4, 7);
        assert!(!sliver.shows_header() && !sliver.shows_footer());
    }

    #[test]
    fn the_board_keeps_a_share_of_every_window() {
        for (w, h) in WINDOWS {
            for app in boards() {
                let l = app.layout(w, h);
                assert!(l.board.w > 0.0 && l.board.h > 0.0, "{w}x{h}");
                assert!(l.token_h > 0.0, "{w}x{h}: tokens shrank to nothing");
            }
        }
    }

    #[test]
    fn a_board_the_size_of_a_postage_stamp_still_draws_every_token() {
        for app in boards() {
            let n = app.heaps().len();
            let f = app.frame(24.0, 24.0);
            // A hit box is not a token (lesson 81): the box is the whole slot
            // and the token is that slot inset, painted by a separate
            // statement, so `is_some()` alone would still say yes with the
            // fill deleted. Ask for a fill that fits *inside* the box —
            // containment this way round (lesson 83) cannot be satisfied by
            // the window's own background, which covers every point.
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
            for (i, &size) in app.heaps().iter().enumerate() {
                for j in 0..size {
                    let box_ = f
                        .rect_of(|t| *t == Target::Token(i, size - j))
                        .unwrap_or_else(|| panic!("heap {i} token {j} of {n} vanished at 24x24"));
                    assert!(
                        fills.iter().any(|r| {
                            r.w > 0.0
                                && r.h > 0.0
                                && r.x >= box_.x - 0.01
                                && r.y >= box_.y - 0.01
                                && r.right() <= box_.right() + 0.01
                                && r.bottom() <= box_.bottom() + 0.01
                        }),
                        "heap {i} token {j} of {n} has a hit box at 24x24 \
                         but nothing was painted in it"
                    );
                }
            }
        }
    }

    #[test]
    fn every_control_can_be_clicked_where_it_was_drawn() {
        // The check the old fixed layout could never pass: the help panel was
        // painted over the fourth heap's tokens, so a click on either reached
        // whichever the coordinates happened to favour.
        for (w, h) in WINDOWS {
            for app in boards() {
                let f = app.frame(w, h);
                let n = app.heaps().len();
                let mut expected: Vec<Target> = vec![Target::Take, Target::Variant];
                expected.extend((0..n).map(Target::Heap));
                for (i, &size) in app.heaps().iter().enumerate() {
                    expected.extend((1..=size).map(|c| Target::Token(i, c)));
                }
                for target in expected {
                    let Some(r) = f.rect_of(|t| *t == target) else {
                        // A band the window is too short for draws nothing,
                        // which is the point of dropping it whole.
                        continue;
                    };
                    let (cx, cy) = r.centre();
                    assert_eq!(
                        f.hit_test(cx, cy),
                        Some(target),
                        "{w}x{h}: {target:?} was drawn at {r:?} but something else is on top"
                    );
                }
            }
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
    fn the_banner_stays_inside_the_board_it_covers() {
        for (w, h) in WINDOWS {
            let mut app = game();
            app.apply(Action::SetPreset(3));
            for _ in 0..3 {
                while app.enabled(Action::More) {
                    app.apply(Action::More);
                }
                app.apply(Action::Take);
                settle(&mut app);
            }
            assert!(matches!(app.state(), GameState::Won(_)));
            let l = app.layout(w, h);
            let f = app.frame(w, h);
            // The topmost `NewGame` box is the banner where there is room for
            // one and the header's New button where there is not. Either way
            // it is on screen, and inside the board when it is the banner.
            let Some(r) = f.rect_of(|t| *t == Target::NewGame) else {
                continue;
            };
            assert!(
                r.x >= -0.01 && r.y >= -0.01 && r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                "{w}x{h}: the banner at {r:?} is outside the window"
            );
            if r.y >= l.board.y - 0.01 {
                assert!(
                    r.right() <= l.board.right() + 0.01 && r.bottom() <= l.board.bottom() + 0.01,
                    "{w}x{h}: the banner at {r:?} runs off {:?}",
                    l.board
                );
            }
        }
    }

    #[test]
    fn a_click_is_read_against_the_size_the_frame_was_drawn_at() {
        // `render` stores the size it was handed, so the next click is hit
        // tested against the frame the window is actually showing rather than
        // against the one it opened with.
        let mut app = game();
        let small = (400.0, 300.0);
        app.render(small.0, small.1);
        assert_eq!(app.width(), small.0);
        assert_eq!(app.height(), small.1);
        let r = probe::rect_of_sized(&app, Target::Token(3, 2), small).expect("heap 3 is drawn");
        let (cx, cy) = r.centre();
        click(&mut app, cx, cy);
        assert_eq!(app.selected_heap(), 3);
        assert_eq!(app.take_count(), 2);
    }

    #[test]
    fn a_resize_event_moves_the_hit_boxes_with_the_window() {
        let mut app = game();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1200,
                height: 900,
            },
        );
        assert_eq!(app.width(), 1200.0);
        assert_eq!(app.height(), 900.0);
        let wide = probe::rect_of_sized(&app, Target::Take, (1200.0, 900.0)).expect("drawn");
        let narrow = probe::rect_of_sized(&app, Target::Take, (400.0, 300.0)).expect("drawn");
        assert!(
            wide.x > narrow.x,
            "the Take button did not move with the window"
        );
    }

    // ── The rules ──────────────────────────────────────────────────────────

    /// Every position reachable from `start` — each heap anywhere from empty
    /// to its starting size.
    fn positions(start: &[u32]) -> Vec<Vec<u32>> {
        let mut out = vec![Vec::new()];
        for &size in start {
            let mut next = Vec::new();
            for prefix in &out {
                for h in 0..=size {
                    let mut p = prefix.clone();
                    p.push(h);
                    next.push(p);
                }
            }
            out = next;
        }
        out
    }

    /// Whether the player to move wins with perfect play, by brute force.
    ///
    /// The point of solving it a second way: the old misère adjustment was
    /// tested against a restatement of itself, which is how it shipped with
    /// its parity inverted. This solver knows only the rule of the game — take
    /// from one heap, and taking the last token wins or loses by variant — so
    /// it cannot agree with `best_move` by sharing its mistake.
    fn winning(
        heaps: &[u32],
        variant: Variant,
        memo: &mut HashMap<(Vec<u32>, bool), bool>,
    ) -> bool {
        if heaps.iter().all(|&h| h == 0) {
            // Nothing to take: the other player has just taken the last token,
            // which misère makes a loss for them and normal play a win.
            return variant == Variant::Misere;
        }
        let key = (heaps.to_vec(), variant == Variant::Misere);
        if let Some(&known) = memo.get(&key) {
            return known;
        }
        let mut win = false;
        'search: for i in 0..heaps.len() {
            for take in 1..=heaps[i] {
                let mut next = heaps.to_vec();
                next[i] -= take;
                if !winning(&next, variant, memo) {
                    win = true;
                    break 'search;
                }
            }
        }
        memo.insert(key, win);
        win
    }

    #[test]
    fn the_computer_wins_from_every_winning_position_of_every_preset() {
        // The fault this pins: the misère adjustment tested that the parity
        // was inverted. It played the move that left an *even* number of
        // single-token heaps — a loss — and when the normal-play move already
        // left an odd number it took one *more* token to spoil it. The
        // "perfect AI" threw every misère endgame it was winning.
        let mut memo = HashMap::new();
        for variant in [Variant::Misere, Variant::Normal] {
            for preset in 0..PRESETS.len() {
                let start = Nim::heaps_from_preset(preset);
                for pos in positions(&start) {
                    let total: u32 = pos.iter().sum();
                    let Some((heap, count)) = best_move(&pos, variant) else {
                        assert_eq!(total, 0, "{variant:?} {pos:?}: no move offered");
                        continue;
                    };
                    assert!(
                        heap < pos.len() && count >= 1 && count <= pos[heap],
                        "{variant:?} {pos:?}: ({heap}, {count}) is not a legal move"
                    );
                    if !winning(&pos, variant, &mut memo) {
                        // Every move loses; any legal one will do.
                        continue;
                    }
                    let mut next = pos.clone();
                    next[heap] -= count;
                    assert!(
                        !winning(&next, variant, &mut memo),
                        "{variant:?}: from {pos:?} it played ({heap}, {count}) \
                         and handed {next:?} back as a won position"
                    );
                }
            }
        }
    }

    #[test]
    fn an_empty_board_offers_no_move() {
        for variant in [Variant::Misere, Variant::Normal] {
            assert_eq!(best_move(&[], variant), None);
            assert_eq!(best_move(&[0, 0, 0], variant), None);
        }
    }

    #[test]
    fn the_misere_endgame_is_played_the_right_way_round() {
        // One heap of two or more left: reduce it to nothing or to one,
        // whichever leaves an *odd* number of single-token heaps, so that the
        // opponent — forced to move — is the one who runs out.
        assert_eq!(best_move(&[1, 1, 3], Variant::Misere), Some((2, 2)));
        // Two single-token heaps already: take the whole big one and leave an
        // odd... no — leave one, because two ones plus one is three.
        assert_eq!(best_move(&[1, 3], Variant::Misere), Some((1, 3)));
        assert_eq!(best_move(&[1, 1, 1, 4], Variant::Misere), Some((3, 4)));
        // Normal play at the same positions is the plain nim-sum rule, which
        // is where the two games differ at all.
        assert_eq!(best_move(&[1, 1, 3], Variant::Normal), Some((2, 3)));
        assert_eq!(best_move(&[1, 3], Variant::Normal), Some((1, 2)));
        // Every heap already at one: take one, and the parity decides it.
        assert_eq!(best_move(&[0, 1, 1], Variant::Misere), Some((1, 1)));
    }

    #[test]
    fn a_perfect_opponent_never_loses_a_game_it_starts_winning() {
        // The whole pipeline, not just the solver: aim, take, tick, reply.
        let mut memo = HashMap::new();
        for variant in [Variant::Misere, Variant::Normal] {
            for preset in 0..PRESETS.len() {
                let start = Nim::heaps_from_preset(preset);
                if winning(&start, variant, &mut memo) {
                    // The human moves first and can win this one; nothing to
                    // prove about the computer.
                    continue;
                }
                let mut app = game();
                app.apply(Action::SetPreset(preset));
                if app.variant() != variant {
                    app.apply(Action::ToggleVariant);
                }
                for _ in 0..200 {
                    if !app.playing() {
                        break;
                    }
                    // A human who always takes one from the first heap left.
                    let heap = app
                        .heaps()
                        .iter()
                        .position(|&h| h > 0)
                        .expect("a live game has a token in it");
                    assert!(app.apply(Action::Aim(heap, 1)) || app.selected_heap() == heap);
                    assert!(app.apply(Action::Take), "the human could not move");
                    settle(&mut app);
                }
                assert_eq!(
                    app.state(),
                    GameState::Won(Player::Computer),
                    "{variant:?} preset {preset}: the computer lost a won game"
                );
            }
        }
    }

    #[test]
    fn misere_hands_the_game_to_whoever_did_not_take_the_last_token() {
        let mut app = game();
        app.apply(Action::SetPreset(3)); // [1, 2, 3]
        app.take(0, 1);
        app.take(1, 2);
        assert_eq!(app.current_player(), Player::Human);
        app.take(2, 3);
        assert_eq!(app.total_remaining(), 0);
        assert_eq!(app.state(), GameState::Won(Player::Computer));
    }

    #[test]
    fn normal_play_hands_it_to_whoever_did() {
        let mut app = game();
        app.apply(Action::SetPreset(3));
        app.apply(Action::ToggleVariant);
        assert_eq!(app.variant(), Variant::Normal);
        app.take(0, 1);
        app.take(1, 2);
        app.take(2, 3);
        assert_eq!(app.state(), GameState::Won(Player::Human));
    }

    #[test]
    fn an_illegal_take_changes_nothing() {
        let mut app = game();
        let before = app.heaps().to_vec();
        assert!(!app.take(0, 0), "taking nothing is not a move");
        assert!(!app.take(0, 2), "heap 0 holds one token");
        assert!(!app.take(9, 1), "there is no heap 9");
        assert_eq!(app.heaps(), &before[..]);
        assert_eq!(app.current_player(), Player::Human);
    }

    #[test]
    fn a_finished_game_takes_no_more_tokens() {
        let mut app = game();
        app.apply(Action::SetPreset(3));
        app.take(0, 1);
        app.take(1, 2);
        app.take(2, 3);
        assert!(!app.playing());
        assert!(!app.take(0, 1));
        assert!(!app.enabled(Action::Take));
        assert!(!app.enabled(Action::More));
        // A new game is still reachable from a finished one.
        assert!(app.enabled(Action::NewGame));
        assert!(app.enabled(Action::ToggleVariant));
    }

    #[test]
    fn the_aim_moves_off_a_heap_that_has_just_been_emptied() {
        // The heaps move under the aim every time anyone takes anything; an
        // aim left on an empty heap can neither take nor be taken from.
        let mut app = game(); // [1, 3, 5, 7], aimed at heap 0
        assert_eq!(app.selected_heap(), 0);
        app.apply(Action::Take);
        assert_eq!(app.heaps()[0], 0);
        assert!(
            app.heaps()[app.selected_heap()] > 0,
            "the aim sat on nothing"
        );
        assert_eq!(app.take_count(), 1);
    }

    #[test]
    fn the_count_never_exceeds_the_heap_it_aims_at() {
        let mut app = game();
        app.apply(Action::Select(3)); // seven tokens
        for _ in 0..20 {
            app.apply(Action::More);
        }
        assert_eq!(app.take_count(), 7);
        assert!(!app.enabled(Action::More));
        app.apply(Action::Select(1)); // three tokens
        assert_eq!(app.take_count(), 1, "the count followed the aim across");
        for _ in 0..20 {
            app.apply(Action::Fewer);
        }
        assert_eq!(app.take_count(), 1, "the count fell below a legal move");
        assert!(!app.enabled(Action::Fewer));
    }

    #[test]
    fn the_aim_steps_over_heaps_with_nothing_in_them() {
        let mut app = game();
        app.take(1, 3); // empty heap 1, and hand the turn back
        app.take(0, 1);
        assert_eq!(app.heaps(), &[0, 0, 5, 7]);
        assert_eq!(app.current_player(), Player::Human);
        app.apply(Action::Select(app.neighbour(true)));
        assert_eq!(app.selected_heap(), 3, "{:?}", app.heaps());
        app.apply(Action::Select(app.neighbour(false)));
        assert_eq!(app.selected_heap(), 2);
        // And stops at the edge rather than wrapping.
        app.apply(Action::Select(app.neighbour(false)));
        assert_eq!(app.selected_heap(), 2);
    }

    #[test]
    fn a_preset_label_spells_the_heaps_it_deals() {
        // `PRESETS` held the sizes and a parallel `PRESET_NAMES` held strings
        // that also spelled them, with nothing keeping the two in step.
        for i in 0..PRESETS.len() {
            let label = Nim::preset_label(i);
            let heaps = Nim::heaps_from_preset(i);
            let inside = label
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or_else(|| panic!("preset {i} has no heap list in {label:?}"));
            let spelled: Vec<u32> = inside
                .split(',')
                .map(|s| s.trim().parse().expect("a heap size"))
                .collect();
            assert_eq!(spelled, heaps, "preset {i} names a board it does not deal");
        }

        let drawn = texts(&game().frame(WINDOW_WIDTH, WINDOW_HEIGHT));
        for i in 0..PRESETS.len() {
            let label = Nim::preset_label(i);
            assert!(drawn.contains(&label), "the footer omits {label:?}");
        }
        assert_eq!(Nim::preset_label(PRESETS.len()), "");
        assert!(Nim::heaps_from_preset(PRESETS.len()).is_empty());
    }

    #[test]
    fn every_preset_key_deals_the_board_its_button_names() {
        for (i, key) in PRESET_KEYS.iter().enumerate() {
            let mut app = game();
            tap(&mut app, *key);
            assert_eq!(app.preset(), i);
            assert_eq!(app.heaps(), &Nim::heaps_from_preset(i)[..]);
            assert_eq!(app.current_player(), Player::Human);
            assert_eq!(app.state(), GameState::Playing);
        }
    }

    #[test]
    fn choosing_a_board_or_a_variant_starts_a_fresh_game() {
        let mut app = game();
        app.apply(Action::Take);
        settle(&mut app);
        assert_ne!(app.heaps(), &Nim::heaps_from_preset(0)[..]);

        app.apply(Action::ToggleVariant);
        assert_eq!(app.heaps(), &Nim::heaps_from_preset(0)[..]);
        assert_eq!(app.current_player(), Player::Human);
        assert!(!app.thinking());

        app.apply(Action::Take);
        settle(&mut app);
        app.apply(Action::SetPreset(4));
        assert_eq!(app.heaps(), &[5, 7, 9]);
        assert_eq!(app.selected_heap(), 0);
        assert_eq!(app.take_count(), 1);
        assert!(!app.thinking());
    }

    #[test]
    fn the_nim_sum_is_the_xor_of_the_heaps() {
        let app = game();
        assert_eq!(app.nim_sum(), 1 ^ 3 ^ 5 ^ 7);
        assert_eq!(app.total_remaining(), 16);
        assert_eq!(app.tallest(), 7);
    }

    #[test]
    fn the_status_line_says_whose_turn_it_is_and_what_it_would_take() {
        let mut app = game();
        assert_eq!(app.status(), "Your turn: take 1 from heap 1");
        app.apply(Action::Aim(2, 4));
        assert_eq!(app.status(), "Your turn: take 4 from heap 3");
        app.apply(Action::Take);
        assert_eq!(app.status(), "Computer thinking...");
        settle(&mut app);
        assert!(app.status().starts_with("Your turn"), "{}", app.status());
    }

    #[test]
    fn modified_keys_are_left_for_the_window_manager() {
        let mut app = game();
        let before = app.selected_heap();
        assert_eq!(
            handle_event(&mut app, &Event::Key(probe::ctrl(Key::Right))),
            EventResult::Ignored
        );
        assert_eq!(app.selected_heap(), before);
    }
}
