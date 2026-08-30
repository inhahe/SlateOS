//! Slate OS FreeCell -- classic FreeCell card game.
//!
//! Standard 52-card deck dealt across 8 tableau columns (7,7,7,7,6,6,6,6).
//! Four free cells for temporary single-card storage, four foundation piles
//! that build up by suit from Ace to King. Every card is dealt face up, so the
//! whole game is on the table and nothing is hidden from the player.
//!
//! Played with either hand: Tab and the arrows move a cursor, Enter or Space
//! picks a card up and puts it down, Z undoes, N deals a new game, A sends home
//! every card that is safe to send. A click does the same as moving the cursor
//! there and pressing Enter, and the controls named along the bottom are
//! buttons rather than a caption.
//!
//! ## What this program was
//!
//! `main` was `let _app = FreeCell::new();`. It dealt fifty-two cards across
//! eight columns and dropped the lot, so no card reached a screen and no key
//! arrived. The drawing pass took no size and filled a hardcoded 900x800: the
//! three header readings sat at bare `x` of 200, 340 and 520, the win banner's
//! three lines were hand-centred at 280, 300 and 270, and a column deeper than
//! about twenty-six cards ran off the bottom with nothing to tighten it. There
//! was no pointer handling at all, under a help line advertising six controls.
//! Twelve blanket `#![allow(...)]` at the top of the file -- `dead_code` among
//! them -- are what kept a compiler from saying so.
//!
//! It now opens a real window, solves the table from the size that window
//! reports each frame, records a hit box for everything it draws, and answers
//! keys and clicks through one body the tests drive too.

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
use std::process::ExitCode;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Card display colors ─────────────────────────────────────────────
//
// Each of these names a *role* -- what the colour is for -- and takes its value
// from the palette above rather than repeating the hex. Six of the seven were
// second copies of a palette entry written out again by hand, which is a colour
// that can drift: retheme the palette and the cards keep the old scheme, with
// nothing to say they had ever agreed.
const CARD_BG: Color = TEXT_COLOR;
const CARD_RED: Color = RED;
const CARD_BLACK: Color = BASE;
const SELECTED_HIGHLIGHT: Color = BLUE;
const CURSOR_HIGHLIGHT: Color = YELLOW;
const EMPTY_PILE: Color = SURFACE0;

// ── The shape of a card ─────────────────────────────────────────────
//
// These are the only fixed numbers left, and none of them is a pixel: they are
// the *proportions* a card and a table keep at whatever size the window gives
// them. Everything drawn is one of these times a card width solved from the
// live window, so widening the window cannot move two things by different
// amounts. What was here before was fourteen pixel counts -- a 70x100 card at
// x = 16, a top row at y = 50, a tableau at y = 175 -- which is a picture that
// fits one window and no other.

/// A card is ten tall for every seven wide, which is close to a real one.
const CARD_ASPECT: f32 = 10.0 / 7.0;
/// The gap between two columns, as a share of a card's width.
const COLUMN_GAP: f32 = 1.0 / 7.0;
/// A card's corner rounding, as a share of its width.
const CARD_CORNER: f32 = 6.0 / 70.0;
/// How far down a card sits from the one it covers, as a share of card height.
const CASCADE_SHARE: f32 = 0.24;
/// The tightest that fan may become before the cards are shrunk instead. Below
/// this the rank in a card's top-left corner is covered by the card over it,
/// and a cascade whose ranks cannot be read is a cascade that cannot be played.
const CASCADE_FLOOR: f32 = 0.13;
/// The strip above the top row that names its two halves, as a share of height.
const ROW_LABEL_SHARE: f32 = 0.20;
/// The strip under the foundations holding each pile's count.
const PILE_COUNT_SHARE: f32 = 0.18;
/// The gap between the top row and the tableau.
const ROW_GAP_SHARE: f32 = 0.30;

/// Number of tableau columns.
const TABLEAU_COLS: usize = 8;
/// How many columns get the extra card: 52 cards over 8 columns is four of
/// seven and four of six.
const DEEP_COLUMNS: usize = 4;
/// Number of free cells.
const FREE_CELL_COUNT: usize = 4;
/// Number of foundation piles.
const FOUNDATION_COUNT: usize = 4;
/// The highest free cell index, for clamping a cursor into that row.
const LAST_FREE_CELL: usize = FREE_CELL_COUNT.saturating_sub(1);
/// The highest tableau column index, for clamping a cursor into that row.
const LAST_TABLEAU_COL: usize = TABLEAU_COLS.saturating_sub(1);

/// The window the game asks for. Nine card widths across is the table itself,
/// and the height is what a seven-card cascade needs under the top row.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 800.0;

// ── What the window can be asked about ──────────────────────────────

/// Everything the drawing pass records a hit box for.
///
/// The same list serves the player and the tests: a click is answered by
/// looking up what was drawn under it, so a thing a test can find is a thing a
/// player can reach, and a thing with no hit box is a thing neither can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The strip along the top holding the title and the two readings.
    Header,
    Title,
    Moves,
    Progress,
    /// The table between the header and the footer.
    Board,
    /// The caption over the free cells.
    FreeCellLabel,
    /// The caption over the foundations.
    FoundationLabel,
    /// A free cell, by index, whether or not it holds a card.
    FreeCell(u8),
    /// A foundation pile, by suit index, whether or not it holds a card.
    Foundation(u8),
    /// The count under a foundation.
    PileCount(u8),
    /// A tableau column's whole reachable area, including the empty space
    /// below its cards -- an empty column has to be clickable to be played to.
    Column(u8),
    /// One card of a tableau column, by column and by depth from the bottom.
    Card(u8, u8),
    /// The strip along the bottom holding the controls.
    Footer,
    /// One control on that strip, by its index in [`Control::ALL`].
    Control(u8),
    /// The reading of how many cells and columns are still empty.
    Room,
    /// The sheet over the table when the game is won.
    Overlay,
    OverlayTitle,
    OverlayMoves,
    /// The line of the sheet that says a new game can be dealt.
    NewGame,
}

/// A control named along the bottom of the window.
///
/// One list, walked by the drawing pass and by the click handler alike. The
/// help line it replaces was a caption -- it named six controls and none of
/// them could be clicked, because there was no pointer handling at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Free,
    Undo,
    Auto,
    NewGame,
}

impl Control {
    /// Every control, in the order they are drawn.
    const ALL: [Self; 4] = [Self::Free, Self::Undo, Self::Auto, Self::NewGame];

    /// What the strip says this control does.
    const fn label(self) -> &'static str {
        match self {
            Self::Free => "F  Free",
            Self::Undo => "Z  Undo",
            Self::Auto => "A  Auto",
            Self::NewGame => "N  New",
        }
    }

    /// The key this control names.
    ///
    /// A click runs the key, rather than calling the same method the key calls.
    /// A button cannot then do something its own caption does not say, and
    /// cannot get past a rule the key obeys -- a won board refuses undo from
    /// either side because there is only one place the refusal lives.
    const fn key(self) -> Key {
        match self {
            Self::Free => Key::F,
            Self::Undo => Key::Z,
            Self::Auto => Key::A,
            Self::NewGame => Key::N,
        }
    }
}

// ── Layout ──────────────────────────────────────────────────────────

/// The bands a window is divided into, solved from the size it reports.
///
/// Every number here is a share of the live window. What it replaces placed the
/// header's three readings at bare `x` of 200, 340 and 520 -- which is where
/// they do not overlap only at one window width and one font size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// The whole window.
    window: Rect,
    /// The title and the readings along the top.
    header: Rect,
    /// What is left for the table.
    body: Rect,
    /// The controls along the bottom.
    footer: Rect,
    /// Font size for a header reading.
    head: f32,
    /// Font size for a footer control and a sheet's body line.
    font: f32,
    /// Font size for a sheet's title.
    title: f32,
    /// Font size for a caption or a pile count.
    small: f32,
    /// The gap between bands, and the margin at the window's edge.
    pad: f32,
}

impl Layout {
    /// Solve the bands for a window of this size.
    #[must_use]
    pub fn new(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        // Held to half the shorter side as well as to the clamp: a floor of two
        // pixels put the bands' left edge at x = 2 in a window one pixel wide,
        // which is a band starting outside the window it is in.
        let pad = (w.min(h) * 0.014).clamp(2.0, 14.0).min(w.min(h) / 2.0);
        let head = (h / 40.0).clamp(9.0, 22.0);
        let font = (h / 50.0).clamp(8.0, 16.0);
        let title = (h / 22.0).clamp(14.0, 40.0);
        let small = (h / 64.0).clamp(7.0, 13.0);

        // Shares of `h`, each held to what there is: a band taller than the
        // window would leave the next one a negative height, and a rectangle of
        // negative height draws inside out.
        let header_h = (h * 0.06).clamp(20.0, 54.0).min(h);
        let footer_h = (h * 0.045).clamp(14.0, 36.0).min(h);
        let header = Rect::new(
            pad,
            pad,
            (w - pad * 2.0).max(0.0),
            (header_h - pad).max(0.0),
        );
        let body_y = (header.bottom() + pad).min(h);
        let footer_y = (h - footer_h).max(body_y);
        Self {
            window: Rect::new(0.0, 0.0, w, h),
            header,
            body: Rect::new(
                pad,
                body_y,
                (w - pad * 2.0).max(0.0),
                (footer_y - body_y - pad).max(0.0),
            ),
            footer: Rect::new(
                pad,
                footer_y,
                (w - pad * 2.0).max(0.0),
                (h - footer_y - pad).max(0.0),
            ),
            head,
            font,
            title,
            small,
            pad,
        }
    }
}

/// Where the eight columns go, and how large a card is drawn.
///
/// One number is solved -- `card_w` -- and every position, gap and fan step
/// follows from it, so the drawing pass and the hit test cannot disagree about
/// where a card is. Eight columns and seven gaps of a seventh of a card come to
/// exactly nine card widths, which is what fixes `card_w` from the width.
///
/// The height matters too, and used not to: a column deeper than about
/// twenty-six cards ran off the bottom of the fixed 800-tall board with nothing
/// to tighten it, and the cards below the fold could be neither seen nor
/// reached. The fan is tightened first, down to [`CASCADE_FLOOR`], and only
/// then is the card itself made smaller -- shrinking the card costs every
/// column, tightening the fan costs only the deep ones.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Table {
    /// The area the table was fitted into.
    area: Rect,
    /// A card's width.
    card_w: f32,
    /// A card's height.
    card_h: f32,
    /// Left edge to left edge between neighbouring columns.
    step: f32,
    /// Left edge of column zero.
    left: f32,
    /// Top of the free cells and the foundations.
    top_row_y: f32,
    /// Top of the tableau's first card.
    tableau_y: f32,
    /// How far down each further card of a cascade sits.
    cascade: f32,
}

impl Table {
    /// Fit a table `deepest` cards deep into `area`.
    #[must_use]
    pub fn new(area: Rect, deepest: usize) -> Self {
        // A backwards area is not reachable through `Layout`, which clamps its
        // bands, but `Table::new` is callable on its own and documents no
        // precondition, so a caller that hands it one gets an empty table
        // rather than a table drawn inside out.
        let aw = area.w.max(0.0);
        let ah = area.h.max(0.0);

        // Everything but the fan, measured in card *heights*, so the two bounds
        // can be compared in one unit.
        let fixed = ROW_LABEL_SHARE + 1.0 + PILE_COUNT_SHARE + ROW_GAP_SHARE + 1.0;
        // The deepest column has `deepest - 1` cards fanned below its first.
        let steps = f32_from_usize(deepest.saturating_sub(1));
        let tallest = fixed + CASCADE_FLOOR * steps;

        // The width bound, and the height bound at the tightest legible fan.
        let by_width = aw / 9.0;
        let by_height = ah / (CARD_ASPECT * tallest);
        let card_w = by_width.min(by_height).max(0.0);
        let card_h = card_w * CARD_ASPECT;

        // Spend whatever height is left over on loosening the fan back towards
        // its natural step -- when the board is width-bound, which is the usual
        // case, this puts it straight back at `CASCADE_SHARE`.
        let used = card_h * fixed;
        let spare = (ah - used).max(0.0);
        let natural = card_h * CASCADE_SHARE;
        let cascade = if steps > 0.0 {
            (spare / steps).min(natural)
        } else {
            natural
        };

        let gap = card_w * COLUMN_GAP;
        let step = card_w + gap;
        // Centred in whatever it was given: the leftover is margin on both
        // sides rather than a band of nothing down one.
        let left = area.x + (aw - card_w * 9.0).max(0.0) / 2.0;
        let top_row_y = area.y + card_h * ROW_LABEL_SHARE;
        let tableau_y = top_row_y + card_h * (1.0 + PILE_COUNT_SHARE + ROW_GAP_SHARE);
        Self {
            area,
            card_w,
            card_h,
            step,
            left,
            top_row_y,
            tableau_y,
            cascade,
        }
    }

    /// The left edge of the slot in position `slot` of a row of eight.
    fn slot_x(&self, slot: usize) -> f32 {
        self.left + f32_from_usize(slot) * self.step
    }

    /// The box a free cell or a foundation occupies.
    fn top_slot(&self, slot: usize) -> Rect {
        Rect::new(self.slot_x(slot), self.top_row_y, self.card_w, self.card_h)
    }

    /// The box the `depth`-th card of column `col` occupies.
    fn card_at(&self, col: usize, depth: usize) -> Rect {
        Rect::new(
            self.slot_x(col),
            self.tableau_y + f32_from_usize(depth) * self.cascade,
            self.card_w,
            self.card_h,
        )
    }

    /// The whole reachable area of column `col`: its cards and the empty space
    /// under them, down to the bottom of the table.
    ///
    /// The empty part matters -- a click below the last card of a column is a
    /// click on that column, and an empty column is only playable to because
    /// its box is still there when it holds nothing.
    fn column(&self, col: usize) -> Rect {
        Rect::new(
            self.slot_x(col),
            self.tableau_y,
            self.card_w,
            (self.area.bottom() - self.tableau_y).max(self.card_h),
        )
    }

    /// The corner rounding a card of this size gets.
    fn corner(&self) -> f32 {
        self.card_w * CARD_CORNER
    }
}

// ── Cursor helpers ──────────────────────────────────────────────────

/// The index before `i` in a ring of `len`, wrapping round at the start.
///
/// `checked_sub` rather than `if i > 0 { i - 1 } else { len - 1 }`, which the
/// three zones each wrote out for themselves: three copies of one rule, and
/// the `len - 1` in each of them underflows on a zone of length zero.
fn wrap_back(i: usize, len: usize) -> usize {
    i.checked_sub(1).unwrap_or_else(|| len.saturating_sub(1))
}

/// The index after `i` in a ring of `len`, wrapping round at the end.
fn wrap_forward(i: usize, len: usize) -> usize {
    let next = i.saturating_add(1);
    if next >= len { 0 } else { next }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// A count as a float, for the arithmetic that places things.
///
/// Eight columns and fifty-two cards are far inside the range an `f32` holds
/// exactly, and the cast is named here so the one `expect` covers every use.
#[expect(
    clippy::cast_precision_loss,
    reason = "a column index and a card depth are small"
)]
fn f32_from_usize(v: usize) -> f32 {
    v as f32
}

/// The same for a window size handed over as a whole number of pixels.
#[expect(
    clippy::cast_precision_loss,
    reason = "a window is not millions of pixels across"
)]
fn f32_from_u32(v: u32) -> f32 {
    v as f32
}

/// A small index as a byte, for naming a hit box.
///
/// Saturating rather than wrapping: eight columns and thirteen cards cannot
/// reach 255, but a `Card(0, 0)` invented by an overflow would be a hit box
/// claiming to be a card that is somewhere else.
fn byte(v: usize) -> u8 {
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// Fill a rectangle, if there is one to fill.
///
/// A rectangle of zero or negative size is not a small picture but a backwards
/// one, and the renderer would draw it inside out. Windows this program can be
/// given -- one pixel tall, or nothing at all while a resize is in flight --
/// produce them, so the guard is on the one place that emits fills rather than
/// repeated at each of the thirty call sites.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, corner_radii: CornerRadii) {
    if r.w <= 0.0 || r.h <= 0.0 {
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

/// Outline a rectangle, under the same guard as [`fill`].
fn stroke(
    f: &mut Frame<Target>,
    r: Rect,
    color: Color,
    line_width: f32,
    corner_radii: CornerRadii,
) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    f.push(RenderCommand::StrokeRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        line_width,
        corner_radii,
    });
}

