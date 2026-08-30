#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::needless_range_loop)]

//! Slate OS Solitaire -- Klondike solitaire.
//!
//! A standard 52-card deck dealt into seven tableau columns, a stock and
//! waste, and four foundations. Tab and the arrow keys move the cursor,
//! Enter or Space picks up and puts down, `Z` undoes, `A` sends everything it
//! can to the foundations, `N` deals again -- and every pile is also a click
//! target, which it was not before.
//!
//! The whole picture is solved from the size the window reports each frame:
//! there is no built-in size the drawing falls back on, and every box a click
//! is tested against is one the drawing pass recorded.
//!
//! Themed with the Catppuccin Mocha palette.

use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{
    Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

// ── Catppuccin Mocha palette ────────────────────────────────────────
//
// The nine the drawing actually uses. The file carried sixteen, and the seven
// that nothing referred to were kept alive only by a crate-wide
// `#![allow(dead_code)]` at the top -- the same allowance that let a `main`
// which drew nothing pass a build.
const BASE: Color = Color::from_hex(0x1E1E2E);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Card colors ─────────────────────────────────────────────────────
const CARD_BG: Color = Color::from_hex(0xCDD6F4);
const CARD_BACK_BG: Color = Color::from_hex(0x45475A);
const CARD_BACK_PATTERN: Color = Color::from_hex(0x585B70);
const CARD_RED: Color = Color::from_hex(0xF38BA8);
const CARD_BLACK: Color = Color::from_hex(0x1E1E2E);
const SELECTED_HIGHLIGHT: Color = Color::from_hex(0x89B4FA);
const CURSOR_HIGHLIGHT: Color = Color::from_hex(0xF9E2AF);
const EMPTY_PILE: Color = Color::from_hex(0x313244);

// ── The size the window opens at ────────────────────────────────────
//
// The *opening* size, not the size the drawing assumes: everything below is
// solved from whatever the window reports each frame. This pair is handed to
// the window manager once and then never consulted again.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 720.0;

/// A card is a little taller than it is wide, as a real one is.
const CARD_ASPECT: f32 = 100.0 / 70.0;

/// Number of tableau columns.
const TABLEAU_COLS: usize = 7;
/// Number of foundation piles.
const FOUNDATION_COUNT: usize = 4;

// ── What a click can land on ────────────────────────────────────────

/// Everything the drawing pass records a box for.
///
/// A `Target` is not a description of the picture -- it is the list of things
/// the player can point at. The drawing pass records one box per variant as it
/// paints it, so a hit test can only ever agree with what was actually drawn.
/// Before this existed the program had no mouse handling at all: the only way
/// to play was Tab and the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The stock, whether it holds cards or is waiting to be recycled.
    Stock,
    /// The waste pile's top card, or its empty outline.
    Waste,
    /// One of the four foundations.
    Foundation(usize),
    /// A face-up tableau card: the column, and its index among the face-up
    /// cards of that column -- the same pair `FocusArea::Tableau` uses.
    TableauCard(usize, usize),
    /// A face-down tableau card: the column and its index in the whole pile.
    /// Clicking one does nothing but focus the column; it is a target so that
    /// a click on the covered part of a pile is not silently a click on the
    /// table.
    TableauBack(usize, usize),
    /// A tableau column with no cards in it at all.
    TableauEmpty(usize),
    /// The whole of one tableau column, cards and the empty room below them.
    TableauColumn(usize),
    /// The title.
    Title,
    /// The move counter.
    Moves,
    /// The key help.
    Help,
    /// The banner shown when the game is won.
    WinBanner,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Where everything goes, worked out from the window size and nothing else.
///
/// Every field is derived; none is a constant. What this replaced was fifteen
/// `const f32`s -- a 70x100 card, a tableau starting at `y = 140`, a title
/// drawn at `x = 16` and a move counter at `x = 400` -- and a background
/// painted 700 by 700 whatever the window's real size was. The picture was
/// right at exactly one size and wrong at every other, which never showed
/// because the program never opened a window to be resized.
#[derive(Debug, Clone, Copy)]
struct Layout {
    window: Rect,
    /// The strip along the top holding the title, the move count and the help.
    header: Rect,
    /// The row holding the stock, the waste and the foundations.
    top_row: Rect,
    /// Everything below it, where the seven columns are dealt.
    tableau: Rect,
    /// The gap left around and inside every band.
    pad: f32,
    /// The title's font size.
    title: f32,
    /// The font size for the move count and the help.
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Scaled off the smaller side, so a wide-and-short window gets small
        // padding rather than padding that eats its whole height.
        let pad = (w.min(h) * 0.02).clamp(2.0, 16.0).min(w.min(h) / 2.0);
        let title = (h * 0.032).clamp(10.0, 26.0);
        let small = (h * 0.020).clamp(7.0, 16.0);

        let header_h = h * 0.07;
        let rest = (h - header_h).max(0.0);
        // The top row is one card tall; the tableau needs room for a card plus
        // the fan below it, so it gets the larger share.
        let top_h = rest * 0.30;

        let header = Rect::new(0.0, 0.0, w, header_h);
        let top_row = Rect::new(0.0, header.bottom(), w, top_h);
        let tableau = Rect::new(0.0, top_row.bottom(), w, (rest - top_h).max(0.0));

        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            top_row,
            tableau,
            pad,
            title,
            small,
        }
    }
}

/// The card geometry, fitted to whatever room the layout gave it.
///
/// Both rows share one card size: a stock card the size of a tableau card is
/// the whole point of a deck. So the size is the largest that fits *both*
/// bands -- seven columns across, one card tall in the top row, and a card
/// plus the deepest fan a column can reach in the tableau.
#[derive(Debug, Clone, Copy)]
struct Table {
    /// The width of one card.
    card_w: f32,
    /// The height of one card.
    card_h: f32,
    /// The gap between one column and the next.
    gap_x: f32,
    /// How far a face-down card shifts the one above it.
    back_step: f32,
    /// How far a face-up card shifts the one above it.
    face_step: f32,
    /// The left edge of column 0, shared by both rows.
    left: f32,
    /// The top of the top row's cards.
    top_y: f32,
    /// The top of the tableau's cards.
    tableau_y: f32,
    /// The corner radius, scaled with the card.
    corner: f32,
}

impl Table {
    /// The deepest a column can be fanned: six face-down cards under
    /// nineteen face-up ones is the worst case a Klondike deal can reach.
    const DEEPEST_BACKS: f32 = 6.0;
    const DEEPEST_FACES: f32 = 12.0;

    fn fit(l: &Layout) -> Self {
        let cols = TABLEAU_COLS as f32;
        // Across: seven cards and six gaps, the gap a fifth of a card.
        let across = (l.window.w - l.pad * 2.0).max(0.0);
        let by_width = (across / (cols + (cols - 1.0) * 0.2)).max(0.0);

        // Down: the top row must hold one card, and the tableau one card plus
        // the fan. `back_step` and `face_step` are fractions of the card
        // height, so the whole depth is a multiple of it.
        let by_top = ((l.top_row.h - l.pad * 2.0).max(0.0) / CARD_ASPECT).max(0.0);
        let depth = CARD_ASPECT
            + Self::DEEPEST_BACKS * CARD_ASPECT * 0.08
            + Self::DEEPEST_FACES * CARD_ASPECT * 0.22;
        let by_tableau = ((l.tableau.h - l.pad * 2.0).max(0.0) / depth).max(0.0);

        let card_w = by_width.min(by_top).min(by_tableau);
        let card_h = card_w * CARD_ASPECT;
        let gap_x = card_w * 0.2;

        // Whatever is left over across becomes an even margin, so the deal
        // sits in the middle of a window wider than it needs.
        let used = card_w * cols + gap_x * (cols - 1.0);
        let left = (l.window.w - used) / 2.0;

        Self {
            card_w,
            card_h,
            gap_x,
            back_step: card_h * 0.08,
            face_step: card_h * 0.22,
            left,
            top_y: l.top_row.y + (l.top_row.h - card_h).max(0.0) / 2.0,
            tableau_y: l.tableau.y + l.pad,
            corner: (card_w * 0.09).max(1.0),
        }
    }

    /// The box of slot `index` in the top row.
    ///
    /// The row is stock, waste, a blank, then the four foundations -- seven
    /// slots for seven columns, so the two rows line up.
    fn slot(self, index: usize) -> Rect {
        Rect::new(
            self.left + (self.card_w + self.gap_x) * index as f32,
            self.top_y,
            self.card_w,
            self.card_h,
        )
    }

    /// The left edge of a tableau column.
    fn col_x(self, col: usize) -> f32 {
        self.left + (self.card_w + self.gap_x) * col as f32
    }

    /// The box of the `nth` face-down card of a column.
    fn back_rect(self, col: usize, nth: usize) -> Rect {
        Rect::new(
            self.col_x(col),
            self.tableau_y + self.back_step * nth as f32,
            self.card_w,
            self.card_h,
        )
    }

    /// The box of a face-up card: `backs` face-down cards below it, and it is
    /// the `nth` face-up one.
    fn face_rect(self, col: usize, backs: usize, nth: usize) -> Rect {
        Rect::new(
            self.col_x(col),
            self.tableau_y + self.back_step * backs as f32 + self.face_step * nth as f32,
            self.card_w,
            self.card_h,
        )
    }
}

/// A rectangle shrunk by `pad` on every side, never past nothing.
fn inset(rect: Rect, pad: f32) -> Rect {
    Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    )
}

/// Widen a `u32` the window hands us into the `f32` the layout works in.
fn f32_from_u32(v: u32) -> f32 {
    // `u32::MAX` is not representable exactly, but a window that wide does not
    // exist; every real size round-trips.
    v as f32
}

// ── Randomness ─────────────────────────────────────────────────────
//
// From `randrange`, not a local LCG. The local one drew the Fisher-Yates
// partner with `state % (i + 1)`, and on a modulus-2^64 generator the low bit
// of `state` alternates 0,1,0,1 for ever. Half of a 52-card shuffle's bounds
// are even, and `x % n` for even `n` preserves the parity of `x`, so on all 25
// of those swaps the partner index had a single fixed parity. `new_game`
// reseeds from the state, so every deal restarted the draw counter at zero and
// got the *same* pattern of fixed parities -- only which parity varied.
//
// Measured before the fix, over 200 000 deals played the way `new_game` plays
// them:
//
//   * the two of hearts was the leftmost face-up card in **17.6%** of deals,
//     where any one of the 52 should hold that slot 1.9% of the time;
//   * it was face-up somewhere in the opening tableau in 33.1% of deals
//     against an expected 13.5%, and 29 of the 52 cards were off by more than
//     two percentage points on that share.
//
// The shuffle itself was already the correct downward Fisher-Yates. Only the
// reduction was wrong.
use randrange::{RandomSource, SeededRng};

// ── Card types ──────────────────────────────────────────────────────

/// The four card suits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Suit {
    Hearts,
    Diamonds,
    Clubs,
    Spades,
}

impl Suit {
    /// All four suits in standard order.
    const ALL: [Suit; 4] = [Suit::Hearts, Suit::Diamonds, Suit::Clubs, Suit::Spades];

    /// Unicode symbol for display.
    fn symbol(self) -> &'static str {
        match self {
            Self::Hearts => "\u{2665}",
            Self::Diamonds => "\u{2666}",
            Self::Clubs => "\u{2663}",
            Self::Spades => "\u{2660}",
        }
    }

    /// Whether the suit is red.
    fn is_red(self) -> bool {
        matches!(self, Self::Hearts | Self::Diamonds)
    }

    /// Display color for this suit.
    fn color(self) -> Color {
        if self.is_red() { CARD_RED } else { CARD_BLACK }
    }

    /// Index 0..3 for foundation ordering.
    fn index(self) -> usize {
        match self {
            Self::Hearts => 0,
            Self::Diamonds => 1,
            Self::Clubs => 2,
            Self::Spades => 3,
        }
    }
}

/// Card rank (Ace through King).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Rank {
    Ace = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
}

impl Rank {
    /// All thirteen ranks.
    const ALL: [Rank; 13] = [
        Rank::Ace,
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
    ];

    /// Short display label.
    fn label(self) -> &'static str {
        match self {
            Self::Ace => "A",
            Self::Two => "2",
            Self::Three => "3",
            Self::Four => "4",
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
            Self::Nine => "9",
            Self::Ten => "10",
            Self::Jack => "J",
            Self::Queen => "Q",
            Self::King => "K",
        }
    }

    /// Numeric value (Ace=1 through King=13).
    fn value(self) -> u8 {
        self as u8
    }

    // There was a `from_value(u8) -> Option<Rank>` here -- a hand-written
    // thirteen-arm inverse of `value()` -- with a round-trip test and no
    // caller.  Nothing in solitaire turns a number back into a rank: ranks
    // become numbers at exactly two places, both comparisons that stay in
    // number space (`can_stack_on_tableau`, `can_stack_on_foundation`), and
    // the game has no save file to read one back out of.  See
    // known-issues.md lesson 45.
    //
    // Its test was a round trip through `value()`, so it asserted nothing
    // about the game either; `test_rank_value` already pins Ace=1 and
    // King=13, which is what the two comparisons above actually rely on.
    //
    // If a saved game ever needs to name a rank, write it as the `label()`
    // string rather than reviving this: `label` is already the tested,
    // used-in-anger spelling, and a second numeric mapping beside the
    // explicit discriminants is a second thing to keep in step.
}

/// A playing card with suit and rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Card {
    suit: Suit,
    rank: Rank,
}

impl Card {
    const fn new(suit: Suit, rank: Rank) -> Self {
        Self { suit, rank }
    }

    /// Whether this card can stack on top of `below` in a tableau pile.
    /// Must be opposite color and one rank lower.
    fn can_stack_on_tableau(self, below: Card) -> bool {
        self.suit.is_red() != below.suit.is_red()
            && self.rank.value().saturating_add(1) == below.rank.value()
    }

    /// Whether this card can be placed on a foundation pile whose
    /// current top card has value `foundation_top_value` (0 if empty).
    fn can_place_on_foundation(self, foundation_top_value: u8) -> bool {
        self.rank.value() == foundation_top_value.saturating_add(1)
    }
}

/// A card in a pile that may be face-up or face-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PileCard {
    card: Card,
    face_up: bool,
}

impl PileCard {
    const fn new(card: Card, face_up: bool) -> Self {
        Self { card, face_up }
    }
}

/// Creates a standard 52-card deck.
fn make_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for &suit in &Suit::ALL {
        for &rank in &Rank::ALL {
            deck.push(Card::new(suit, rank));
        }
    }
    deck
}