/// Draw a line of text with its left edge at `x`, and answer the box it fills.
///
/// The box is measured, not guessed, so a hit box taken from it is the width of
/// the words that were actually drawn.
fn label(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) -> Rect {
    f.push(RenderCommand::Text {
        x,
        y,
        text: s.to_string(),
        color,
        font_size: size,
        font_weight: weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
    Rect::new(
        x,
        y,
        text::measure(s, size, weight),
        text::line_height(size, weight),
    )
}

/// The same, centred on `cx` by measuring the string.
fn centred(
    f: &mut Frame<Target>,
    cx: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) -> Rect {
    let w = text::measure(s, size, weight);
    label(f, cx - w / 2.0, y, s, color, size, weight)
}

// ── Randomness ──────────────────────────────────────────────────
//
// From `randrange`, not a local LCG. The local one drew the Fisher-Yates
// partner with `state % (i + 1)`, and on a modulus-2^64 generator the low bit
// of `state` alternates 0,1,0,1 for ever. Half of a 52-card shuffle's bounds
// are even, and `x % n` for even `n` preserves the parity of `x`, so on all 25
// of those swaps the partner index had a single fixed parity. `new_game`
// reseeds from the state, so every deal restarted the draw counter at zero and
// got the same pattern of fixed parities; only which parity varied.
//
// FreeCell deals every card face-up, so the whole layout is the symptom.
// Measured before the fix, over 200 000 deals played the way `new_game` plays
// them:
//
//   * the ace of hearts was the *bottom* card of column 0 -- the single worst
//     place for an ace to be -- in **17.4%** of deals, where any one of the 52
//     should hold that slot 1.9% of the time;
//   * how deeply that ace was buried alternated with depth rather than
//     tapering: 8.5% at depth 0, 15.5% at 1, 9.7% at 2, 16.2% at 3. The
//     difficulty of the deal was tied to the draw counter's parity.
//
// The shuffle itself was already the correct downward Fisher-Yates. Only the
// reduction was wrong.
use randrange::{RandomSource, SeededRng, seed_from_system};

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

    /// Whether the suit is red (Hearts or Diamonds).
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

    /// Whether this card can stack on top of `below` in a tableau column.
    /// Must be opposite color and one rank lower.
    fn can_stack_on_tableau(self, below: Card) -> bool {
        self.suit.is_red() != below.suit.is_red()
            && self.rank.value().checked_add(1) == Some(below.rank.value())
    }

    /// Whether this card can be placed on a foundation pile whose
    /// current top card has value `foundation_top_value` (0 if empty).
    ///
    /// The successor is taken with `checked_add` and compared as an `Option`
    /// rather than `== top + 1`. `top` is a `u8` that the caller supplies; the
    /// bare form wraps 255 to 0 and would call an Ace placeable on it, which is
    /// the shape of bug that only shows up on input the tests do not deal.
    fn can_place_on_foundation(self, foundation_top_value: u8) -> bool {
        foundation_top_value.checked_add(1) == Some(self.rank.value())
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

// ── Focus / Selection ───────────────────────────────────────────────

/// Which area of the game the cursor is focused on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusArea {
    /// Free cell 0..3.
    FreeCell(usize),
    /// Foundation pile 0..3.
    Foundation(usize),
    /// Tableau column 0..7 (cursor always targets the top card).
    Tableau(usize),
}

impl FocusArea {
    /// The default starting focus.
    fn default_focus() -> Self {
        Self::Tableau(0)
    }
}

/// What the player has selected to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    /// A card from a free cell.
    FreeCell(usize),
    /// The top card from a tableau column.
    Tableau(usize),
}

// ── Undo ────────────────────────────────────────────────────────────

/// One reversible step: a single card moved from one place to another.
///
/// A press can move more than one card. Placing a card can set off a run of
/// cards flying home to the foundations, and all of that is one press as far as
/// the player is concerned -- so it must be one press as far as the move counter
/// and the undo key are concerned too. `player` is what says so: it is true on
/// the step the player asked for and false on every step the game added on top
/// of it, and `undo` unwinds added steps until it has unwound one asked-for one.
///
/// The two used to be separate variants, `Move` and `AutoMove`, and the counting
/// did not follow from them: a run of auto-moves incremented nothing and yet each
/// undo of one decremented the counter, so undoing a placement that sent three
/// cards home took two presses and left the count two lower than the one press
/// that made it had raised it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct UndoStep {
    from: MoveLocation,
    to: MoveLocation,
    /// Whether the player asked for this step, rather than the game adding it.
    player: bool,
}

/// Who set off a run of cards to the foundations.
///
/// The same run means two different things depending on the answer, which is why
/// it cannot be inferred inside the run itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AutoRun {
    /// The player pressed the auto-move key. The run *is* the player's move, so
    /// its first card is the step `undo` stops at and the counter counts one.
    Asked,
    /// The run followed a placement. Every card in it belongs to that placement's
    /// press, so undoing the press has to take the whole run with it.
    Followed,
}

/// Location for move tracking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MoveLocation {
    FreeCell(usize),
    Foundation(usize),
    Tableau(usize),
}

// ── Game state ──────────────────────────────────────────────────────

/// Full game state for FreeCell.
struct GameState {
    /// Four free cells (each holds at most one card).
    free_cells: [Option<Card>; FREE_CELL_COUNT],
    /// Four foundation piles, indexed by `Suit::index()`.
    foundations: [Vec<Card>; FOUNDATION_COUNT],
    /// Eight tableau columns, all cards face-up.
    tableau: [Vec<Card>; TABLEAU_COLS],
    /// Current cursor focus.
    focus: FocusArea,
    /// Current selection (if any).
    selection: Option<Selection>,
    /// Undo history.
    undo_stack: Vec<UndoStep>,
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
            free_cells: [None; FREE_CELL_COUNT],
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: [
                Vec::new(),
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

    /// Deal all 52 cards across 8 columns (7,7,7,7,6,6,6,6).
    fn deal(&mut self) {
        let mut deck = make_deck();
        self.rng.shuffle(&mut deck);

        // Clear everything.
        for fc in &mut self.free_cells {
            *fc = None;
        }
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

        // Deal cards: first 4 columns get 7 cards, last 4 get 6 cards.
        //
        // Drawn from the deck as an iterator rather than by a running index.
        // The index version read `deck[idx]` fifty-two times against a deck
        // whose length nothing here checks, so a `make_deck` that returned one
        // card short would not have dealt a short column -- it would have
        // panicked, in the middle of a deal, with the board half built.
        let mut deck = deck.into_iter();
        for (col, pile) in self.tableau.iter_mut().enumerate() {
            let count = if col < DEEP_COLUMNS { 7 } else { 6 };
            pile.extend(deck.by_ref().take(count));
        }
    }

    /// Start a new game using the next RNG value as seed.
    fn new_game(&mut self) {
        let seed = self.rng.next_u64();
        self.rng = SeededRng::new(seed);
        self.deal();
    }

    // ── Accessors ───────────────────────────────────────────────────

    /// Get the top card of a foundation pile (by suit index).
    fn foundation_top(&self, idx: usize) -> Option<Card> {
        self.foundations.get(idx).and_then(|f| f.last().copied())
    }

    /// Get the top value of a foundation pile (0 if empty).
    fn foundation_top_value(&self, idx: usize) -> u8 {
        self.foundation_top(idx)
            .map(|c| c.rank.value())
            .unwrap_or(0)
    }

    /// Get the top card of a tableau column.
    fn tableau_top(&self, col: usize) -> Option<Card> {
        self.tableau.get(col).and_then(|t| t.last().copied())
    }

    /// Record that one move happened.
    ///
    /// Every move path counts through here rather than writing `move_count +=
    /// 1` for itself, so "what counts as a move" is one decision in one place
    /// instead of eight copies free to disagree. Saturating, because the
    /// counter is a reading on the header: a game long enough to overflow a
    /// `u32` should show a stuck number, not panic.
    fn count_move(&mut self) {
        self.move_count = self.move_count.saturating_add(1);
    }

    /// Count empty free cells.
    fn empty_free_cell_count(&self) -> usize {
        self.free_cells.iter().filter(|c| c.is_none()).count()
    }

    /// Find the first empty free cell index, if any.
    fn first_empty_free_cell(&self) -> Option<usize> {
        self.free_cells.iter().position(|c| c.is_none())
    }

    /// Count empty tableau columns.
    fn empty_tableau_count(&self) -> usize {
        self.tableau.iter().filter(|t| t.is_empty()).count()
    }

    /// The card in free cell `idx`, if that cell exists and is occupied.
    fn free_cell(&self, idx: usize) -> Option<Card> {
        self.free_cells.get(idx).copied().flatten()
    }

    /// How deep the longest tableau column is.
    ///
    /// The drawing pass needs this to fit the cascade: a column deeper than
    /// the window can show has to tighten the whole board rather than run off
    /// the bottom.
    fn deepest_column(&self) -> usize {
        self.tableau.iter().map(Vec::len).max().unwrap_or(0)
    }

    // ── Move logic ──────────────────────────────────────────────────

    /// Check if a card can be placed on a tableau column.
    ///
    /// Asked of the column itself rather than of `tableau_top`, which answers
    /// `None` both for a column that is *empty* and for a column that does not
    /// *exist* -- and empty means "any card may go here". Conflating the two
    /// meant this said yes to a move onto column 99, and the bound had to be
    /// spelled a second time above the question to stop it. One lookup answers
    /// both now, so there is no second copy of the bound to drift.
    fn can_place_on_tableau(&self, card: Card, col: usize) -> bool {
        self.tableau.get(col).is_some_and(|pile| match pile.last() {
            Some(top) => card.can_stack_on_tableau(*top),
            // Any card can go on an empty column.
            None => true,
        })
    }

    /// Try to move a card from a free cell to a tableau column.
    fn try_freecell_to_tableau(&mut self, fc_idx: usize, col: usize) -> bool {
        let Some(card) = self.free_cell(fc_idx) else {
            return false;
        };
        if !self.can_place_on_tableau(card, col) {
            return false;
        }
        // The column is found before the cell is emptied. Emptying first and
        // then failing to find the column would not misplace the card, it
        // would destroy it -- the one outcome a card game must not have.
        let Some(pile) = self.tableau.get_mut(col) else {
            return false;
        };
        pile.push(card);
        if let Some(cell) = self.free_cells.get_mut(fc_idx) {
            *cell = None;
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::FreeCell(fc_idx),
            to: MoveLocation::Tableau(col),
            player: true,
        });
        self.count_move();
        true
    }

    /// Try to move a card from a free cell to its foundation.
    fn try_freecell_to_foundation(&mut self, fc_idx: usize) -> bool {
        let Some(card) = self.free_cell(fc_idx) else {
            return false;
        };
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            return false;
        }
        let Some(pile) = self.foundations.get_mut(fidx) else {
            return false;
        };
        pile.push(card);
        if let Some(cell) = self.free_cells.get_mut(fc_idx) {
            *cell = None;
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::FreeCell(fc_idx),
            to: MoveLocation::Foundation(fidx),
            player: true,
        });
        self.count_move();
        self.check_win();
        true
    }

    /// Try to move the top card from a tableau column to a free cell.
    fn try_tableau_to_freecell(&mut self, col: usize) -> bool {
        let card = match self.tableau_top(col) {
            Some(c) => c,
            None => return false,
        };
        let Some(fc_idx) = self.first_empty_free_cell() else {
            return false;
        };
        // The cell is found before the card leaves the column, for the same
        // reason as above: a card taken off the board and then not placed is a
        // card gone.
        let Some(cell) = self.free_cells.get_mut(fc_idx) else {
            return false;
        };
        *cell = Some(card);
        if let Some(pile) = self.tableau.get_mut(col) {
            pile.pop();
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::FreeCell(fc_idx),
            player: true,
        });
        self.count_move();
        true
    }

    /// Try to move the top card from a tableau column to a specific free cell.
    fn try_tableau_to_specific_freecell(&mut self, col: usize, fc_idx: usize) -> bool {
        // One lookup answers both "is there such a cell" and "is it free".
        // It used to be a `fc_idx >= FREE_CELL_COUNT` bound followed by an
        // index -- the bound a second spelling of the array's own length.
        if !self.free_cells.get(fc_idx).is_some_and(Option::is_none) {
            return false;
        }
        let Some(card) = self.tableau_top(col) else {
            return false;
        };
        let Some(cell) = self.free_cells.get_mut(fc_idx) else {
            return false;
        };
        *cell = Some(card);
        if let Some(pile) = self.tableau.get_mut(col) {
            pile.pop();
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::FreeCell(fc_idx),
            player: true,
        });
        self.count_move();
        true
    }

    /// Try to move the top card from a tableau column to its foundation.
    fn try_tableau_to_foundation(&mut self, col: usize) -> bool {
        let card = match self.tableau_top(col) {
            Some(c) => c,
            None => return false,
        };
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            return false;
        }
        let Some(pile) = self.foundations.get_mut(fidx) else {
            return false;
        };
        pile.push(card);
        if let Some(from) = self.tableau.get_mut(col) {
            from.pop();
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::Foundation(fidx),
            player: true,
        });
        self.count_move();
        self.check_win();
        true
    }

    /// Try to move the top card from one tableau column to another.
    fn try_tableau_to_tableau(&mut self, from_col: usize, to_col: usize) -> bool {
        // Only the "not onto itself" rule is stated here. The two bounds that
        // used to sit beside it were a second spelling of the array's length,
        // and `tableau_top` / `can_place_on_tableau` each answer their own.
        if from_col == to_col {
            return false;
        }
        let Some(card) = self.tableau_top(from_col) else {
            return false;
        };
        if !self.can_place_on_tableau(card, to_col) {
            return false;
        }
        let Some(to) = self.tableau.get_mut(to_col) else {
            return false;
        };
        to.push(card);
        if let Some(from) = self.tableau.get_mut(from_col) {
            from.pop();
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::Tableau(from_col),
            to: MoveLocation::Tableau(to_col),
            player: true,
        });
        self.count_move();
        true
    }

    /// Try to move a card from a free cell to a specific free cell (swap).
    fn try_freecell_to_freecell(&mut self, from: usize, to: usize) -> bool {
        // As above: the bounds are the arrays' own, asked by `get`.
        if from == to
            || self.free_cell(from).is_none()
            || !self.free_cells.get(to).is_some_and(Option::is_none)
        {
            return false;
        }
        // Destination first, as everywhere else: taking the card out and then
        // failing to find the cell to put it in would lose it.
        let Some(card) = self.free_cell(from) else {
            return false;
        };
        let Some(cell) = self.free_cells.get_mut(to) else {
            return false;
        };
        *cell = Some(card);
        if let Some(src) = self.free_cells.get_mut(from) {
            *src = None;
        }
        self.undo_stack.push(UndoStep {
            from: MoveLocation::FreeCell(from),
            to: MoveLocation::FreeCell(to),
            player: true,
        });
        self.count_move();
        true
    }

    // ── Auto-move ───────────────────────────────────────────────────

    /// Check if a card is safe to auto-move to its foundation.
    /// A card is safe if both cards of the opposite color with rank one
    /// less are already on their foundations (so no future tableau
    /// stacking needs this card).
    fn is_safe_to_auto_move(&self, card: Card) -> bool {
        if card.rank == Rank::Ace {
            return true;
        }
        if card.rank == Rank::Two {
            return true;
        }
        // The card is safe if both opposite-color suits have at least (rank - 1)
        // on their foundations.
        let needed = card.rank.value().saturating_sub(1);
        let is_red = card.suit.is_red();
        for &s in &Suit::ALL {
            if s.is_red() != is_red && self.foundation_top_value(s.index()) < needed {
                return false;
            }
        }
        true
    }

    /// Send `card` home from `from`, if it belongs there and is safe to send.
    ///
    /// `player` says whether this step is one the player pressed for; undo
    /// stops on those and walks past the rest.
    ///
    /// The auto-run's two loops used to write this out twice, each with four
    /// bare index expressions -- and in each copy the card left its pile on the
    /// line *before* the foundation was reached, so an index the arrays did not
    /// have would have taken the card off the board and not put it anywhere.
    /// Here the destination is confirmed first, and both loops share the one
    /// copy, so the rule for what may go home is stated once.
    fn send_home(&mut self, from: MoveLocation, card: Card, player: bool) -> bool {
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx))
            || !self.is_safe_to_auto_move(card)
            || self.foundations.get(fidx).is_none()
            || self.take_card_from(from).is_none()
        {
            return false;
        }
        let to = MoveLocation::Foundation(fidx);
        self.put_card_at(to, card);
        self.undo_stack.push(UndoStep { from, to, player });
        true
    }

    /// Send every card that is safe to send home, over and over until none is.
    /// Returns how many cards moved.
    ///
    /// `run` says whose move this is; see [`AutoRun`]. It is the caller's answer
    /// and not something this can work out for itself, because the run looks
    /// identical either way -- the difference is only in what the player pressed.
    fn auto_move_to_foundations(&mut self, run: AutoRun) -> usize {
        let mut total: usize = 0;
        loop {
            let mut moved_any = false;

            // Check free cells.
            for fc_idx in 0..FREE_CELL_COUNT {
                let Some(card) = self.free_cell(fc_idx) else {
                    continue;
                };
                if self.send_home(
                    MoveLocation::FreeCell(fc_idx),
                    card,
                    run == AutoRun::Asked && total == 0,
                ) {
                    total = total.saturating_add(1);
                    moved_any = true;
                }
            }

            // Check tableau tops.
            for col in 0..TABLEAU_COLS {
                let Some(card) = self.tableau_top(col) else {
                    continue;
                };
                if self.send_home(
                    MoveLocation::Tableau(col),
                    card,
                    run == AutoRun::Asked && total == 0,
                ) {
                    total = total.saturating_add(1);
                    moved_any = true;
                }
            }

            if !moved_any {
                break;
            }
        }
        if total > 0 {
            // A run the player asked for is one move however many cards it sent
            // home, which is the same rule every other press follows. A run that
            // followed a placement is already counted by that placement.
            if run == AutoRun::Asked {
                self.count_move();
            }
            self.check_win();
        }
        total
    }

    // ── Undo ────────────────────────────────────────────────────────

    /// Undo the last press: reverse the cards the game sent home on its own, and
    /// then the one card the player actually moved.
    ///
    /// Written as a loop rather than the recursion it replaced. The recursion
    /// decided whether to keep going by looking at the *next* entry, so the last
    /// auto-move of a chain -- the one sitting directly on the player's move --
    /// stopped, leaving the board in a state the player never chose and needing
    /// a second press to finish. The condition belongs on the entry being undone,
    /// not on the one below it, and then there is nothing to look ahead at.
    fn undo(&mut self) {
        let mut undid_a_press = false;
        while let Some(step) = self.undo_stack.pop() {
            // Reverse: take from `to`, put back at `from`.
            if let Some(card) = self.take_card_from(step.to) {
                self.put_card_at(step.from, card);
            }
            self.won = false;
            if step.player {
                undid_a_press = true;
                break;
            }
        }
        if undid_a_press {
            // Saturating because the counter is unsigned, not because it could
            // legitimately be zero here: every `player` step incremented it once.
            self.move_count = self.move_count.saturating_sub(1);
        }
    }

    /// Take a card from a location (used by undo).
    fn take_card_from(&mut self, loc: MoveLocation) -> Option<Card> {
        match loc {
            MoveLocation::FreeCell(i) => self.free_cells.get_mut(i).and_then(|c| c.take()),
            MoveLocation::Foundation(i) => self.foundations.get_mut(i).and_then(|f| f.pop()),
            MoveLocation::Tableau(i) => self.tableau.get_mut(i).and_then(|t| t.pop()),
        }
    }

    /// Put a card at a location (used by undo).
    fn put_card_at(&mut self, loc: MoveLocation, card: Card) {
        match loc {
            MoveLocation::FreeCell(i) => {
                if let Some(cell) = self.free_cells.get_mut(i) {
                    *cell = Some(card);
                }
            }
            MoveLocation::Foundation(i) => {
                if let Some(f) = self.foundations.get_mut(i) {
                    f.push(card);
                }
            }
            MoveLocation::Tableau(i) => {
                if let Some(t) = self.tableau.get_mut(i) {
                    t.push(card);
                }
            }
        }
    }

    // ── Win detection ───────────────────────────────────────────────

    /// Check if all 52 cards are on foundations.
    fn check_win(&mut self) {
        let total: usize = self.foundations.iter().map(|f| f.len()).sum();
        if total == 52 {
            self.won = true;
        }
    }

    /// Total cards on all foundations.
    fn foundation_total(&self) -> usize {
        self.foundations.iter().map(|f| f.len()).sum()
    }

    // ── Input handling ──────────────────────────────────────────────

    /// Handle a key press, and say whether the game did anything with it.
    ///
    /// The answer is the whole point of the return value: the window redraws on
    /// `Consumed` and stays still on `Ignored`, so a key this does not know
    /// costs nothing. It used to return `()`, which meant every keystroke that
    /// reached the program looked exactly like every one that did not.
    /// Park the top card of the focused column in the first free cell.
    ///
    /// `try_tableau_to_freecell` was written, tested and reachable by nothing:
    /// no key ran it and there was no pointer handling at all, so the one move
    /// freecell is named after could only be made the long way, by selecting a
    /// card and then selecting a cell. A move the game implements and the
    /// player cannot make is the same as a move the game does not have.
    ///
    /// The cursor has to be on a column for this to mean anything -- a free
    /// cell or a foundation has no card to send to a cell -- so from either of
    /// those the press does nothing rather than guessing a column.
    fn park_focused_column(&mut self) {
        if let FocusArea::Tableau(col) = self.focus {
            // `try_tableau_to_freecell` counts the move itself, as every
            // `try_*` does; counting it again here would make one press read
            // as two.
            if self.try_tableau_to_freecell(col) {
                self.selection = None;
                self.auto_move_to_foundations(AutoRun::Followed);
            }
        }
    }

    fn handle_key(&mut self, key: Key, _modifiers: Modifiers) -> EventResult {
        // A won board takes only a new game. Every other key is still answered
        // -- the sheet is up, and a key that does nothing is not a key that
        // should fall through to the board underneath it.
        if self.won {
            if key == Key::N {
                self.new_game();
            }
            return EventResult::Consumed;
        }

        match key {
            Key::N => self.new_game(),
            Key::Z => self.undo(),
            Key::Tab => self.navigate_next_zone(),
            Key::Left => self.navigate_left(),
            Key::Right => self.navigate_right(),
            Key::Up => self.navigate_up(),
            Key::Down => self.navigate_down(),
            Key::Enter | Key::Space => self.activate(),
            Key::Escape => {
                self.selection = None;
            }
            Key::A => {
                self.auto_move_to_foundations(AutoRun::Asked);
            }
            Key::F => self.park_focused_column(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Move the cursor to `focus` and press Enter there.
    ///
    /// What a click does, expressed as what the keyboard already does. A click
    /// that reached into the move functions directly would be a second set of
    /// rules for the same move, and the two would drift.
    fn focus_and_act(&mut self, focus: FocusArea) -> EventResult {
        self.focus = focus;
        self.activate();
        EventResult::Consumed
    }

    /// Cycle focus between zones: free cells -> foundations -> tableau.
    fn navigate_next_zone(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(_) => FocusArea::Foundation(0),
            FocusArea::Foundation(_) => FocusArea::Tableau(0),
            FocusArea::Tableau(_) => FocusArea::FreeCell(0),
        };
    }

    /// Navigate left within the current zone, wrapping round.
    fn navigate_left(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => FocusArea::FreeCell(wrap_back(i, FREE_CELL_COUNT)),
            FocusArea::Foundation(i) => FocusArea::Foundation(wrap_back(i, FOUNDATION_COUNT)),
            FocusArea::Tableau(i) => FocusArea::Tableau(wrap_back(i, TABLEAU_COLS)),
        };
    }

    /// Navigate right within the current zone, wrapping round.
    fn navigate_right(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => FocusArea::FreeCell(wrap_forward(i, FREE_CELL_COUNT)),
            FocusArea::Foundation(i) => FocusArea::Foundation(wrap_forward(i, FOUNDATION_COUNT)),
            FocusArea::Tableau(i) => FocusArea::Tableau(wrap_forward(i, TABLEAU_COLS)),
        };
    }

    /// Navigate up to the top row from tableau.
    ///
    /// The top row is the free cells followed by the foundations, so the column
    /// a cursor rises into is a free cell while its index is below the number of
    /// free cells and a foundation after that. Both places used to say `4`,
    /// which is that count written as a literal: change `FREE_CELL_COUNT` and
    /// the cursor would have gone to the wrong half of a row it still drew
    /// correctly.
    fn navigate_up(&mut self) {
        self.focus = match self.focus {
            FocusArea::Tableau(i) => {
                if i < FREE_CELL_COUNT {
                    FocusArea::FreeCell(i)
                } else {
                    FocusArea::Foundation(i.saturating_sub(FREE_CELL_COUNT))
                }
            }
            FocusArea::Foundation(i) => FocusArea::FreeCell(i.min(LAST_FREE_CELL)),
            // Named rather than `other => other`: a wildcard here matches any
            // zone added to `FocusArea` later, so a new zone would silently
            // have no `Up` at all instead of failing to compile.
            FocusArea::FreeCell(i) => FocusArea::FreeCell(i),
        };
    }

    /// Navigate down to tableau from the top row.
    fn navigate_down(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => FocusArea::Tableau(i.min(LAST_TABLEAU_COL)),
            FocusArea::Foundation(i) => {
                FocusArea::Tableau(i.saturating_add(FREE_CELL_COUNT).min(LAST_TABLEAU_COL))
            }
            FocusArea::Tableau(i) => FocusArea::Tableau(i),
        };
    }

    /// Activate: select a card or place the selected card.
    fn activate(&mut self) {
        if let Some(sel) = self.selection {
            // We have a selection -- try to place it at the focused location.
            let placed = self.try_place_selection(sel);
            if placed {
                self.selection = None;
                self.auto_move_to_foundations(AutoRun::Followed);
            } else {
                // If placing failed and we're clicking the same spot, deselect.
                let same_spot = match (sel, self.focus) {
                    (Selection::FreeCell(a), FocusArea::FreeCell(b)) => a == b,
                    (Selection::Tableau(a), FocusArea::Tableau(b)) => a == b,
                    _ => false,
                };
                if same_spot {
                    self.selection = None;
                } else {
                    // Try to select a new card from the focused location.
                    self.selection = None;
                    self.try_select();
                }
            }
        } else {
            self.try_select();
        }
    }

    /// Try to select a card at the current focus.
    fn try_select(&mut self) {
        match self.focus {
            FocusArea::FreeCell(i) => {
                if self.free_cells.get(i).copied().flatten().is_some() {
                    self.selection = Some(Selection::FreeCell(i));
                }
            }
            FocusArea::Tableau(i) => {
                if self.tableau_top(i).is_some() {
                    self.selection = Some(Selection::Tableau(i));
                }
            }
            FocusArea::Foundation(_) => {
                // Foundations are not selectable for moving cards out.
            }
        }
    }

    /// Try to place the selected card at the focused destination.
    fn try_place_selection(&mut self, sel: Selection) -> bool {
        match sel {
            Selection::FreeCell(fc_idx) => match self.focus {
                FocusArea::Tableau(col) => self.try_freecell_to_tableau(fc_idx, col),
                FocusArea::Foundation(_) => self.try_freecell_to_foundation(fc_idx),
                FocusArea::FreeCell(to) => self.try_freecell_to_freecell(fc_idx, to),
            },
            Selection::Tableau(from_col) => match self.focus {
                FocusArea::Tableau(to_col) => self.try_tableau_to_tableau(from_col, to_col),
                FocusArea::Foundation(_) => self.try_tableau_to_foundation(from_col),
                FocusArea::FreeCell(fc) => self.try_tableau_to_specific_freecell(from_col, fc),
            },
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Draw one frame at the size the window reports, recording where each
    /// thing landed.
    ///
    /// Everything is solved from `w` and `h` on the way through, so nothing
    /// here remembers where it put something last frame and there is no second
    /// copy of the layout for a hit test to disagree with. What it replaces
    /// took no size at all and filled a fixed 900x800.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::new(w, h);
        let t = Table::new(l.body, self.deepest_column());
        let mut f = Frame::new(w, h);

        // Everything is drawn inside the window. Without this a reading too
        // wide for a narrow window spilled past the edge, and a zero-sized
        // window still recorded a clickable `Press N` at its centre -- a
        // control in a window with no pixels to show it.
        f.clip(l.window);
        fill(&mut f, l.window, BASE, CornerRadii::ZERO);
        self.draw_header(&mut f, &l);

        // The table's own box goes down before anything on it, so a card drawn
        // over it answers a hit test first.
        f.hit(Target::Board, l.body);
        f.clip(l.body);
        self.draw_top_row(&mut f, &l, &t);
        self.draw_tableau(&mut f, &t);
        f.unclip();

        self.draw_footer(&mut f, &l);
        if self.won {
            self.draw_win_sheet(&mut f, &l);
        }
        f.unclip();
        f
    }

    /// The title and the two readings along the top, each placed by measuring
    /// it rather than by a hardcoded `x` of 200, 340 and 520.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, CornerRadii::all(l.pad * 0.5));
        f.hit(Target::Header, l.header);
        if l.header.is_empty() {
            return;
        }
        // A reading too wide for the band is cut off at the band rather than
        // written across the table below it.
        f.clip(l.header);
        let inner = l.pad.max(2.0);
        let bold = FontWeightHint::Bold;
        let regular = FontWeightHint::Regular;
        let y = l.header.y + (l.header.h - text::line_height(l.head, bold)) / 2.0;

        let r = label(f, l.header.x + inner, y, "FreeCell", LAVENDER, l.head, bold);
        f.hit(Target::Title, r);

        let moves = format!("Moves: {}", self.move_count);
        let r = centred(f, l.header.centre().0, y, &moves, SUBTEXT0, l.head, regular);
        f.hit(Target::Moves, r);

        // Right-aligned by measuring the string, so it stays against the right
        // edge when the count reaches two digits and when the window is resized.
        let done = format!("Foundations: {}/52", self.foundation_total());
        let width = text::measure(&done, l.head, regular);
        let r = label(
            f,
            l.header.right() - inner - width,
            y,
            &done,
            if self.foundation_total() == 52 {
                GREEN
            } else {
                TEAL
            },
            l.head,
            regular,
        );
        f.hit(Target::Progress, r);
        f.unclip();
    }

    /// The four free cells and the four foundations, with the caption over each
    /// half and a count under each pile.
    fn draw_top_row(&self, f: &mut Frame<Target>, l: &Layout, t: &Table) {
        let regular = FontWeightHint::Regular;
        // The caption sits in the strip above the row, its bottom edge on the
        // row's top edge, so it cannot overlap the cards however tall the font.
        let cap_y = (t.top_row_y - text::line_height(l.small, regular)).max(t.area.y);
        let r = label(
            f,
            t.slot_x(0),
            cap_y,
            "Free Cells",
            SUBTEXT0,
            l.small,
            regular,
        );
        f.hit(Target::FreeCellLabel, r);
        let r = label(
            f,
            t.slot_x(FREE_CELL_COUNT),
            cap_y,
            "Foundations",
            SUBTEXT0,
            l.small,
            regular,
        );
        f.hit(Target::FoundationLabel, r);

        for idx in 0..FREE_CELL_COUNT {
            let rect = t.top_slot(idx);
            f.hit(Target::FreeCell(byte(idx)), rect);
            let focused = self.focus == FocusArea::FreeCell(idx);
            let selected = self.selection == Some(Selection::FreeCell(idx));
            match self.free_cell(idx) {
                Some(card) => self.draw_card(f, card, rect, t, focused, selected),
                None => draw_empty(f, rect, t, focused),
            }
        }

        for idx in 0..FOUNDATION_COUNT {
            let rect = t.top_slot(idx.saturating_add(FREE_CELL_COUNT));
            f.hit(Target::Foundation(byte(idx)), rect);
            let focused = self.focus == FocusArea::Foundation(idx);
            match self.foundation_top(idx) {
                Some(card) => self.draw_card(f, card, rect, t, focused, false),
                None => {
                    draw_empty(f, rect, t, focused);
                    // The suit a pile is waiting for, ghosted on the empty slot.
                    if let Some(suit) = Suit::ALL.get(idx) {
                        let (cx, cy) = rect.centre();
                        centred(
                            f,
                            cx,
                            cy - text::line_height(t.card_w * 0.29, regular) / 2.0,
                            suit.symbol(),
                            OVERLAY0,
                            t.card_w * 0.29,
                            regular,
                        );
                    }
                }
            }
            let count = format!("{}/13", self.foundations.get(idx).map_or(0, Vec::len));
            let r = centred(
                f,
                rect.centre().0,
                rect.bottom() + t.card_h * PILE_COUNT_SHARE * 0.1,
                &count,
                SUBTEXT0,
                l.small,
                regular,
            );
            f.hit(Target::PileCount(byte(idx)), r);
        }

        // The rule that separates the two rows, drawn across the table's own
        // width rather than to a column position that assumes eight of them.
        let rule_y = (t.top_row_y + t.card_h * (1.0 + PILE_COUNT_SHARE + ROW_GAP_SHARE * 0.5))
            .min(t.area.bottom());
        f.push(RenderCommand::Line {
            x1: t.slot_x(0),
            y1: rule_y,
            x2: t.slot_x(TABLEAU_COLS - 1) + t.card_w,
            y2: rule_y,
            color: SURFACE1,
            width: 1.0,
        });
    }

    /// The eight columns.
    fn draw_tableau(&self, f: &mut Frame<Target>, t: &Table) {
        for col in 0..TABLEAU_COLS {
            // The column's whole strip goes down first, so a click in the empty
            // space under its cards still reaches the column -- which is the
            // only way an empty column can be played to at all.
            f.hit(Target::Column(byte(col)), t.column(col));
            let focused = self.focus == FocusArea::Tableau(col);
            let selected = self.selection == Some(Selection::Tableau(col));
            let Some(pile) = self.tableau.get(col) else {
                continue;
            };
            if pile.is_empty() {
                draw_empty(f, t.card_at(col, 0), t, focused);
                continue;
            }
            let last = pile.len().saturating_sub(1);
            for (depth, &card) in pile.iter().enumerate() {
                let top = depth == last;
                let rect = t.card_at(col, depth);
                self.draw_card(f, card, rect, t, top && focused, top && selected);
                f.hit(Target::Card(byte(col), byte(depth)), rect);
            }
        }
    }

    /// One face-up card.
    ///
    /// Every offset inside it is a share of the card, so the four corner
    /// readings stay in their corners at any size. They used to be nudged by
    /// a bare `-22`, `-38`, `-8` and `-10`, which put them where they belong
    /// only on a 70x100 card in a 16-point font.
    fn draw_card(
        &self,
        f: &mut Frame<Target>,
        card: Card,
        r: Rect,
        t: &Table,
        focused: bool,
        selected: bool,
    ) {
        let corner = t.corner();
        if selected {
            let g = t.card_w * 0.03;
            stroke(
                f,
                Rect::new(r.x - g, r.y - g, r.w + g * 2.0, r.h + g * 2.0),
                SELECTED_HIGHLIGHT,
                (t.card_w * 0.036).max(1.0),
                CornerRadii::all(corner + g),
            );
        } else if focused {
            let g = t.card_w * 0.015;
            stroke(
                f,
                Rect::new(r.x - g, r.y - g, r.w + g * 2.0, r.h + g * 2.0),
                CURSOR_HIGHLIGHT,
                (t.card_w * 0.029).max(1.0),
                CornerRadii::all(corner + g),
            );
        }
        fill(f, r, CARD_BG, CornerRadii::all(corner));
        if r.is_empty() {
            return;
        }

        let color = card.suit.color();
        let bold = FontWeightHint::Bold;
        let regular = FontWeightHint::Regular;
        let pip = t.card_w * 0.23;
        let big = t.card_w * 0.29;
        let inset = t.card_w * 0.07;

        label(
            f,
            r.x + inset,
            r.y + inset * 0.6,
            card.rank.label(),
            color,
            pip,
            bold,
        );
        label(
            f,
            r.x + inset,
            r.y + inset * 0.6 + text::line_height(pip, bold),
            card.suit.symbol(),
            color,
            pip,
            regular,
        );
        let (cx, cy) = r.centre();
        centred(
            f,
            cx,
            cy - text::line_height(big, regular) / 2.0,
            card.suit.symbol(),
            color,
            big,
            regular,
        );

        // The bottom pair is right-aligned by measuring, not by stepping a
        // fixed 22 pixels in from the right edge -- which is where a one-glyph
        // rank ends and a two-glyph one ("10") does not.
        let rank = card.rank.label();
        let suit = card.suit.symbol();
        let widest = text::measure(rank, pip, bold).max(text::measure(suit, pip, regular));
        let right = r.right() - inset - widest;
        let bottom = r.bottom() - inset * 0.6 - text::line_height(pip, bold);
        label(f, right, bottom, rank, color, pip, bold);
        label(
            f,
            right,
            bottom - text::line_height(pip, regular),
            suit,
            color,
            pip,
            regular,
        );
    }

    /// The controls along the bottom.
    ///
    /// Buttons, not a caption. What was here was a help line naming six
    /// controls in a program with no pointer handling at all, so every one of
    /// the six was a promise only the keyboard could keep.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, CornerRadii::all(l.pad * 0.5));
        f.hit(Target::Footer, l.footer);
        if l.footer.is_empty() {
            return;
        }
        f.clip(l.footer);
        let regular = FontWeightHint::Regular;
        let inner = l.pad.max(2.0);
        let text_y = l.footer.y + (l.footer.h - text::line_height(l.font, regular)) / 2.0;
        let mut x = l.footer.x + inner;
        for (i, control) in Control::ALL.into_iter().enumerate() {
            let w = text::measure(control.label(), l.font, regular) + inner * 2.0;
            let button = Rect::new(l.footer.x.max(x), l.footer.y, w, l.footer.h);
            fill(f, button, SURFACE0, CornerRadii::all(l.pad * 0.4));
            label(
                f,
                button.x + inner,
                text_y,
                control.label(),
                TEXT_COLOR,
                l.font,
                regular,
            );
            f.hit(Target::Control(byte(i)), button);
            x = button.right() + inner;
        }

        // How much room there is left to manoeuvre in. This is the number a
        // freecell player plans against -- it is what caps the length of a run
        // that can be shifted -- and it appears nowhere else on screen, so it
        // is the *last* thing dropped as the window narrows.
        let room = format!(
            "{} cells   {} columns",
            self.empty_free_cell_count(),
            self.empty_tableau_count()
        );
        let room_w = text::measure(&room, l.font, regular);
        let room_x = l.footer.right() - inner - room_w;
        if room_x < x {
            f.unclip();
            return;
        }
        let drawn = label(f, room_x, text_y, &room, SUBTEXT0, l.font, regular);
        f.hit(Target::Room, drawn);

        // What the keyboard can do that the buttons do not. A reminder, not a
        // fact about this board, so this is the caption that goes first when
        // there is not width for both it and the reading.
        let hint = "Tab/Arrows move   Enter picks up and puts down   Esc drops";
        let hint_w = text::measure(hint, l.font, regular);
        let hint_x = room_x - inner * 2.0 - hint_w;
        if hint_x >= x {
            label(f, hint_x, text_y, hint, OVERLAY0, l.font, regular);
        }
        f.unclip();
    }

    /// The sheet over the table when the game is won.
    ///
    /// Centred on the window it is in. Its three lines used to be placed at a
    /// hardcoded x of 280, 300 and 270 and a y of 300, 350 and 390 -- three
    /// separate guesses at the middle of one fixed 900x800 board.
    fn draw_win_sheet(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, Color::rgba(17, 17, 27, 200), CornerRadii::ZERO);
        f.hit(Target::Overlay, l.window);
        if l.window.is_empty() {
            return;
        }
        let regular = FontWeightHint::Regular;
        let bold = FontWeightHint::Bold;
        let (cx, cy) = l.window.centre();
        let gap = l.pad;

        let title_h = text::line_height(l.title, bold);
        let line_h = text::line_height(l.font, regular);
        // The block is measured, then centred as a block: three lines each
        // centred on their own guess is three lines that do not line up.
        let block = title_h + line_h * 2.0 + gap * 2.0;
        let mut y = cy - block / 2.0;

        let r = centred(f, cx, y, "You Win!", GREEN, l.title, bold);
        f.hit(Target::OverlayTitle, r);
        y += title_h + gap;

        let moves = format!("Moves: {}", self.move_count);
        let r = centred(f, cx, y, &moves, SUBTEXT0, l.font, regular);
        f.hit(Target::OverlayMoves, r);
        y += line_h + gap;

        let r = centred(f, cx, y, "Press N for a new game", MAUVE, l.font, regular);
        f.hit(Target::NewGame, r);
    }
}