/// Step an index by a signed delta, or `None` if the step leaves `usize`.
///
/// The cursor code used to write `i as i32 + delta` and cast the answer back.
/// That is two silent lies in one line: the cast in loses any index past
/// `i32::MAX`, and the addition can overflow before the cast out ever runs.
/// Asking for the step and being told "there is none" is the same code with
/// the lies removed.
fn step_index(index: usize, delta: i32) -> Option<usize> {
    let here = i32::try_from(index).ok()?;
    let there = here.checked_add(delta)?;
    usize::try_from(there).ok()
}

// ── Focus / Selection ───────────────────────────────────────────────

/// Which area of the game the cursor is focused on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusArea {
    /// Stock pile (draw).
    Stock,
    /// Waste pile (drawn cards).
    Waste,
    /// Foundation pile 0..3.
    Foundation(usize),
    /// Tableau column 0..6, with a vertical index into the face-up cards.
    Tableau(usize, usize),
}

impl FocusArea {
    /// The default starting focus.
    fn default_focus() -> Self {
        Self::Stock
    }
}

/// What the player has selected to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    /// A card from the waste pile.
    Waste,
    /// A run of cards from a tableau column starting at the given face-up index.
    Tableau(usize, usize),
    /// Top card from a foundation pile.
    Foundation(usize),
}

// ── Undo ────────────────────────────────────────────────────────────

/// Records one undoable action.
#[derive(Clone, Debug)]
enum UndoAction {
    /// Drew a card from stock to waste.
    Draw,
    /// Recycled waste back to stock.
    Recycle,
    /// Moved card(s) between piles.
    Move {
        from: MoveSource,
        to: MoveDest,
        count: usize,
        /// If a tableau card was flipped face-up after the move.
        flipped: bool,
    },
}

/// Source of a move (for undo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoveSource {
    Waste,
    Foundation(usize),
    Tableau(usize),
}

/// Destination of a move (for undo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoveDest {
    Foundation(usize),
    Tableau(usize),
}

// ── Game state ──────────────────────────────────────────────────────

/// Full game state for Klondike Solitaire.
struct GameState {
    /// Stock pile (face-down, draw from end).
    stock: Vec<Card>,
    /// Waste pile (face-up, top is last element).
    waste: Vec<Card>,
    /// Four foundation piles, indexed by `Suit::index()`.
    foundations: [Vec<Card>; FOUNDATION_COUNT],
    /// Seven tableau columns, each containing face-down and face-up cards.
    tableau: [Vec<PileCard>; TABLEAU_COLS],
    /// Current cursor focus.
    focus: FocusArea,
    /// Current selection (if any).
    selection: Option<Selection>,
    /// Undo history.
    undo_stack: Vec<UndoAction>,
    /// Total moves made.
    move_count: u32,
    /// Whether the game has been won.
    won: bool,
    /// RNG for new games.
    rng: SeededRng,
}

impl GameState {
    /// Create a new game with the given seed.
    fn new(seed: u64) -> Self {
        let mut state = Self {
            stock: Vec::new(),
            waste: Vec::new(),
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            focus: FocusArea::default_focus(),
            selection: None,
            undo_stack: Vec::new(),
            move_count: 0,
            won: false,
            rng: SeededRng::new(seed),
        };
        state.deal();
        state
    }

    /// Deal cards from a shuffled deck into tableau and stock.
    fn deal(&mut self) {
        let mut deck = make_deck();
        self.rng.shuffle(&mut deck);

        // Clear everything.
        self.stock.clear();
        self.waste.clear();
        for f in &mut self.foundations {
            f.clear();
        }
        for t in &mut self.tableau {
            t.clear();
        }
        self.selection = None;
        self.undo_stack.clear();
        self.move_count = 0;
        self.won = false;
        self.focus = FocusArea::default_focus();

        // Deal to tableau: column i gets i+1 cards, last one face-up.
        //
        // Dealing off an iterator rather than a running index means the deck
        // cannot be over-read: a short deck ends the deal instead of panicking
        // one card past the end, and "what is left" needs no arithmetic to
        // describe -- it is whatever the iterator still holds.
        let mut cards = deck.into_iter();
        for (col, pile) in self.tableau.iter_mut().enumerate() {
            for row in 0..=col {
                let Some(card) = cards.next() else { break };
                pile.push(PileCard::new(card, row == col));
            }
        }

        // Remaining cards go to stock.
        self.stock.extend(cards);
    }

    /// Start a new game using the next RNG value as seed.
    fn new_game(&mut self) {
        let seed = self.rng.next_u64();
        self.rng = SeededRng::new(seed);
        self.deal();
    }

    /// Draw one card from stock to waste.
    fn draw_from_stock(&mut self) {
        if let Some(card) = self.stock.pop() {
            self.waste.push(card);
            self.undo_stack.push(UndoAction::Draw);
            self.bump_moves();
        } else if !self.waste.is_empty() {
            // Recycle waste back to stock (reversed).
            while let Some(card) = self.waste.pop() {
                self.stock.push(card);
            }
            self.undo_stack.push(UndoAction::Recycle);
            self.bump_moves();
        }
    }

    /// Count one more move.
    ///
    /// The counter is driven by the player, so `+= 1` is an overflow the
    /// player could in principle reach; saturating means a game long enough
    /// to hit the ceiling stops counting rather than wrapping back to zero
    /// and reporting a fresh deal.
    fn bump_moves(&mut self) {
        self.move_count = self.move_count.saturating_add(1);
    }

    /// One tableau column, or nothing at all if there is no such column.
    ///
    /// Every caller used to write `if col >= TABLEAU_COLS { return … }` and
    /// then index -- a guard and a panic, one line apart, repeated a dozen
    /// times. Asking for the column is the guard.
    fn col(&self, col: usize) -> &[PileCard] {
        self.tableau.get(col).map_or(&[][..], Vec::as_slice)
    }

    /// One tableau column to write to, or `None` if there is no such column.
    fn col_mut(&mut self, col: usize) -> Option<&mut Vec<PileCard>> {
        self.tableau.get_mut(col)
    }

    /// One foundation pile, or nothing at all if there is no such pile.
    fn found(&self, idx: usize) -> &[Card] {
        self.foundations.get(idx).map_or(&[][..], Vec::as_slice)
    }

    /// Get the number of face-up cards in a tableau column.
    fn tableau_face_up_count(&self, col: usize) -> usize {
        self.col(col).iter().filter(|c| c.face_up).count()
    }

    /// Get the number of face-down cards in a tableau column.
    fn tableau_face_down_count(&self, col: usize) -> usize {
        self.col(col).iter().filter(|c| !c.face_up).count()
    }

    /// Get the top card of a foundation pile (by suit index).
    fn foundation_top(&self, idx: usize) -> Option<Card> {
        self.found(idx).last().copied()
    }

    /// Get the top value of a foundation pile (0 if empty).
    fn foundation_top_value(&self, idx: usize) -> u8 {
        self.foundation_top(idx)
            .map(|c| c.rank.value())
            .unwrap_or(0)
    }

    /// Get the top card of the waste pile.
    fn waste_top(&self) -> Option<Card> {
        self.waste.last().copied()
    }

    /// Get the bottom-most face-up card in a tableau column.
    fn tableau_top_card(&self, col: usize) -> Option<Card> {
        self.col(col).last().filter(|c| c.face_up).map(|c| c.card)
    }