/// An empty slot: a free cell with nothing in it, a foundation not started, or
/// a column played out.
///
/// A free function rather than a method: it reads no game state, and a slot
/// that looked at the board to decide how to draw itself would be a second
/// place for the board to be misread.
fn draw_empty(f: &mut Frame<Target>, r: Rect, t: &Table, focused: bool) {
    let corner = t.corner();
    fill(f, r, EMPTY_PILE, CornerRadii::all(corner));
    stroke(
        f,
        r,
        if focused { CURSOR_HIGHLIGHT } else { OVERLAY0 },
        if focused {
            (t.card_w * 0.029).max(1.0)
        } else {
            1.0
        },
        CornerRadii::all(corner),
    );
}

// ── Application wrapper ─────────────────────────────────────────────

/// The FreeCell application: a game, and the size the window last reported.
struct FreeCell {
    state: GameState,
    /// The size the last frame was drawn at, and so the size the next click is
    /// read against. A click has to be measured against the picture the player
    /// was looking at, which is the one the last `render` drew.
    size: (f32, f32),
}

impl FreeCell {
    /// A game dealt from the system's randomness, so two players -- and two
    /// launches -- do not get the same board. It used to be `GameState::new(42)`
    /// for every launch on every machine for ever.
    fn new() -> Self {
        Self::with_seed(seed_from_system(0x4652_4545_4345_4C4C))
    }

    /// The same, from a seed the caller chooses. This is what the tests use:
    /// a game whose deal is a fact rather than a draw.
    fn with_seed(seed: u64) -> Self {
        Self {
            state: GameState::new(seed),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
        }
    }

    /// The size the next frame will be drawn at, and the next click read
    /// against.
    fn resize(&mut self, width: f32, height: f32) {
        self.size = (width.max(0.0), height.max(0.0));
    }

    /// The size the last frame was drawn at.
    const fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Draw one frame at this size.
    fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        self.state.frame(w, h)
    }

    /// Act on a click at window coordinates, by asking the frame what was drawn
    /// there.
    ///
    /// There is one picture and one answer: the boxes a click is tested against
    /// are the ones the drawing pass recorded, so a control that moved cannot
    /// leave its hit box behind.
    fn click(&mut self, x: f32, y: f32, button: MouseButton) -> EventResult {
        if button != MouseButton::Left {
            return EventResult::Ignored;
        }
        let (w, h) = self.size();
        let Some(target) = self.frame(w, h).hit_test(x, y) else {
            return EventResult::Ignored;
        };
        // While the sheet is up the board behind it is not reachable: a click
        // that falls on the sheet stops there rather than moving a card the
        // player cannot see.
        if self.state.won {
            return match target {
                Target::NewGame => {
                    self.state.handle_key(Key::N, Modifiers::default());
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }
        match target {
            Target::FreeCell(i) => self
                .state
                .focus_and_act(FocusArea::FreeCell(usize::from(i))),
            Target::Foundation(i) | Target::PileCount(i) => self
                .state
                .focus_and_act(FocusArea::Foundation(usize::from(i))),
            Target::Column(c) | Target::Card(c, _) => {
                self.state.focus_and_act(FocusArea::Tableau(usize::from(c)))
            }
            Target::Control(i) => {
                let Some(control) = Control::ALL.get(usize::from(i)) else {
                    return EventResult::Ignored;
                };
                self.state.handle_key(control.key(), Modifiers::default())
            }
            // The chrome. A click here is answered -- it does nothing, but it
            // does not fall through to whatever is behind it either.
            Target::Header
            | Target::Title
            | Target::Moves
            | Target::Progress
            | Target::Board
            | Target::FreeCellLabel
            | Target::FoundationLabel
            | Target::Footer
            | Target::Room
            | Target::Overlay
            | Target::OverlayTitle
            | Target::OverlayMoves
            | Target::NewGame => EventResult::Consumed,
        }
    }
}

/// The one body every event goes through, whichever side it arrives from.
///
/// The window calls it and the tests call it, so a key the tests prove works is
/// the same key the window delivers.
fn handle_event(app: &mut FreeCell, event: &Event) -> EventResult {
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

impl App for FreeCell {
    fn title(&self) -> String {
        "FreeCell".to_string()
    }

    fn app_id(&self) -> String {
        "freecell".to_string()
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the natural size is two small positive whole numbers"
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
        // The size the frame is drawn at is the size the next click is read
        // against, which is the only reason it is stored at all.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for FreeCell {
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
    let mut game = FreeCell::new();
    app::launch("freecell", &mut game)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Panicking on bad data is what a test is for; these are the lints the
    // production code above is held to and the test code below is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation
    )]

    use guitk::probe;

    use super::*;

    /// The window sizes every layout claim is checked at.
    ///
    /// The natural size, the extremes a compositor can hand a program mid-drag,
    /// and a few shapes in between. A layout that is only right at one size is
    /// the fault this program was rewritten to fix, so one size is not a test.
    const SIZES: [(f32, f32); 10] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (0.0, 0.0),
        (1.0, 1.0),
        (320.0, 240.0),
        (640.0, 480.0),
        (900.0, 400.0),
        (400.0, 900.0),
        (1280.0, 1024.0),
        (1920.0, 1080.0),
        (3840.0, 2160.0),
    ];

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

    /// Build a game with a specific tableau setup for testing.
    fn empty_game() -> GameState {
        GameState {
            free_cells: [None; FREE_CELL_COUNT],
            foundations: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            tableau: [
                Vec::new(),
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
            rng: SeededRng::new(99),
        }
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
    fn test_suit_color() {
        assert_eq!(Suit::Hearts.color(), CARD_RED);
        assert_eq!(Suit::Diamonds.color(), CARD_RED);
        assert_eq!(Suit::Clubs.color(), CARD_BLACK);
        assert_eq!(Suit::Spades.color(), CARD_BLACK);
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
    fn the_numeric_value_of_a_rank_is_its_position_in_the_order() {
        for (i, r) in Rank::ALL.iter().enumerate() {
            assert_eq!(usize::from(r.value()), i + 1, "{r:?} is out of order");
        }
    }

    #[test]
    fn test_rank_all_has_13() {
        assert_eq!(Rank::ALL.len(), 13);
    }

    #[test]
    fn test_card_can_stack_on_tableau() {
        // Red 5 on black 6: valid.
        let r5 = card(Suit::Hearts, Rank::Five);
        let b6 = card(Suit::Spades, Rank::Six);
        assert!(r5.can_stack_on_tableau(b6));

        // Black 5 on red 6: valid.
        let b5 = card(Suit::Clubs, Rank::Five);
        let r6 = card(Suit::Diamonds, Rank::Six);
        assert!(b5.can_stack_on_tableau(r6));
    }

    #[test]
    fn test_card_cannot_stack_same_color() {
        let r5 = card(Suit::Hearts, Rank::Five);
        let r6 = card(Suit::Diamonds, Rank::Six);
        assert!(!r5.can_stack_on_tableau(r6));
    }

    #[test]
    fn test_card_cannot_stack_wrong_rank() {
        let r5 = card(Suit::Hearts, Rank::Five);
        let b7 = card(Suit::Spades, Rank::Seven);
        assert!(!r5.can_stack_on_tableau(b7));
    }

    #[test]
    fn test_card_cannot_stack_ascending() {
        let r6 = card(Suit::Hearts, Rank::Six);
        let b5 = card(Suit::Spades, Rank::Five);
        assert!(!r6.can_stack_on_tableau(b5));
    }

    #[test]
    fn test_card_can_place_on_foundation_ace() {
        let ace = card(Suit::Hearts, Rank::Ace);
        assert!(ace.can_place_on_foundation(0));
    }

    #[test]
    fn test_card_can_place_on_foundation_next() {
        let two = card(Suit::Hearts, Rank::Two);
        assert!(two.can_place_on_foundation(1));
    }

    #[test]
    fn test_card_cannot_place_on_foundation_wrong() {
        let three = card(Suit::Hearts, Rank::Three);
        assert!(!three.can_place_on_foundation(0));
    }

    // ── Deal tests ─────────────────────────────────────────────────

    #[test]
    fn test_deal_52_cards() {
        let state = new_game();
        let total: usize = state.tableau.iter().map(|t| t.len()).sum();
        assert_eq!(total, 52);
    }

    #[test]
    fn test_deal_column_sizes() {
        let state = new_game();
        // First 4 columns have 7 cards, last 4 have 6.
        for i in 0..4 {
            assert_eq!(state.tableau[i].len(), 7, "column {} should have 7", i);
        }
        for i in 4..8 {
            assert_eq!(state.tableau[i].len(), 6, "column {} should have 6", i);
        }
    }

    #[test]
    fn test_deal_unique_cards() {
        let state = new_game();
        let mut all_cards = Vec::new();
        for col in &state.tableau {
            for &c in col {
                all_cards.push(c);
            }
        }
        assert_eq!(all_cards.len(), 52);
        let mut seen = std::collections::HashSet::new();
        for c in &all_cards {
            assert!(seen.insert((c.suit, c.rank)));
        }
    }

    #[test]
    fn test_deal_free_cells_empty() {
        let state = new_game();
        for fc in &state.free_cells {
            assert!(fc.is_none());
        }
    }

    #[test]
    fn test_deal_foundations_empty() {
        let state = new_game();
        for f in &state.foundations {
            assert!(f.is_empty());
        }
    }

    #[test]
    fn test_deal_initial_state() {
        let state = new_game();
        assert_eq!(state.move_count, 0);
        assert!(!state.won);
        assert!(state.selection.is_none());
        assert!(state.undo_stack.is_empty());
    }

    // ── Free cell operations ───────────────────────────────────────

    #[test]
    fn test_empty_free_cell_count_initial() {
        let state = new_game();
        assert_eq!(state.empty_free_cell_count(), 4);
    }

    #[test]
    fn test_first_empty_free_cell() {
        let mut state = empty_game();
        assert_eq!(state.first_empty_free_cell(), Some(0));
        state.free_cells[0] = Some(card(Suit::Hearts, Rank::Ace));
        assert_eq!(state.first_empty_free_cell(), Some(1));
        state.free_cells[1] = Some(card(Suit::Clubs, Rank::Two));
        assert_eq!(state.first_empty_free_cell(), Some(2));
    }

    #[test]
    fn test_no_empty_free_cell() {
        let mut state = empty_game();
        for i in 0..4 {
            state.free_cells[i] = Some(card(Suit::Hearts, Rank::ALL[i]));
        }
        assert_eq!(state.first_empty_free_cell(), None);
        assert_eq!(state.empty_free_cell_count(), 0);
    }

    #[test]
    fn test_tableau_to_freecell() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Ace);
        state.tableau[0].push(c);
        assert!(state.try_tableau_to_freecell(0));
        assert_eq!(state.free_cells[0], Some(c));
        assert!(state.tableau[0].is_empty());
        assert_eq!(state.move_count, 1);
    }

    #[test]
    fn test_tableau_to_freecell_full() {
        let mut state = empty_game();
        for i in 0..4 {
            state.free_cells[i] = Some(card(Suit::Hearts, Rank::ALL[i]));
        }
        state.tableau[0].push(card(Suit::Spades, Rank::King));
        assert!(!state.try_tableau_to_freecell(0));
    }

    #[test]
    fn test_tableau_to_specific_freecell() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.tableau[0].push(c);
        assert!(state.try_tableau_to_specific_freecell(0, 2));
        assert_eq!(state.free_cells[2], Some(c));
        assert!(state.free_cells[0].is_none());
        assert!(state.tableau[0].is_empty());
    }

    #[test]
    fn test_tableau_to_specific_freecell_occupied() {
        let mut state = empty_game();
        state.free_cells[1] = Some(card(Suit::Clubs, Rank::King));
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        assert!(!state.try_tableau_to_specific_freecell(0, 1));
    }

    #[test]
    fn test_freecell_to_tableau() {
        let mut state = empty_game();
        let top = card(Suit::Spades, Rank::Six);
        state.tableau[0].push(top);
        let fc_card = card(Suit::Hearts, Rank::Five);
        state.free_cells[0] = Some(fc_card);
        assert!(state.try_freecell_to_tableau(0, 0));
        assert_eq!(state.tableau[0].len(), 2);
        assert_eq!(*state.tableau[0].last().unwrap(), fc_card);
        assert!(state.free_cells[0].is_none());
    }

    #[test]
    fn test_freecell_to_tableau_invalid() {
        let mut state = empty_game();
        let top = card(Suit::Spades, Rank::Six);
        state.tableau[0].push(top);
        // Same color: should fail.
        let fc_card = card(Suit::Clubs, Rank::Five);
        state.free_cells[0] = Some(fc_card);
        assert!(!state.try_freecell_to_tableau(0, 0));
    }

    #[test]
    fn test_freecell_to_empty_tableau() {
        let mut state = empty_game();
        let fc_card = card(Suit::Hearts, Rank::King);
        state.free_cells[0] = Some(fc_card);
        assert!(state.try_freecell_to_tableau(0, 0));
        assert_eq!(state.tableau[0].len(), 1);
    }

    #[test]
    fn test_freecell_to_foundation() {
        let mut state = empty_game();
        let ace = card(Suit::Hearts, Rank::Ace);
        state.free_cells[0] = Some(ace);
        assert!(state.try_freecell_to_foundation(0));
        assert_eq!(state.foundations[0].len(), 1);
        assert!(state.free_cells[0].is_none());
    }

    #[test]
    fn test_freecell_to_foundation_invalid() {
        let mut state = empty_game();
        let two = card(Suit::Hearts, Rank::Two);
        state.free_cells[0] = Some(two);
        // Foundation is empty, can only place Ace.
        assert!(!state.try_freecell_to_foundation(0));
    }

    #[test]
    fn test_freecell_to_freecell() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.free_cells[0] = Some(c);
        assert!(state.try_freecell_to_freecell(0, 2));
        assert!(state.free_cells[0].is_none());
        assert_eq!(state.free_cells[2], Some(c));
    }

    #[test]
    fn test_freecell_to_freecell_occupied() {
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Hearts, Rank::Five));
        state.free_cells[1] = Some(card(Suit::Clubs, Rank::King));
        assert!(!state.try_freecell_to_freecell(0, 1));
    }

    #[test]
    fn test_freecell_to_freecell_same() {
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Hearts, Rank::Five));
        assert!(!state.try_freecell_to_freecell(0, 0));
    }

    // ── Tableau operations ─────────────────────────────────────────

    #[test]
    fn test_tableau_to_tableau() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Six));
        state.tableau[1].push(card(Suit::Hearts, Rank::Five));
        assert!(state.try_tableau_to_tableau(1, 0));
        assert_eq!(state.tableau[0].len(), 2);
        assert!(state.tableau[1].is_empty());
    }

    #[test]
    fn test_tableau_to_tableau_same_col() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Six));
        assert!(!state.try_tableau_to_tableau(0, 0));
    }

    #[test]
    fn test_tableau_to_tableau_wrong_color() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Six));
        state.tableau[1].push(card(Suit::Clubs, Rank::Five));
        // Both black: invalid.
        assert!(!state.try_tableau_to_tableau(1, 0));
    }

    #[test]
    fn test_tableau_to_tableau_wrong_rank() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Six));
        state.tableau[1].push(card(Suit::Hearts, Rank::Four));
        // 4 on 6 is wrong (need 5).
        assert!(!state.try_tableau_to_tableau(1, 0));
    }

    #[test]
    fn test_tableau_to_tableau_empty_dest() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        assert!(state.try_tableau_to_tableau(0, 1));
        assert!(state.tableau[0].is_empty());
        assert_eq!(state.tableau[1].len(), 1);
    }

    #[test]
    fn test_tableau_to_foundation() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        assert!(state.try_tableau_to_foundation(0));
        assert_eq!(state.foundations[0].len(), 1);
        assert!(state.tableau[0].is_empty());
    }

    #[test]
    fn test_tableau_to_foundation_sequence() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        assert!(state.try_tableau_to_foundation(0));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        assert!(state.try_tableau_to_foundation(1));
        assert_eq!(state.foundations[0].len(), 2);
    }

    #[test]
    fn test_tableau_to_foundation_wrong_order() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Two));
        assert!(!state.try_tableau_to_foundation(0));
    }

    #[test]
    fn test_tableau_to_foundation_empty() {
        let mut state = empty_game();
        assert!(!state.try_tableau_to_foundation(0));
    }

    #[test]
    fn test_can_place_on_tableau_empty() {
        let state = empty_game();
        // Any card can go on an empty column.
        assert!(state.can_place_on_tableau(card(Suit::Hearts, Rank::Ace), 0));
        assert!(state.can_place_on_tableau(card(Suit::Spades, Rank::King), 0));
    }

    #[test]
    fn test_can_place_on_tableau_valid() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Ten));
        assert!(state.can_place_on_tableau(card(Suit::Hearts, Rank::Nine), 0));
    }

    #[test]
    fn test_can_place_on_tableau_invalid_color() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Ten));
        assert!(!state.can_place_on_tableau(card(Suit::Clubs, Rank::Nine), 0));
    }

    #[test]
    fn test_can_place_on_tableau_invalid_rank() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Ten));
        assert!(!state.can_place_on_tableau(card(Suit::Hearts, Rank::Eight), 0));
    }

    #[test]
    fn test_can_place_on_tableau_out_of_bounds() {
        let state = empty_game();
        assert!(!state.can_place_on_tableau(card(Suit::Hearts, Rank::Ace), 99));
    }

    // ── Auto-move tests ────────────────────────────────────────────

    #[test]
    fn test_auto_move_ace() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 1);
        assert_eq!(state.foundations[0].len(), 1);
    }

    #[test]
    fn test_auto_move_ace_from_freecell() {
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Clubs, Rank::Ace));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 1);
        assert_eq!(state.foundations[2].len(), 1);
        assert!(state.free_cells[0].is_none());
    }

    #[test]
    fn test_auto_move_two_after_ace() {
        let mut state = empty_game();
        // Put all aces on foundations first.
        for &s in &Suit::ALL {
            state.foundations[s.index()].push(card(s, Rank::Ace));
        }
        // Now a two should be safe to auto-move.
        state.tableau[0].push(card(Suit::Hearts, Rank::Two));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 1);
        assert_eq!(state.foundations[0].len(), 2);
    }

    #[test]
    fn test_auto_move_chain() {
        let mut state = empty_game();
        // Set up ace, then two in different columns.
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        // Ace moves first, then two becomes eligible.
        assert_eq!(moved, 2);
        assert_eq!(state.foundations[0].len(), 2);
    }

    #[test]
    fn test_auto_move_not_safe() {
        let mut state = empty_game();
        // Hearts A on foundation.
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        // Hearts 2 is safe only if all black suit foundations have >= 1.
        // Clubs and Spades are empty, so red 2 is NOT safe yet.
        // Wait -- actually rank 2 is always safe (the rule is rank-1 for opposite).
        // For rank=2, needed=1, opposite colors need at least 1.
        // So if clubs/spades have no aces, red 2 is not safe.
        state.tableau[0].push(card(Suit::Hearts, Rank::Two));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        // Two is always safe per our rule (rank 2 returns true early).
        assert_eq!(moved, 1);
    }

    #[test]
    fn test_auto_move_three_not_safe() {
        let mut state = empty_game();
        // Hearts: A, 2 on foundation.
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        state.foundations[0].push(card(Suit::Hearts, Rank::Two));
        // Hearts 3 needs opposite colors (black) to have at least rank 2 on foundations.
        // Clubs and Spades are empty, so 3 is NOT safe.
        state.tableau[0].push(card(Suit::Hearts, Rank::Three));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 0);
    }

    #[test]
    fn test_auto_move_three_safe() {
        let mut state = empty_game();
        // Hearts: A, 2 on foundation.
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        state.foundations[0].push(card(Suit::Hearts, Rank::Two));
        // Make both black suits have at least 2.
        state.foundations[2].push(card(Suit::Clubs, Rank::Ace));
        state.foundations[2].push(card(Suit::Clubs, Rank::Two));
        state.foundations[3].push(card(Suit::Spades, Rank::Ace));
        state.foundations[3].push(card(Suit::Spades, Rank::Two));
        state.tableau[0].push(card(Suit::Hearts, Rank::Three));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 1);
    }

    #[test]
    fn test_is_safe_to_auto_move_ace() {
        let state = empty_game();
        assert!(state.is_safe_to_auto_move(card(Suit::Hearts, Rank::Ace)));
    }

    #[test]
    fn test_is_safe_to_auto_move_two() {
        let state = empty_game();
        assert!(state.is_safe_to_auto_move(card(Suit::Hearts, Rank::Two)));
    }

    #[test]
    fn test_auto_move_does_nothing_on_empty() {
        let mut state = empty_game();
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 0);
    }

    // ── Undo tests ─────────────────────────────────────────────────

    #[test]
    fn test_undo_tableau_to_freecell() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.tableau[0].push(c);
        state.try_tableau_to_freecell(0);
        assert_eq!(state.free_cells[0], Some(c));
        state.undo();
        assert!(state.free_cells[0].is_none());
        assert_eq!(state.tableau[0].last(), Some(&c));
    }

    #[test]
    fn test_undo_freecell_to_tableau() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.free_cells[0] = Some(c);
        state.try_freecell_to_tableau(0, 0);
        assert_eq!(state.tableau[0].last(), Some(&c));
        state.undo();
        assert_eq!(state.free_cells[0], Some(c));
        assert!(state.tableau[0].is_empty());
    }

    #[test]
    fn test_undo_tableau_to_tableau() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Six));
        let c = card(Suit::Hearts, Rank::Five);
        state.tableau[1].push(c);
        state.try_tableau_to_tableau(1, 0);
        assert_eq!(state.tableau[0].len(), 2);
        state.undo();
        assert_eq!(state.tableau[0].len(), 1);
        assert_eq!(state.tableau[1].last(), Some(&c));
    }

    #[test]
    fn test_undo_tableau_to_foundation() {
        let mut state = empty_game();
        let ace = card(Suit::Hearts, Rank::Ace);
        state.tableau[0].push(ace);
        state.try_tableau_to_foundation(0);
        assert_eq!(state.foundations[0].len(), 1);
        state.undo();
        assert!(state.foundations[0].is_empty());
        assert_eq!(state.tableau[0].last(), Some(&ace));
    }

    #[test]
    fn test_undo_freecell_to_foundation() {
        let mut state = empty_game();
        let ace = card(Suit::Clubs, Rank::Ace);
        state.free_cells[0] = Some(ace);
        state.try_freecell_to_foundation(0);
        assert_eq!(state.foundations[2].len(), 1);
        state.undo();
        assert!(state.foundations[2].is_empty());
        assert_eq!(state.free_cells[0], Some(ace));
    }

    #[test]
    fn test_undo_move_count() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.try_tableau_to_freecell(0);
        assert_eq!(state.move_count, 1);
        state.undo();
        assert_eq!(state.move_count, 0);
    }

    #[test]
    fn test_undo_empty() {
        let mut state = empty_game();
        // Should not crash.
        state.undo();
        assert_eq!(state.move_count, 0);
    }

    #[test]
    fn test_undo_multiple() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        state.tableau[1].push(card(Suit::Spades, Rank::King));
        state.try_tableau_to_freecell(0);
        state.try_tableau_to_freecell(1);
        assert_eq!(state.move_count, 2);
        state.undo();
        assert_eq!(state.move_count, 1);
        state.undo();
        assert_eq!(state.move_count, 0);
    }

    // ── Win detection ──────────────────────────────────────────────

    #[test]
    fn test_win_detection() {
        let mut state = empty_game();
        // Fill all foundations.
        for &suit in &Suit::ALL {
            for &rank in &Rank::ALL {
                state.foundations[suit.index()].push(card(suit, rank));
            }
        }
        state.check_win();
        assert!(state.won);
    }

    #[test]
    fn test_no_win_partial() {
        let mut state = empty_game();
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        state.check_win();
        assert!(!state.won);
    }

    #[test]
    fn test_foundation_total() {
        let mut state = empty_game();
        assert_eq!(state.foundation_total(), 0);
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        assert_eq!(state.foundation_total(), 1);
        state.foundations[2].push(card(Suit::Clubs, Rank::Ace));
        assert_eq!(state.foundation_total(), 2);
    }

    // ── Navigation tests ───────────────────────────────────────────

    #[test]
    fn test_default_focus() {
        let state = new_game();
        assert_eq!(state.focus, FocusArea::Tableau(0));
    }

    #[test]
    fn test_navigate_right_tableau() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(0);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::Tableau(1));
    }

    #[test]
    fn test_navigate_right_tableau_wrap() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(7);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::Tableau(0));
    }

    #[test]
    fn test_navigate_left_tableau() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(3);
        state.navigate_left();
        assert_eq!(state.focus, FocusArea::Tableau(2));
    }

    #[test]
    fn test_navigate_left_tableau_wrap() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(0);
        state.navigate_left();
        assert_eq!(state.focus, FocusArea::Tableau(7));
    }

    #[test]
    fn test_navigate_right_freecell() {
        let mut state = new_game();
        state.focus = FocusArea::FreeCell(0);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::FreeCell(1));
    }

    #[test]
    fn test_navigate_right_freecell_wrap() {
        let mut state = new_game();
        state.focus = FocusArea::FreeCell(3);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::FreeCell(0));
    }

    #[test]
    fn test_navigate_right_foundation() {
        let mut state = new_game();
        state.focus = FocusArea::Foundation(0);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::Foundation(1));
    }

    #[test]
    fn test_navigate_right_foundation_wrap() {
        let mut state = new_game();
        state.focus = FocusArea::Foundation(3);
        state.navigate_right();
        assert_eq!(state.focus, FocusArea::Foundation(0));
    }

    #[test]
    fn test_navigate_up_from_tableau() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(2);
        state.navigate_up();
        assert_eq!(state.focus, FocusArea::FreeCell(2));
    }

    #[test]
    fn test_navigate_up_from_tableau_right() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(5);
        state.navigate_up();
        assert_eq!(state.focus, FocusArea::Foundation(1));
    }

    #[test]
    fn test_navigate_down_from_freecell() {
        let mut state = new_game();
        state.focus = FocusArea::FreeCell(2);
        state.navigate_down();
        assert_eq!(state.focus, FocusArea::Tableau(2));
    }

    #[test]
    fn test_navigate_down_from_foundation() {
        let mut state = new_game();
        state.focus = FocusArea::Foundation(1);
        state.navigate_down();
        assert_eq!(state.focus, FocusArea::Tableau(5));
    }

    #[test]
    fn test_navigate_next_zone() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(3);
        state.navigate_next_zone();
        assert_eq!(state.focus, FocusArea::FreeCell(0));
        state.navigate_next_zone();
        assert_eq!(state.focus, FocusArea::Foundation(0));
        state.navigate_next_zone();
        assert_eq!(state.focus, FocusArea::Tableau(0));
    }

    #[test]
    fn test_navigate_up_from_foundation() {
        let mut state = new_game();
        state.focus = FocusArea::Foundation(2);
        state.navigate_up();
        assert_eq!(state.focus, FocusArea::FreeCell(2));
    }

    // ── Selection tests ────────────────────────────────────────────

    #[test]
    fn test_select_tableau() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(0);
        state.try_select();
        assert_eq!(state.selection, Some(Selection::Tableau(0)));
    }

    #[test]
    fn test_select_empty_tableau() {
        let mut state = empty_game();
        state.focus = FocusArea::Tableau(0);
        state.try_select();
        assert!(state.selection.is_none());
    }

    #[test]
    fn test_select_freecell() {
        let mut state = empty_game();
        state.free_cells[1] = Some(card(Suit::Hearts, Rank::Five));
        state.focus = FocusArea::FreeCell(1);
        state.try_select();
        assert_eq!(state.selection, Some(Selection::FreeCell(1)));
    }

    #[test]
    fn test_select_empty_freecell() {
        let mut state = empty_game();
        state.focus = FocusArea::FreeCell(0);
        state.try_select();
        assert!(state.selection.is_none());
    }

    #[test]
    fn test_select_foundation_not_allowed() {
        let mut state = empty_game();
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        state.focus = FocusArea::Foundation(0);
        state.try_select();
        assert!(state.selection.is_none());
    }

    #[test]
    fn test_escape_clears_selection() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(0);
        press(&mut state, Key::Enter);
        assert!(state.selection.is_some());
        press(&mut state, Key::Escape);
        assert!(state.selection.is_none());
    }

    // ── Keyboard action tests ──────────────────────────────────────

    #[test]
    fn test_press_enter_selects() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(0);
        press(&mut state, Key::Enter);
        assert_eq!(state.selection, Some(Selection::Tableau(0)));
    }

    #[test]
    fn test_press_space_selects() {
        let mut state = new_game();
        state.focus = FocusArea::Tableau(1);
        press(&mut state, Key::Space);
        assert_eq!(state.selection, Some(Selection::Tableau(1)));
    }

    #[test]
    fn test_press_n_new_game() {
        let mut state = new_game();
        let _old_top_0 = state.tableau_top(0);
        press(&mut state, Key::N);
        // After new game, cards are reshuffled (different seed).
        // At least move count resets.
        assert_eq!(state.move_count, 0);
    }

    #[test]
    fn test_press_z_undo() {
        let mut state = empty_game();
        // Use a non-ace card so auto-move doesn't kick in.
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        state.focus = FocusArea::Tableau(0);
        press(&mut state, Key::Enter);
        state.focus = FocusArea::FreeCell(0);
        press(&mut state, Key::Enter);
        // Card should be in free cell now.
        assert!(state.free_cells[0].is_some());
        press(&mut state, Key::Z);
        assert!(state.free_cells[0].is_none());
    }

    #[test]
    fn test_press_tab_navigates() {
        let mut state = new_game();
        assert_eq!(state.focus, FocusArea::Tableau(0));
        press(&mut state, Key::Tab);
        assert_eq!(state.focus, FocusArea::FreeCell(0));
    }

    #[test]
    fn test_press_a_auto_moves() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        press(&mut state, Key::A);
        assert_eq!(state.foundations[0].len(), 1);
    }

    // ── Activate / place tests ─────────────────────────────────────

    #[test]
    fn test_activate_select_then_place() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.tableau[0].push(c);
        state.focus = FocusArea::Tableau(0);
        state.activate();
        assert_eq!(state.selection, Some(Selection::Tableau(0)));
        state.focus = FocusArea::FreeCell(0);
        state.activate();
        assert!(state.selection.is_none());
        assert_eq!(state.free_cells[0], Some(c));
    }

    #[test]
    fn test_activate_deselect_on_same_spot() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        state.focus = FocusArea::Tableau(0);
        state.activate();
        assert!(state.selection.is_some());
        // Activate on the same spot deselects.
        state.activate();
        assert!(state.selection.is_none());
    }

    #[test]
    fn test_activate_reselect_on_different() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Five));
        state.tableau[1].push(card(Suit::Spades, Rank::King));
        state.focus = FocusArea::Tableau(0);
        state.activate();
        assert_eq!(state.selection, Some(Selection::Tableau(0)));
        // Move focus to col 1 and try to place -- fails (5 on K wrong), reselects col 1.
        state.focus = FocusArea::Tableau(1);
        state.activate();
        assert_eq!(state.selection, Some(Selection::Tableau(1)));
    }

    // ── Win state input tests ──────────────────────────────────────

    #[test]
    fn test_won_state_only_n() {
        let mut state = empty_game();
        // Set up a win.
        for &suit in &Suit::ALL {
            for &rank in &Rank::ALL {
                state.foundations[suit.index()].push(card(suit, rank));
            }
        }
        state.won = true;
        // Only N should work.
        let old_focus = state.focus;
        press(&mut state, Key::Left);
        assert_eq!(state.focus, old_focus);
        press(&mut state, Key::Right);
        assert_eq!(state.focus, old_focus);
        press(&mut state, Key::N);
        // Should be a new game.
        assert!(!state.won);
    }

    // ────────── Deal fairness ──────────
    //
    // These replace five tests that asked whether the generator was
    // deterministic, whether two seeds differ, whether a bounded draw stays in
    // range, whether a zero bound is safe, and whether a shuffle keeps its
    // elements while changing their order. All five are `randrange`'s
    // properties and are tested there. None of the five could see what was
    // wrong here, because a shuffle can keep its elements, rearrange them, and
    // still draw from a handful of the 52! orderings.

    /// Index a card into `0..52`, in the order `make_deck` builds them.
    fn deck_index(c: Card) -> usize {
        Suit::ALL.iter().position(|&s| s == c.suit).unwrap_or(0) * 13
            + Rank::ALL.iter().position(|&r| r == c.rank).unwrap_or(0)
    }

    #[test]
    fn no_card_owns_the_bottom_of_the_first_column() {
        // Column 0 is dealt first, so `tableau[0][0]` is the deepest card on
        // the board -- the worst place for a card you need early. Each of the
        // 52 should land there about 1/52 = 1.9% of the time; the old
        // generator put the ace of hearts there in 17.4% of deals.
        const DEALS: u32 = 20_000;
        let mut counts = [0_u32; 52];
        let mut state = GameState::new(42);
        for _ in 0..DEALS {
            if let Some(&c) = state.tableau[0].first() {
                counts[deck_index(c)] += 1;
            }
            // `new_game` is how a player reaches the next deal, and it reseeds
            // from the generator state -- so the draw counter restarts at zero
            // every deal. A counter-dependent defect survives that on purpose.
            state.new_game();
        }
        for (index, &count) in counts.iter().enumerate() {
            let share = 100.0 * f64::from(count) / f64::from(DEALS);
            assert!(
                share < 5.0,
                "card {index} was the bottom of column 0 in {share:.1}% of deals, not about 1.9%"
            );
        }
    }

    #[test]
    fn how_deeply_an_ace_is_buried_does_not_alternate() {
        // An ace's depth is the number of cards on top of it, and it is the
        // single biggest driver of how hard a FreeCell deal is. Depth should
        // fall off smoothly: eight columns of six or seven make depths 0..=5
        // equally likely and depth 6 rarer. The old generator gave 8.5%,
        // 15.5%, 9.7%, 16.2% -- alternating, because the depth inherited the
        // draw counter's parity.
        const DEALS: u32 = 20_000;
        let mut depths = [0_u32; 7];
        let mut state = GameState::new(42);
        for _ in 0..DEALS {
            for pile in &state.tableau {
                if let Some(row) = pile
                    .iter()
                    .position(|c| c.suit == Suit::Hearts && c.rank == Rank::Ace)
                {
                    depths[pile.len().saturating_sub(1).saturating_sub(row)] += 1;
                }
            }
            state.new_game();
        }
        // Compare each even depth against the odd one after it. Under a fair
        // shuffle the two are within a whisker of each other; under the old
        // one the odd depth was consistently the larger, by five points or
        // more of all deals.
        for pair in 0..3_usize {
            let even = f64::from(depths[pair * 2]);
            let odd = f64::from(depths[pair * 2 + 1]);
            let gap = 100.0 * (odd - even).abs() / f64::from(DEALS);
            assert!(
                gap < 3.0,
                "depths {} and {} differ by {gap:.1} points of the deals",
                pair * 2,
                pair * 2 + 1
            );
        }
    }

    // ── The rules the zones share ──────────────────────────────────

    #[test]
    fn stepping_back_from_the_start_of_a_ring_lands_on_its_end() {
        assert_eq!(wrap_back(0, 4), 3);
        assert_eq!(wrap_back(1, 4), 0);
        assert_eq!(wrap_back(3, 4), 2);
    }

    #[test]
    fn stepping_forward_off_the_end_of_a_ring_lands_on_its_start() {
        assert_eq!(wrap_forward(3, 4), 0);
        assert_eq!(wrap_forward(0, 4), 1);
        assert_eq!(wrap_forward(2, 4), 3);
    }

    #[test]
    fn a_ring_with_nothing_in_it_does_not_underflow() {
        // The three zones each wrote `len - 1` for themselves, which is a
        // subtraction below zero the moment a zone is empty. Nothing empties a
        // zone today; the point is that nothing has to be checked before this
        // is called.
        assert_eq!(wrap_back(0, 0), 0);
        assert_eq!(wrap_forward(0, 0), 0);
        assert_eq!(wrap_back(0, 1), 0);
        assert_eq!(wrap_forward(0, 1), 0);
    }

    #[test]
    fn an_index_past_the_end_of_a_ring_is_brought_back_inside_it() {
        assert_eq!(wrap_forward(9, 4), 0);
        assert_eq!(wrap_back(9, 4), 8);
    }

    #[test]
    fn the_cursor_walks_all_the_way_round_every_zone_and_back() {
        for (start, len) in [
            (FocusArea::FreeCell(0), FREE_CELL_COUNT),
            (FocusArea::Foundation(0), FOUNDATION_COUNT),
            (FocusArea::Tableau(0), TABLEAU_COLS),
        ] {
            let mut state = empty_game();
            state.focus = start;
            // Every slot is visited once, not merely the same slot arrived back
            // at: a cursor that never moves at all also ends where it began, so
            // "came back round" on its own is satisfied by standing still.
            let mut forward = Vec::new();
            for _ in 0..len {
                state.navigate_right();
                forward.push(state.focus);
            }
            assert_eq!(state.focus, start, "{start:?} did not come back round");

            let mut backward = Vec::new();
            for _ in 0..len {
                state.navigate_left();
                backward.push(state.focus);
            }
            assert_eq!(
                state.focus, start,
                "{start:?} did not come back the other way"
            );

            for (name, mut seen) in [("right", forward), ("left", backward)] {
                let visited = seen.len();
                seen.sort_by_key(|f| format!("{f:?}"));
                seen.dedup();
                assert_eq!(
                    seen.len(),
                    visited,
                    "walking {name} from {start:?} stood still on some slot"
                );
                assert_eq!(seen.len(), len, "walking {name} from {start:?} skipped one");
            }
        }
    }

    #[test]
    fn the_top_row_the_cursor_rises_into_is_split_where_the_cells_end() {
        // Both halves used to be found with a literal `4`, which is
        // `FREE_CELL_COUNT` written out.
        for col in 0..TABLEAU_COLS {
            let mut state = empty_game();
            state.focus = FocusArea::Tableau(col);
            state.navigate_up();
            let want = if col < FREE_CELL_COUNT {
                FocusArea::FreeCell(col)
            } else {
                FocusArea::Foundation(col - FREE_CELL_COUNT)
            };
            assert_eq!(state.focus, want, "column {col} rose to the wrong slot");
        }
    }

    #[test]
    fn a_foundation_the_cursor_drops_from_lands_on_the_column_under_it() {
        for idx in 0..FOUNDATION_COUNT {
            let mut state = empty_game();
            state.focus = FocusArea::Foundation(idx);
            state.navigate_down();
            assert_eq!(
                state.focus,
                FocusArea::Tableau(idx + FREE_CELL_COUNT),
                "foundation {idx} dropped to the wrong column"
            );
        }
    }

    #[test]
    fn a_king_is_never_placeable_on_a_foundation_that_is_already_full() {
        // `== top + 1` on a `u8` wraps 255 to 0, which would have called an ace
        // placeable on a foundation whose top read 255.
        let ace = card(Suit::Hearts, Rank::Ace);
        assert!(ace.can_place_on_foundation(0));
        assert!(!ace.can_place_on_foundation(u8::MAX));
        assert!(!ace.can_place_on_foundation(13));
        assert!(card(Suit::Hearts, Rank::King).can_place_on_foundation(12));
    }

    #[test]
    fn a_card_sent_home_is_in_exactly_one_place_afterwards() {
        // Both auto-run loops used to take the card off the board on the line
        // before the foundation was reached.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.free_cells[0] = Some(card(Suit::Spades, Rank::Ace));

        let moved = state.auto_move_to_foundations(AutoRun::Asked);

        assert_eq!(moved, 2, "not both aces went home");
        assert!(state.tableau[0].is_empty());
        assert_eq!(state.free_cells[0], None);
        assert_eq!(state.foundation_total(), 2, "a card was made or lost");
    }

    #[test]
    fn a_card_that_is_not_there_is_not_sent_home() {
        let mut state = empty_game();
        let ace = card(Suit::Hearts, Rank::Ace);
        // The column is empty, so there is nothing to take.
        assert!(!state.send_home(MoveLocation::Tableau(0), ace, true));
        assert_eq!(state.foundation_total(), 0, "a card came from nowhere");
        assert!(state.undo_stack.is_empty(), "a step was recorded anyway");
    }

    #[test]
    fn a_card_that_cannot_go_home_stays_where_it_is() {
        // The card is really there and is really refused: the Five of Hearts
        // has no Ace under it on the foundation. What is being pinned down is
        // the *order* of the two -- a version that lifts the card off the board
        // before finding out whether it has anywhere to go loses it entirely,
        // and an empty column cannot show that, because there is nothing there
        // to lose.
        let mut state = empty_game();
        let five = card(Suit::Hearts, Rank::Five);
        if let Some(col) = state.tableau.get_mut(0) {
            col.push(five);
        }
        assert!(!state.send_home(MoveLocation::Tableau(0), five, true));
        assert_eq!(
            state.tableau.first().map(Vec::len),
            Some(1),
            "the card was taken off the board and never put back"
        );
        assert_eq!(state.foundation_total(), 0, "it went home regardless");
        assert!(state.undo_stack.is_empty(), "a step was recorded anyway");
    }

    #[test]
    fn a_column_the_board_does_not_have_sends_nothing_home() {
        let mut state = empty_game();
        let ace = card(Suit::Hearts, Rank::Ace);
        assert!(!state.send_home(MoveLocation::Tableau(99), ace, true));
        assert!(!state.send_home(MoveLocation::FreeCell(99), ace, true));
        assert_eq!(state.foundation_total(), 0);
    }

    // ── Rendering tests ────────────────────────────────────────────

    /// The window wrapped around a game whose deal is a fact, not a draw.
    fn app() -> FreeCell {
        FreeCell::with_seed(42)
    }

    /// Where a control sits on the strip.
    ///
    /// Found rather than written down, so a test says which *button* it means
    /// and keeps meaning it when the strip gains one.
    fn control_index(want: Control) -> usize {
        Control::ALL
            .iter()
            .position(|c| *c == want)
            .expect("every control is in Control::ALL")
    }

    /// The same window around a board a test built by hand.
    fn app_with(state: GameState) -> FreeCell {
        let mut a = FreeCell::with_seed(42);
        a.state = state;
        a
    }

    /// A won board: every card home.
    fn won_board() -> GameState {
        let mut state = empty_game();
        for &suit in &Suit::ALL {
            for &rank in &Rank::ALL {
                state.foundations[suit.index()].push(card(suit, rank));
            }
        }
        state.won = true;
        state
    }

    /// Every string the frame draws, in paint order.
    fn drawn_text(app: &FreeCell, size: (f32, f32)) -> Vec<String> {
        app.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    // ── The room reading ───────────────────────────────────────────

    #[test]
    fn the_footer_reports_how_much_room_is_left() {
        // The number a freecell player plans against -- it caps how long a run
        // can be shifted -- and it is on screen nowhere else.
        //
        // One cell of four is occupied, deliberately not two: with two, the
        // count of free cells and the count of full ones are both 2, and a
        // reading that reports the wrong one of them reads the same.
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Spades, Rank::King));
        state.tableau[0].push(card(Suit::Clubs, Rank::King));
        let app = app_with(state);

        let text = drawn_text(&app, FreeCell::SIZE);
        assert!(
            text.iter().any(|t| t == "3 cells   7 columns"),
            "the room reading is not what the board says: {text:?}"
        );
    }

    #[test]
    fn the_room_reading_follows_the_board() {
        let mut app = app();
        let before = drawn_text(&app, FreeCell::SIZE);
        app.state.focus = FocusArea::Tableau(0);
        app.key_at(&probe::press(Key::F), FreeCell::SIZE);
        let after = drawn_text(&app, FreeCell::SIZE);

        assert!(
            before.iter().any(|t| t.starts_with("4 cells")),
            "a fresh deal should have four cells free: {before:?}"
        );
        assert!(
            after.iter().any(|t| t.starts_with("3 cells")),
            "parking a card did not change the reading: {after:?}"
        );
    }

    #[test]
    fn the_keyboard_hint_goes_before_the_room_reading_does() {
        // Two captions share the right of the strip and only one of them is a
        // fact about this board; the reminder is what a narrowing window drops
        // first.
        let app = app();
        let wide = drawn_text(&app, (1600.0, 800.0));
        let narrow = drawn_text(&app, (760.0, 800.0));

        let hint = |t: &[String]| t.iter().any(|s| s.starts_with("Tab/Arrows"));
        let room = |t: &[String]| t.iter().any(|s| s.ends_with("columns"));

        assert!(hint(&wide) && room(&wide), "a wide window lost one of them");
        assert!(!hint(&narrow), "the reminder held its ground: {narrow:?}");
        assert!(room(&narrow), "the reading went first: {narrow:?}");
    }

    #[test]
    fn a_footer_with_no_room_for_either_caption_still_draws_its_buttons() {
        let app = app();
        for width in [0.0, 1.0, 120.0, 320.0] {
            let text = drawn_text(&app, (width, 800.0));
            assert!(
                !text.iter().any(|s| s.ends_with("columns")),
                "the reading was squeezed in at {width}: {text:?}"
            );
        }
        // And nothing about the narrow cases escapes the window.
        for width in [0.0, 1.0, 120.0, 320.0] {
            assert!(
                app.frame(width, 800.0).is_balanced(),
                "the frame is unbalanced at {width}"
            );
        }
    }

    #[test]
    fn a_click_on_the_room_reading_does_not_move_a_card() {
        let mut app = app();
        let cells = app.state.free_cells;
        let moves = app.state.move_count;
        assert_eq!(
            probe::click(&mut app, Target::Room),
            EventResult::Consumed,
            "the reading let the click fall through it"
        );
        assert_eq!(app.state.free_cells, cells);
        assert_eq!(app.state.move_count, moves);
    }

    #[test]
    fn the_frame_draws_something() {
        assert!(
            !app()
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands()
                .is_empty()
        );
        let empty = app_with(empty_game());
        assert!(
            !empty
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands()
                .is_empty(),
            "a board with no cards on it drew nothing at all"
        );
    }

    #[test]
    fn the_header_names_the_game_and_reports_both_numbers() {
        let text = drawn_text(&app(), (WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(text.iter().any(|t| t == "FreeCell"), "no title: {text:?}");
        assert!(
            text.iter().any(|t| t.starts_with("Moves: ")),
            "no move count: {text:?}"
        );
        assert!(
            text.iter().any(|t| t.starts_with("Foundations: ")),
            "no progress reading: {text:?}"
        );
    }

    #[test]
    fn the_win_sheet_says_so_and_offers_a_new_game() {
        let app = app_with(won_board());
        let text = drawn_text(&app, (WINDOW_WIDTH, WINDOW_HEIGHT));
        assert!(text.iter().any(|t| t == "You Win!"), "no banner: {text:?}");
        for target in [
            Target::Overlay,
            Target::OverlayTitle,
            Target::OverlayMoves,
            Target::NewGame,
        ] {
            assert!(
                probe::is_visible(&app, target),
                "{target:?} is missing from the win sheet"
            );
        }
    }

    #[test]
    fn the_win_sheet_is_only_there_once_the_game_is_won() {
        assert!(
            !probe::is_visible(&app(), Target::Overlay),
            "the win sheet covered a game still being played"
        );
    }

    #[test]
    fn the_cursor_and_the_selection_are_both_drawn() {
        // Two separate marks with two separate colours: the cursor says where
        // the next press lands, the selection says which card is in hand. A
        // frame that showed only one of them would leave the player guessing
        // which column they had picked up from.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.selection = Some(Selection::Tableau(0));
        state.focus = FocusArea::Tableau(3);
        let app = app_with(state);
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let strokes: Vec<Color> = f
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert!(
            strokes.contains(&SELECTED_HIGHLIGHT),
            "the card in hand was not marked"
        );
        assert!(
            strokes.contains(&CURSOR_HIGHLIGHT),
            "the cursor was not marked"
        );
    }

    // ── Layout position tests ──────────────────────────────────────

    #[test]
    fn the_columns_are_evenly_spaced_at_every_size() {
        for (w, h) in SIZES {
            let t = Table::new(Layout::new(w, h).body, 7);
            let steps: Vec<f32> = (1..TABLEAU_COLS)
                .map(|c| t.slot_x(c) - t.slot_x(c - 1))
                .collect();
            for (i, step) in steps.iter().enumerate() {
                assert!(
                    (step - t.step).abs() < 0.01,
                    "at {w}x{h} column {} sat {step} from its neighbour, not {}",
                    i + 1,
                    t.step
                );
            }
            // Evenly spaced is not enough on its own: a step of exactly one
            // card width is even too, and draws eight columns edge to edge.
            if t.card_w > 0.0 {
                assert!(
                    t.step > t.card_w,
                    "at {w}x{h} the columns are drawn with no gap between them"
                );
            }
        }
    }

    #[test]
    fn the_table_is_centred_in_whatever_it_was_given() {
        // The leftover width is margin on both sides, rather than a band of
        // nothing down one of them.
        for (w, h) in SIZES {
            let body = Layout::new(w, h).body;
            let t = Table::new(body, 7);
            let left = t.slot_x(0) - body.x;
            let right = body.right() - (t.slot_x(TABLEAU_COLS - 1) + t.card_w);
            assert!(
                (left - right).abs() < 0.01,
                "at {w}x{h} the table sits {left} from the left and {right} from the right"
            );
        }
    }

    #[test]
    fn the_top_row_and_the_tableau_stand_on_one_grid() {
        // Eight slots above and eight columns below. They used to be placed by
        // two functions that happened to compute the same thing, which is two
        // places for one grid to be described and one of them to drift.
        for (w, h) in SIZES {
            let t = Table::new(Layout::new(w, h).body, 7);
            for col in 0..TABLEAU_COLS {
                let above = t.top_slot(col).x;
                let below = t.card_at(col, 0).x;
                assert!(
                    (above - below).abs() < 0.01,
                    "at {w}x{h} column {col} sat at {below} under a slot at {above}"
                );
            }
            // Under, not over. Sharing a column of x tells you nothing about
            // which of the two rows is drawn on top of the other.
            assert!(
                t.tableau_y >= t.top_slot(0).bottom() - 0.01,
                "at {w}x{h} the tableau starts at {} inside a top row ending at {}",
                t.tableau_y,
                t.top_slot(0).bottom()
            );
        }
    }

    #[test]
    fn the_whole_table_fits_the_space_it_was_given() {
        // Nine card widths across, and a deepest column that ends inside the
        // area rather than off the bottom of it. A column deeper than about
        // twenty-six cards used to run off a fixed 800-tall board with nothing
        // to tighten it, so its lowest cards could be neither seen nor reached.
        for (w, h) in SIZES {
            for deepest in [0_usize, 1, 7, 19, 26, 40, 52] {
                let body = Layout::new(w, h).body;
                let t = Table::new(body, deepest);
                let right = t.slot_x(TABLEAU_COLS - 1) + t.card_w;
                assert!(
                    right <= body.right() + 0.01,
                    "at {w}x{h} the table ended at {right}, past {}",
                    body.right()
                );
                let bottom = t.card_at(0, deepest.saturating_sub(1)).bottom();
                assert!(
                    bottom <= body.bottom() + 0.01,
                    "at {w}x{h} a {deepest}-card column ended at {bottom}, past {}",
                    body.bottom()
                );
            }
        }
    }

    #[test]
    fn the_bands_stay_inside_the_window_at_every_size() {
        // What this replaces filled a hardcoded 900x800 whatever it was given.
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, r) in [("header", l.header), ("body", l.body), ("footer", l.footer)] {
                assert!(
                    r.x >= -0.01 && r.y >= -0.01,
                    "the {name} at {w}x{h} starts at {r:?}, outside the window"
                );
                assert!(
                    r.right() <= w + 0.01 && r.bottom() <= h + 0.01,
                    "the {name} at {w}x{h} ends at {r:?}, past {w}x{h}"
                );
                assert!(
                    r.w >= 0.0 && r.h >= 0.0,
                    "the {name} at {w}x{h} is {r:?}, which draws inside out"
                );
            }
        }
    }

    #[test]
    fn the_bands_run_down_the_window_in_order_and_do_not_overlap() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            assert!(
                l.header.bottom() <= l.body.y + 0.01,
                "at {w}x{h} the header reaches into the body"
            );
            assert!(
                l.body.bottom() <= l.footer.y + 0.01,
                "at {w}x{h} the body reaches into the footer"
            );
        }
    }

    #[test]
    fn the_bands_are_shares_of_the_height_not_the_width() {
        // A band cut from the width grows when the window is only made wider,
        // which is the wrong axis: the header holds one line of text whatever
        // shape the window is. Widening does trim it a little, because the
        // padding round it is a share of the shorter side -- so the rule is
        // that widening never makes a band taller.
        let narrow = Layout::new(400.0, 900.0);
        let wide = Layout::new(1600.0, 900.0);
        assert!(
            wide.header.h <= narrow.header.h + 0.01,
            "the header grew when only the width did"
        );
        assert!(
            wide.footer.h <= narrow.footer.h + 0.01,
            "the footer grew when only the width did"
        );
        let short = Layout::new(900.0, 300.0);
        let tall = Layout::new(900.0, 900.0);
        assert!(
            short.header.h < tall.header.h,
            "the header did not shrink with the window's height"
        );
        assert!(
            short.footer.h < tall.footer.h,
            "the footer did not shrink with the window's height"
        );
    }

    #[test]
    fn the_type_sizes_come_from_the_window_and_stay_legible() {
        for (w, h) in SIZES {
            let l = Layout::new(w, h);
            for (name, size) in [
                ("head", l.head),
                ("font", l.font),
                ("title", l.title),
                ("small", l.small),
            ] {
                assert!(
                    size >= 7.0,
                    "the {name} type is {size} at {w}x{h}, below anything readable"
                );
            }
            assert!(l.title > l.font, "the title is not the largest reading");
            assert!(l.small < l.font, "the small type is not the smallest");
        }
        assert!(
            Layout::new(900.0, 2160.0).title > Layout::new(900.0, 400.0).title,
            "the title did not grow with the window"
        );
    }

    #[test]
    fn a_deeper_column_is_never_drawn_looser_than_a_shallow_one() {
        // The fan tightens as the column deepens, and never the other way: a
        // board that spread its cards further apart the more of them there were
        // would run off the bottom faster the more it needed not to.
        let body = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).body;
        let mut last = f32::INFINITY;
        for deepest in 1..=52 {
            let cascade = Table::new(body, deepest).cascade;
            assert!(
                cascade <= last + 0.01,
                "a {deepest}-card column fanned by {cascade}, wider than {last}"
            );
            last = cascade;
        }
    }

    #[test]
    fn a_column_of_one_card_is_fitted_as_if_nothing_hung_below_it() {
        // The deepest column has `deepest - 1` cards fanned below its first, not
        // `deepest`. Counting one step too many makes the board solve for a
        // height it does not need, and every card comes out smaller than the
        // space allows.
        let area = Rect::new(0.0, 0.0, 900.0, 200.0);
        let none = Table::new(area, 0);
        let one = Table::new(area, 1);
        assert!(
            one.card_w < area.w / 9.0,
            "the area is not height-bound, so this proves nothing"
        );
        assert!(
            (none.card_w - one.card_w).abs() < 0.01,
            "a single card was fitted as {} where nothing at all was fitted as {}",
            one.card_w,
            none.card_w
        );
        assert!(
            Table::new(area, 2).card_w < one.card_w,
            "a second card cost the board no height at all"
        );
    }

    #[test]
    fn a_column_can_be_clicked_below_its_last_card() {
        // A column's reachable box runs to the table floor, not to the bottom of
        // the cards in it. Otherwise a short column -- and an empty one, which is
        // the only kind you can move a king to -- has almost nothing to aim at.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::King));
        let mut app = app_with(state);

        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let card_rect = f.rect_of(|t| matches!(t, Target::Card(0, _))).unwrap();
        let body = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT).body;
        let below = card_rect.bottom() + (body.bottom() - card_rect.bottom()) / 2.0;
        assert!(
            below > card_rect.bottom(),
            "there is no empty space under the card to click"
        );

        assert_eq!(
            app.click(card_rect.centre().0, below, MouseButton::Left),
            EventResult::Consumed,
            "the space under the column reached nothing"
        );
        assert_eq!(
            app.state.selection,
            Some(Selection::Tableau(0)),
            "a click under the column did not pick its card up"
        );
    }

    #[test]
    fn an_empty_column_can_still_be_clicked() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::King));
        let mut app = app_with(state);
        app.state.focus = FocusArea::Tableau(0);
        app.state.activate();
        assert_eq!(app.state.selection, Some(Selection::Tableau(0)));

        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (x, y) = f.rect_of(|t| *t == Target::Column(3)).unwrap().centre();
        app.click(x, y, MouseButton::Left);

        assert_eq!(
            app.state.tableau[3],
            vec![card(Suit::Spades, Rank::King)],
            "the king could not be moved to the empty column"
        );
    }

    // ── Edge case tests ────────────────────────────────────────────

    #[test]
    fn test_foundation_top_empty() {
        let state = empty_game();
        assert!(state.foundation_top(0).is_none());
    }

    #[test]
    fn test_foundation_top_value_empty() {
        let state = empty_game();
        assert_eq!(state.foundation_top_value(0), 0);
    }

    #[test]
    fn test_foundation_top_value_with_card() {
        let mut state = empty_game();
        state.foundations[0].push(card(Suit::Hearts, Rank::Ace));
        assert_eq!(state.foundation_top_value(0), 1);
    }

    #[test]
    fn test_tableau_top_empty() {
        let state = empty_game();
        assert!(state.tableau_top(0).is_none());
    }

    #[test]
    fn test_tableau_top_with_cards() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[0].push(card(Suit::Spades, Rank::Two));
        assert_eq!(state.tableau_top(0), Some(card(Suit::Spades, Rank::Two)));
    }

    #[test]
    fn test_empty_tableau_count() {
        let state = empty_game();
        assert_eq!(state.empty_tableau_count(), 8);
    }

    #[test]
    fn test_empty_tableau_count_with_cards() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[3].push(card(Suit::Clubs, Rank::King));
        assert_eq!(state.empty_tableau_count(), 6);
    }

    #[test]
    fn test_new_game_resets() {
        let mut state = new_game();
        state.move_count = 50;
        state.free_cells[0] = Some(card(Suit::Hearts, Rank::Ace));
        state.new_game();
        assert_eq!(state.move_count, 0);
        assert!(state.free_cells[0].is_none());
    }

    #[test]
    fn test_try_tableau_to_foundation_out_of_bounds() {
        let mut state = empty_game();
        assert!(!state.try_tableau_to_foundation(99));
    }

    #[test]
    fn test_try_tableau_to_tableau_out_of_bounds() {
        let mut state = empty_game();
        assert!(!state.try_tableau_to_tableau(0, 99));
        assert!(!state.try_tableau_to_tableau(99, 0));
    }

    #[test]
    fn test_try_freecell_to_foundation_empty_cell() {
        let mut state = empty_game();
        assert!(!state.try_freecell_to_foundation(0));
    }

    #[test]
    fn test_try_freecell_to_tableau_empty_cell() {
        let mut state = empty_game();
        assert!(!state.try_freecell_to_tableau(0, 0));
    }

    #[test]
    fn test_try_tableau_to_freecell_empty_col() {
        let mut state = empty_game();
        assert!(!state.try_tableau_to_freecell(0));
    }

    #[test]
    fn test_try_specific_freecell_out_of_bounds() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        assert!(!state.try_tableau_to_specific_freecell(0, 99));
    }

    // ── App wrapper tests ──────────────────────────────────────────

    #[test]
    fn a_new_game_starts_at_nothing_played_and_nothing_won() {
        let app = app();
        assert_eq!(app.state.move_count, 0);
        assert!(!app.state.won);
        assert_eq!(app.size(), (WINDOW_WIDTH, WINDOW_HEIGHT));
    }

    #[test]
    fn a_launch_does_not_deal_the_one_board_that_was_hardcoded() {
        // The deal used to be `GameState::new(42)`, so every launch on every
        // machine got one board for ever -- a fifty-two card game with one
        // layout in it.
        //
        // Two `new()` games cannot be compared with each other here: off
        // Slate OS there is no kernel randomness to open, so `seed_from_system`
        // answers with its fallback and the two would agree for a reason that
        // has nothing to do with this program. What can be checked is that the
        // seed is no longer the literal 42.
        let dealt = FreeCell::new();
        let old = FreeCell::with_seed(42);
        let differs = (0..TABLEAU_COLS).any(|c| dealt.state.tableau[c] != old.state.tableau[c]);
        assert!(differs, "a fresh game still deals the hardcoded board");
    }

    #[test]
    fn a_key_press_reaches_the_game_and_a_release_does_not() {
        // A release that moved the cursor would move it twice for every press,
        // because a key that goes down also comes back up.
        let mut app = app();
        assert_eq!(
            app.key_at(&probe::press(Key::Right), FreeCell::SIZE),
            EventResult::Consumed,
            "the arrow key was not acted on"
        );
        assert_eq!(app.state.focus, FocusArea::Tableau(1));
        assert_eq!(
            app.key_at(&probe::release(Key::Right), FreeCell::SIZE),
            EventResult::Ignored,
            "the key coming back up was acted on too"
        );
        assert_eq!(app.state.focus, FocusArea::Tableau(1));
    }

    #[test]
    fn a_key_the_game_does_not_know_is_answered_as_ignored() {
        // `Ignored` is what stops the window redrawing for a keystroke that
        // changed nothing. Every key used to look the same, because the handler
        // returned nothing at all.
        let mut app = app();
        assert_eq!(
            app.key_at(&probe::press(Key::F1), FreeCell::SIZE),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_click_picks_a_card_up_and_puts_it_down() {
        // The whole pointer half of the program: there was none at all before,
        // under a help line advertising six controls.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Seven));
        let mut app = app_with(state);

        assert_eq!(
            probe::click(&mut app, Target::Card(0, 0)),
            EventResult::Consumed,
            "the card was not clickable"
        );
        assert_eq!(app.state.selection, Some(Selection::Tableau(0)));

        assert_eq!(
            probe::click(&mut app, Target::Column(3)),
            EventResult::Consumed,
            "the empty column was not clickable"
        );
        assert!(app.state.tableau[0].is_empty(), "the card did not leave");
        assert_eq!(app.state.tableau[3].len(), 1, "the card did not arrive");
    }

    #[test]
    fn a_click_on_a_free_cell_puts_a_card_there() {
        // A seven rather than an ace: an ace parked in a free cell is safe to
        // send home, so the cascade would empty the cell again in the same
        // press and the test would be reading a board it never meant to make.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Clubs, Rank::Seven));
        let mut app = app_with(state);
        probe::click(&mut app, Target::Card(0, 0));
        probe::click(&mut app, Target::FreeCell(2));
        assert_eq!(
            app.state.free_cells[2],
            Some(card(Suit::Clubs, Rank::Seven)),
            "the free cell stayed empty"
        );
        assert!(app.state.tableau[0].is_empty(), "the card did not leave");
    }

    #[test]
    fn a_click_on_a_foundation_sends_a_card_home() {
        let mut state = empty_game();
        state.free_cells[1] = Some(card(Suit::Clubs, Rank::Ace));
        let mut app = app_with(state);
        probe::click(&mut app, Target::FreeCell(1));
        assert_eq!(app.state.selection, Some(Selection::FreeCell(1)));
        probe::click(&mut app, Target::Foundation(byte(Suit::Clubs.index())));
        assert_eq!(
            app.state.foundations[Suit::Clubs.index()].len(),
            1,
            "the ace did not reach its foundation"
        );
    }

    #[test]
    fn every_control_on_the_strip_does_what_its_caption_says() {
        // The strip is walked from `Control::ALL`, so a control that is drawn is
        // a control that is wired. This checks the wiring end: each button runs
        // the key it names.
        for (i, control) in Control::ALL.into_iter().enumerate() {
            let mut by_click = app();
            let mut by_key = app();
            assert_eq!(
                probe::click(&mut by_click, Target::Control(byte(i))),
                EventResult::Consumed,
                "{control:?} was not clickable"
            );
            by_key.key_at(&probe::press(control.key()), FreeCell::SIZE);
            assert_eq!(
                by_click.state.move_count, by_key.state.move_count,
                "{control:?} and its key left different move counts"
            );
        }
    }

    #[test]
    fn the_undo_button_is_refused_on_a_won_board_just_as_the_key_is() {
        // The button runs the key rather than the method the key calls, which is
        // what makes one refusal cover both ways in.
        let mut app = app_with(won_board());
        probe::click(
            &mut app,
            Target::Control(byte(control_index(Control::Undo))),
        );
        assert!(app.state.won, "the win was undone from the sheet");
    }

    #[test]
    fn a_click_on_the_win_sheet_does_not_reach_the_board_behind_it() {
        let mut state = won_board();
        state.foundations[0].pop();
        state.tableau[0].push(card(Suit::Hearts, Rank::King));
        state.won = true;
        let mut app = app_with(state);
        // Where the first column's card is, under the sheet.
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let rect = f.rect_of(|t| matches!(t, Target::Card(0, _))).unwrap();
        let (x, y) = rect.centre();
        // The sheet is guarded twice over, and this is the first of the two:
        // it records a hit box across the whole window, so what a click at the
        // card's own position *finds* is the sheet, not the card. The second
        // guard -- the `won` branch in `click` -- catches anything that gets
        // past this one. Either alone stops the board being played; asserting
        // only the outcome would therefore pass with either one deleted, so
        // the covering hit box is asserted here in its own right.
        assert_eq!(
            f.hit_test(x, y),
            Some(Target::Overlay),
            "the sheet left the card underneath it reachable"
        );
        assert_eq!(
            app.click(x, y, MouseButton::Left),
            EventResult::Consumed,
            "the click fell through the sheet"
        );
        assert!(
            app.state.selection.is_none(),
            "a card under the sheet was picked up"
        );
        // Not picking it up is not enough on its own: the card left under the
        // sheet is a King with its Queen already home, so a click that reaches
        // the board sends it straight to the foundation without ever becoming
        // a selection. What the sheet has to stop is the board changing at all.
        assert_eq!(
            app.state.tableau.first().map(Vec::len),
            Some(1),
            "the card under the sheet was played"
        );
        assert_eq!(
            app.state.foundation_total(),
            51,
            "a card reached the foundation through the sheet"
        );
        assert_eq!(
            app.state.move_count, 0,
            "a move was counted through the sheet"
        );
    }

    #[test]
    fn a_click_on_the_new_game_line_deals_a_new_board() {
        let mut app = app_with(won_board());
        assert_eq!(
            probe::click(&mut app, Target::NewGame),
            EventResult::Consumed
        );
        assert!(!app.state.won, "the sheet stayed up");
        assert_eq!(app.state.foundation_total(), 0, "the old board was kept");
    }

    #[test]
    fn a_click_on_a_button_that_is_not_the_left_one_does_nothing() {
        let mut app = app();
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (x, y) = f
            .rect_of(|t| matches!(t, Target::Card(0, _)))
            .unwrap()
            .centre();
        assert_eq!(app.click(x, y, MouseButton::Right), EventResult::Ignored);
        assert!(app.state.selection.is_none());
    }

    // ── The Free move ──────────────────────────────────────────────

    #[test]
    fn f_parks_the_focused_column_in_a_free_cell() {
        // The move the game is named after. It was implemented and tested and
        // reachable by nothing before the window went on.
        let mut state = empty_game();
        state.tableau[3].push(card(Suit::Spades, Rank::Nine));
        state.focus = FocusArea::Tableau(3);
        let mut app = app_with(state);

        app.key_at(&probe::press(Key::F), FreeCell::SIZE);

        assert_eq!(
            app.state.free_cells[0],
            Some(card(Suit::Spades, Rank::Nine)),
            "the card did not reach a cell"
        );
        assert!(
            app.state.tableau[3].is_empty(),
            "the card is in both places"
        );
        assert_eq!(app.state.move_count, 1, "one press is one move");
    }

    #[test]
    fn the_free_button_parks_a_card_just_as_the_key_does() {
        let mut by_click = {
            let mut state = empty_game();
            state.tableau[0].push(card(Suit::Spades, Rank::Nine));
            app_with(state)
        };
        let mut by_key = {
            let mut state = empty_game();
            state.tableau[0].push(card(Suit::Spades, Rank::Nine));
            app_with(state)
        };

        probe::click(
            &mut by_click,
            Target::Control(byte(control_index(Control::Free))),
        );
        by_key.key_at(&probe::press(Key::F), FreeCell::SIZE);

        // That the two agree is only half of it: a button and a key that both
        // do nothing agree perfectly. The card has to have actually moved.
        assert_eq!(
            by_key.state.free_cells[0],
            Some(card(Suit::Spades, Rank::Nine)),
            "the key parked nothing"
        );
        assert!(
            by_key.state.tableau[0].is_empty(),
            "the card was left behind"
        );
        assert_eq!(
            by_click.state.free_cells, by_key.state.free_cells,
            "the button and the key parked different cards"
        );
        assert_eq!(by_click.state.move_count, by_key.state.move_count);
        assert_eq!(by_click.state.move_count, 1, "no move was counted");
    }

    #[test]
    fn f_does_nothing_when_the_cursor_is_not_on_a_column() {
        // A free cell and a foundation have no card to send to a cell, so the
        // press does nothing rather than guessing a column.
        for focus in [FocusArea::FreeCell(0), FocusArea::Foundation(0)] {
            let mut state = empty_game();
            state.tableau[0].push(card(Suit::Spades, Rank::Nine));
            state.focus = focus;
            let mut app = app_with(state);

            app.key_at(&probe::press(Key::F), FreeCell::SIZE);

            assert_eq!(
                app.state.free_cells[0], None,
                "{focus:?} guessed a column to park"
            );
            assert_eq!(app.state.move_count, 0, "{focus:?} counted a move");
        }
    }

    #[test]
    fn f_does_nothing_when_every_cell_is_full() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Nine));
        for (i, rank) in [Rank::King, Rank::Queen, Rank::Jack, Rank::Ten]
            .into_iter()
            .enumerate()
        {
            state.free_cells[i] = Some(card(Suit::Spades, rank));
        }
        let mut app = app_with(state);

        app.key_at(&probe::press(Key::F), FreeCell::SIZE);

        assert_eq!(
            app.state.tableau[0].len(),
            1,
            "the card left a column with nowhere to go"
        );
        assert_eq!(app.state.move_count, 0);
    }

    #[test]
    fn a_parked_card_comes_back_on_one_undo() {
        let mut state = empty_game();
        state.tableau[2].push(card(Suit::Spades, Rank::Nine));
        state.focus = FocusArea::Tableau(2);
        let mut app = app_with(state);

        app.key_at(&probe::press(Key::F), FreeCell::SIZE);
        app.key_at(&probe::press(Key::Z), FreeCell::SIZE);

        assert_eq!(
            app.state.tableau[2],
            vec![card(Suit::Spades, Rank::Nine)],
            "one undo did not put the parked card back"
        );
        assert_eq!(app.state.free_cells[0], None);
        assert_eq!(app.state.move_count, 0, "the count did not come back too");
    }

    #[test]
    fn parking_an_ace_sends_it_straight_home() {
        // Parking runs the same follow-on auto-move every placement does, so a
        // card that is safe to send home does not sit in a cell for a turn.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.focus = FocusArea::Tableau(0);
        let mut app = app_with(state);

        app.key_at(&probe::press(Key::F), FreeCell::SIZE);

        assert_eq!(
            app.state.foundations[Suit::Hearts.index()],
            vec![card(Suit::Hearts, Rank::Ace)],
            "the ace stopped in a cell"
        );
        assert_eq!(app.state.free_cells[0], None);
    }

    #[test]
    fn f_is_refused_on_a_won_board() {
        // The board is marked won but still has a card in a column with the
        // cursor on it, so `F` would have something to do if the win guard let
        // it through. A board with nothing left in any column cannot tell a
        // refused press from a press with no work to do.
        let mut state = won_board();
        state.foundations[0].pop();
        state.tableau[0].push(card(Suit::Hearts, Rank::King));
        state.focus = FocusArea::Tableau(0);
        let mut app = app_with(state);
        app.key_at(&probe::press(Key::F), FreeCell::SIZE);
        assert!(app.state.won, "the win was played out of");
        assert_eq!(app.state.free_cells, [None; FREE_CELL_COUNT]);
        assert_eq!(
            app.state.tableau.first().map(Vec::len),
            Some(1),
            "the card was played off a won board"
        );
        assert_eq!(app.state.move_count, 0, "a move was counted on a won board");
    }

    #[test]
    fn a_click_outside_everything_drawn_is_ignored() {
        let mut app = app();
        assert_eq!(
            app.click(-10.0, -10.0, MouseButton::Left),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_resize_is_what_the_next_click_is_read_against() {
        // A click has to be measured against the picture the player was looking
        // at. If the size were not carried, a window resized to twice its width
        // would still answer clicks against the old one.
        let mut app = app();
        handle_event(
            &mut app,
            &Event::Resize {
                width: 1400,
                height: 1000,
            },
        );
        assert_eq!(app.size(), (1400.0, 1000.0));
        let f = app.frame(1400.0, 1000.0);
        let (x, y) = f
            .rect_of(|t| matches!(t, Target::Card(0, _)))
            .unwrap()
            .centre();
        assert_eq!(app.click(x, y, MouseButton::Left), EventResult::Consumed);
        assert_eq!(app.state.selection, Some(Selection::Tableau(0)));
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_in_both_states() {
        // Every clip and translate is undone. A frame that ends unbalanced is a
        // frame whose next drawing is clipped to whatever the last one forgot to
        // put back.
        for (w, h) in SIZES {
            for state in [empty_game(), GameState::new(42), won_board()] {
                let app = app_with(state);
                assert!(
                    app.frame(w, h).is_balanced(),
                    "the frame was left unbalanced at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_whole_frame_is_clipped_to_the_window() {
        // The clip is the guard: a reading too wide for a narrow window used to
        // spill past the edge, and a zero-sized window still recorded a
        // clickable line at its centre -- a control in a window with no pixels.
        //
        // What used to stand here was a loop over `f.hits()` asserting every
        // box lay inside the window, under a comment saying the boxes could not
        // see a missing clip. Both halves were wrong. `Frame::hit` trims a box
        // to the clip in force and drops it outright if nothing of it survives,
        // so the boxes are the *only* thing that can see the clip vanish -- and
        // by the same token they can never fail that assertion while it is
        // there, whatever the layout does. A band the solver walked off the
        // edge comes back cropped and the loop waves it through. So the clip
        // itself is asserted here, and whether a box is where it belongs is
        // measured against the layout, next test down.
        for (w, h) in SIZES {
            let app = app();
            let f = app.frame(w, h);
            let outer = f.commands().iter().find_map(|c| match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some((*x, *y, *width, *height)),
                _ => None,
            });
            assert_eq!(
                outer,
                Some((0.0, 0.0, w, h)),
                "a {w}x{h} window was not clipped to itself"
            );
        }
    }

    #[test]
    fn at_a_size_that_fits_the_clip_crops_nothing() {
        // The counterpart to the test above, and the one that can actually
        // fail: the boxes are compared against the table the layout solved, not
        // against the window that clipped them. A slot the solver put half a
        // window away comes back merely *smaller* -- still inside the window,
        // still passing any "is it inside?" check -- but it no longer matches
        // the rect it was laid out at.
        let app = app();
        let f = app.frame(WINDOW_WIDTH, WINDOW_HEIGHT);
        let l = Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT);
        let t = Table::new(l.body, app.state.deepest_column());
        let same = |got: Rect, want: Rect| {
            (got.x - want.x).abs() < 0.01
                && (got.y - want.y).abs() < 0.01
                && (got.w - want.w).abs() < 0.01
                && (got.h - want.h).abs() < 0.01
        };
        for slot in 0..FREE_CELL_COUNT {
            let want = t.top_slot(slot);
            let got = f
                .rect_of(|t| *t == Target::FreeCell(byte(slot)))
                .unwrap_or_else(|| panic!("free cell {slot} kept no box at all"));
            assert!(
                same(got, want),
                "free cell {slot} was recorded at {got:?}, laid out at {want:?}"
            );
        }
        for idx in 0..FOUNDATION_COUNT {
            let want = t.top_slot(idx.saturating_add(FREE_CELL_COUNT));
            let got = f
                .rect_of(|t| *t == Target::Foundation(byte(idx)))
                .unwrap_or_else(|| panic!("foundation {idx} kept no box at all"));
            assert!(
                same(got, want),
                "foundation {idx} was recorded at {got:?}, laid out at {want:?}"
            );
        }
        for col in 0..TABLEAU_COLS {
            let want = t.card_at(col, 0);
            let got = f
                .rect_of(|t| *t == Target::Card(byte(col), 0))
                .unwrap_or_else(|| panic!("the bottom card of column {col} kept no box at all"));
            assert!(
                same(got, want),
                "the bottom card of column {col} was recorded at {got:?}, laid out at {want:?}"
            );
        }
        assert!(f.is_balanced(), "a clip was pushed and never popped");
    }

    // ── Full flow tests ────────────────────────────────────────────

    #[test]
    fn test_full_flow_move_to_freecell_and_back() {
        let mut state = empty_game();
        let c = card(Suit::Hearts, Rank::Five);
        state.tableau[0].push(c);

        // Select tableau 0.
        state.focus = FocusArea::Tableau(0);
        state.activate();
        assert_eq!(state.selection, Some(Selection::Tableau(0)));

        // Place at free cell 0.
        state.focus = FocusArea::FreeCell(0);
        state.activate();
        assert!(state.selection.is_none());
        assert_eq!(state.free_cells[0], Some(c));

        // Select free cell 0.
        state.focus = FocusArea::FreeCell(0);
        state.activate();
        assert_eq!(state.selection, Some(Selection::FreeCell(0)));

        // Place back at empty tableau 0.
        state.focus = FocusArea::Tableau(0);
        state.activate();
        assert!(state.selection.is_none());
        assert_eq!(state.tableau[0].last(), Some(&c));
    }

    #[test]
    fn test_full_flow_build_foundation() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        state.tableau[2].push(card(Suit::Hearts, Rank::Three));

        // Auto-move chains: ace moves first, then two becomes eligible
        // (rank 2 is always safe), so both move in one call.
        state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(state.foundations[0].len(), 2);
        // 3 won't move since opposite-color foundations don't have rank 2.
        assert_eq!(state.tableau[2].len(), 1);
    }

    #[test]
    fn test_full_flow_select_place_foundation() {
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));

        state.focus = FocusArea::Tableau(0);
        state.activate();
        state.focus = FocusArea::Foundation(0);
        state.activate();
        // After placing ace, auto-move triggers. Ace should be on foundation.
        assert_eq!(state.foundations[0].len(), 1);
    }

    #[test]
    fn test_deterministic_deal() {
        let s1 = GameState::new(42);
        let s2 = GameState::new(42);
        for col in 0..TABLEAU_COLS {
            assert_eq!(s1.tableau[col], s2.tableau[col]);
        }
    }

    #[test]
    fn test_different_seed_different_deal() {
        let s1 = GameState::new(42);
        let s2 = GameState::new(99);
        // At least one column should differ.
        let any_diff = (0..TABLEAU_COLS).any(|col| s1.tableau[col] != s2.tableau[col]);
        assert!(any_diff);
    }

    /// Builds a board where placing the hearts ace sends four cards home: the
    /// ace itself, and then the twos and three that the ace unblocks.
    fn board_that_cascades() -> GameState {
        let mut state = empty_game();
        // Three of the four aces are already home, so the fourth completes the
        // rank and the twos become safe behind it.
        state.foundations[Suit::Spades.index()].push(card(Suit::Spades, Rank::Ace));
        state.foundations[Suit::Diamonds.index()].push(card(Suit::Diamonds, Rank::Ace));
        state.foundations[Suit::Clubs.index()].push(card(Suit::Clubs, Rank::Ace));
        // The card the player will move, and the cards it frees.
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        state.tableau[2].push(card(Suit::Spades, Rank::Two));
        state.tableau[3].push(card(Suit::Diamonds, Rank::Two));
        state
    }

    #[test]
    fn one_press_of_undo_takes_back_one_press_of_play() {
        // The player makes one move -- ace of hearts to its foundation -- and the
        // game answers by sending three more cards home unasked. Pressing undo
        // once must give back the board as it was before that one press, not a
        // half-unwound board the player never saw and never chose.
        let mut state = board_that_cascades();
        let before = state.tableau.clone();
        state.focus = FocusArea::Tableau(0);
        state.activate();
        state.focus = FocusArea::Foundation(Suit::Hearts.index());
        state.activate();
        assert_eq!(
            state.foundation_total(),
            7,
            "the cascade did not run, so this test is not testing what it says"
        );

        state.undo();
        assert_eq!(
            state.tableau, before,
            "one undo left the board partway through the cascade"
        );
        assert_eq!(
            state.foundation_total(),
            3,
            "cards stayed home after the undo"
        );
    }

    #[test]
    fn the_move_count_returns_to_where_it_started() {
        // The counter is what the player is shown, so an undo that overshoots it
        // is a visible lie about how many moves the game has taken. A cascade
        // adds no moves of its own, so undoing the press that caused it must
        // subtract exactly the one move that press added.
        let mut state = board_that_cascades();
        state.focus = FocusArea::Tableau(0);
        state.activate();
        state.focus = FocusArea::Foundation(Suit::Hearts.index());
        state.activate();
        assert_eq!(
            state.move_count, 1,
            "the press counted as {} moves",
            state.move_count
        );

        state.undo();
        assert_eq!(
            state.move_count, 0,
            "undoing one press left the count at {}",
            state.move_count
        );
    }

    #[test]
    fn asking_for_the_auto_move_is_itself_one_move() {
        // Pressing the auto-move key changes the board, so it is a move and the
        // counter must say so -- once, however many cards flew home. It used to
        // say nothing at all, which then made the next undo subtract from a
        // count the run had never added to.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        let moved = state.auto_move_to_foundations(AutoRun::Asked);
        assert_eq!(moved, 2);
        assert_eq!(
            state.move_count, 1,
            "two cards home counted as {} moves",
            state.move_count
        );

        state.undo();
        assert_eq!(state.move_count, 0);
        assert_eq!(
            state.foundation_total(),
            0,
            "undo left part of the run home"
        );
    }

    #[test]
    fn a_run_the_player_asked_for_is_not_swallowed_by_the_move_before_it() {
        // The trap in unwinding a cascade is unwinding too far: the auto-move key
        // and the cascade after a placement look identical from inside the run,
        // so an undo that pops until it finds a player's move would, after the
        // key, run on into the move the player made *before* pressing it.
        let mut state = empty_game();
        state.tableau[0].push(card(Suit::Spades, Rank::Two));
        state.tableau[1].push(card(Suit::Hearts, Rank::Ace));

        // A plain move first: the two goes to a free cell.
        assert!(state.try_tableau_to_freecell(0));
        assert_eq!(state.move_count, 1);

        // Then the auto-move key, which sends the ace home.
        assert_eq!(state.auto_move_to_foundations(AutoRun::Asked), 1);
        assert_eq!(state.move_count, 2);

        // One undo takes back the run only.
        state.undo();
        assert_eq!(state.move_count, 1);
        assert_eq!(state.foundation_total(), 0);
        assert!(
            state.free_cells[0].is_some(),
            "the undo ran past the run and took back the move before it"
        );
    }

    #[test]
    fn a_run_that_empties_a_free_cell_is_the_player_s_move_too() {
        // Same rule as the test above, on the other of the two loops the run is
        // made of. The run walks the columns and then the cells, and each loop
        // decides for itself whether the step it is making is the player's --
        // so a cell loop that always answers "not the player's" leaves a run
        // the player asked for with no step of their own, and the next undo
        // runs straight past it into the move before.
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Hearts, Rank::Ace));
        state.tableau[0].push(card(Suit::Spades, Rank::Two));

        assert!(state.try_tableau_to_freecell(0));
        assert_eq!(state.move_count, 1);

        assert_eq!(state.auto_move_to_foundations(AutoRun::Asked), 1);
        assert_eq!(state.move_count, 2);

        state.undo();
        assert_eq!(state.move_count, 1);
        assert_eq!(state.foundation_total(), 0);
        assert!(
            state.free_cells[1].is_some(),
            "the undo ran past the run and took back the move before it"
        );
    }

    #[test]
    fn a_won_game_that_is_undone_is_no_longer_won() {
        // The win banner takes over the whole window and refuses every key but
        // `N`, so a board still marked won after an undo is a board the player
        // cannot go on playing.
        let mut state = empty_game();
        for &s in &Suit::ALL {
            for r in Rank::ALL {
                if r == Rank::King && s == Suit::Hearts {
                    continue;
                }
                state.foundations[s.index()].push(card(s, r));
            }
        }
        state.tableau[0].push(card(Suit::Hearts, Rank::King));
        assert_eq!(state.auto_move_to_foundations(AutoRun::Asked), 1);
        assert!(
            state.won,
            "the board did not reach a win, so this proves nothing"
        );

        state.undo();
        assert!(
            !state.won,
            "the game stayed won with a card back on the tableau"
        );
    }
}