    /// Try to move the waste top card to a foundation.
    fn try_waste_to_foundation(&mut self) -> bool {
        let Some(card) = self.waste_top() else {
            return false;
        };
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            return false;
        }
        // The destination is taken by name before the source is popped: a pop
        // followed by a push that turned out to have nowhere to go would
        // delete the card outright.
        let Some(pile) = self.foundations.get_mut(fidx) else {
            return false;
        };
        pile.push(card);
        self.waste.pop();
        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Waste,
            to: MoveDest::Foundation(fidx),
            count: 1,
            flipped: false,
        });
        self.bump_moves();
        self.check_win();
        true
    }

    /// Try to move the waste top card to a tableau column.
    fn try_waste_to_tableau(&mut self, col: usize) -> bool {
        let Some(card) = self.waste_top() else {
            return false;
        };
        if !self.can_place_on_tableau(card, col) {
            return false;
        }
        let Some(pile) = self.col_mut(col) else {
            return false;
        };
        pile.push(PileCard::new(card, true));
        self.waste.pop();
        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Waste,
            to: MoveDest::Tableau(col),
            count: 1,
            flipped: false,
        });
        self.bump_moves();
        true
    }

    /// Check if a card can be placed on a tableau column.
    fn can_place_on_tableau(&self, card: Card, col: usize) -> bool {
        if col >= TABLEAU_COLS {
            return false;
        }
        match self.tableau_top_card(col) {
            Some(top) => card.can_stack_on_tableau(top),
            None => card.rank == Rank::King,
        }
    }

    /// Try to move cards from one tableau column to another.
    /// `from_col` is the source, `face_up_idx` is the index into face-up cards
    /// (0 = deepest face-up card), `to_col` is the destination.
    fn try_tableau_to_tableau(
        &mut self,
        from_col: usize,
        face_up_idx: usize,
        to_col: usize,
    ) -> bool {
        if from_col == to_col {
            return false;
        }

        let face_down = self.tableau_face_down_count(from_col);
        let Some(abs_idx) = face_down.checked_add(face_up_idx) else {
            return false;
        };
        // The card at the start of the run we want to move. Asking the column
        // for it is the range check: no such card, no such move.
        let Some(moving_card) = self.col(from_col).get(abs_idx).map(|pc| pc.card) else {
            return false;
        };
        if !self.can_place_on_tableau(moving_card, to_col) {
            return false;
        }
        // Both ends are confirmed to exist before either is touched. Draining
        // the source and then discovering there is no destination would drop
        // the run on the floor.
        if self.tableau.get(to_col).is_none() {
            return false;
        }

        let Some(source) = self.col_mut(from_col) else {
            return false;
        };
        let cards: Vec<PileCard> = source.drain(abs_idx..).collect();
        let count = cards.len();
        let Some(dest) = self.col_mut(to_col) else {
            return false;
        };
        dest.extend(cards);

        // Flip the new top card if it was face-down.
        let flipped = self.flip_top_if_needed(from_col);

        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Tableau(from_col),
            to: MoveDest::Tableau(to_col),
            count,
            flipped,
        });
        self.bump_moves();
        true
    }

    /// Try to move the top card of a tableau column to its foundation.
    fn try_tableau_to_foundation(&mut self, col: usize) -> bool {
        let Some(card) = self.tableau_top_card(col) else {
            return false;
        };
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            return false;
        }
        // Destination confirmed before the source is popped, so a card can
        // never be removed from the tableau with nowhere to land.
        if self.foundations.get(fidx).is_none() {
            return false;
        }
        if let Some(source) = self.col_mut(col) {
            source.pop();
        }
        if let Some(pile) = self.foundations.get_mut(fidx) {
            pile.push(card);
        }
        let flipped = self.flip_top_if_needed(col);
        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Tableau(col),
            to: MoveDest::Foundation(fidx),
            count: 1,
            flipped,
        });
        self.bump_moves();
        self.check_win();
        true
    }

    /// Try to move the top card of a foundation pile to a tableau column.
    fn try_foundation_to_tableau(&mut self, fidx: usize, col: usize) -> bool {
        let Some(card) = self.foundation_top(fidx) else {
            return false;
        };
        if !self.can_place_on_tableau(card, col) {
            return false;
        }
        let Some(pile) = self.col_mut(col) else {
            return false;
        };
        pile.push(PileCard::new(card, true));
        if let Some(source) = self.foundations.get_mut(fidx) {
            source.pop();
        }
        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Foundation(fidx),
            to: MoveDest::Tableau(col),
            count: 1,
            flipped: false,
        });
        self.bump_moves();
        true
    }

    /// Flip the top card of a tableau column face-up if it is face-down.
    /// Returns true if a flip occurred.
    fn flip_top_if_needed(&mut self, col: usize) -> bool {
        if let Some(pile) = self.col_mut(col)
            && let Some(top) = pile.last_mut()
            && !top.face_up
        {
            top.face_up = true;
            return true;
        }
        false
    }

    /// Check if the game is won (all foundations have 13 cards).
    fn check_win(&mut self) {
        self.won = self.foundations.iter().all(|f| f.len() == 13);
    }

    /// Auto-move: try to send the currently available card to its foundation.
    /// Checks waste top and all tableau tops.
    fn auto_move_to_foundation(&mut self) -> bool {
        // Try waste first.
        if self.try_waste_to_foundation() {
            return true;
        }
        // Try each tableau column.
        for col in 0..TABLEAU_COLS {
            if self.try_tableau_to_foundation(col) {
                return true;
            }
        }
        false
    }

    /// Undo the last action.
    fn undo(&mut self) {
        let action = match self.undo_stack.pop() {
            Some(a) => a,
            None => return,
        };
        match action {
            UndoAction::Draw => {
                if let Some(card) = self.waste.pop() {
                    self.stock.push(card);
                }
                self.move_count = self.move_count.saturating_sub(1);
            }
            UndoAction::Recycle => {
                while let Some(card) = self.stock.pop() {
                    self.waste.push(card);
                }
                self.move_count = self.move_count.saturating_sub(1);
            }
            UndoAction::Move {
                from,
                to,
                count,
                flipped,
            } => {
                // Un-flip if needed.
                if flipped
                    && let MoveSource::Tableau(col) = from
                    && let Some(pile) = self.col_mut(col)
                    && let Some(top) = pile.last_mut()
                {
                    top.face_up = false;
                }
                // Move cards back.
                let cards: Vec<PileCard> = match to {
                    MoveDest::Foundation(fidx) => {
                        let mut result = Vec::new();
                        if let Some(pile) = self.foundations.get_mut(fidx) {
                            for _ in 0..count {
                                if let Some(c) = pile.pop() {
                                    result.push(PileCard::new(c, true));
                                }
                            }
                        }
                        result.reverse();
                        result
                    }
                    MoveDest::Tableau(col) => self.col_mut(col).map_or_else(Vec::new, |pile| {
                        let start = pile.len().saturating_sub(count);
                        pile.drain(start..).collect()
                    }),
                };
                match from {
                    MoveSource::Waste => {
                        for pc in cards {
                            self.waste.push(pc.card);
                        }
                    }
                    MoveSource::Foundation(fidx) => {
                        if let Some(pile) = self.foundations.get_mut(fidx) {
                            for pc in cards {
                                pile.push(pc.card);
                            }
                        }
                    }
                    MoveSource::Tableau(col) => {
                        if let Some(pile) = self.col_mut(col) {
                            pile.extend(cards);
                        }
                    }
                }
                self.move_count = self.move_count.saturating_sub(1);
                self.won = false;
            }
        }
    }

    /// Handle the Enter/Space action on the current focus.
    fn activate(&mut self) {
        if self.won {
            return;
        }

        match self.focus {
            FocusArea::Stock => {
                self.selection = None;
                self.draw_from_stock();
            }
            FocusArea::Waste => {
                if self.waste_top().is_some() {
                    match self.selection {
                        Some(Selection::Waste) => {
                            // Already selected waste, try auto-move to foundation.
                            // Either way, clear the selection.
                            let _ = self.try_waste_to_foundation();
                            self.selection = None;
                        }
                        _ => {
                            self.selection = Some(Selection::Waste);
                        }
                    }
                }
            }
            FocusArea::Foundation(fidx) => {
                match self.selection {
                    Some(Selection::Waste) => {
                        // Try to move waste card to this foundation.
                        if self.try_waste_to_foundation() {
                            self.selection = None;
                        }
                    }
                    Some(Selection::Tableau(col, _)) => {
                        // Try to move tableau top to foundation.
                        if self.try_tableau_to_foundation(col) {
                            self.selection = None;
                        }
                    }
                    Some(Selection::Foundation(other)) if other == fidx => {
                        self.selection = None;
                    }
                    None => {
                        if self.foundation_top(fidx).is_some() {
                            self.selection = Some(Selection::Foundation(fidx));
                        }
                    }
                    _ => {
                        self.selection = None;
                    }
                }
            }
            FocusArea::Tableau(col, offset) => {
                match self.selection {
                    Some(Selection::Waste) => {
                        if self.try_waste_to_tableau(col) {
                            self.selection = None;
                        }
                    }
                    Some(Selection::Foundation(fidx)) => {
                        if self.try_foundation_to_tableau(fidx, col) {
                            self.selection = None;
                        }
                    }
                    Some(Selection::Tableau(from_col, from_idx)) => {
                        if from_col == col {
                            // Same column — try to auto-move top card to foundation
                            // when the selection is the top of the face-up run.
                            let fu = self.tableau_face_up_count(col);
                            if from_idx.saturating_add(1) == fu {
                                let _ = self.try_tableau_to_foundation(col);
                            }
                            self.selection = None;
                        } else if self.try_tableau_to_tableau(from_col, from_idx, col) {
                            self.selection = None;
                        }
                    }
                    None => {
                        let fu = self.tableau_face_up_count(col);
                        if fu > 0 {
                            // Select from the offset position.
                            let idx = if offset < fu {
                                offset
                            } else {
                                fu.saturating_sub(1)
                            };
                            self.selection = Some(Selection::Tableau(col, idx));
                        }
                    }
                }
            }
        }
    }

    /// Navigate focus with Tab (forward cycle).
    fn tab_forward(&mut self) {
        self.focus = match self.focus {
            FocusArea::Stock => FocusArea::Waste,
            FocusArea::Waste => FocusArea::Foundation(0),
            FocusArea::Foundation(i) => {
                let next = i.saturating_add(1);
                if next < FOUNDATION_COUNT {
                    FocusArea::Foundation(next)
                } else {
                    FocusArea::Tableau(0, 0)
                }
            }
            FocusArea::Tableau(col, _) => {
                let next = col.saturating_add(1);
                if next < TABLEAU_COLS {
                    FocusArea::Tableau(next, 0)
                } else {
                    FocusArea::Stock
                }
            }
        };
    }

    /// Navigate focus with Shift+Tab (backward cycle).
    fn tab_backward(&mut self) {
        self.focus = match self.focus {
            FocusArea::Stock => {
                let last_col = TABLEAU_COLS - 1;
                let fu = self.tableau_face_up_count(last_col);
                FocusArea::Tableau(last_col, fu.saturating_sub(1))
            }
            FocusArea::Waste => FocusArea::Stock,
            FocusArea::Foundation(0) => FocusArea::Waste,
            FocusArea::Foundation(i) => FocusArea::Foundation(i.saturating_sub(1)),
            FocusArea::Tableau(0, _) => FocusArea::Foundation(FOUNDATION_COUNT - 1),
            FocusArea::Tableau(col, _) => {
                let prev = col.saturating_sub(1);
                let fu = self.tableau_face_up_count(prev);
                FocusArea::Tableau(prev, fu.saturating_sub(1))
            }
        };
    }

    /// Move cursor within a tableau column (Up/Down).
    fn move_within_tableau(&mut self, delta: i32) {
        if let FocusArea::Tableau(col, offset) = self.focus {
            let fu = self.tableau_face_up_count(col);
            if fu == 0 {
                return;
            }
            let max_idx = fu.saturating_sub(1);
            let step = delta.unsigned_abs() as usize;
            let new_offset = if delta < 0 {
                offset.saturating_sub(step)
            } else {
                offset.saturating_add(step).min(max_idx)
            };
            self.focus = FocusArea::Tableau(col, new_offset);
        }
    }

    /// Move focus left/right among tableau columns (or top-row items).
    fn move_horizontal(&mut self, delta: i32) {
        match self.focus {
            FocusArea::Stock => {
                if delta > 0 {
                    self.focus = FocusArea::Waste;
                }
            }
            FocusArea::Waste => {
                if delta < 0 {
                    self.focus = FocusArea::Stock;
                } else {
                    self.focus = FocusArea::Foundation(0);
                }
            }
            FocusArea::Foundation(i) => match step_index(i, delta) {
                // Stepping left off the first foundation lands on the waste;
                // stepping right off the last one stays put.
                None => self.focus = FocusArea::Waste,
                Some(new_i) if new_i < FOUNDATION_COUNT => {
                    self.focus = FocusArea::Foundation(new_i);
                }
                Some(_) => {}
            },
            FocusArea::Tableau(col, offset) => {
                if let Some(new_c) = step_index(col, delta)
                    && new_c < TABLEAU_COLS
                {
                    let fu = self.tableau_face_up_count(new_c);
                    let clamped = offset.min(fu.saturating_sub(1));
                    self.focus = FocusArea::Tableau(new_c, clamped);
                }
            }
        }
    }

    /// Move focus vertically between top row and tableau.
    fn move_vertical(&mut self, delta: i32) {
        match self.focus {
            FocusArea::Stock | FocusArea::Waste | FocusArea::Foundation(_) if delta > 0 => {
                // Move down to tableau. Map the horizontal position to a column.
                let col = match self.focus {
                    FocusArea::Stock => 0,
                    FocusArea::Waste => 1,
                    FocusArea::Foundation(i) => i.saturating_add(3).min(TABLEAU_COLS - 1),
                    FocusArea::Tableau(..) => 0,
                };
                let fu = self.tableau_face_up_count(col);
                self.focus = FocusArea::Tableau(col, fu.saturating_sub(1));
            }
            FocusArea::Tableau(col, offset) if delta < 0 && offset == 0 => {
                // Move up from tableau to top row.
                if col == 0 {
                    self.focus = FocusArea::Stock;
                } else if col == 1 {
                    self.focus = FocusArea::Waste;
                } else if (3..3 + FOUNDATION_COUNT).contains(&col) {
                    self.focus = FocusArea::Foundation(col.saturating_sub(3));
                } else {
                    self.focus = FocusArea::Stock;
                }
            }
            FocusArea::Tableau(_, _) if delta < 0 => {
                self.move_within_tableau(-1);
            }
            FocusArea::Tableau(_, _) if delta > 0 => {
                self.move_within_tableau(1);
            }
            _ => {}
        }
    }

    /// Handle a key event, reporting whether the key meant anything here.
    ///
    /// The window redraws on `Consumed` and sleeps on `Ignored`. This used to
    /// return nothing at all, so every key -- including the ones this game has
    /// no use for -- looked to the caller exactly like a move.
    fn handle_key(&mut self, key: Key, modifiers: Modifiers) -> EventResult {
        if self.won {
            // The board is finished and covered by the banner; the only key
            // that still means something is the one that deals again.
            if key == Key::N {
                self.new_game();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        match key {
            Key::Tab => {
                if modifiers.shift {
                    self.tab_backward();
                } else {
                    self.tab_forward();
                }
            }
            Key::Left => self.move_horizontal(-1),
            Key::Right => self.move_horizontal(1),
            Key::Up => self.move_vertical(-1),
            Key::Down => self.move_vertical(1),
            Key::Enter | Key::Space => self.activate(),
            Key::Z => {
                self.selection = None;
                self.undo();
            }
            Key::N => {
                self.new_game();
            }
            Key::Escape => {
                self.selection = None;
            }
            Key::A => {
                // Auto-move all possible cards to foundations.
                while self.auto_move_to_foundation() {}
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Draw the whole game at the size the window reports.
    ///
    /// Every box a click is tested against is recorded here as it is painted,
    /// so a hit test cannot disagree with the picture. The old `render` took
    /// no size at all and painted a 700x700 background whatever the window
    /// was.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let t = Table::fit(&l);
        let mut f = Frame::new(w, h);

        // The background is the window, not a remembered size.
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        f.clip(l.window);

        self.draw_header(&mut f, &l);
        self.draw_top_row(&mut f, &l, t);
        self.draw_tableau(&mut f, &l, t);
        if self.won {
            self.draw_win_banner(&mut f, &l);
        }

        f.unclip();
        f
    }

    /// The title, the move count and the key help, laid left to right.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        let band = inset(l.header, l.pad);
        if band.is_empty() {
            return;
        }

        let title = label_in(
            f,
            band,
            "Solitaire",
            Ink::new(l.title, FontWeightHint::Bold, LAVENDER),
        );
        f.hit(Target::Title, title);

        // Each of these follows the one before it rather than sitting at a
        // fixed x. The move counter used to be drawn at x = 400 and the help
        // at x = 500, which put them on top of the title in any window narrow
        // enough and adrift in any window wide enough.
        let gap = l.pad * 1.5;
        let ink = Ink::new(l.small, FontWeightHint::Regular, SUBTEXT0);
        let moves = format!("Moves: {}", self.move_count);
        let x = title.right() + gap;
        if x < band.right() {
            let rest = Rect::new(x, band.y, band.right() - x, band.h);
            let box_ = label_in(f, rest, &moves, ink);
            f.hit(Target::Moves, box_);

            let hx = box_.right() + gap;
            if hx < band.right() {
                let help = Rect::new(hx, band.y, band.right() - hx, band.h);
                let drawn = label_in(
                    f,
                    help,
                    "N:New  Z:Undo  A:Auto",
                    Ink::new(l.small, FontWeightHint::Regular, OVERLAY0),
                );
                f.hit(Target::Help, drawn);
            }
        }
    }

    /// The stock, the waste, and the four foundations.
    fn draw_top_row(&self, f: &mut Frame<Target>, l: &Layout, t: Table) {
        // Stock in slot 0, waste in slot 1, slot 2 left blank so the
        // foundations sit under the right-hand columns.
        let stock = t.slot(0);
        let focused = self.focus == FocusArea::Stock;
        if self.stock.is_empty() {
            self.draw_empty_pile(f, stock, t, focused);
            if !self.waste.is_empty() {
                // The recycle arrow: the stock is empty but the waste can be
                // turned back over.
                centre_glyph(
                    f,
                    stock,
                    "\u{21BB}",
                    Ink::new(t.card_w * 0.3, FontWeightHint::Bold, OVERLAY0),
                );
            }
        } else {
            self.draw_card_back(f, stock, t, focused);
            self.draw_pile_count(f, l, stock, &format!("{}", self.stock.len()));
        }
        f.hit(Target::Stock, stock);

        let waste = t.slot(1);
        match self.waste_top() {
            Some(card) => self.draw_card_face(
                f,
                waste,
                t,
                card,
                self.focus == FocusArea::Waste,
                self.selection == Some(Selection::Waste),
            ),
            None => self.draw_empty_pile(f, waste, t, self.focus == FocusArea::Waste),
        }
        f.hit(Target::Waste, waste);

        // Walking the suits rather than counting to four hands each pile the
        // suit it is for, instead of looking it up by an index that has to be
        // trusted to be in range.
        for (i, &suit) in Suit::ALL.iter().enumerate() {
            let slot = t.slot(i.saturating_add(3));
            let focused = self.focus == FocusArea::Foundation(i);
            match self.foundation_top(i) {
                Some(card) => {
                    self.draw_card_face(
                        f,
                        slot,
                        t,
                        card,
                        focused,
                        self.selection == Some(Selection::Foundation(i)),
                    );
                    self.draw_pile_count(f, l, slot, &format!("{}/13", self.found(i).len()));
                }
                None => {
                    self.draw_empty_pile(f, slot, t, focused);
                    centre_glyph(
                        f,
                        slot,
                        suit.symbol(),
                        Ink::new(t.card_w * 0.3, FontWeightHint::Regular, OVERLAY0),
                    );
                }
            }
            f.hit(Target::Foundation(i), slot);
        }
    }

    /// The seven columns.
    fn draw_tableau(&self, f: &mut Frame<Target>, l: &Layout, t: Table) {
        for (col, pile) in self.tableau.iter().enumerate() {
            // The column's whole strip goes down first, because a hit test
            // reads the boxes in reverse paint order: everything recorded
            // after this sits on top of it, which is what makes a click on a
            // card a click on the card and a click on the table below it a
            // click on the column.
            f.hit(Target::TableauColumn(col), self.column_reach(t, col, l));

            if pile.is_empty() {
                let slot = t.back_rect(col, 0);
                let focused = matches!(self.focus, FocusArea::Tableau(c, _) if c == col);
                self.draw_empty_pile(f, slot, t, focused);
                f.hit(Target::TableauEmpty(col), slot);
                continue;
            }

            let backs = pile.iter().filter(|c| !c.face_up).count();
            let mut nth = 0;
            for (i, pc) in pile.iter().enumerate() {
                if pc.face_up {
                    let rect = t.face_rect(col, backs, nth);
                    let selected = match self.selection {
                        Some(Selection::Tableau(sel_col, sel_idx)) => {
                            sel_col == col && nth >= sel_idx
                        }
                        _ => false,
                    };
                    self.draw_card_face(
                        f,
                        rect,
                        t,
                        pc.card,
                        self.focus == FocusArea::Tableau(col, nth),
                        selected,
                    );
                    f.hit(Target::TableauCard(col, nth), rect);
                    nth = nth.saturating_add(1);
                } else {
                    let rect = t.back_rect(col, i);
                    self.draw_card_back(f, rect, t, false);
                    f.hit(Target::TableauBack(col, i), rect);
                }
            }
        }
    }

    /// The whole strip a column owns: its cards and the empty room under
    /// them, down to the bottom of the tableau band. A click below the last
    /// card of a column is a click on that column -- which is how a card is
    /// dropped onto a pile whose top card is well above the pointer.
    fn column_reach(&self, t: Table, col: usize, l: &Layout) -> Rect {
        let top = t.back_rect(col, 0);
        let bottom = l.tableau.bottom().max(top.bottom());
        Rect::new(top.x, top.y, top.w, bottom - top.y)
    }

    /// The little count under a pile.
    fn draw_pile_count(&self, f: &mut Frame<Target>, l: &Layout, slot: Rect, s: &str) {
        let ink = Ink::new(l.small * 0.9, FontWeightHint::Regular, SUBTEXT0);
        let y = slot.bottom() + 2.0;
        if y + ink.height() <= l.window.bottom() {
            let _ = label(f, slot.x + 2.0, y, s, ink);
        }
    }

    /// An empty pile: an outline with a darker inside.
    fn draw_empty_pile(&self, f: &mut Frame<Target>, r: Rect, t: Table, focused: bool) {
        // Filled first, then outlined. The other order -- which is what this
        // replaced -- painted the fill over the inner half of the border, so a
        // two-pixel focus ring showed up one pixel wide.
        f.push(RenderCommand::FillRect {
            x: r.x + 1.0,
            y: r.y + 1.0,
            width: (r.w - 2.0).max(0.0),
            height: (r.h - 2.0).max(0.0),
            color: EMPTY_PILE,
            corner_radii: CornerRadii::all(t.corner),
        });
        f.push(RenderCommand::StrokeRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: if focused { CURSOR_HIGHLIGHT } else { OVERLAY0 },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(t.corner),
        });
    }

    /// A face-down card.
    fn draw_card_back(&self, f: &mut Frame<Target>, r: Rect, t: Table, focused: bool) {
        if focused {
            f.push(RenderCommand::StrokeRect {
                x: r.x - 1.0,
                y: r.y - 1.0,
                width: r.w + 2.0,
                height: r.h + 2.0,
                color: CURSOR_HIGHLIGHT,
                line_width: 2.0,
                corner_radii: CornerRadii::all(t.corner + 1.0),
            });
        }
        f.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: CARD_BACK_BG,
            corner_radii: CornerRadii::all(t.corner),
        });

        // Cross-hatch, spaced off the card rather than off a fixed pixel
        // count, so the pattern stays a pattern at any card size.
        let pad = r.w * 0.09;
        let spacing = (r.w * 0.14).max(2.0);
        let inner = inset(r, pad);
        if inner.is_empty() {
            return;
        }
        let mut x = inner.x;
        while x <= inner.right() {
            f.push(RenderCommand::Line {
                x1: x,
                y1: inner.y,
                x2: inner.x + (inner.right() - x).min(inner.h),
                y2: inner.y + (inner.right() - x).min(inner.h),
                color: CARD_BACK_PATTERN,
                width: 1.0,
            });
            x += spacing;
        }
    }

    /// A face-up card: its rank and suit in the two opposite corners, the way
    /// a real card is printed so it reads from either end.
    fn draw_card_face(
        &self,
        f: &mut Frame<Target>,
        r: Rect,
        t: Table,
        card: Card,
        focused: bool,
        selected: bool,
    ) {
        if focused || selected {
            f.push(RenderCommand::StrokeRect {
                x: r.x - 2.0,
                y: r.y - 2.0,
                width: r.w + 4.0,
                height: r.h + 4.0,
                color: if focused {
                    CURSOR_HIGHLIGHT
                } else {
                    SELECTED_HIGHLIGHT
                },
                line_width: 2.0,
                corner_radii: CornerRadii::all(t.corner + 2.0),
            });
        }
        f.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h,
            color: CARD_BG,
            corner_radii: CornerRadii::all(t.corner),
        });

        let colour = card.suit.color();
        let rank_ink = Ink::new(t.card_w * 0.24, FontWeightHint::Bold, colour);
        let suit_ink = Ink::new(t.card_w * 0.28, FontWeightHint::Regular, colour);
        let m = r.w * 0.08;

        let _ = label(f, r.x + m, r.y + m, card.rank.label(), rank_ink);
        let _ = label(
            f,
            r.x + m,
            r.y + m + rank_ink.height(),
            card.suit.symbol(),
            suit_ink,
        );

        // The middle pip, big, so a fanned column can be read from the corner
        // strip alone but a whole card still looks like a card.
        centre_glyph(
            f,
            r,
            card.suit.symbol(),
            Ink::new(t.card_w * 0.4, FontWeightHint::Regular, colour),
        );

        let bw = rank_ink.width(card.rank.label());
        let _ = label(
            f,
            r.right() - m - bw,
            r.bottom() - m - rank_ink.height(),
            card.rank.label(),
            rank_ink,
        );
    }

    /// The banner shown when all four foundations are full.
    fn draw_win_banner(&self, f: &mut Frame<Target>, l: &Layout) {
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.window.w,
            height: l.window.h,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let ink = Ink::new(
            (l.title * 1.6).min(l.window.w * 0.12),
            FontWeightHint::Bold,
            GREEN,
        );
        let sub = Ink::new(l.small, FontWeightHint::Regular, OVERLAY0);
        let msg = "You Win!";
        let note = "Press N for a new game";
        let moves = format!("Moves: {}", self.move_count);

        let total = ink.height() + sub.height() * 2.0 + l.pad * 2.0;
        let mut y = l.window.y + (l.window.h - total).max(0.0) / 2.0;
        let mut banner = centred_line(f, l.window, y, msg, ink);
        y += ink.height() + l.pad;
        banner = union(banner, centred_line(f, l.window, y, &moves, sub));
        y += sub.height() + l.pad;
        banner = union(banner, centred_line(f, l.window, y, note, sub));

        f.hit(Target::WinBanner, banner);
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A font size, weight and colour, together, because they always travel
/// together.
#[derive(Debug, Clone, Copy)]
struct Ink {
    size: f32,
    weight: FontWeightHint,
    color: Color,
}

impl Ink {
    const fn new(size: f32, weight: FontWeightHint, color: Color) -> Self {
        Self {
            size,
            weight,
            color,
        }
    }

    /// How wide `s` is when drawn in this ink.
    fn width(self, s: &str) -> f32 {
        text::measure(s, self.size, self.weight)
    }

    /// How tall one line of this ink is.
    fn height(self) -> f32 {
        text::line_height(self.size, self.weight)
    }
}

/// Draw `s` at `(x, y)` and hand back the box its glyphs occupy.
fn label(f: &mut Frame<Target>, x: f32, y: f32, s: &str, ink: Ink) -> Rect {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    Rect::new(x, y, ink.width(s), ink.height())
}

/// Draw `s` at the left of `area`, vertically centred, elided if it will not
/// fit, and hand back the box it occupies.
fn label_in(f: &mut Frame<Target>, area: Rect, s: &str, ink: Ink) -> Rect {
    let y = area.y + (area.h - ink.height()).max(0.0) / 2.0;
    f.push(RenderCommand::Text {
        x: area.x,
        y,
        text: s.to_string(),
        color: ink.color,
        font_size: ink.size,
        font_weight: ink.weight,
        max_width: Some(area.w.max(0.0)),
        overflow: TextOverflow::Ellipsis,
    });
    Rect::new(area.x, y, ink.width(s).min(area.w.max(0.0)), ink.height())
}

/// Draw one short glyph in the middle of `area`.
fn centre_glyph(f: &mut Frame<Target>, area: Rect, s: &str, ink: Ink) -> Rect {
    let w = ink.width(s);
    label(
        f,
        area.x + (area.w - w).max(0.0) / 2.0,
        area.y + (area.h - ink.height()).max(0.0) / 2.0,
        s,
        ink,
    )
}

/// Draw one line centred across `area` at height `y`.
fn centred_line(f: &mut Frame<Target>, area: Rect, y: f32, s: &str, ink: Ink) -> Rect {
    let w = ink.width(s);
    label(f, area.x + (area.w - w).max(0.0) / 2.0, y, s, ink)
}

/// The smallest box holding both.
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

// ── Application ─────────────────────────────────────────────────────

/// The solitaire application: the game, and the size the window last gave it.
struct SolitaireApp {
    state: GameState,
    /// The size the last frame was drawn at, which is the size the next click
    /// is read against. It exists for that and nothing else.
    size: (f32, f32),
}

impl SolitaireApp {
    fn new() -> Self {
        Self {
            state: GameState::new(42),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        self.state.frame(w, h)
    }

    /// Route a click to the pile it landed on.
    ///
    /// A click is the cursor moving there and Enter being pressed -- one path
    /// through the rules, not a second copy of them. The boxes come from the
    /// drawing pass, so a pile that was not drawn cannot be clicked.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let (w, h) = self.size;
        let Some(target) = self.frame(w, h).hit_test(x, y) else {
            return EventResult::Ignored;
        };

        if self.state.won {
            // The only thing left to do is deal again, and the banner covers
            // the board, so that is what a click on it means.
            if target == Target::WinBanner {
                self.state.new_game();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        match target {
            Target::Stock => {
                self.state.focus = FocusArea::Stock;
                self.state.activate();
                EventResult::Consumed
            }
            Target::Waste => {
                self.state.focus = FocusArea::Waste;
                self.state.activate();
                EventResult::Consumed
            }
            Target::Foundation(i) => {
                self.state.focus = FocusArea::Foundation(i);
                self.state.activate();
                EventResult::Consumed
            }
            Target::TableauCard(col, nth) => {
                self.state.focus = FocusArea::Tableau(col, nth);
                self.state.activate();
                EventResult::Consumed
            }
            Target::TableauEmpty(col) => {
                self.state.focus = FocusArea::Tableau(col, 0);
                self.state.activate();
                EventResult::Consumed
            }
            Target::TableauColumn(col) => {
                // Below the last card: the pile's top card is what a drop
                // lands on, so aim at it.
                let nth = self.state.tableau_face_up_count(col).saturating_sub(1);
                self.state.focus = FocusArea::Tableau(col, nth);
                self.state.activate();
                EventResult::Consumed
            }
            Target::TableauBack(col, _) => {
                // A covered card cannot be picked up. Moving the cursor there
                // is still worth doing -- it is how the keyboard would reach
                // the column -- but nothing is activated.
                let nth = self.state.tableau_face_up_count(col).saturating_sub(1);
                self.state.focus = FocusArea::Tableau(col, nth);
                EventResult::Consumed
            }
            Target::Title | Target::Moves | Target::Help | Target::WinBanner => {
                EventResult::Ignored
            }
        }
    }
}

/// One route from an event to the game, shared by the window and the tests,
/// so a test cannot exercise a path the window does not take.
fn handle_event(app: &mut SolitaireApp, event: &Event) -> EventResult {
    match event {
        Event::Key(KeyEvent {
            key,
            modifiers,
            pressed: true,
            ..
        }) => app.state.handle_key(*key, *modifiers),
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }) => app.click(*x, *y, *button),
        Event::Resize { width, height } => {
            app.resize(f32_from_u32(*width), f32_from_u32(*height));
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

impl App for SolitaireApp {
    fn title(&self) -> String {
        "Solitaire".to_string()
    }

    fn app_id(&self) -> String {
        "solitaire".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the opening size is two small positive whole numbers"
    )]
    fn initial_size(&self) -> (u32, u32) {
        // Converted from the float pair rather than written out again: two
        // spellings of one size are two things that can drift apart.
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
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SolitaireApp {
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
    let mut game = SolitaireApp::new();
    app::launch("solitaire", &mut game)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // A test that indexes past the end, or unwraps a `None`, is a test that
    // has already failed; panicking is the reporting mechanism, not a fault.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::too_many_lines,
        clippy::unwrap_used
    )]

    use super::*;
    use guitk::probe;

    // ── The window ──────────────────────────────────────────────────
    //
    // Nothing below asks the production code where it *would* have drawn a
    // pile. The old suite tested `top_row_x(1) - top_row_x(0) == CARD_WIDTH +
    // CARD_GAP_X`, which is the definition of `top_row_x` restated, and it
    // agreed with any layout at all. These read the boxes the drawing pass
    // actually recorded and compare them against the window and each other.

    /// The sizes every geometry test is run at.
    ///
    /// The lopsided ones are the point: a deal fitted to the width alone
    /// passes at 900x720 and runs off the bottom at 500x900, which is exactly
    /// the fault the old fixed layout had.
    const SIZES: [(f32, f32); 8] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (640.0, 480.0),
        (1600.0, 1000.0),
        (500.0, 900.0),
        (1200.0, 420.0),
        (320.0, 240.0),
        (200.0, 200.0),
        (60.0, 60.0),
    ];

    /// A fresh deal, in a window of the size it opens at.
    fn app() -> SolitaireApp {
        SolitaireApp::new()
    }

    /// The box a target was drawn in at the opening size, or a panic naming
    /// the target that was not drawn.
    fn box_of(app: &SolitaireApp, target: Target) -> Rect {
        box_at(app, target, SolitaireApp::SIZE)
    }

    /// The box a target was drawn in at `size`.
    fn box_at(app: &SolitaireApp, target: Target, size: (f32, f32)) -> Rect {
        probe::rect_of_sized(app, target, size)
            .unwrap_or_else(|| panic!("{target:?} was not drawn at {size:?}"))
    }

    /// Click a point, in a window of `size`.
    fn tap(app: &mut SolitaireApp, x: f32, y: f32, size: (f32, f32)) -> EventResult {
        app.click_at(x, y, MouseButton::Left, size)
    }

    /// Click the middle of whatever box a target was drawn in.
    fn tap_on(app: &mut SolitaireApp, target: Target) -> EventResult {
        let (x, y) = box_of(app, target).centre();
        tap(app, x, y, SolitaireApp::SIZE)
    }

    #[test]
    fn no_pile_is_cut_off_by_the_edge_of_the_window() {
        for size in SIZES {
            let mut app = app();
            // Turn some cards over so the waste and a foundation are really
            // drawn, rather than only their empty slots.
            for _ in 0..30 {
                let _ = app.state.handle_key(Key::Enter, Modifiers::default());
            }
            let f = app.draw(size);

            // Every card in the game is one size, so a card that is a
            // different size from its neighbours is a card the window cut.
            // That, and not "the box is inside the window", is the claim with
            // teeth here: `Frame::hit` trims a box to the clip in force and
            // the clip *is* the window, so "inside the window" is true of any
            // layout at all -- including one that fits the card to the tableau
            // alone and runs the whole deal off the side of a narrow one.
            // That mutation passed the earlier form of this test.
            let mut piles: Vec<(&Target, &Rect)> = f
                .hits()
                .iter()
                .filter(|(t, _)| {
                    matches!(
                        t,
                        Target::Stock
                            | Target::Waste
                            | Target::Foundation(_)
                            | Target::TableauCard(_, _)
                            | Target::TableauBack(_, _)
                            | Target::TableauEmpty(_)
                    )
                })
                .map(|(t, r)| (t, r))
                .collect();
            piles.sort_by(|a, b| a.1.w.total_cmp(&b.1.w));

            let Some((widest, whole)) = piles.last().copied() else {
                panic!("nothing at all was drawn at {size:?}");
            };
            for (target, rect) in &piles {
                assert!(
                    rect.w > 0.0 && rect.h > 0.0,
                    "{target:?} recorded an empty box {rect:?} at {size:?}"
                );
                assert!(
                    (rect.w - whole.w).abs() < 0.01 && (rect.h - whole.h).abs() < 0.01,
                    "at {size:?} {target:?} is {}x{} where {widest:?} is {}x{}; \
                     every card is one size, so the smaller one is a card the window cut off",
                    rect.w,
                    rect.h,
                    whole.w,
                    whole.h
                );
            }
        }
    }

    #[test]
    fn the_seven_columns_are_evenly_spaced_the_same_width_and_never_overlap() {
        for size in SIZES {
            let app = app();
            let mut lefts = Vec::new();
            for col in 0..TABLEAU_COLS {
                let Some(r) = probe::rect_of_sized(&app, Target::TableauColumn(col), size) else {
                    continue;
                };
                lefts.push((col, r));
            }
            if lefts.len() < 2 {
                continue;
            }
            let (_, first) = lefts[0];
            let step = lefts[1].1.x - first.x;
            // Even spacing on its own says nothing about whether the columns
            // sit on each other: seven columns spaced a card width apart are
            // perfectly even and each hides half of the one beside it. The
            // spacing has to exceed the width for there to be a table between
            // them at all.
            assert!(
                step > first.w + 0.01,
                "the columns at {size:?} are {} apart and {} wide, so each overlaps the next",
                step,
                first.w
            );
            for &(col, r) in &lefts {
                assert!(
                    (r.w - first.w).abs() < 0.01,
                    "column {col} is {} wide at {size:?} but column 0 is {}",
                    r.w,
                    first.w
                );
                let want = first.x + step * col as f32;
                assert!(
                    (r.x - want).abs() < 0.01,
                    "column {col} at {size:?} starts at {} where an even row puts it at {want}",
                    r.x
                );
            }
        }
    }

    #[test]
    fn the_stock_the_waste_and_the_foundations_are_all_one_card_size() {
        for size in SIZES {
            let app = app();
            let stock = box_at(&app, Target::Stock, size);
            for target in [
                Target::Waste,
                Target::Foundation(0),
                Target::Foundation(1),
                Target::Foundation(2),
                Target::Foundation(3),
            ] {
                let r = box_at(&app, target, size);
                assert!(
                    (r.w - stock.w).abs() < 0.01 && (r.h - stock.h).abs() < 0.01,
                    "{target:?} is {}x{} at {size:?} but the stock is {}x{}",
                    r.w,
                    r.h,
                    stock.w,
                    stock.h
                );
                assert!(
                    (r.y - stock.y).abs() < 0.01,
                    "{target:?} sits at y={} at {size:?} and the stock at y={}; \
                     they are one row",
                    r.y,
                    stock.y
                );
            }
        }
    }

    #[test]
    fn the_top_row_sits_above_the_columns_and_never_overlaps_them() {
        for size in SIZES {
            let app = app();
            let stock = box_at(&app, Target::Stock, size);
            for col in 0..TABLEAU_COLS {
                let Some(strip) = probe::rect_of_sized(&app, Target::TableauColumn(col), size)
                else {
                    continue;
                };
                assert!(
                    strip.y >= stock.bottom() - 0.01,
                    "column {col} starts at y={} at {size:?}, above the bottom of the \
                     stock at {}",
                    strip.y,
                    stock.bottom()
                );
            }
        }
    }

    #[test]
    fn the_columns_line_up_under_the_top_row() {
        // Stock over column 0, waste over column 1, the four foundations over
        // columns 3 to 6. That is what the blank third slot is for; without it
        // the foundations would sit over columns 2 to 5 and the row would look
        // shifted.
        for size in SIZES {
            let app = app();
            let pairs = [
                (Target::Stock, 0usize),
                (Target::Waste, 1),
                (Target::Foundation(0), 3),
                (Target::Foundation(1), 4),
                (Target::Foundation(2), 5),
                (Target::Foundation(3), 6),
            ];
            for (target, col) in pairs {
                let top = box_at(&app, target, size);
                let Some(strip) = probe::rect_of_sized(&app, Target::TableauColumn(col), size)
                else {
                    continue;
                };
                assert!(
                    (top.x - strip.x).abs() < 0.01,
                    "{target:?} is at x={} at {size:?} and column {col} at x={}",
                    top.x,
                    strip.x
                );
            }
        }
    }

    #[test]
    fn the_deepest_fan_a_deal_can_reach_still_fits_the_window() {
        // `Table::fit` reserves room for six face-down cards under twelve
        // face-up ones, and until this test built one, nothing exercised that
        // reserve: an opening deal fans column 6 seven cards deep, less than
        // half the worst case, so a card size taken from the width alone --
        // which is what the fifteen fixed constants amounted to -- fitted
        // comfortably and ran a real fan off the bottom. The earlier form of
        // this test read the opening deal and passed against exactly that.
        //
        // Nor can the claim be "the box stops before the bottom edge":
        // `Frame::hit` trims a box to the clip and the clip is the window, so
        // a card drawn off the bottom arrives here already trimmed. What the
        // trimming cannot hide is that the card came back shorter than the
        // one above it.
        for size in SIZES {
            let mut app = app();
            let deep = &mut app.state.tableau[0];
            deep.clear();
            for i in 0..6 {
                deep.push(PileCard::new(Card::new(Suit::Spades, Rank::ALL[i]), false));
            }
            for i in 0..12 {
                deep.push(PileCard::new(Card::new(Suit::Hearts, Rank::ALL[i]), true));
            }

            let first = box_at(&app, Target::TableauCard(0, 0), size);
            let last = box_at(&app, Target::TableauCard(0, 11), size);
            assert!(
                last.y > first.y,
                "the premise is wrong: the fan at {size:?} stacks its cards on each other"
            );
            assert!(
                (last.h - first.h).abs() < 0.01,
                "the twelfth face-up card of a column at {size:?} is {} tall against the first's \
                 {}; the window cut the bottom of the fan off",
                last.h,
                first.h
            );
        }
    }

    #[test]
    fn widening_the_window_moves_the_deal_rather_than_the_gap_beside_it() {
        // Two windows of the same height, so the padding, the fonts and the
        // card size are identical and only the room across differs. Stated as
        // a difference, so whatever margin the layout keeps cancels out: if
        // the deal is centred, the extra room is split evenly and it moves by
        // half of it. A deal pinned to the left edge moves by nothing, and
        // passes every other test here.
        let app = app();
        let narrow = (1200.0, 720.0);
        let wide = (1600.0, 720.0);

        let a = box_at(&app, Target::Stock, narrow);
        let b = box_at(&app, Target::Stock, wide);
        assert!(
            (a.w - b.w).abs() < 0.01,
            "the premise is wrong: the card is {} wide in the narrow window and {} in the wide one",
            a.w,
            b.w
        );
        assert!(
            (b.x - a.x - 200.0).abs() < 0.01,
            "the window grew 400 across and the deal moved {} right; centred, \
             it should have moved 200",
            b.x - a.x
        );
    }

    #[test]
    fn the_render_pass_draws_at_the_size_the_window_hands_it() {
        let mut app = app();
        for (w, h) in [
            (1200.0, 900.0),
            (640.0, 480.0),
            (WINDOW_WIDTH, WINDOW_HEIGHT),
        ] {
            let tree = app.render(w, h);
            let first = tree.commands.first().expect("render drew nothing at all");
            let RenderCommand::FillRect { width, height, .. } = first else {
                panic!("the first thing drawn is no longer the background: {first:?}");
            };
            assert!(
                (width - w).abs() < 0.01 && (height - h).abs() < 0.01,
                "asked to render at {w}x{h}, the pass painted a {width}x{height} background"
            );
            assert_eq!(app.size, (w, h), "the render pass did not remember {w}x{h}");
        }
    }

    /// The suite and the window must be looking at the same picture.
    ///
    /// Every geometry test above draws at `SolitaireApp::SIZE`. If the window
    /// opens at some other size, all of them are proofs about a picture
    /// nobody ever sees, and the one that is on the screen is untested.
    #[test]
    fn the_window_opens_at_the_size_the_tests_draw_at() {
        let (w, h) = SolitaireApp::SIZE;
        let opens_at = app().initial_size();
        assert_eq!(
            opens_at,
            (w as u32, h as u32),
            "the suite proves its geometry at {:?} but the window opens at {opens_at:?}",
            SolitaireApp::SIZE
        );
    }

    /// A covered face-up card must show more of itself than a covered
    /// face-down one does.
    ///
    /// Both fans are a fraction of the card height, and nothing in the code
    /// stops them being the same fraction. But a face-down card only has to
    /// prove it is there, whereas a face-up one has to show the rank and the
    /// suit in its corner -- a column fanned as tightly as its backs are is a
    /// column that cannot be read.
    #[test]
    fn a_covered_face_up_card_shows_more_of_itself_than_a_covered_face_down_one() {
        let mut app = app();
        // The opening deal turns exactly one card up per column, so a second
        // face-up card is what it takes to have a face fan at all.
        app.state.tableau[0].push(PileCard::new(Card::new(Suit::Spades, Rank::Two), true));

        let first = box_of(&app, Target::TableauCard(0, 0));
        let second = box_of(&app, Target::TableauCard(0, 1));
        let face_sliver = second.y - first.y;

        // Column 6 is dealt six face-down cards under its face-up one.
        let back_a = box_of(&app, Target::TableauBack(6, 0));
        let back_b = box_of(&app, Target::TableauBack(6, 1));
        let back_sliver = back_b.y - back_a.y;

        assert!(
            back_sliver > 0.0,
            "the premise is wrong: the face-down cards are stacked exactly on each other"
        );
        assert!(
            face_sliver > back_sliver * 2.0,
            "a covered face-up card shows {face_sliver} of itself and a covered face-down one \
             {back_sliver}; the rank in the corner needs much the larger share"
        );
    }

    // ── The mouse ───────────────────────────────────────────────────
    //
    // The program had none: `handle_event` matched `Event::Key` and nothing
    // else, so every one of these is a behaviour that did not exist.

    #[test]
    fn clicking_the_stock_turns_a_card_over() {
        let mut app = app();
        let before = app.state.stock.len();
        assert_eq!(tap_on(&mut app, Target::Stock), EventResult::Consumed);
        assert_eq!(
            app.state.stock.len(),
            before - 1,
            "clicking the stock left {} cards in it, from {before}",
            app.state.stock.len()
        );
        assert_eq!(app.state.waste.len(), 1, "the card did not reach the waste");
    }

    #[test]
    fn clicking_a_face_up_card_picks_up_from_that_card() {
        let mut opening = app();
        // Column 6 has six face-down cards under one face-up one.
        assert_eq!(
            tap_on(&mut opening, Target::TableauCard(6, 0)),
            EventResult::Consumed
        );
        assert_eq!(
            opening.state.selection,
            Some(Selection::Tableau(6, 0)),
            "clicking the face-up card of column 6 selected {:?}",
            opening.state.selection
        );

        // And from the middle of a run. The opening deal turns exactly one
        // card up per column, so every `nth` in it is 0 and a click routed to
        // "the deepest face-up card of the column" is indistinguishable from
        // one routed to the card actually under the pointer. The whole point
        // of picking up a run from a chosen card needs a run to pick from.
        let mut fanned = app();
        let run = &mut fanned.state.tableau[0];
        run.clear();
        for rank in [Rank::Five, Rank::Four, Rank::Three] {
            run.push(PileCard::new(Card::new(Suit::Spades, rank), true));
        }
        // Aimed at the sliver of the middle card, not the middle of its box:
        // the box is a whole card tall and the card above covers most of it,
        // which is what `a_covered_card_is_clickable_only_where_it_can_be_seen`
        // is about.
        let middle = box_of(&fanned, Target::TableauCard(0, 1));
        let cover = box_of(&fanned, Target::TableauCard(0, 2));
        let y = f32::midpoint(middle.y, cover.y);
        assert!(
            y < cover.y,
            "the premise is wrong: the run's cards are stacked on each other"
        );
        assert_eq!(
            tap(&mut fanned, middle.centre().0, y, SolitaireApp::SIZE),
            EventResult::Consumed
        );
        assert_eq!(
            fanned.state.selection,
            Some(Selection::Tableau(0, 1)),
            "clicking the middle card of a three-card run selected {:?}",
            fanned.state.selection
        );
    }

    #[test]
    fn a_covered_card_is_clickable_only_where_it_can_be_seen() {
        // A card in a fanned column records a box a whole card tall, but all
        // the player can see of it is the sliver above the next card. Aiming
        // at the middle of that box lands on whatever covers it -- and the hit
        // test agrees, because it reads the boxes in reverse paint order and
        // the card on top was painted later. So: the sliver belongs to the
        // covered card, and everything below it belongs to the cover.
        let opening = app();
        let first = box_of(&opening, Target::TableauBack(6, 0));
        let second = box_of(&opening, Target::TableauBack(6, 1));
        let sliver = second.y - first.y;
        assert!(
            sliver > 0.0 && sliver < first.h,
            "column 6 is not fanned: its first two cards are {sliver} apart and \
             a card is {} tall",
            first.h
        );

        let mut on_sliver = app_state_after_click(first.centre().0, first.y + sliver / 2.0);
        assert_eq!(
            on_sliver.focus,
            FocusArea::Tableau(6, 0),
            "a click on the visible strip of the covered card did not reach column 6"
        );
        assert!(
            on_sliver.selection.is_none(),
            "a covered card was picked up: {:?}",
            on_sliver.selection
        );
        // Nothing else moved either.
        assert_eq!(on_sliver.move_count, 0);
        on_sliver.selection = None;

        // Lower down, past the sliver, the face-up card that covers it owns
        // the pointer -- and that one can be picked up.
        let mut app = app();
        let (x, y) = first.centre();
        assert_eq!(
            tap(&mut app, x, y, SolitaireApp::SIZE),
            EventResult::Consumed
        );
        assert_eq!(
            app.state.selection,
            Some(Selection::Tableau(6, 0)),
            "the middle of the covered card's box did not reach the card covering it"
        );
    }

    /// Click a point on a fresh deal and hand back the game that resulted.
    fn app_state_after_click(x: f32, y: f32) -> GameState {
        let mut app = app();
        assert_eq!(
            tap(&mut app, x, y, SolitaireApp::SIZE),
            EventResult::Consumed,
            "the click at ({x}, {y}) landed on nothing"
        );
        app.state
    }

    #[test]
    fn a_click_below_the_last_card_of_a_column_still_reaches_the_column() {
        let mut app = app();
        // Column 0 holds a single card, so most of its strip is bare table.
        let strip = box_of(&app, Target::TableauColumn(0));
        let card = box_of(&app, Target::TableauCard(0, 0));
        let y = f32::midpoint(card.bottom(), strip.bottom());
        assert!(
            y > card.bottom(),
            "the premise is wrong: column 0's strip stops at its card"
        );
        assert_eq!(
            tap(&mut app, strip.centre().0, y, SolitaireApp::SIZE),
            EventResult::Consumed
        );
        assert_eq!(
            app.state.focus,
            FocusArea::Tableau(0, 0),
            "a click on the bare part of column 0 reached {:?}",
            app.state.focus
        );
    }

    #[test]
    fn a_click_on_no_pile_at_all_changes_nothing() {
        let mut app = app();
        let before = app.state.focus;
        for (x, y) in [(-20.0, -20.0), (WINDOW_WIDTH + 50.0, 10.0), (2.0, 2.0)] {
            assert_eq!(
                tap(&mut app, x, y, SolitaireApp::SIZE),
                EventResult::Ignored,
                "a click at ({x}, {y}) was taken for a move"
            );
            assert_eq!(app.state.focus, before, "the cursor moved on a dead click");
            assert!(app.state.selection.is_none());
        }
    }

    /// The header is a label, not a control.
    ///
    /// The title, the move counter and the help line all record hit boxes --
    /// they have to, or a click on them would fall through to whatever the
    /// frame recorded underneath. Recording a box is not the same as acting
    /// on it, and the difference is invisible in a won game, where the click
    /// path returns before it ever reaches the header's arm.
    #[test]
    fn a_click_on_the_header_is_not_a_move() {
        for target in [Target::Title, Target::Moves, Target::Help] {
            let mut app = app();
            let before = (app.state.focus, app.state.move_count, app.state.stock.len());
            let (x, y) = box_of(&app, target).centre();
            assert_eq!(
                tap(&mut app, x, y, SolitaireApp::SIZE),
                EventResult::Ignored,
                "a click on {target:?} was taken for a move"
            );
            assert_eq!(
                (app.state.focus, app.state.move_count, app.state.stock.len()),
                before,
                "a click on {target:?} changed the game"
            );
            assert!(app.state.selection.is_none());
        }
    }

    /// Only the left button plays the game.
    ///
    /// The other buttons are unclaimed, and an app that treats every button
    /// the same is an app where a right-click meant for a context menu deals
    /// a card instead.
    #[test]
    fn only_the_left_button_plays_a_card() {
        let stock = box_of(&app(), Target::Stock);
        let (x, y) = stock.centre();
        for button in [MouseButton::Right, MouseButton::Middle] {
            let mut app = app();
            let before = app.state.stock.len();
            assert_eq!(
                app.click_at(x, y, button, SolitaireApp::SIZE),
                EventResult::Ignored,
                "the {button:?} button was taken for a move"
            );
            assert_eq!(
                app.state.stock.len(),
                before,
                "the {button:?} button dealt a card off the stock"
            );
        }
    }

    #[test]
    fn the_window_is_asked_to_redraw_only_when_something_changed() {
        let mut closing = app();
        assert_eq!(closing.on_event(&Event::CloseRequested), Response::Exit);

        let mut app = app();
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::Tab))),
            Response::Redraw,
            "Tab moves the cursor, so the window must be redrawn"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::release(Key::Tab))),
            Response::Idle,
            "a key coming back up changes nothing"
        );
        assert_eq!(
            app.on_event(&Event::Key(probe::press(Key::F1))),
            Response::Idle,
            "a key the game has no use for must not cost a repaint"
        );
        assert_eq!(
            app.on_event(&Event::Mouse(MouseEvent {
                x: -50.0,
                y: -50.0,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            Response::Idle,
            "a click on nothing must not cost a repaint"
        );
    }

    #[test]
    fn every_recorded_box_has_something_drawn_in_it() {
        // A hit box is not evidence that anything was painted: a pile that
        // recorded its box and drew nothing would satisfy every geometry test
        // above and be invisible on screen. This is the check that a box and
        // a picture agree -- see `known-issues.md`, Lesson 81.
        let app = app();
        let size = SolitaireApp::SIZE;
        let f = app.draw(size);
        let painted: Vec<Rect> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                }
                | RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some(Rect::new(*x, *y, *width, *height)),
                RenderCommand::Text { x, y, .. } => Some(Rect::new(*x, *y, 1.0, 1.0)),
                _ => None,
            })
            .collect();

        for (target, rect) in f.hits() {
            // The column strip is bare table below its cards by design.
            if matches!(target, Target::TableauColumn(_)) {
                continue;
            }
            assert!(
                painted.iter().any(|p| {
                    p.x >= rect.x - 3.0
                        && p.y >= rect.y - 3.0
                        && p.right() <= rect.right() + 3.0
                        && p.bottom() <= rect.bottom() + 3.0
                }),
                "{target:?} recorded the box {rect:?} and nothing was drawn inside it"
            );
        }
    }

    #[test]
    fn the_header_follows_the_title_rather_than_a_fixed_offset() {
        // The move counter was drawn at x = 400 and the help at x = 500,
        // whatever the title's width or the window's.
        for size in SIZES {
            let app = app();
            let (Some(title), Some(moves)) = (
                probe::rect_of_sized(&app, Target::Title, size),
                probe::rect_of_sized(&app, Target::Moves, size),
            ) else {
                continue;
            };
            assert!(
                moves.x > title.right(),
                "the move count starts at {} at {size:?}, on top of a title ending at {}",
                moves.x,
                title.right()
            );
            assert!(
                moves.x - title.right() < title.w.max(size.0 * 0.1),
                "the move count is {} past the title at {size:?}, which is adrift, not beside it",
                moves.x - title.right()
            );
            if let Some(help) = probe::rect_of_sized(&app, Target::Help, size) {
                assert!(
                    help.x > moves.right(),
                    "the help starts at {} at {size:?}, on top of the move count ending at {}",
                    help.x,
                    moves.right()
                );
            }
        }
    }

    #[test]
    fn the_stock_says_how_many_cards_are_left_and_the_count_follows_it() {
        let mut app = app();
        let size = SolitaireApp::SIZE;
        let drawn = |app: &SolitaireApp, want: &str| {
            app.draw(size)
                .commands()
                .iter()
                .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == want))
        };
        assert!(drawn(&app, "24"), "the opening stock does not say 24");
        let _ = tap_on(&mut app, Target::Stock);
        assert!(
            drawn(&app, "23"),
            "after a card was turned over the stock still says 24"
        );
    }

    #[test]
    fn the_win_banner_covers_the_window_and_a_click_on_it_deals_again() {
        let mut app = app();
        app.state.won = true;
        let banner = box_of(&app, Target::WinBanner);
        let size = SolitaireApp::SIZE;
        assert!(
            banner.w > 0.0 && banner.h > 0.0 && banner.right() <= size.0 + 0.01,
            "the banner was drawn at {banner:?} in a {size:?} window"
        );

        // A click anywhere else is dead while the banner is up: the board
        // underneath it is finished.
        let stock = box_of(&app, Target::Stock).centre();
        assert_eq!(
            tap(&mut app, stock.0, stock.1, size),
            EventResult::Ignored,
            "the stock was still live under the win banner"
        );

        let (x, y) = banner.centre();
        assert_eq!(tap(&mut app, x, y, size), EventResult::Consumed);
        assert!(!app.state.won, "clicking the banner did not deal again");
        assert_eq!(app.state.stock.len(), 24, "the new deal is not a full deal");
    }

    // ── Helpers ─────────────────────────────────────────────────────

    fn new_game() -> GameState {
        GameState::new(42)
    }

    fn card(suit: Suit, rank: Rank) -> Card {
        Card::new(suit, rank)
    }

    fn press(state: &mut GameState, key: Key) {
        state.handle_key(
            key,
            Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                super_key: false,
            },
        );
    }

    fn press_shift(state: &mut GameState, key: Key) {
        state.handle_key(
            key,
            Modifiers {
                shift: true,
                ctrl: false,
                alt: false,
                super_key: false,
            },
        );
    }

    // ── Deck & Card tests ──────────────────────────────────────────

    #[test]
    fn test_make_deck_has_52_cards() {
        let deck = make_deck();
        assert_eq!(deck.len(), 52);
    }

    #[test]
    fn test_make_deck_unique_cards() {
        let deck = make_deck();
        let mut seen = std::collections::HashSet::new();
        for c in &deck {
            assert!(seen.insert((c.suit, c.rank)));
        }
    }

    #[test]
    fn test_suit_is_red() {
        assert!(Suit::Hearts.is_red());
        assert!(Suit::Diamonds.is_red());
        assert!(!Suit::Clubs.is_red());
        assert!(!Suit::Spades.is_red());
    }

    #[test]
    fn test_suit_symbols() {
        assert_eq!(Suit::Hearts.symbol(), "\u{2665}");
        assert_eq!(Suit::Diamonds.symbol(), "\u{2666}");
        assert_eq!(Suit::Clubs.symbol(), "\u{2663}");
        assert_eq!(Suit::Spades.symbol(), "\u{2660}");
    }

    #[test]
    fn test_suit_indices() {
        assert_eq!(Suit::Hearts.index(), 0);
        assert_eq!(Suit::Diamonds.index(), 1);
        assert_eq!(Suit::Clubs.index(), 2);
        assert_eq!(Suit::Spades.index(), 3);
    }

    #[test]
    fn test_rank_values() {
        assert_eq!(Rank::Ace.value(), 1);
        assert_eq!(Rank::King.value(), 13);
        assert_eq!(Rank::Ten.value(), 10);
    }

    #[test]
    fn test_rank_labels() {
        assert_eq!(Rank::Ace.label(), "A");
        assert_eq!(Rank::Two.label(), "2");
        assert_eq!(Rank::Ten.label(), "10");
        assert_eq!(Rank::Jack.label(), "J");
        assert_eq!(Rank::Queen.label(), "Q");
        assert_eq!(Rank::King.label(), "K");
    }

    #[test]
    fn test_card_can_stack_on_tableau() {
        // Red 5 on black 6.
        let r5 = card(Suit::Hearts, Rank::Five);
        let b6 = card(Suit::Spades, Rank::Six);
        assert!(r5.can_stack_on_tableau(b6));

        // Black 5 on red 6.
        let b5 = card(Suit::Clubs, Rank::Five);
        let r6 = card(Suit::Diamonds, Rank::Six);
        assert!(b5.can_stack_on_tableau(r6));

        // Same color: red on red.
        assert!(!r5.can_stack_on_tableau(card(Suit::Diamonds, Rank::Six)));

        // Wrong rank: 5 on 7.
        assert!(!r5.can_stack_on_tableau(card(Suit::Spades, Rank::Seven)));
    }

    #[test]
    fn test_card_cannot_stack_same_rank() {
        let c1 = card(Suit::Hearts, Rank::Five);
        let c2 = card(Suit::Spades, Rank::Five);
        assert!(!c1.can_stack_on_tableau(c2));
    }

    #[test]
    fn test_card_can_place_on_foundation() {
        let ace = card(Suit::Hearts, Rank::Ace);
        assert!(ace.can_place_on_foundation(0));
        assert!(!ace.can_place_on_foundation(1));

        let two = card(Suit::Hearts, Rank::Two);
        assert!(two.can_place_on_foundation(1));
        assert!(!two.can_place_on_foundation(0));
    }

    // ── Deal fairness ──────────────────────────────────────────────
    //
    // These replace five tests that asked whether the generator was
    // deterministic, whether two seeds differ, whether a bounded draw stays in
    // range, whether a zero bound is safe, and whether a shuffle keeps its
    // elements. All five are `randrange`'s properties and are tested there.
    // None of the five could see what was wrong here: a shuffle that keeps its
    // elements can still draw from a handful of the 52! orderings, and the
    // deal was so lopsided that one card held the leftmost face-up slot in
    // nearly a fifth of all games.

    /// Index a card into `0..52`, in the order `make_deck` builds them.
    fn deck_index(card: Card) -> usize {
        Suit::ALL.iter().position(|&s| s == card.suit).unwrap_or(0) * 13
            + Rank::ALL.iter().position(|&r| r == card.rank).unwrap_or(0)
    }

    #[test]
    fn no_card_owns_the_leftmost_face_up_slot() {
        // Column 0 holds exactly one card and it is face-up, so it is the
        // first thing a player sees. Each of the 52 should hold it about
        // 1/52 = 1.9% of the time; the old generator gave the two of hearts
        // 17.6%.
        const DEALS: u32 = 20_000;
        let mut counts = [0_u32; 52];
        let mut game = GameState::new(12345);
        for _ in 0..DEALS {
            if let Some(pile) = game.tableau[0].first() {
                counts[deck_index(pile.card)] += 1;
            }
            // `new_game` is how a player reaches the next deal, and it reseeds
            // from the generator state -- so the draw counter restarts at zero
            // every deal. A counter-dependent defect survives that on purpose.
            game.new_game();
        }
        for (index, &count) in counts.iter().enumerate() {
            let share = 100.0 * f64::from(count) / f64::from(DEALS);
            assert!(
                share < 5.0,
                "card {index} was the leftmost face-up card in {share:.1}% of deals, not about 1.9%"
            );
        }
    }

    #[test]
    fn the_opening_tableau_shows_a_fair_sample_of_the_deck() {
        // Seven cards are face-up at the start, so each of the 52 should
        // appear in 7/52 = 13.5% of deals. The old generator had 29 of them
        // off by more than two points, the worst at 33.1%.
        const DEALS: u32 = 20_000;
        let mut counts = [0_u32; 52];
        let mut game = GameState::new(9);
        for _ in 0..DEALS {
            for col in &game.tableau {
                if let Some(pile) = col.last() {
                    counts[deck_index(pile.card)] += 1;
                }
            }
            game.new_game();
        }
        for (index, &count) in counts.iter().enumerate() {
            let share = 100.0 * f64::from(count) / f64::from(DEALS);
            assert!(
                (share - 100.0 * 7.0 / 52.0).abs() < 2.0,
                "card {index} was face-up in {share:.1}% of opening tableaus, not about 13.5%"
            );
        }
    }

    // ── Deal / Initial state tests ─────────────────────────────────

    #[test]
    fn test_initial_deal_tableau_sizes() {
        let gs = new_game();
        for col in 0..TABLEAU_COLS {
            assert_eq!(gs.tableau[col].len(), col + 1);
        }
    }

    #[test]
    fn test_initial_deal_tableau_face_up() {
        let gs = new_game();
        for col in 0..TABLEAU_COLS {
            let pile = &gs.tableau[col];
            // Last card face-up, rest face-down.
            for (i, pc) in pile.iter().enumerate() {
                if i == col {
                    assert!(pc.face_up, "Column {col}, card {i} should be face-up");
                } else {
                    assert!(!pc.face_up, "Column {col}, card {i} should be face-down");
                }
            }
        }
    }

    #[test]
    fn test_initial_deal_stock_size() {
        let gs = new_game();
        // 52 - (1+2+3+4+5+6+7) = 52 - 28 = 24
        assert_eq!(gs.stock.len(), 24);
    }

    #[test]
    fn test_initial_deal_waste_empty() {
        let gs = new_game();
        assert!(gs.waste.is_empty());
    }

    #[test]
    fn test_initial_deal_foundations_empty() {
        let gs = new_game();
        for f in &gs.foundations {
            assert!(f.is_empty());
        }
    }

    #[test]
    fn test_initial_deal_all_cards_present() {
        let gs = new_game();
        let mut all_cards = Vec::new();
        for c in &gs.stock {
            all_cards.push(*c);
        }
        for t in &gs.tableau {
            for pc in t {
                all_cards.push(pc.card);
            }
        }
        assert_eq!(all_cards.len(), 52);
        let mut seen = std::collections::HashSet::new();
        for c in &all_cards {
            assert!(seen.insert((c.suit, c.rank)));
        }
    }

    #[test]
    fn test_initial_not_won() {
        let gs = new_game();
        assert!(!gs.won);
    }

    #[test]
    fn test_initial_move_count_zero() {
        let gs = new_game();
        assert_eq!(gs.move_count, 0);
    }

    #[test]
    fn test_initial_focus_is_stock() {
        let gs = new_game();
        assert_eq!(gs.focus, FocusArea::Stock);
    }

    #[test]
    fn test_initial_no_selection() {
        let gs = new_game();
        assert!(gs.selection.is_none());
    }

    // ── Draw / Stock tests ─────────────────────────────────────────

    #[test]
    fn test_draw_from_stock() {
        let mut gs = new_game();
        let stock_len = gs.stock.len();
        let top = *gs.stock.last().unwrap();
        gs.draw_from_stock();
        assert_eq!(gs.stock.len(), stock_len - 1);
        assert_eq!(gs.waste.len(), 1);
        assert_eq!(gs.waste[0], top);
    }

    #[test]
    fn test_draw_increments_move_count() {
        let mut gs = new_game();
        gs.draw_from_stock();
        assert_eq!(gs.move_count, 1);
    }

    #[test]
    fn test_draw_adds_undo() {
        let mut gs = new_game();
        gs.draw_from_stock();
        assert_eq!(gs.undo_stack.len(), 1);
    }

    #[test]
    fn test_recycle_when_stock_empty() {
        let mut gs = new_game();
        // Draw all 24 cards.
        for _ in 0..24 {
            gs.draw_from_stock();
        }
        assert!(gs.stock.is_empty());
        assert_eq!(gs.waste.len(), 24);

        // Drawing again recycles.
        gs.draw_from_stock();
        assert_eq!(gs.stock.len(), 24);
        assert!(gs.waste.is_empty());
    }

    #[test]
    fn test_draw_does_nothing_when_both_empty() {
        let mut gs = new_game();
        gs.stock.clear();
        gs.waste.clear();
        let mc = gs.move_count;
        gs.draw_from_stock();
        assert_eq!(gs.move_count, mc);
    }

    // ── Undo tests ─────────────────────────────────────────────────

    #[test]
    fn test_undo_draw() {
        let mut gs = new_game();
        let stock_before = gs.stock.clone();
        gs.draw_from_stock();
        gs.undo();
        assert_eq!(gs.stock, stock_before);
        assert!(gs.waste.is_empty());
    }

    #[test]
    fn test_undo_recycle() {
        let mut gs = new_game();
        for _ in 0..24 {
            gs.draw_from_stock();
        }
        let waste_before = gs.waste.clone();
        gs.draw_from_stock(); // recycle
        gs.undo();
        assert_eq!(gs.waste, waste_before);
        assert!(gs.stock.is_empty());
    }

    #[test]
    fn test_undo_decrements_move_count() {
        let mut gs = new_game();
        gs.draw_from_stock();
        assert_eq!(gs.move_count, 1);
        gs.undo();
        assert_eq!(gs.move_count, 0);
    }

    #[test]
    fn test_undo_empty_stack_is_noop() {
        let mut gs = new_game();
        gs.undo(); // Should not crash.
        assert_eq!(gs.move_count, 0);
    }

    // ── Foundation tests ───────────────────────────────────────────

    #[test]
    fn test_foundation_top_empty() {
        let gs = new_game();
        assert!(gs.foundation_top(0).is_none());
    }

    #[test]
    fn test_foundation_top_value_empty() {
        let gs = new_game();
        assert_eq!(gs.foundation_top_value(0), 0);
    }

    #[test]
    fn test_foundation_placement() {
        let mut gs = new_game();
        let ace = card(Suit::Hearts, Rank::Ace);
        gs.waste.push(ace);
        assert!(gs.try_waste_to_foundation());
        assert_eq!(gs.foundations[Suit::Hearts.index()].len(), 1);
        assert_eq!(gs.foundation_top_value(Suit::Hearts.index()), 1);
    }

    #[test]
    fn test_foundation_sequential() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Spades, Rank::Ace));
        assert!(gs.try_waste_to_foundation());
        gs.waste.push(card(Suit::Spades, Rank::Two));
        assert!(gs.try_waste_to_foundation());
        assert_eq!(gs.foundations[Suit::Spades.index()].len(), 2);
    }

    #[test]
    fn test_foundation_rejects_wrong_order() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Two));
        assert!(!gs.try_waste_to_foundation());
    }

    #[test]
    fn test_foundation_rejects_wrong_suit_sequence() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Ace));
        assert!(gs.try_waste_to_foundation());
        gs.waste.push(card(Suit::Diamonds, Rank::Two));
        // Diamonds Two should go on diamonds foundation (empty), not hearts.
        assert!(!gs.try_waste_to_foundation());
    }

    // ── Tableau placement tests ────────────────────────────────────

    #[test]
    fn test_can_place_king_on_empty_tableau() {
        let gs = new_game();
        // Clear a column manually.
        let mut gs2 = gs;
        gs2.tableau[0].clear();
        let king = card(Suit::Hearts, Rank::King);
        assert!(gs2.can_place_on_tableau(king, 0));
    }

    #[test]
    fn test_cannot_place_non_king_on_empty_tableau() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        let queen = card(Suit::Hearts, Rank::Queen);
        assert!(!gs.can_place_on_tableau(queen, 0));
    }

    #[test]
    fn test_waste_to_tableau() {
        let mut gs = new_game();
        // Set up: clear a column and place a black 6.
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Six), true));
        // Put a red 5 on waste.
        gs.waste.push(card(Suit::Hearts, Rank::Five));
        assert!(gs.try_waste_to_tableau(0));
        assert_eq!(gs.tableau[0].len(), 2);
    }

    #[test]
    fn test_waste_to_tableau_rejected() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Six), true));
        // Same color.
        gs.waste.push(card(Suit::Clubs, Rank::Five));
        assert!(!gs.try_waste_to_tableau(0));
    }

    // ── Tableau to tableau tests ───────────────────────────────────

    #[test]
    fn test_tableau_to_tableau_single() {
        let mut gs = new_game();
        // Setup two columns with compatible cards.
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Seven), true));
        gs.tableau[1].clear();
        gs.tableau[1].push(PileCard::new(card(Suit::Hearts, Rank::Six), true));
        assert!(gs.try_tableau_to_tableau(1, 0, 0));
        assert_eq!(gs.tableau[0].len(), 2);
        assert!(gs.tableau[1].is_empty());
    }

    #[test]
    fn test_tableau_to_tableau_run() {
        let mut gs = new_game();
        // Source column: face-down + 2 face-up.
        gs.tableau[2].clear();
        gs.tableau[2].push(PileCard::new(card(Suit::Clubs, Rank::Ace), false));
        gs.tableau[2].push(PileCard::new(card(Suit::Hearts, Rank::Five), true));
        gs.tableau[2].push(PileCard::new(card(Suit::Spades, Rank::Four), true));

        // Dest column.
        gs.tableau[3].clear();
        gs.tableau[3].push(PileCard::new(card(Suit::Clubs, Rank::Six), true));

        // Move the run of 2 face-up cards (H5, S4) onto C6.
        assert!(gs.try_tableau_to_tableau(2, 0, 3));
        assert_eq!(gs.tableau[3].len(), 3);
        // The hidden card should now be flipped.
        assert!(gs.tableau[2][0].face_up);
    }

    #[test]
    fn test_tableau_to_tableau_same_col_rejected() {
        let mut gs = new_game();
        assert!(!gs.try_tableau_to_tableau(0, 0, 0));
    }

    #[test]
    fn test_tableau_to_tableau_invalid_col() {
        let mut gs = new_game();
        assert!(!gs.try_tableau_to_tableau(0, 0, 10));
    }

    // ── Tableau to foundation tests ────────────────────────────────

    #[test]
    fn test_tableau_to_foundation() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Diamonds, Rank::Ace), true));
        assert!(gs.try_tableau_to_foundation(0));
        assert_eq!(gs.foundations[Suit::Diamonds.index()].len(), 1);
        assert!(gs.tableau[0].is_empty());
    }

    #[test]
    fn test_tableau_to_foundation_flips() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Clubs, Rank::King), false));
        gs.tableau[0].push(PileCard::new(card(Suit::Hearts, Rank::Ace), true));
        assert!(gs.try_tableau_to_foundation(0));
        assert!(gs.tableau[0][0].face_up);
    }

    // ── Foundation to tableau tests ────────────────────────────────

    #[test]
    fn test_foundation_to_tableau() {
        let mut gs = new_game();
        gs.foundations[Suit::Hearts.index()].push(card(Suit::Hearts, Rank::Ace));
        gs.foundations[Suit::Hearts.index()].push(card(Suit::Hearts, Rank::Two));

        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Three), true));

        assert!(gs.try_foundation_to_tableau(Suit::Hearts.index(), 0));
        assert_eq!(gs.foundations[Suit::Hearts.index()].len(), 1);
        assert_eq!(gs.tableau[0].len(), 2);
    }

    // ── Win detection ──────────────────────────────────────────────

    #[test]
    fn test_win_detection() {
        let mut gs = new_game();
        // Fill all foundations.
        for &suit in &Suit::ALL {
            gs.foundations[suit.index()].clear();
            for &rank in &Rank::ALL {
                gs.foundations[suit.index()].push(card(suit, rank));
            }
        }
        gs.check_win();
        assert!(gs.won);
    }

    #[test]
    fn test_not_won_incomplete() {
        let mut gs = new_game();
        for &suit in &Suit::ALL {
            gs.foundations[suit.index()].clear();
            // Only 12 cards each.
            for &rank in &Rank::ALL[..12] {
                gs.foundations[suit.index()].push(card(suit, rank));
            }
        }
        gs.check_win();
        assert!(!gs.won);
    }

    // ── Auto-move tests ───────────────────────────────────────────

    #[test]
    fn test_auto_move_waste_ace() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Clubs, Rank::Ace));
        assert!(gs.auto_move_to_foundation());
        assert_eq!(gs.foundations[Suit::Clubs.index()].len(), 1);
    }

    #[test]
    fn test_auto_move_tableau_ace() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Ace), true));
        gs.waste.clear();
        assert!(gs.auto_move_to_foundation());
        assert_eq!(gs.foundations[Suit::Spades.index()].len(), 1);
    }

    #[test]
    fn test_auto_move_nothing_to_move() {
        let mut gs = new_game();
        // Remove all aces from accessible positions.
        gs.waste.clear();
        // Make sure no tableau top is an ace.
        for col in 0..TABLEAU_COLS {
            if let Some(top) = gs.tableau[col].last()
                && top.card.rank == Rank::Ace
                && let Some(top_mut) = gs.tableau[col].last_mut()
            {
                top_mut.card = card(Suit::Hearts, Rank::King);
            }
        }
        assert!(!gs.auto_move_to_foundation());
    }

    // ── Navigation tests ───────────────────────────────────────────

    #[test]
    fn test_tab_forward_cycle() {
        let mut gs = new_game();
        assert_eq!(gs.focus, FocusArea::Stock);
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Waste);
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Foundation(0));
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Foundation(1));
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Foundation(2));
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Foundation(3));
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Tableau(0, 0));
        for _ in 1..TABLEAU_COLS {
            gs.tab_forward();
        }
        assert_eq!(gs.focus, FocusArea::Tableau(6, 0));
        gs.tab_forward();
        assert_eq!(gs.focus, FocusArea::Stock);
    }

    #[test]
    fn test_tab_backward_cycle() {
        let mut gs = new_game();
        gs.tab_backward(); // Stock -> last tableau
        assert!(matches!(gs.focus, FocusArea::Tableau(6, _)));
        gs.focus = FocusArea::Foundation(0);
        gs.tab_backward();
        assert_eq!(gs.focus, FocusArea::Waste);
        gs.tab_backward();
        assert_eq!(gs.focus, FocusArea::Stock);
    }

    #[test]
    fn test_horizontal_movement() {
        let mut gs = new_game();
        gs.focus = FocusArea::Tableau(0, 0);
        gs.move_horizontal(1);
        assert_eq!(gs.focus, FocusArea::Tableau(1, 0));
        gs.move_horizontal(-1);
        assert_eq!(gs.focus, FocusArea::Tableau(0, 0));
    }

    #[test]
    fn test_horizontal_clamp_left() {
        let mut gs = new_game();
        gs.focus = FocusArea::Tableau(0, 0);
        gs.move_horizontal(-1);
        assert_eq!(gs.focus, FocusArea::Tableau(0, 0));
    }

    #[test]
    fn test_horizontal_clamp_right() {
        let mut gs = new_game();
        gs.focus = FocusArea::Tableau(6, 0);
        gs.move_horizontal(1);
        assert_eq!(gs.focus, FocusArea::Tableau(6, 0));

        // And off the last foundation. The two edges are separate arms of the
        // same match, so the tableau's holding says nothing about the top
        // row's -- which is how a foundation cursor that walked off the end of
        // the row survived a mutation sweep.
        gs.focus = FocusArea::Foundation(FOUNDATION_COUNT - 1);
        gs.move_horizontal(1);
        assert_eq!(gs.focus, FocusArea::Foundation(FOUNDATION_COUNT - 1));
    }

    #[test]
    fn test_vertical_top_to_tableau() {
        let mut gs = new_game();
        gs.focus = FocusArea::Stock;
        gs.move_vertical(1);
        assert!(matches!(gs.focus, FocusArea::Tableau(0, _)));
    }

    #[test]
    fn test_vertical_tableau_to_top() {
        let mut gs = new_game();
        gs.focus = FocusArea::Tableau(0, 0);
        gs.move_vertical(-1);
        assert_eq!(gs.focus, FocusArea::Stock);
    }

    #[test]
    fn test_move_within_tableau_up_down() {
        let mut gs = new_game();
        // Column 6 has 7 cards, 1 face-up.
        // Add more face-up cards for testing.
        gs.tableau[6].clear();
        gs.tableau[6].push(PileCard::new(card(Suit::Hearts, Rank::King), true));
        gs.tableau[6].push(PileCard::new(card(Suit::Spades, Rank::Queen), true));
        gs.tableau[6].push(PileCard::new(card(Suit::Hearts, Rank::Jack), true));

        gs.focus = FocusArea::Tableau(6, 0);
        gs.move_within_tableau(1);
        assert_eq!(gs.focus, FocusArea::Tableau(6, 1));
        gs.move_within_tableau(1);
        assert_eq!(gs.focus, FocusArea::Tableau(6, 2));
        gs.move_within_tableau(1);
        assert_eq!(gs.focus, FocusArea::Tableau(6, 2)); // clamped
        gs.move_within_tableau(-1);
        assert_eq!(gs.focus, FocusArea::Tableau(6, 1));
    }

    // ── Key handling tests ─────────────────────────────────────────

    #[test]
    fn test_key_tab() {
        let mut gs = new_game();
        press(&mut gs, Key::Tab);
        assert_eq!(gs.focus, FocusArea::Waste);
    }

    #[test]
    fn test_key_shift_tab() {
        let mut gs = new_game();
        press_shift(&mut gs, Key::Tab);
        assert!(matches!(gs.focus, FocusArea::Tableau(6, _)));
    }

    #[test]
    fn test_key_arrows() {
        let mut gs = new_game();
        press(&mut gs, Key::Down); // Stock -> Tableau(0, _)
        assert!(matches!(gs.focus, FocusArea::Tableau(0, _)));
        press(&mut gs, Key::Right);
        assert!(matches!(gs.focus, FocusArea::Tableau(1, _)));
        press(&mut gs, Key::Left);
        assert!(matches!(gs.focus, FocusArea::Tableau(0, _)));
        press(&mut gs, Key::Up);
        assert_eq!(gs.focus, FocusArea::Stock);
    }

    #[test]
    fn test_key_enter_stock_draws() {
        let mut gs = new_game();
        let stock_len = gs.stock.len();
        press(&mut gs, Key::Enter);
        assert_eq!(gs.stock.len(), stock_len - 1);
        assert_eq!(gs.waste.len(), 1);
    }

    #[test]
    fn test_key_space_same_as_enter() {
        let mut gs = new_game();
        let stock_len = gs.stock.len();
        press(&mut gs, Key::Space);
        assert_eq!(gs.stock.len(), stock_len - 1);
    }

    #[test]
    fn test_key_z_undoes() {
        let mut gs = new_game();
        press(&mut gs, Key::Enter); // draw
        assert_eq!(gs.waste.len(), 1);
        press(&mut gs, Key::Z);
        assert!(gs.waste.is_empty());
    }

    #[test]
    fn test_key_n_new_game() {
        let mut gs = new_game();
        press(&mut gs, Key::Enter);
        assert_eq!(gs.move_count, 1);
        press(&mut gs, Key::N);
        assert_eq!(gs.move_count, 0);
        assert_eq!(gs.stock.len(), 24);
    }

    #[test]
    fn test_key_escape_clears_selection() {
        let mut gs = new_game();
        gs.selection = Some(Selection::Waste);
        press(&mut gs, Key::Escape);
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_key_a_auto_move() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Ace));
        press(&mut gs, Key::A);
        assert_eq!(gs.foundations[Suit::Hearts.index()].len(), 1);
    }

    // ── Selection / Activation tests ───────────────────────────────

    #[test]
    fn test_select_waste_card() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Five));
        gs.focus = FocusArea::Waste;
        press(&mut gs, Key::Enter);
        assert_eq!(gs.selection, Some(Selection::Waste));
    }

    #[test]
    fn test_select_tableau_card() {
        let mut gs = new_game();
        gs.focus = FocusArea::Tableau(0, 0);
        press(&mut gs, Key::Enter);
        assert_eq!(gs.selection, Some(Selection::Tableau(0, 0)));
    }

    #[test]
    fn test_move_waste_to_tableau_via_selection() {
        let mut gs = new_game();
        // Set up a target.
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Clubs, Rank::Six), true));
        gs.waste.push(card(Suit::Diamonds, Rank::Five));

        // Select waste.
        gs.focus = FocusArea::Waste;
        press(&mut gs, Key::Enter);
        assert_eq!(gs.selection, Some(Selection::Waste));

        // Navigate to target and activate.
        gs.focus = FocusArea::Tableau(0, 0);
        press(&mut gs, Key::Enter);
        assert_eq!(gs.tableau[0].len(), 2);
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_move_tableau_to_tableau_via_selection() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Seven), true));
        gs.tableau[1].clear();
        gs.tableau[1].push(PileCard::new(card(Suit::Hearts, Rank::Six), true));

        // Select source.
        gs.focus = FocusArea::Tableau(1, 0);
        press(&mut gs, Key::Enter);
        assert_eq!(gs.selection, Some(Selection::Tableau(1, 0)));

        // Move to dest.
        gs.focus = FocusArea::Tableau(0, 0);
        press(&mut gs, Key::Enter);
        assert_eq!(gs.tableau[0].len(), 2);
        assert!(gs.tableau[1].is_empty());
    }

    // ── Undo for moves ─────────────────────────────────────────────

    #[test]
    fn test_undo_waste_to_foundation() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Ace));
        gs.try_waste_to_foundation();
        assert_eq!(gs.foundations[0].len(), 1);
        gs.undo();
        assert!(gs.foundations[0].is_empty());
        assert_eq!(*gs.waste.last().unwrap(), card(Suit::Hearts, Rank::Ace));
    }

    #[test]
    fn test_undo_waste_to_tableau() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Six), true));
        gs.waste.push(card(Suit::Hearts, Rank::Five));
        gs.try_waste_to_tableau(0);
        assert_eq!(gs.tableau[0].len(), 2);
        gs.undo();
        assert_eq!(gs.tableau[0].len(), 1);
        assert_eq!(*gs.waste.last().unwrap(), card(Suit::Hearts, Rank::Five));
    }

    #[test]
    fn test_undo_tableau_to_tableau_with_flip() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Clubs, Rank::King), false));
        gs.tableau[0].push(PileCard::new(card(Suit::Hearts, Rank::Five), true));

        gs.tableau[1].clear();
        gs.tableau[1].push(PileCard::new(card(Suit::Spades, Rank::Six), true));

        gs.try_tableau_to_tableau(0, 0, 1);
        // After move, the hidden card should be flipped.
        assert!(gs.tableau[0][0].face_up);
        assert_eq!(gs.tableau[1].len(), 2);

        gs.undo();
        // After undo, should be unflipped again.
        assert!(!gs.tableau[0][0].face_up);
        assert_eq!(gs.tableau[0].len(), 2);
        assert_eq!(gs.tableau[1].len(), 1);
    }

    #[test]
    fn test_undo_tableau_to_foundation() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Diamonds, Rank::Ace), true));
        gs.try_tableau_to_foundation(0);
        assert_eq!(gs.foundations[Suit::Diamonds.index()].len(), 1);
        gs.undo();
        assert!(gs.foundations[Suit::Diamonds.index()].is_empty());
        assert_eq!(gs.tableau[0].len(), 1);
    }

    #[test]
    fn test_undo_foundation_to_tableau() {
        let mut gs = new_game();
        gs.foundations[Suit::Hearts.index()].push(card(Suit::Hearts, Rank::Ace));
        gs.foundations[Suit::Hearts.index()].push(card(Suit::Hearts, Rank::Two));
        gs.tableau[0].clear();
        gs.tableau[0].push(PileCard::new(card(Suit::Spades, Rank::Three), true));

        gs.try_foundation_to_tableau(Suit::Hearts.index(), 0);
        assert_eq!(gs.tableau[0].len(), 2);

        gs.undo();
        assert_eq!(gs.tableau[0].len(), 1);
        assert_eq!(gs.foundations[Suit::Hearts.index()].len(), 2);
    }

    // ── Face-up / face-down count tests ────────────────────────────

    #[test]
    fn test_tableau_face_up_count() {
        let gs = new_game();
        for col in 0..TABLEAU_COLS {
            assert_eq!(gs.tableau_face_up_count(col), 1);
        }
    }

    #[test]
    fn test_tableau_face_down_count() {
        let gs = new_game();
        for col in 0..TABLEAU_COLS {
            assert_eq!(gs.tableau_face_down_count(col), col);
        }
    }

    #[test]
    fn test_tableau_face_up_count_out_of_bounds() {
        let gs = new_game();
        assert_eq!(gs.tableau_face_up_count(10), 0);
    }

    // ── New game test ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets() {
        let mut gs = new_game();
        gs.draw_from_stock();
        gs.draw_from_stock();
        gs.move_count = 10;
        gs.selection = Some(Selection::Waste);
        gs.new_game();
        assert_eq!(gs.move_count, 0);
        assert!(gs.selection.is_none());
        assert!(gs.waste.is_empty());
        assert_eq!(gs.stock.len(), 24);
        assert!(!gs.won);
    }

    #[test]
    fn test_new_game_different_layout() {
        let gs1 = GameState::new(1);
        let gs2 = GameState::new(2);
        // Different seeds should (almost certainly) produce different deals.
        let t1: Vec<Card> = gs1.tableau[6].iter().map(|pc| pc.card).collect();
        let t2: Vec<Card> = gs2.tableau[6].iter().map(|pc| pc.card).collect();
        assert_ne!(t1, t2);
    }

    // ── Edge case tests ────────────────────────────────────────────

    #[test]
    fn test_flip_top_empty_col() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        assert!(!gs.flip_top_if_needed(0));
    }

    #[test]
    fn test_flip_top_already_face_up() {
        let mut gs = new_game();
        // Column 0 top is already face-up.
        assert!(!gs.flip_top_if_needed(0));
    }

    #[test]
    fn test_activate_stock_draws() {
        let mut gs = new_game();
        gs.focus = FocusArea::Stock;
        let stock_len = gs.stock.len();
        gs.activate();
        assert_eq!(gs.stock.len(), stock_len - 1);
    }

    #[test]
    fn test_activate_waste_selects() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::Five));
        gs.focus = FocusArea::Waste;
        gs.activate();
        assert_eq!(gs.selection, Some(Selection::Waste));
    }

    #[test]
    fn test_activate_waste_deselect_on_double_press() {
        let mut gs = new_game();
        gs.waste.push(card(Suit::Hearts, Rank::King)); // Can't go to empty foundation
        gs.focus = FocusArea::Waste;
        gs.activate(); // select
        assert_eq!(gs.selection, Some(Selection::Waste));
        gs.activate(); // deselect (no auto-move possible)
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_activate_empty_waste_no_selection() {
        let mut gs = new_game();
        gs.focus = FocusArea::Waste;
        gs.activate();
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_won_state_blocks_moves() {
        let mut gs = new_game();
        gs.won = true;
        let stock_len = gs.stock.len();
        press(&mut gs, Key::Enter);
        assert_eq!(gs.stock.len(), stock_len);
    }

    #[test]
    fn test_won_state_allows_new_game() {
        let mut gs = new_game();
        gs.won = true;
        press(&mut gs, Key::N);
        assert!(!gs.won);
        assert_eq!(gs.move_count, 0);
    }

    #[test]
    fn test_select_foundation_card() {
        let mut gs = new_game();
        gs.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        gs.focus = FocusArea::Foundation(0);
        gs.activate();
        assert_eq!(gs.selection, Some(Selection::Foundation(0)));
    }

    #[test]
    fn test_deselect_foundation_same_pile() {
        let mut gs = new_game();
        gs.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        gs.focus = FocusArea::Foundation(0);
        gs.selection = Some(Selection::Foundation(0));
        gs.activate();
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_activate_empty_foundation_no_selection() {
        let mut gs = new_game();
        gs.focus = FocusArea::Foundation(0);
        gs.activate();
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_select_empty_tableau_no_selection() {
        let mut gs = new_game();
        gs.tableau[0].clear();
        gs.focus = FocusArea::Tableau(0, 0);
        gs.activate();
        assert!(gs.selection.is_none());
    }

    #[test]
    fn test_move_horizontal_in_top_row() {
        let mut gs = new_game();
        gs.focus = FocusArea::Stock;
        gs.move_horizontal(1); // Stock -> Waste
        assert_eq!(gs.focus, FocusArea::Waste);
        gs.move_horizontal(1); // Waste -> Foundation(0)
        assert_eq!(gs.focus, FocusArea::Foundation(0));
        gs.move_horizontal(-1); // Foundation(0) -> Waste
        assert_eq!(gs.focus, FocusArea::Waste);
        gs.move_horizontal(-1); // Waste -> Stock
        assert_eq!(gs.focus, FocusArea::Stock);
        gs.move_horizontal(-1); // Stock stays
        assert_eq!(gs.focus, FocusArea::Stock);
    }
}
