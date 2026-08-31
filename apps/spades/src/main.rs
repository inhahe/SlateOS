//! Slate OS Spades -- four-player partnership Spades in a real window.
//!
//! The human sits south, partnered with north, against east and west. A round
//! deals thirteen cards each, every seat bids the number of tricks it expects
//! to take -- nil is a bid of none, worth a hundred won or lost -- and then
//! thirteen tricks are played with spades as trump. A partnership that makes
//! its bid scores ten a trick plus a bag for each overtrick, and ten bags cost
//! a hundred; five hundred wins the game.
//!
//! Every rectangle on the screen is solved from the live window size each
//! frame, and everything the pointer can reach is a hit box recorded by the
//! pass that painted it. What this replaced did neither: `main` built the game
//! and dropped it on the next line, `render` took no window size at all and
//! painted a 900x700 picture into a window of any other size, and both the hand
//! and the bid pad were hit-tested from a second copy of their geometry that
//! disagreed with the first by twenty-five pixels.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use std::cmp::Ordering;
use std::process::ExitCode;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

const CARD_FACE: Color = Color::from_hex(0xEFF1F5);
const CARD_INK: Color = Color::from_hex(0x1E1E2E);
const CARD_INK_RED: Color = Color::from_hex(0xD20F39);
const CARD_TRUMP_INK: Color = Color::from_hex(0x6C33A8);
const FELT: Color = Color::from_hex(0x14352B);
const MANTLE: Color = Color::from_hex(0x181825);

// ── Constants ───────────────────────────────────────────────────────

/// What the window is called.
const TITLE: &str = "Spades";

/// The window asked for at launch. Nothing else is derived from it -- every
/// rectangle comes from the size the compositor actually gives us.
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 700.0;

/// Seats at the table, running clockwise from the human's.
const SEATS: usize = 4;

/// The same count where a seat number is wanted rather than an array length.
const SEATS_U8: u8 = 4;

/// Who sits with whom. Spades is a partnership game and the pairing is fixed:
/// the human and the seat opposite against the two seats beside them. The old
/// program wrote this pairing out longhand in four separate places.
const TEAM_SEATS: [(PlayerId, PlayerId); 2] = [
    (PlayerId::SOUTH, PlayerId::NORTH),
    (PlayerId::EAST, PlayerId::WEST),
];

/// How often the clock ticks, in milliseconds.
const TICK_MS: u64 = 40;

/// How long a machine player appears to think before it bids or plays.
///
/// The old program ran every machine seat inside the human's own click, so
/// three opponents answered instantly and simultaneously and the game only
/// moved when the human touched it.
const THINK_MS: u32 = 320;

/// How long a settled trick stays face up on the table before it is swept.
const SWEEP_MS: u32 = 900;

/// The largest a card is drawn, however large the window is.
const MAX_CARD_W: f32 = 78.0;

/// A card is this many times taller than it is wide.
const CARD_ASPECT: f32 = 1.4;

/// The highest bid there is. Thirteen tricks, so thirteen is the ceiling.
const MAX_BID: u8 = 13;

/// How many bid buttons sit in a row of the bid pad.
const BID_COLS: u8 = 7;

/// Narrower than this and the seat panel is left out rather than squeezed.
const MIN_PANEL_W: f32 = 128.0;

// ── Card types ──────────────────────────────────────────────────────

/// Card suits in standard order. Spades are trump.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];

    fn symbol(self) -> &'static str {
        match self {
            Suit::Clubs => "\u{2663}",
            Suit::Diamonds => "\u{2666}",
            Suit::Hearts => "\u{2665}",
            Suit::Spades => "\u{2660}",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Suit::Clubs => "Clubs",
            Suit::Diamonds => "Diamonds",
            Suit::Hearts => "Hearts",
            Suit::Spades => "Spades",
        }
    }

    /// The ink a pip of this suit is printed in **on a card face**, which is
    /// near-white.  The old `color` returned the palette's GREEN and BLUE for
    /// clubs and diamonds -- fine on the dark background it was chosen for,
    /// illegible on paper.
    fn ink(self) -> Color {
        match self {
            Suit::Clubs => CARD_INK,
            Suit::Spades => CARD_TRUMP_INK,
            Suit::Diamonds | Suit::Hearts => CARD_INK_RED,
        }
    }

    fn is_trump(self) -> bool {
        self == Suit::Spades
    }
}

/// Card rank (2-14, where 11=Jack, 12=Queen, 13=King, 14=Ace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Rank(u8);

impl Rank {
    const TWO: Rank = Rank(2);
    const THREE: Rank = Rank(3);
    const FOUR: Rank = Rank(4);
    const FIVE: Rank = Rank(5);
    const SIX: Rank = Rank(6);
    const SEVEN: Rank = Rank(7);
    const EIGHT: Rank = Rank(8);
    const NINE: Rank = Rank(9);
    const TEN: Rank = Rank(10);
    const JACK: Rank = Rank(11);
    const QUEEN: Rank = Rank(12);
    const KING: Rank = Rank(13);
    const ACE: Rank = Rank(14);

    /// Every rank there is, low to high.
    ///
    /// `standard_deck` used to build its thirteen cards from a bare `2..=14`,
    /// so the thirteen named ranks above and the range that actually made the
    /// deck were two statements of the same fact that nothing checked against
    /// each other.
    const ALL: [Rank; 13] = [
        Rank::TWO,
        Rank::THREE,
        Rank::FOUR,
        Rank::FIVE,
        Rank::SIX,
        Rank::SEVEN,
        Rank::EIGHT,
        Rank::NINE,
        Rank::TEN,
        Rank::JACK,
        Rank::QUEEN,
        Rank::KING,
        Rank::ACE,
    ];

    fn label(self) -> &'static str {
        match self.0 {
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            9 => "9",
            10 => "10",
            11 => "J",
            12 => "Q",
            13 => "K",
            14 => "A",
            _ => "?",
        }
    }

    fn value(self) -> u8 {
        self.0
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

    /// Sort key: by suit first, then by rank within suit.
    fn sort_key_suit(self) -> u16 {
        u16::from(self.suit as u8)
            .saturating_mul(100)
            .saturating_add(u16::from(self.rank.value()))
    }

    /// Sort key: by rank first, then by suit.
    fn sort_key_rank(self) -> u16 {
        u16::from(self.rank.value())
            .saturating_mul(10)
            .saturating_add(u16::from(self.suit as u8))
    }

    /// Whether this card beats `other` given the led suit.
    /// Trump (spades) beats non-trump. Within the same suit, higher rank wins.
    fn beats(self, other: Card, led_suit: Suit) -> bool {
        if self.suit == other.suit {
            self.rank > other.rank
        } else if self.suit.is_trump() {
            true
        } else if other.suit.is_trump() {
            false
        } else {
            // Neither is trump, different suits: the led suit wins
            self.suit == led_suit
        }
    }
}

/// Build a standard 52-card deck.
fn standard_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for &suit in &Suit::ALL {
        for rank in Rank::ALL {
            deck.push(Card::new(suit, rank));
        }
    }
    deck
}

// ── The shuffle ────────────────────────────────────────────────────

/// The deck's generator.
///
/// The game used to carry a linear congruential generator here, and — worse
/// than the generator itself — a fixed starting seed of `42`. Every fresh
/// launch therefore dealt exactly the same first hand to exactly the same
/// player, on every machine. The seed now comes from the kernel, so the first
/// hand of a session is a new one.
///
/// Cards are not secrets, so unlike the password generator this does *not*
/// refuse to work when the kernel has no entropy for it: a card game that will
/// not deal is worse than one that deals predictably, and the three opponents
/// are AI running in this same process, so there is nobody the deck could be
/// hidden from. What is lost without entropy is variety across launches, not
/// confidentiality, and the fallback says so at the point where it is chosen.
type Rng = SeededRng;

/// A generator for a whole session, seeded from the kernel where possible.
///
/// Without kernel entropy the deal falls back to a fixed sequence, so every
/// launch on this machine plays the same first hand until entropy is
/// available. Nothing here is confidential, so this degrades the game rather
/// than compromising it; the reasoning is written out once, at
/// [`guitk::rng::seeded_from_system`].
fn session_rng() -> Rng {
    seeded_from_system(FALLBACK_SEED)
}

/// The seed used when the kernel has no entropy to give. Arbitrary.
const FALLBACK_SEED: u64 = 0x5350_4144_4553_2121;

/// How many cards each of the four players is dealt.
const HAND_SIZE: usize = 13;

// ── Game phase ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Bidding,
    Playing,
    TrickDone,
    RoundOver,
    GameOver,
}

// ── Player ──────────────────────────────────────────────────────────

/// One of four players: 0=South(human), 1=East, 2=North, 3=West.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlayerId(u8);

impl PlayerId {
    const SOUTH: PlayerId = PlayerId(0);
    const EAST: PlayerId = PlayerId(1);
    const NORTH: PlayerId = PlayerId(2);
    const WEST: PlayerId = PlayerId(3);

    fn index(self) -> usize {
        usize::from(self.0)
    }

    fn next(self) -> PlayerId {
        PlayerId(self.0.wrapping_add(1) % SEATS_U8)
    }

    fn name(self) -> &'static str {
        match self.0 {
            0 => "You",
            1 => "East",
            2 => "North",
            3 => "West",
            _ => "?",
        }
    }

    // There was a `position_label` here, identical to `name` except that it
    // spelled seat 0 "South" instead of "You".  Nothing called it: the seat
    // panel draws `name()` with a " (NS)"/" (EW)" team marker, and the four
    // seats are already laid out on screen in their compass positions, so a
    // second textual spelling of the same seat had nowhere to go.  See
    // known-issues.md lesson 45 -- it had a test, which is what made it look
    // alive.
    //
    // If a screen ever does need the compass name of the human's seat (a
    // rules help page, say), add it back next to that screen and give it a
    // caller in the same change.

    /// Team number (0 = NS team, 1 = EW team).
    fn team(self) -> usize {
        (self.0 % 2) as usize
    }

    fn is_human(self) -> bool {
        self.0 == 0
    }
}

// ── Team data ───────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct TeamState {
    score: i32,
    bags: u32,
}

impl TeamState {
    fn new() -> Self {
        Self { score: 0, bags: 0 }
    }
}

// ── Round bidding + trick tracking per player ───────────────────────

#[derive(Clone, Debug)]
struct PlayerRound {
    bid: Option<u8>,
    tricks_won: u8,
}

impl PlayerRound {
    fn new() -> Self {
        Self {
            bid: None,
            tricks_won: 0,
        }
    }

    fn bid_value(&self) -> u8 {
        self.bid.unwrap_or(0)
    }

    fn is_nil(&self) -> bool {
        self.bid == Some(0)
    }
}

// ── Trick ───────────────────────────────────────────────────────────

/// A single trick: up to 4 cards played, tracking who played what.
///
/// It used to carry a `leader` seat as well, written by every construction and
/// read by nothing but one test -- and redundant besides, since the seat that
/// led is `cards.first()`'s and the seat to play is the game's `current_player`.
#[derive(Clone, Debug)]
struct Trick {
    cards: Vec<(PlayerId, Card)>,
}

impl Trick {
    fn new() -> Self {
        Self {
            cards: Vec::with_capacity(4),
        }
    }

    /// Who led this trick, once anyone has.
    fn leader(&self) -> Option<PlayerId> {
        self.cards.first().map(|&(p, _)| p)
    }

    fn led_suit(&self) -> Option<Suit> {
        self.cards.first().map(|(_, c)| c.suit)
    }

    fn is_complete(&self) -> bool {
        self.cards.len() == 4
    }

    fn add(&mut self, player: PlayerId, card: Card) {
        self.cards.push((player, card));
    }

    /// Determine the trick winner: highest trump if any, else highest of led suit.
    fn winner(&self) -> Option<PlayerId> {
        let mut played = self.cards.iter();
        let &(mut best_player, mut best_card) = played.next()?;
        let led = best_card.suit;
        for &(player, card) in played {
            if card.beats(best_card, led) {
                best_card = card;
                best_player = player;
            }
        }
        Some(best_player)
    }

    // There was a `contains_spade` here -- "did any card in this trick have
    // the spade suit" -- with a test and no caller.  It is deleted rather
    // than wired in, because the rule it looks like it serves is already
    // implemented, and implemented *better*, one level up: `play_card` sets
    // `spades_broken` the instant a spade hits the table.
    //
    // That distinction is the actual rule, not a detail.  Spades break the
    // moment one is played, so the third player of a trick may already lead
    // them next trick; asking the finished trick whether it "contained" a
    // spade would break them one trick late.  Reviving this as a shortcut
    // would be reintroducing a subtly wrong second answer to a question
    // that already has a right one.  See known-issues.md lesson 45.
}

// ── Sort order toggle ───────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortOrder {
    BySuit,
    ByRank,
}

impl SortOrder {
    fn toggle(self) -> Self {
        match self {
            SortOrder::BySuit => SortOrder::ByRank,
            SortOrder::ByRank => SortOrder::BySuit,
        }
    }
}

// ── What a click can land on ────────────────────────────────────────

/// A verb with a key and a button in the footer that do the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// Whatever Enter means in this phase.
    Confirm,
    /// Re-sort the hand: by suit, or by rank across suits.
    Sort,
    /// Abandon this game and deal a new one.
    New,
    /// Show or hide the help card.
    Help,
}

const BUTTONS: [Button; 4] = [Button::Confirm, Button::Sort, Button::New, Button::Help];

impl Button {
    /// The key that does the same thing.
    const fn key(self) -> Key {
        match self {
            Button::Confirm => Key::Enter,
            Button::Sort => Key::S,
            Button::New => Key::N,
            Button::Help => Key::H,
        }
    }

    /// The name that key is known by on the help card.
    const fn key_name(self) -> &'static str {
        match self {
            Button::Confirm => "Enter",
            Button::Sort => "S",
            Button::New => "N",
            Button::Help => "H",
        }
    }
}

/// What the `Confirm` button will do if it is pressed now.
///
/// One function, read by the button that carries the label, by the help card
/// that explains it and by the test that holds the two together. Enter means
/// five different things across the five phases, and a label written out beside
/// each of them would be that rule written six times.
const fn confirm_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Bidding => "Bid",
        Phase::Playing => "Play",
        Phase::TrickDone => "Next trick",
        Phase::RoundOver => "Next round",
        Phase::GameOver => "New game",
    }
}

/// Everything the pointer can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The `i`th card of the human's hand, as the hand is currently sorted.
    Card(usize),
    /// A bid of this many tricks, on the bid pad.
    Bid(u8),
    Button(Button),
    /// The help card itself, which swallows the click that dismisses it.
    Help,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Every rectangle in the picture, solved from the live window size.
///
/// What this replaced was eleven compile-time coordinates -- `HAND_Y = 580`,
/// `TRICK_CENTER_X = 390`, `SIDEBAR_X = 720` -- and a `render` that took no
/// window size at all, so the picture was drawn at 900x700 in a window of any
/// other size and the sidebar hung off the right edge of anything narrower.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The bar carrying the title and the round.
    header: Rect,
    /// The felt: the trick in the middle, the seats round it, the panel.
    table: Rect,
    /// The strip the human's cards are fanned across.
    hand: Rect,
    /// The row of buttons.
    footer: Rect,
    /// The one line of prose under the buttons.
    status: Rect,
    /// The panel of scores and bids. Empty when the window cannot pay for one.
    panel: Rect,
    /// A card's size, in the hand and on the table alike.
    card: (f32, f32),
    pad: f32,
    title: f32,
    font: f32,
    small: f32,
}

impl Layout {
    fn solve(w: f32, h: f32) -> Self {
        let w = w.max(0.0);
        let h = h.max(0.0);
        let window = Rect::new(0.0, 0.0, w, h);
        let pad = (w.min(h) * 0.03).clamp(4.0, 18.0);
        let font = (h / 44.0).clamp(9.0, 17.0);
        let title = (font * 1.4).clamp(12.0, 24.0);
        let small = (font - 2.0).max(8.0);

        let header = Rect::new(0.0, 0.0, w, (title + pad * 1.6).min(h));
        let status_h = (font + pad * 1.1).min((h - header.h).max(0.0));
        let status = Rect::new(0.0, h - status_h, w, status_h);

        // A card is sized by what the hand strip and the table can both pay
        // for. Taking the width alone survives every containment test and eats
        // the table in a wide, short window.
        let free_h = (status.y - header.bottom()).max(0.0);
        let card_w = (w / 11.0).min(free_h / 5.6).clamp(0.0, MAX_CARD_W);
        let card_h = card_w * CARD_ASPECT;

        // The hand strip is taller than a card on purpose: the card the player
        // is pointing at lifts out of the fan, and it has to lift into
        // something. The old program lifted it ten pixels into the table's
        // airspace and then hit-tested the row it had left.
        let footer_h = (small * 2.4).min(free_h);
        let hand_h = (card_h * 1.14).min((free_h - footer_h).max(0.0));
        let footer = Rect::new(0.0, (status.y - footer_h).max(header.bottom()), w, footer_h);
        let hand = Rect::new(
            pad,
            (footer.y - hand_h).max(header.bottom()),
            (w - pad * 2.0).max(0.0),
            hand_h,
        );
        let table = Rect::new(
            pad,
            header.bottom(),
            (w - pad * 2.0).max(0.0),
            (hand.y - header.bottom()).max(0.0),
        );

        // The panel sits in the top-right of the felt and is left out rather
        // than squeezed when what is left would not hold a name -- the answer
        // the old sidebar could not give, being pinned at x = 720.
        let panel_w = (w * 0.24).clamp(0.0, 196.0);
        let panel_h = small.mul_add(17.1, pad * 2.0);
        let panel = if panel_w >= MIN_PANEL_W && table.h >= panel_h + pad * 2.0 {
            Rect::new(table.right() - panel_w, table.y + pad, panel_w, panel_h)
        } else {
            Rect::EMPTY
        };

        Self {
            window,
            header,
            table,
            hand,
            footer,
            status,
            panel,
            card: (card_w, card_h),
            pad,
            title,
            font,
            small,
        }
    }

    /// How far the chosen card is lifted out of the fan.
    ///
    /// It is exactly the slack the hand strip carries over a card's height, so
    /// a lifted card is still inside the strip and its hit box -- recorded by
    /// the pass that paints it -- cannot fall outside what was drawn.
    fn hand_lift(&self) -> f32 {
        (self.hand.h - self.card.1).max(0.0)
    }

    /// How far apart the cards of an `n`-card hand are drawn.
    ///
    /// Cards overlap only as much as they must. The old program stepped a fixed
    /// forty pixels whatever the window was, so a thirteen-card hand was 540
    /// wide in every window there has ever been -- and it was centred on
    /// `TRICK_CENTER_X = 390`, sixty pixels left of the middle of the 900-wide
    /// window it was drawn in.
    fn hand_step(&self, n: usize) -> f32 {
        let (cw, _) = self.card;
        if n <= 1 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand is at most thirteen cards; exact in f32"
        )]
        let gaps = n.saturating_sub(1) as f32;
        ((self.hand.w - cw) / gaps).clamp(0.0, cw)
    }

    /// Where the `i`th card of an `n`-card hand is drawn.
    ///
    /// This is the only place the hand's geometry exists. `handle_mouse_playing`
    /// used to compute its own from its own copies of `CARD_SPACING`, `CARD_W`
    /// and `TRICK_CENTER_X`, and searched the strip `HAND_Y ..= HAND_Y +
    /// CARD_H` -- while `render_hand` drew the selected card ten pixels higher.
    fn hand_card(&self, i: usize, n: usize) -> Rect {
        let (cw, ch) = self.card;
        if i >= n || cw <= 0.0 || ch <= 0.0 {
            return Rect::EMPTY;
        }
        let step = self.hand_step(n);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand is at most thirteen cards; exact in f32"
        )]
        let (fi, gaps) = (i as f32, (n.saturating_sub(1)) as f32);
        let span = step.mul_add(gaps, cw);
        let x0 = self.hand.x + (self.hand.w - span) / 2.0;
        Rect::new(
            step.mul_add(fi, x0),
            self.hand.y + (self.hand.h - ch).max(0.0),
            cw,
            ch,
        )
    }

    /// Where the card played by `seat` sits in the middle of the table.
    ///
    /// South is the human and the seats run clockwise from there -- east,
    /// north, west -- which is also the order the turn passes in.
    fn trick_card(&self, seat: usize) -> Rect {
        let (cw, ch) = self.card;
        if cw <= 0.0 || ch <= 0.0 {
            return Rect::EMPTY;
        }
        let (cx, cy) = self.table.centre();
        let (dx, dy) = match seat % SEATS {
            0 => (0.0, ch * 0.55),
            1 => (cw * 1.05, 0.0),
            2 => (0.0, -ch * 0.55),
            _ => (-cw * 1.05, 0.0),
        };
        Rect::new(cx + dx - cw / 2.0, cy + dy - ch / 2.0, cw, ch)
    }

    /// Where a seat's name and remaining cards are written on the felt.
    fn seat_label(&self, seat: usize) -> Rect {
        let (cw, ch) = self.card;
        let w = (cw * 1.9).min(self.table.w / 3.0);
        let h = self.small * 2.4;
        if w <= 0.0 || h > self.table.h {
            return Rect::EMPTY;
        }
        let (cx, cy) = self.table.centre();
        let label = match seat % SEATS {
            0 => Rect::new(
                cx - w / 2.0,
                (cy + ch * 1.15).min(self.table.bottom() - h),
                w,
                h,
            ),
            1 => Rect::new(
                (cx + cw * 1.7).min(self.table.right() - w),
                cy - h / 2.0,
                w,
                h,
            ),
            2 => Rect::new(cx - w / 2.0, (cy - ch * 1.15 - h).max(self.table.y), w, h),
            _ => Rect::new((cx - cw * 1.7 - w).max(self.table.x), cy - h / 2.0, w, h),
        };
        // Clamping is what keeps a label on the felt, but a felt small enough
        // to need clamping is one where the clamp can push the label over the
        // card it describes, or over the panel. Left out rather than drawn
        // through either -- the same answer the panel itself gives.
        if label.intersect(self.trick_card(seat)).is_some() || label.intersect(self.panel).is_some()
        {
            return Rect::EMPTY;
        }
        label
    }

    /// The side of one square button on the bid pad.
    fn bid_cell(&self) -> f32 {
        (self.table.w / 9.5)
            .min(self.table.h / 7.5)
            .clamp(0.0, 46.0)
    }

    /// The card the human bids from, centred on the felt.
    ///
    /// Empty when the felt is too small to hold it, in which case Up, Down and
    /// Enter are still a whole way to bid.
    fn bid_pad(&self) -> Rect {
        let cell = self.bid_cell();
        let gap = cell * 0.14;
        let (cols, rows) = (f32::from(BID_COLS), 2.0_f32);
        let w = cols.mul_add(cell, (cols - 1.0) * gap) + self.pad * 2.0;
        let h = rows.mul_add(cell, gap) + self.font * 2.6 + self.pad * 2.0;
        if cell < 12.0 || w > self.table.w || h > self.table.h {
            return Rect::EMPTY;
        }
        let (cx, cy) = self.table.centre();
        Rect::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }

    /// The button for a bid of `value` tricks.
    ///
    /// The old program painted this grid at `TRICK_CENTER_X - 140` and
    /// `overlay_y + 75`, and hit-tested it at `TRICK_CENTER_X - 120` and
    /// `overlay_y + 50`: every bid button answered to a square twenty pixels
    /// right and twenty-five pixels below the one it was drawn on.
    fn bid_button(&self, value: u8) -> Rect {
        let pad = self.bid_pad();
        if pad.is_empty() || value > MAX_BID {
            return Rect::EMPTY;
        }
        let cell = self.bid_cell();
        let gap = cell * 0.14;
        let (col, row) = (f32::from(value % BID_COLS), f32::from(value / BID_COLS));
        Rect::new(
            (cell + gap).mul_add(col, pad.x + self.pad),
            (cell + gap).mul_add(row, pad.y + self.pad + self.font * 2.6),
            cell,
            cell,
        )
    }
}

// ── Main game state ─────────────────────────────────────────────────

struct SpadesGame {
    rng: Rng,
    phase: Phase,
    hands: [Vec<Card>; 4],
    teams: [TeamState; 2],
    player_rounds: [PlayerRound; 4],
    current_trick: Trick,
    tricks_played: u8,
    current_player: PlayerId,
    dealer: PlayerId,
    spades_broken: bool,
    /// For bidding UI: the currently selected bid value for the human.
    bid_selection: u8,
    /// Index of selected card in human hand.
    selected_card: usize,
    /// Hand sort order.
    sort_order: SortOrder,
    /// Status message shown in the footer area.
    status_message: String,
    /// Last completed trick (kept visible briefly).
    last_trick: Option<Trick>,
    /// Round number (1-based).
    round_number: u32,
    /// Winner message on game over.
    winner_message: String,
    /// The size the renderer last drew at.
    ///
    /// A click is resolved against the frame that was actually painted, so the
    /// handler has to know what size that was.
    size: (f32, f32),
    /// Milliseconds left before the machine seat now to act bids or plays.
    think_ms: u32,
    /// Milliseconds a settled trick still has on the table.
    sweep_ms: u32,
    /// Whether the help card is up.
    show_help: bool,
}

impl SpadesGame {
    /// A new game, dealt from a deck the kernel shuffled.
    fn new() -> Self {
        Self::with_rng(session_rng())
    }

    /// A new game dealt from a named seed, so a test can name the hand it
    /// wants to reason about. The game itself never takes this path — a fixed
    /// seed is exactly the defect the kernel-seeded constructor exists to fix.
    #[cfg(test)]
    fn with_seed(seed: u64) -> Self {
        Self::with_rng(Rng::new(seed))
    }

    fn with_rng(rng: Rng) -> Self {
        let mut game = Self {
            rng,
            phase: Phase::Bidding,
            hands: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            teams: [TeamState::new(), TeamState::new()],
            player_rounds: [
                PlayerRound::new(),
                PlayerRound::new(),
                PlayerRound::new(),
                PlayerRound::new(),
            ],
            current_trick: Trick::new(),
            tricks_played: 0,
            current_player: PlayerId::EAST,
            dealer: PlayerId::SOUTH,
            spades_broken: false,
            bid_selection: 3,
            selected_card: 0,
            sort_order: SortOrder::BySuit,
            status_message: String::new(),
            last_trick: None,
            round_number: 1,
            winner_message: String::new(),
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            think_ms: 0,
            sweep_ms: 0,
            show_help: false,
        };
        game.deal();
        game.begin_bidding();
        game
    }

    fn new_game(&mut self) {
        self.rng = Rng::new(self.rng.next_u64());
        self.phase = Phase::Bidding;
        self.teams = [TeamState::new(), TeamState::new()];
        self.dealer = PlayerId::SOUTH;
        self.round_number = 1;
        self.winner_message.clear();
        self.start_round();
    }

    fn start_round(&mut self) {
        self.player_rounds = [
            PlayerRound::new(),
            PlayerRound::new(),
            PlayerRound::new(),
            PlayerRound::new(),
        ];
        self.tricks_played = 0;
        self.spades_broken = false;
        self.last_trick = None;
        self.selected_card = 0;
        self.bid_selection = 3;
        self.phase = Phase::Bidding;
        self.sweep_ms = 0;
        // Player to left of dealer leads bidding and first trick
        self.current_player = self.dealer.next();
        self.deal();
        self.begin_bidding();
    }

    fn deal(&mut self) {
        let mut deck = standard_deck();
        self.rng.shuffle(&mut deck);
        // `chunks_exact` states the hand size once and cannot run off the end
        // of a deck that is not the size this expects — the index arithmetic
        // it replaces would have panicked on any other deck.
        for (hand, cards) in self.hands.iter_mut().zip(deck.chunks_exact(HAND_SIZE)) {
            *hand = cards.to_vec();
        }
        self.sort_hand(PlayerId::SOUTH);
        self.sort_hand(PlayerId::EAST);
        self.sort_hand(PlayerId::NORTH);
        self.sort_hand(PlayerId::WEST);
    }

    /// The cards a seat still holds. Empty for a seat that does not exist,
    /// which is every seat but four.
    fn hand_of(&self, player: PlayerId) -> &[Card] {
        self.hands
            .get(player.index())
            .map_or(&[][..], Vec::as_slice)
    }

    fn sort_hand(&mut self, player: PlayerId) {
        let order = self.sort_order;
        if let Some(hand) = self.hands.get_mut(player.index()) {
            hand.sort_by_key(|c| match order {
                SortOrder::BySuit => c.sort_key_suit(),
                SortOrder::ByRank => c.sort_key_rank(),
            });
        }
    }

    fn sort_all_hands(&mut self) {
        let order = self.sort_order;
        for hand in &mut self.hands {
            hand.sort_by_key(|c| match order {
                SortOrder::BySuit => c.sort_key_suit(),
                SortOrder::ByRank => c.sort_key_rank(),
            });
        }
    }

    // ── Bidding ─────────────────────────────────────────────────────

    /// AI bidding heuristic: count high cards and spades to estimate tricks.
    fn ai_bid(&self, player: PlayerId) -> u8 {
        let hand = self.hand_of(player);
        let length_of = |suit: Suit| hand.iter().filter(|c| c.suit == suit).count();
        let mut estimate: u8 = 0;

        // Count aces and kings as likely tricks
        for card in hand {
            if card.rank == Rank::ACE {
                estimate = estimate.saturating_add(1);
            } else if card.rank == Rank::KING && length_of(card.suit) >= 3 {
                // King is usually good if you have 3+ cards in the suit
                estimate = estimate.saturating_add(1);
            }
        }

        // Count spades (trump) as partial tricks
        let spade_count = length_of(Suit::Spades);
        if spade_count >= 3 {
            estimate = estimate.saturating_add(1);
        }
        if spade_count >= 5 {
            estimate = estimate.saturating_add(1);
        }

        // Queens in long suits
        for card in hand {
            if card.rank == Rank::QUEEN && length_of(card.suit) >= 4 {
                estimate = estimate.saturating_add(1);
            }
        }

        // Clamp to reasonable range
        estimate.clamp(1, 6)
    }

    /// Open the auction on the seat left of the dealer.
    ///
    /// The old program ran every machine bid before the human's in one loop
    /// here, and the rest of them inside the human's own click; four seats
    /// therefore bid in a single event and the player never saw a bid arrive.
    /// The clock deals them out one at a time now.
    fn begin_bidding(&mut self) {
        self.arm_think();
        self.set_turn_status();
    }

    fn advance_bidder(&mut self) {
        self.current_player = self.current_player.next();
        // Check if all 4 have bid
        if self.player_rounds.iter().all(|pr| pr.bid.is_some()) {
            self.phase = Phase::Playing;
            self.current_player = self.dealer.next();
            self.current_trick = Trick::new();
        }
        self.arm_think();
        self.set_turn_status();
    }

    /// What the status line says while it is somebody's turn to bid or play.
    ///
    /// One function, so that the four places the turn moves cannot each invent
    /// their own wording for the same fact.
    fn set_turn_status(&mut self) {
        self.status_message = match self.phase {
            Phase::Bidding => {
                if self.current_player.is_human() {
                    format!(
                        "Your bid: {} \u{2014} {} to confirm",
                        bid_name(self.bid_selection),
                        Button::Confirm.key_name()
                    )
                } else {
                    format!("{} is bidding", self.current_player.name())
                }
            }
            Phase::Playing => {
                if self.current_player.is_human() {
                    if self.current_trick.cards.is_empty() {
                        String::from("Your turn to lead")
                    } else {
                        String::from("Your turn to play")
                    }
                } else {
                    format!("{} is thinking", self.current_player.name())
                }
            }
            _ => return,
        };
    }

    /// Record the human's bid and hand the auction on.
    fn submit_human_bid(&mut self) -> bool {
        if self.phase != Phase::Bidding || !self.current_player.is_human() {
            return false;
        }
        self.player_rounds[0].bid = Some(self.bid_selection);
        self.advance_bidder();
        true
    }

    // ── The clock ───────────────────────────────────────────────────

    /// Set the machine's pause, or clear it when the seat now to act is the
    /// human's or nobody is owed a turn at all.
    ///
    /// The single writer of `think_ms`, so no caller has to decide whether a
    /// pause is owed and `tick` does not have to ask a second time.
    fn arm_think(&mut self) {
        let waiting = matches!(self.phase, Phase::Bidding | Phase::Playing)
            && !self.current_player.is_human();
        self.think_ms = if waiting { THINK_MS } else { 0 };
    }

    /// Advance the two timers. Answers whether anything changed.
    fn tick(&mut self, elapsed_ms: u64) -> bool {
        let ms = u32::try_from(elapsed_ms).unwrap_or(u32::MAX);
        if self.sweep_ms > 0 {
            self.sweep_ms = self.sweep_ms.saturating_sub(ms);
            if self.sweep_ms == 0 {
                self.advance_after_trick();
            }
            return true;
        }
        // `arm_think` is the only writer of the pause and clears it to zero
        // whenever one is not owed, so repeating its phase and seat tests here
        // would be the same fact written in two places.
        if self.think_ms > 0 {
            self.think_ms = self.think_ms.saturating_sub(ms);
            if self.think_ms == 0 {
                self.machine_acts();
            }
            return true;
        }
        false
    }

    /// One machine seat bids, or plays one card.
    ///
    /// Which of the two is a question about the phase, not a second copy of
    /// `arm_think`'s question about whose turn it is.
    fn machine_acts(&mut self) {
        if self.phase == Phase::Bidding {
            let bid = self.ai_bid(self.current_player);
            if let Some(pr) = self.player_rounds.get_mut(self.current_player.index()) {
                pr.bid = Some(bid);
            }
            self.advance_bidder();
        } else {
            let legal = self.legal_plays(self.current_player);
            if legal.is_empty() {
                // A seat with no legal card would otherwise hold the game for
                // ever; there is no such hand, and
                // `every_seat_always_has_a_legal_card` says so.
                self.arm_think();
            } else {
                let choice = self.ai_choose_card(self.current_player, &legal);
                self.play_card(self.current_player, choice);
            }
        }
    }

    // ── Card play logic ─────────────────────────────────────────────

    /// Get legal cards the player can play from their hand.
    fn legal_plays(&self, player: PlayerId) -> Vec<usize> {
        let hand = self.hand_of(player);
        if hand.is_empty() {
            return Vec::new();
        }

        let mut indices: Vec<usize> = Vec::new();

        if let Some(led) = self.current_trick.led_suit() {
            // Must follow suit if possible
            let has_led_suit = hand.iter().any(|c| c.suit == led);
            if has_led_suit {
                for (i, card) in hand.iter().enumerate() {
                    if card.suit == led {
                        indices.push(i);
                    }
                }
            } else {
                // Can play anything
                for i in 0..hand.len() {
                    indices.push(i);
                }
            }
        } else {
            // Leading the trick
            if !self.spades_broken {
                // Can't lead spades unless broken or hand is all spades
                let has_non_spade = hand.iter().any(|c| c.suit != Suit::Spades);
                if has_non_spade {
                    for (i, card) in hand.iter().enumerate() {
                        if card.suit != Suit::Spades {
                            indices.push(i);
                        }
                    }
                } else {
                    // All spades: can lead spades
                    for i in 0..hand.len() {
                        indices.push(i);
                    }
                }
            } else {
                for i in 0..hand.len() {
                    indices.push(i);
                }
            }
        }

        indices
    }

    /// Play a card from a player's hand (by index).
    fn play_card(&mut self, player: PlayerId, hand_index: usize) {
        let Some(card) = self
            .hands
            .get_mut(player.index())
            .filter(|hand| hand_index < hand.len())
            .map(|hand| hand.remove(hand_index))
        else {
            return;
        };
        self.current_trick.add(player, card);

        if card.suit == Suit::Spades {
            self.spades_broken = true;
        }

        if self.current_trick.is_complete() {
            self.resolve_trick();
        } else {
            self.current_player = self.current_player.next();
            self.clamp_selected_card();
            self.arm_think();
            self.set_turn_status();
        }
    }

    /// Settle a full trick and leave it face up for `SWEEP_MS`.
    ///
    /// The old program kept a `last_trick` and drew it, but nothing timed it:
    /// the phase sat at `TrickDone` until the human pressed Enter, which is
    /// also how the machine seats were unblocked, so a game left alone stopped.
    fn resolve_trick(&mut self) {
        let winner = self.current_trick.winner().unwrap_or(PlayerId::SOUTH);
        if let Some(pr) = self.player_rounds.get_mut(winner.index()) {
            pr.tricks_won = pr.tricks_won.saturating_add(1);
        }
        self.tricks_played = self.tricks_played.saturating_add(1);
        self.last_trick = Some(self.current_trick.clone());
        self.phase = Phase::TrickDone;
        self.status_message = format!("{} wins the trick", winner.name());
        self.current_player = winner;
        self.sweep_ms = SWEEP_MS;
        self.arm_think();
    }

    /// Sweep the settled trick and start the next one, or score the round.
    fn advance_after_trick(&mut self) {
        self.sweep_ms = 0;
        if self.tricks_played >= 13 {
            self.score_round();
            self.arm_think();
            return;
        }
        self.phase = Phase::Playing;
        self.current_trick = Trick::new();
        self.clamp_selected_card();
        self.arm_think();
        self.set_turn_status();
    }

    // ── AI play ─────────────────────────────────────────────────────

    /// AI card selection strategy.
    fn ai_choose_card(&self, player: PlayerId, legal: &[usize]) -> usize {
        if legal.is_empty() {
            return 0;
        }
        if let [only] = *legal {
            return only;
        }

        let hand = self.hand_of(player);
        let is_nil = self
            .player_rounds
            .get(player.index())
            .is_some_and(PlayerRound::is_nil);

        if self.current_trick.cards.is_empty() {
            // Leading: play lowest non-trump if possible (or lowest overall)
            if is_nil {
                // Nil bidder: lead lowest card to avoid winning
                return self.pick_lowest(hand, legal);
            }
            // Lead with a low card from a short suit to try to set up trumping later
            return self.pick_lead(hand, legal);
        }

        let led_suit = self.current_trick.led_suit().unwrap_or(Suit::Clubs);

        if is_nil {
            // Nil bidder: try to play lowest card that won't win
            return self.pick_lowest_non_winning(hand, legal, led_suit);
        }

        // Normal play: try to win with the smallest winning card
        self.pick_smart(hand, legal, led_suit)
    }

    /// The first index in `legal` naming the lowest card in `hand`.
    ///
    /// `legal` indexes `hand`, and every caller builds it from `legal_plays`,
    /// which builds it by enumerating that same hand. It is still read with
    /// `get` rather than `[]`: the invariant is one function away from here,
    /// and a card game that panics is worse than one that plays the first card.
    fn pick_lowest(&self, hand: &[Card], legal: &[usize]) -> usize {
        let mut best = legal.first().copied().unwrap_or(0);
        let mut best_rank = hand.get(best).map_or(Rank(u8::MAX), |c| c.rank);
        for &i in legal.iter().skip(1) {
            if let Some(card) = hand.get(i)
                && card.rank < best_rank
            {
                best_rank = card.rank;
                best = i;
            }
        }
        best
    }

    fn pick_lead(&self, hand: &[Card], legal: &[usize]) -> usize {
        // Prefer leading a low card from a non-trump suit
        let mut best = legal.first().copied().unwrap_or(0);
        let mut best_score = u16::MAX;
        for &i in legal {
            let Some(card) = hand.get(i) else { continue };
            // Avoid leading trump: fifty is more than any rank, so every
            // non-trump is preferred to every trump.
            let penalty = if card.suit.is_trump() { 50 } else { 0 };
            let score = u16::from(card.rank.value()).saturating_add(penalty);
            if score < best_score {
                best_score = score;
                best = i;
            }
        }
        best
    }

    fn pick_lowest_non_winning(&self, hand: &[Card], legal: &[usize], led_suit: Suit) -> usize {
        // For nil bidders: find the lowest card that doesn't currently beat the trick
        let current_winner_card = self
            .current_trick
            .cards
            .iter()
            .map(|(_, c)| *c)
            .reduce(|best, c| if c.beats(best, led_suit) { c } else { best });

        if let Some(winner_card) = current_winner_card {
            // Try to find a card that loses to the current winner
            let mut best_losing: Option<usize> = None;
            let mut best_losing_rank = Rank(0);

            for &i in legal {
                let Some(&card) = hand.get(i) else { continue };
                if !card.beats(winner_card, led_suit)
                    && (best_losing.is_none() || card.rank > best_losing_rank)
                {
                    // Play highest losing card to conserve low cards
                    best_losing = Some(i);
                    best_losing_rank = card.rank;
                }
            }

            if let Some(idx) = best_losing {
                return idx;
            }
        }

        // Must win: play lowest
        self.pick_lowest(hand, legal)
    }

    fn pick_smart(&self, hand: &[Card], legal: &[usize], led_suit: Suit) -> usize {
        let current_winner = self
            .current_trick
            .cards
            .iter()
            .map(|(p, c)| (*p, *c))
            .reduce(|(bp, bc), (p, c)| {
                if c.beats(bc, led_suit) {
                    (p, c)
                } else {
                    (bp, bc)
                }
            });

        if let Some((winner_pid, winner_card)) = current_winner {
            // If partner is winning, don't waste a high card
            if winner_pid.team() == self.current_player.team()
                && self.current_trick.cards.len() == 3
            {
                // Partner is winning and we're last to play: dump lowest
                return self.pick_lowest(hand, legal);
            }

            // Try to beat with smallest possible winner
            let mut best_winning: Option<usize> = None;
            let mut best_winning_rank = Rank(15);

            for &i in legal {
                let Some(&card) = hand.get(i) else { continue };
                if card.beats(winner_card, led_suit) && card.rank < best_winning_rank {
                    best_winning = Some(i);
                    best_winning_rank = card.rank;
                }
            }

            if let Some(idx) = best_winning {
                return idx;
            }

            // Can't win: dump lowest
            self.pick_lowest(hand, legal)
        } else {
            // First to play (should not happen here since we check for empty trick above)
            self.pick_lowest(hand, legal)
        }
    }

    // ── Scoring ─────────────────────────────────────────────────────

    /// What one partnership scores this round, and the bags it is left holding.
    ///
    /// A pure function of the two seats' rounds and the bags carried in, so the
    /// scoring can be read without also reading how the totals are stored.
    fn team_round_score(&self, seats: (PlayerId, PlayerId), bags: u32) -> (i32, u32) {
        let (Some(p1r), Some(p2r)) = (
            self.player_rounds.get(seats.0.index()),
            self.player_rounds.get(seats.1.index()),
        ) else {
            return (0, bags);
        };

        // A nil is scored on its own, made or set, and takes no part in the
        // partnership's contract either way.
        let mut score: i32 = 0;
        for pr in [p1r, p2r] {
            if pr.is_nil() {
                score = score.saturating_add(if pr.tricks_won == 0 { 100 } else { -100 });
            }
        }

        let contract = |pr: &PlayerRound| -> (i32, i32) {
            if pr.is_nil() {
                (0, 0)
            } else {
                (i32::from(pr.bid_value()), i32::from(pr.tricks_won))
            }
        };
        let (b1, t1) = contract(p1r);
        let (b2, t2) = contract(p2r);
        let team_bid = b1.saturating_add(b2);
        let tricks = t1.saturating_add(t2);

        if team_bid <= 0 {
            return (score, bags);
        }
        if tricks < team_bid {
            // Set: lose ten a trick bid.
            return (score.saturating_sub(team_bid.saturating_mul(10)), bags);
        }

        let overtricks = tricks.saturating_sub(team_bid);
        score = score
            .saturating_add(team_bid.saturating_mul(10))
            .saturating_add(overtricks);
        let new_bags = bags.saturating_add(u32::try_from(overtricks).unwrap_or(0));
        let penalties = i32::try_from(new_bags / 10).unwrap_or(0);
        (
            score.saturating_sub(penalties.saturating_mul(100)),
            new_bags % 10,
        )
    }

    fn score_round(&mut self) {
        self.phase = Phase::RoundOver;

        for index in 0..TEAM_SEATS.len() {
            let bags = self.teams.get(index).map_or(0, |t| t.bags);
            let (round_score, new_bags) = self.team_round_score(Self::seats_of(index), bags);
            if let Some(team) = self.teams.get_mut(index) {
                team.score = team.score.saturating_add(round_score);
                team.bags = new_bags;
            }
        }

        self.check_game_over();
        if self.phase != Phase::GameOver {
            let ns_score = self.team_score(0);
            let ew_score = self.team_score(1);
            self.status_message = format!(
                "Round {} over! NS: {} EW: {} (Enter to continue)",
                self.round_number, ns_score, ew_score
            );
        }
    }

    /// A partnership's running total, or zero for a partnership there is not.
    fn team_score(&self, team: usize) -> i32 {
        self.teams.get(team).map_or(0, |t| t.score)
    }

    fn check_game_over(&mut self) {
        let ns = self.team_score(0);
        let ew = self.team_score(1);

        // Both reach 500: higher score wins
        if ns >= 500 || ew >= 500 {
            if ns >= 500 && ew >= 500 {
                match ns.cmp(&ew) {
                    Ordering::Greater => {
                        self.phase = Phase::GameOver;
                        self.winner_message = format!("Your team wins! {ns} to {ew}");
                    }
                    Ordering::Less => {
                        self.phase = Phase::GameOver;
                        self.winner_message = format!("East-West wins! {ew} to {ns}");
                    }
                    // Tie at five hundred: keep playing.
                    Ordering::Equal => return,
                }
            } else if ns >= 500 {
                self.phase = Phase::GameOver;
                self.winner_message = format!("Your team wins! {} to {}", ns, ew);
            } else {
                self.phase = Phase::GameOver;
                self.winner_message = format!("East-West wins! {} to {}", ew, ns);
            }
            return;
        }

        // Team at -200 loses
        if ns <= -200 {
            self.phase = Phase::GameOver;
            self.winner_message = format!("East-West wins (NS at {})!", ns);
        } else if ew <= -200 {
            self.phase = Phase::GameOver;
            self.winner_message = format!("Your team wins (EW at {})!", ew);
        }
    }

    fn advance_round(&mut self) {
        self.round_number = self.round_number.saturating_add(1);
        self.dealer = self.dealer.next();
        self.start_round();
    }

    // ── Human input helpers ─────────────────────────────────────────

    fn clamp_selected_card(&mut self) {
        let hand_len = self.hands[0].len();
        self.selected_card = self.selected_card.min(hand_len.saturating_sub(1));
    }

    fn try_play_selected(&mut self) -> bool {
        if self.phase != Phase::Playing || !self.current_player.is_human() {
            return false;
        }
        let legal = self.legal_plays(PlayerId::SOUTH);
        if legal.contains(&self.selected_card) {
            self.play_card(PlayerId::SOUTH, self.selected_card);
        } else {
            self.status_message = String::from("That card cannot be played on this trick");
        }
        true
    }

    /// Move the keyboard's pointer along the hand.
    ///
    /// One function for both directions. `Key::Left` used to carry its bound in
    /// the match arm's guard and `Key::Right` its own in the arm's body, so the
    /// two directions were not the same code in any sense a reader could check.
    fn move_selection(&mut self, step: isize) -> bool {
        let len = self.hands[0].len();
        if len == 0 {
            return false;
        }
        let last = len.saturating_sub(1);
        let next = if step < 0 {
            self.selected_card.saturating_sub(step.unsigned_abs())
        } else {
            self.selected_card
                .saturating_add(step.unsigned_abs())
                .min(last)
        };
        if next == self.selected_card {
            return false;
        }
        self.selected_card = next;
        true
    }

    /// Move the bid the human is about to make.
    ///
    /// The same shape as `move_selection`, and for the same reason: `Key::Up`
    /// carried its ceiling in a match guard and `Key::Down` its floor in
    /// another, two bounds written a line apart in two different places.
    fn move_bid(&mut self, step: i16) -> bool {
        if self.phase != Phase::Bidding || !self.current_player.is_human() {
            return false;
        }
        let next = i16::from(self.bid_selection)
            .saturating_add(step)
            .clamp(0, i16::from(MAX_BID));
        let next = u8::try_from(next).unwrap_or(0);
        if next == self.bid_selection {
            return false;
        }
        self.bid_selection = next;
        self.set_turn_status();
        true
    }

    /// Whatever Enter means in this phase.
    ///
    /// The button in the footer and the key both call this, which is why
    /// `confirm_label` can promise what pressing it will do.
    fn confirm(&mut self) -> bool {
        match self.phase {
            Phase::Bidding => self.submit_human_bid(),
            Phase::Playing => self.try_play_selected(),
            Phase::TrickDone => {
                self.advance_after_trick();
                true
            }
            Phase::RoundOver => {
                self.advance_round();
                true
            }
            Phase::GameOver => {
                self.new_game();
                true
            }
        }
    }

    /// Re-sort every hand, keeping the pointer on the card it was on.
    fn resort(&mut self) -> bool {
        let held = self.hands[0].get(self.selected_card).copied();
        self.sort_order = self.sort_order.toggle();
        self.sort_all_hands();
        // The pointer follows the card, not the index: a re-sort that left the
        // pointer where it was would silently move it to a different card.
        if let Some(card) = held
            && let Some(i) = self.hands[0].iter().position(|c| *c == card)
        {
            self.selected_card = i;
        }
        self.clamp_selected_card();
        true
    }

    /// Do what a button says, whether it was clicked or its key was pressed.
    fn press(&mut self, button: Button) -> bool {
        match button {
            Button::Confirm => self.confirm(),
            Button::Sort => self.resort(),
            Button::New => {
                self.new_game();
                true
            }
            Button::Help => {
                self.show_help = !self.show_help;
                true
            }
        }
    }

    /// Click or choose the `i`th card of the human's hand.
    fn touch_card(&mut self, index: usize) -> bool {
        if index >= self.hands[0].len() {
            return false;
        }
        self.selected_card = index;
        self.try_play_selected();
        true
    }

    /// Click a bid on the pad.
    fn touch_bid(&mut self, value: u8) -> bool {
        if value > MAX_BID {
            return false;
        }
        self.bid_selection = value;
        self.submit_human_bid()
    }

    // ── Team bid total for a team ───────────────────────────────────

    /// The two seats of a partnership. `TEAM_SEATS` is the one statement of
    /// which seats sit together; this used to be written out four times.
    fn seats_of(team: usize) -> (PlayerId, PlayerId) {
        TEAM_SEATS
            .get(team)
            .copied()
            .unwrap_or((PlayerId::SOUTH, PlayerId::NORTH))
    }

    /// What a partnership between them contracted for.
    fn team_bid(&self, team: usize) -> u8 {
        self.team_total(team, PlayerRound::bid_value)
    }

    /// What a partnership between them has actually taken.
    fn team_tricks(&self, team: usize) -> u8 {
        self.team_total(team, |pr| pr.tricks_won)
    }

    fn team_total(&self, team: usize, of: impl Fn(&PlayerRound) -> u8) -> u8 {
        let (p1, p2) = Self::seats_of(team);
        let one = |p: PlayerId| self.player_rounds.get(p.index()).map_or(0, &of);
        one(p1).saturating_add(one(p2))
    }

    // ── Events ──────────────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> EventResult {
        if !event.pressed {
            return EventResult::Ignored;
        }
        // A key with a modifier on it belongs to the window, not to the game:
        // Ctrl+N is the compositor's new window, and it used to throw away the
        // round in progress because no modifier was ever examined anywhere.
        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {
            return EventResult::Ignored;
        }
        let changed = match event.key {
            Key::Left => self.move_selection(-1),
            Key::Right => self.move_selection(1),
            Key::Up => self.move_bid(1),
            Key::Down => self.move_bid(-1),
            Key::Space => self.confirm(),
            key => match BUTTONS.into_iter().find(|b| b.key() == key) {
                Some(button) => self.press(button),
                None => return EventResult::Ignored,
            },
        };
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        if !matches!(event.kind, MouseEventKind::Press(MouseButton::Left)) {
            return EventResult::Ignored;
        }
        // Resolved against the frame the renderer last drew, so a click lands
        // on the card the player is looking at. The old handlers rebuilt both
        // the hand and the bid grid from their own copies of the constants,
        // and the bid grid's copy was twenty pixels right and twenty-five
        // pixels below where the pad was actually painted.
        let (w, h) = self.size;
        let changed = match self.frame(w, h).hit_test(event.x, event.y) {
            Some(Target::Card(i)) => self.touch_card(i),
            Some(Target::Bid(v)) => self.touch_bid(v),
            Some(Target::Button(b)) => self.press(b),
            // The help card covers the table; a click on it dismisses the card
            // rather than falling through to whatever is underneath.
            Some(Target::Help) => {
                self.show_help = false;
                true
            }
            None => false,
        };
        if changed {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────────

impl SpadesGame {
    /// The whole window: every rectangle solved from the size handed in, and
    /// everything the pointer can reach recorded as a hit box.
    ///
    /// The old renderer took no size at all. It filled a 900x700 rectangle with
    /// the background colour and then drew five panels at compile-time
    /// coordinates, so in any other window the picture was the wrong size and
    /// the sidebar at `SIDEBAR_X = 720` hung off the edge.
    fn frame(&self, width: f32, height: f32) -> Frame<Target> {
        let mut f = Frame::new(width, height);
        let l = Layout::solve(width, height);
        fill(&mut f, l.window, BASE, 0.0);
        self.draw_header(&mut f, &l);
        self.draw_table(&mut f, &l);
        self.draw_hand(&mut f, &l);
        self.draw_footer(&mut f, &l);
        self.draw_status(&mut f, &l);
        if self.show_help {
            self.draw_help(&mut f, &l);
        }
        f
    }

    /// The title bar: the game's name, and which round is being played.
    fn draw_header(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.header, MANTLE, 0.0);
        if l.header.is_empty() {
            return;
        }
        text_at(
            f,
            l.pad,
            l.header.y + (l.header.h - l.title) / 2.0,
            TITLE,
            LAVENDER,
            l.title,
            FontWeightHint::Bold,
        );
        let right = format!("Round {}", self.round_number);
        let w = text::measure(&right, l.small, FontWeightHint::Regular);
        let x = l.header.right() - l.pad - w;
        // Measured against the title rather than assumed to clear it: the two
        // are left out rather than painted through one another.
        if x > l.pad + text::measure(TITLE, l.title, FontWeightHint::Bold) + l.pad {
            text_at(
                f,
                x,
                l.header.y + (l.header.h - l.small) / 2.0,
                &right,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    /// The felt, and everything on it.
    fn draw_table(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.table, FELT, l.pad * 0.6);
        self.draw_trick(f, l);
        self.draw_seats(f, l);
        self.draw_panel(f, l);
        if self.phase == Phase::Bidding && self.current_player.is_human() {
            self.draw_bid_pad(f, l);
        }
    }

    /// The cards in play, and a ring round the one that took them.
    ///
    /// A settled trick stays here for `SWEEP_MS`. The old program kept the
    /// finished trick in `last_trick` and drew it, but nothing ever timed it
    /// out: the game sat at `TrickDone` until the human pressed Enter, and
    /// pressing Enter was also the only thing that let the machines play, so a
    /// game left alone stopped where it stood.
    fn draw_trick(&self, f: &mut Frame<Target>, l: &Layout) {
        let settled = self.phase == Phase::TrickDone;
        let trick = if settled {
            self.last_trick.as_ref().unwrap_or(&self.current_trick)
        } else {
            &self.current_trick
        };
        let taker = if settled { trick.winner() } else { None };
        // While the trick is live, the card that was led is the one everyone
        // else must follow, so it is marked; once the trick has settled the
        // taker's ring replaces it, because that is then the fact that matters.
        let leader = if settled { None } else { trick.leader() };
        let stroke = (l.card.0 * 0.07).clamp(1.0, 4.0);
        for &(player, card) in &trick.cards {
            let r = l.trick_card(player.index());
            draw_card_face(f, r, card, false);
            if taker == Some(player) {
                outline(f, r, GREEN, stroke);
            } else if leader == Some(player) {
                outline(f, r, LAVENDER, stroke);
            }
        }
        if trick.cards.is_empty()
            && let Some(message) = self.table_message()
        {
            let (_, cy) = l.table.centre();
            centred(
                f,
                l.table.x,
                l.table.w,
                cy - l.font / 2.0,
                &message,
                MAUVE,
                l.font,
                FontWeightHint::Bold,
            );
        }
        if self.spades_broken {
            bounded(
                f,
                (l.table.x + l.pad, l.table.bottom() - l.pad - l.small),
                (l.table.w - l.pad * 2.0).max(0.0),
                &format!("{} broken", Suit::Spades.name()),
                LAVENDER,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    /// What is written across the middle of an empty table.
    ///
    /// Deliberately not the status line: the two say different things, so a
    /// test that searched the frame for a string cannot pass for the wrong
    /// reason.
    fn table_message(&self) -> Option<String> {
        match self.phase {
            Phase::Bidding | Phase::Playing | Phase::TrickDone => None,
            Phase::RoundOver => Some(format!(
                "Press {} for the next round",
                Button::Confirm.key_name()
            )),
            Phase::GameOver => Some(self.winner_message.clone()),
        }
    }

    /// The four seats: who they are, how much they still hold, who is to act.
    ///
    /// The old program wrote three compass letters at literal offsets from a
    /// literal centre and put everything else in a sidebar pinned at x = 720.
    fn draw_seats(&self, f: &mut Frame<Target>, l: &Layout) {
        for index in 0..SEATS {
            let r = l.seat_label(index);
            if r.is_empty() {
                continue;
            }
            let pid = seat(index);
            let acting = pid == self.current_player && self.sweep_ms == 0;
            fill(f, r, if acting { SURFACE1 } else { SURFACE0 }, l.pad * 0.3);
            let budget = (r.w - l.pad * 0.6).max(0.0);
            bounded(
                f,
                (r.x + l.pad * 0.3, r.y + r.h * 0.08),
                budget,
                pid.name(),
                if acting { YELLOW } else { TEXT_COLOR },
                l.small,
                FontWeightHint::Bold,
            );
            bounded(
                f,
                (r.x + l.pad * 0.3, r.y + r.h * 0.52),
                budget,
                &format!("{} left", self.hand_of(pid).len()),
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
        }
    }

    /// The scores, the bags and every seat's bid against what it has taken.
    ///
    /// Left out rather than squeezed when the window cannot pay for it, which
    /// is the answer the old sidebar could not give: it was drawn at x = 720
    /// whatever the window was, so in anything narrower than 890 it was off the
    /// right-hand edge entirely.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.is_empty() {
            return;
        }
        fill(f, l.panel, SURFACE0, l.pad * 0.4);
        let x = l.panel.x + l.pad * 0.6;
        let budget = (l.panel.w - l.pad * 1.2).max(0.0);
        let step = l.small * 1.9;
        let mut y = l.panel.y + l.pad;
        let mut row = |f: &mut Frame<Target>, text: &str, color: Color, weight| {
            bounded(f, (x, y), budget, text, color, l.small, weight);
            y += step;
        };
        row(f, "Scores", LAVENDER, FontWeightHint::Bold);
        for (index, team) in self.teams.iter().enumerate() {
            let text = format!(
                "{}: {} \u{00b7} {} bags",
                team_name(index),
                team.score,
                team.bags
            );
            let color = if index == 0 { GREEN } else { PEACH };
            row(f, &text, color, FontWeightHint::Bold);
            // The partnership's contract against what it has taken -- the one
            // number that says whether the round is being made or set, and the
            // number the old sidebar never showed at all.
            let contract = format!(
                "  bid {}, won {}",
                self.team_bid(index),
                self.team_tricks(index)
            );
            row(f, &contract, SUBTEXT0, FontWeightHint::Regular);
        }
        for (index, pr) in self.player_rounds.iter().enumerate() {
            let bid = pr.bid.map_or_else(|| String::from("\u{2014}"), bid_name);
            let text = format!(
                "{}: bid {} \u{00b7} {} won",
                seat(index).name(),
                bid,
                pr.tricks_won
            );
            row(f, &text, SUBTEXT0, FontWeightHint::Regular);
        }
    }

    /// The pad the human bids from: fourteen buttons, nil to thirteen.
    ///
    /// Every button is recorded as a hit box by the pass that paints it, which
    /// is the whole fix: the old overlay was drawn from `overlay_x =
    /// TRICK_CENTER_X - 140.0` and `grid_start_y = overlay_y + 75.0`, and hit
    /// tested against `overlay_x = TRICK_CENTER_X - 120.0` and `overlay_y +
    /// 50.0`, so a click on the button marked 5 was answered by the square
    /// drawn one row up and half a cell left of it.
    fn draw_bid_pad(&self, f: &mut Frame<Target>, l: &Layout) {
        let pad = l.bid_pad();
        if pad.is_empty() {
            return;
        }
        fill(f, pad, MANTLE, l.pad * 0.6);
        outline(f, pad, LAVENDER, (l.pad * 0.12).clamp(1.0, 3.0));
        centred(
            f,
            pad.x,
            pad.w,
            pad.y + l.pad * 0.7,
            &format!("Your bid: {}", bid_name(self.bid_selection)),
            TEXT_COLOR,
            l.font,
            FontWeightHint::Bold,
        );
        for value in 0..=MAX_BID {
            let r = l.bid_button(value);
            if r.is_empty() {
                continue;
            }
            let on = value == self.bid_selection;
            fill(f, r, if on { BLUE } else { SURFACE1 }, r.w * 0.2);
            let size = (r.w * 0.42).max(7.0);
            centred(
                f,
                r.x,
                r.w,
                r.y + (r.h - size) / 2.0,
                &bid_key_label(value),
                if on { BASE } else { TEXT_COLOR },
                size,
                FontWeightHint::Bold,
            );
            f.hit(Target::Bid(value), r);
        }
    }

    /// The human's hand, and the hit box of every card in it.
    ///
    /// The card the pointer is on is lifted out of the fan, and the hit box
    /// recorded is the lifted rectangle -- the one that was painted. The old
    /// program drew a selected card ten pixels high and searched the strip it
    /// had left, so the top ten pixels of the card the player was aiming at
    /// were the one part of it that could not be clicked.
    fn draw_hand(&self, f: &mut Frame<Target>, l: &Layout) {
        let hand = &self.hands[0];
        let n = hand.len();
        let choosing = self.phase == Phase::Playing && self.current_player.is_human();
        let legal = if choosing {
            self.legal_plays(PlayerId::SOUTH)
        } else {
            Vec::new()
        };
        for (i, &card) in hand.iter().enumerate() {
            let on = choosing && i == self.selected_card;
            let r = if on {
                l.hand_card(i, n).translated(0.0, -l.hand_lift())
            } else {
                l.hand_card(i, n)
            };
            draw_card_face(f, r, card, choosing && !legal.contains(&i));
            if on {
                outline(f, r, YELLOW, (l.card.0 * 0.07).clamp(1.0, 4.0));
            }
            f.hit(Target::Card(i), r);
        }
    }

    /// The row of buttons, each of which duplicates a key.
    ///
    /// The old window had no buttons at all: every verb was a keystroke, and
    /// the only list of them was one line of grey text in the footer.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, BASE, 0.0);
        if l.footer.is_empty() {
            return;
        }
        let size = (l.small * 0.95).max(7.0);
        let gap = l.pad * 0.4;
        let h = (l.footer.h - gap).max(0.0);
        let y = l.footer.y + (l.footer.h - h) / 2.0;
        let mut x = l.pad;
        for button in BUTTONS {
            let label = self.button_label(button);
            let w = text::measure(label, size, FontWeightHint::Bold) + l.pad;
            // A button that would not fit whole is left out rather than drawn
            // off the edge of the window.
            if x + w > l.footer.right() - l.pad {
                break;
            }
            let r = Rect::new(x, y, w, h);
            let on = button == Button::Help && self.show_help;
            fill(f, r, if on { SURFACE1 } else { SURFACE0 }, h * 0.25);
            let (cx, cy) = r.centre();
            text_at(
                f,
                cx - text::measure(label, size, FontWeightHint::Bold) / 2.0,
                cy - size / 2.0,
                label,
                if on { TEXT_COLOR } else { SUBTEXT0 },
                size,
                FontWeightHint::Bold,
            );
            f.hit(Target::Button(button), r);
            x += w + gap;
        }
    }

    /// The one line of prose, in a bar of its own.
    ///
    /// The old program drew the status at `FOOTER_Y - 25.0` and a list of
    /// keystrokes at `FOOTER_Y`, both unbounded, so a status naming a seat ran
    /// straight off the right edge of any window narrower than the one the
    /// coordinates were written for.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.status, MANTLE, 0.0);
        if l.status.is_empty() {
            return;
        }
        let y = l.status.y + (l.status.h - l.font) / 2.0;
        let mut budget = (l.status.w - l.pad * 2.0).max(0.0);

        if matches!(self.phase, Phase::Playing | Phase::TrickDone) {
            let counter = format!("Trick {}/13", self.tricks_played.saturating_add(1).min(13));
            let w = text::measure(&counter, l.small, FontWeightHint::Regular);
            // Drawn only if the status still has room to say something after
            // it; the prose is what the player is reading.
            if budget > w + l.pad * 4.0 {
                text_at(
                    f,
                    l.status.right() - l.pad - w,
                    l.status.y + (l.status.h - l.small) / 2.0,
                    &counter,
                    SUBTEXT0,
                    l.small,
                    FontWeightHint::Regular,
                );
                budget = (budget - w - l.pad).max(0.0);
            }
        }

        let color = match self.phase {
            Phase::GameOver => RED,
            Phase::RoundOver => YELLOW,
            Phase::TrickDone => TEAL,
            Phase::Bidding | Phase::Playing => TEXT_COLOR,
        };
        bounded(
            f,
            (l.pad, y),
            budget,
            &self.status_message,
            color,
            l.font,
            FontWeightHint::Regular,
        );
    }

    /// The help card, sized from the rows it holds.
    ///
    /// The keys it lists are `Button::key_name` and the labels are
    /// `button_label` -- the same two functions the footer draws from, so a
    /// button whose label changes cannot leave the help card describing the old
    /// one. The old window had no help at all.
    fn draw_help(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);

        let mut rows: Vec<(String, String)> = vec![
            (
                String::from("\u{2190} \u{2192}"),
                String::from("Move along your hand"),
            ),
            (
                String::from("\u{2191} \u{2193}"),
                String::from("Raise or lower your bid"),
            ),
            (String::from("Space"), String::from("Same as Enter")),
        ];
        rows.extend(BUTTONS.iter().map(|&b| {
            (
                String::from(b.key_name()),
                String::from(self.button_label(b)),
            )
        }));

        let key_w = rows.iter().fold(0.0f32, |acc, (k, _)| {
            acc.max(text::measure(k, l.small, FontWeightHint::Bold))
        });
        let desc_w = rows.iter().fold(0.0f32, |acc, (_, d)| {
            acc.max(text::measure(d, l.small, FontWeightHint::Regular))
        });
        let heading = "How to play";
        let inner =
            (key_w + desc_w + l.pad).max(text::measure(heading, l.title, FontWeightHint::Bold));
        #[expect(clippy::cast_precision_loss, reason = "seven rows; exact in f32")]
        let rows_h = rows.len() as f32 * l.small * 1.8;
        let card_w = (inner + l.pad * 2.0).min(l.window.w);
        let card_h = (rows_h + l.title * 2.2 + l.pad).min(l.window.h);
        let card = Rect::new(
            (l.window.w - card_w) / 2.0,
            (l.window.h - card_h) / 2.0,
            card_w,
            card_h,
        );
        fill(f, card, MANTLE, l.pad * 0.6);
        centred(
            f,
            card.x,
            card.w,
            card.y + l.pad,
            heading,
            TEXT_COLOR,
            l.title,
            FontWeightHint::Bold,
        );

        let mut y = card.y + l.title * 1.8;
        for (key, desc) in &rows {
            if y + l.small > card.bottom() {
                break;
            }
            text_at(
                f,
                card.x + l.pad,
                y,
                key,
                BLUE,
                l.small,
                FontWeightHint::Bold,
            );
            text_at(
                f,
                card.x + l.pad + key_w + l.pad,
                y,
                desc,
                SUBTEXT0,
                l.small,
                FontWeightHint::Regular,
            );
            y += l.small * 1.8;
        }
        // The card swallows the click that dismisses it, so a player reaching
        // for a line of the help does not play the card underneath it.
        f.hit(Target::Help, card);
    }

    /// What a button says now.
    ///
    /// `Confirm` says five different things across the phases, which is why it
    /// asks `confirm_label`: the footer, the help card and the test that holds
    /// the two together all read the one function.
    fn button_label(&self, button: Button) -> &'static str {
        match button {
            Button::Confirm => confirm_label(self.phase),
            Button::Sort => match self.sort_order {
                SortOrder::BySuit => "Sort by rank",
                SortOrder::ByRank => "Sort by suit",
            },
            Button::New => "New game",
            Button::Help => "Help",
        }
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// The seat at `index`, counting clockwise from the human's.
///
/// Total, so that no caller has to carry a bound: seats are produced by
/// `0..SEATS` and by `% SEATS`, so the wrap is unreachable.
fn seat(index: usize) -> PlayerId {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "index % SEATS is 0..4, which is a u8"
    )]
    PlayerId((index % SEATS) as u8)
}

/// What a partnership is called.
fn team_name(team: usize) -> &'static str {
    if team == 0 { "NS" } else { "EW" }
}

/// A bid, spelled the way the player reads it.
fn bid_name(value: u8) -> String {
    if value == 0 {
        String::from("Nil")
    } else {
        value.to_string()
    }
}

/// What is written on the bid pad's button for `value`.
///
/// Nil is "N": the cell is a square about as wide as two digits, and "Nil"
/// spelled out would be drawn over its neighbour.
fn bid_key_label(value: u8) -> String {
    if value == 0 {
        String::from("N")
    } else {
        value.to_string()
    }
}

/// A filled rectangle, skipped when there is nothing to fill.
fn fill(f: &mut Frame<Target>, r: Rect, color: Color, radius: f32) {
    if r.is_empty() {
        return;
    }
    f.push(RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    });
}

/// A ring round a rectangle: the selection, and the taker of the trick.
fn outline(f: &mut Frame<Target>, r: Rect, color: Color, line_width: f32) {
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
        corner_radii: CornerRadii::all(r.w * 0.12),
    });
}

/// One run of text.
fn text_at(
    f: &mut Frame<Target>,
    x: f32,
    y: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    f.push(RenderCommand::Text {
        x,
        y,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: None,
        overflow: TextOverflow::Clip,
    });
}

/// One run of text that is cut with an ellipsis rather than allowed to run on.
///
/// Every string the old program drew was `max_width: None`, including the four
/// panel columns and the status line.
fn bounded(
    f: &mut Frame<Target>,
    at: (f32, f32),
    width: f32,
    s: &str,
    color: Color,
    font_size: f32,
    font_weight: FontWeightHint,
) {
    f.push(RenderCommand::Text {
        x: at.0,
        y: at.1,
        text: String::from(s),
        color,
        font_size,
        font_weight,
        max_width: Some(width),
        overflow: TextOverflow::Ellipsis,
    });
}

/// A run of text centred in `[x, x + w)`, by measuring it.
///
/// The old program centred by subtracting a literal -- `x + CARD_W / 2.0 - 8.0`
/// for the suit in the middle of a card, `overlay_x + 80.0` for a heading --
/// which is half of one particular string at one particular size, in a program
/// that links `guitk::text`.
fn centred(
    f: &mut Frame<Target>,
    x: f32,
    w: f32,
    y: f32,
    s: &str,
    color: Color,
    size: f32,
    weight: FontWeightHint,
) {
    let measured = text::measure(s, size, weight);
    text_at(f, x + (w - measured) / 2.0, y, s, color, size, weight);
}

/// A card, face up: rank and suit in the corner, the suit again in the middle.
///
/// `dim` is a card the rules will not allow to be played now.
fn draw_card_face(f: &mut Frame<Target>, r: Rect, card: Card, dim: bool) {
    if r.is_empty() {
        return;
    }
    fill(f, r, if dim { SUBTEXT0 } else { CARD_FACE }, r.w * 0.12);
    let ink = if dim { OVERLAY0 } else { card.suit.ink() };
    let corner = (r.w * 0.30).max(6.0);
    // A card too small to letter is left blank rather than scribbled over:
    // below about twenty pixels the smallest legible rank is wider than the
    // card it would be written on.
    if corner * 2.0 > r.w {
        return;
    }
    text_at(
        f,
        r.x + r.w * 0.10,
        r.y + r.h * 0.06,
        card.rank.label(),
        ink,
        corner,
        FontWeightHint::Bold,
    );
    text_at(
        f,
        r.x + r.w * 0.10,
        corner.mul_add(1.05, r.y + r.h * 0.06),
        card.suit.symbol(),
        ink,
        corner,
        FontWeightHint::Regular,
    );
    let big = r.w * 0.46;
    let measured = text::measure(card.suit.symbol(), big, FontWeightHint::Regular);
    text_at(
        f,
        r.right() - r.w * 0.12 - measured,
        r.bottom() - r.h * 0.10 - big,
        card.suit.symbol(),
        ink,
        big,
        FontWeightHint::Regular,
    );
}

// ── The window ──────────────────────────────────────────────────────

impl App for SpadesGame {
    fn title(&self) -> String {
        String::from(TITLE)
    }

    fn app_id(&self) -> String {
        String::from("spades")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Without a tick the machine seats never act: the old program bid and
        // played for all three of them inside the human's own event handler.
        Some(std::time::Duration::from_millis(TICK_MS))
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.size = (width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for SpadesGame {
    type Target = Target;
    type Outcome = EventResult;
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> EventResult {
        self.size = size;
        self.handle_event(&Event::Key(key.clone()))
    }
}

fn main() -> ExitCode {
    let mut app = SpadesGame::new();
    app::launch("spades", &mut app)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        clippy::panic,
        reason = "a test that indexes past the end should fail loudly"
    )]

    use super::*;
    use guitk::probe::press;

    /// Run the clock until the game stops asking for time.
    ///
    /// Every test that wants to see what the machine seats did has to say so,
    /// because they act on a clock now. The old program ran all three of them
    /// inside the human's own event, so a test could not tell the difference
    /// between "the machines answered" and "the human's click was handled".
    fn settle(game: &mut SpadesGame) {
        let step = u64::from(THINK_MS.max(SWEEP_MS));
        for _ in 0..10_000 {
            if !game.tick(step) {
                return;
            }
        }
        panic!("the clock never stopped");
    }

    // ── RNG tests ───────────────────────────────────────────────────

    // The generator itself is `guitk::rng`'s to test; what belongs here is
    // the deal — that the shuffle turns a deck into four honest hands, and
    // that a session's cards are not the same cards every session.

    #[test]
    fn a_deal_gives_every_player_a_full_hand_and_uses_the_whole_deck() {
        for seed in 0..24u64 {
            let game = SpadesGame::with_seed(seed);
            let mut dealt: Vec<Card> = Vec::new();
            for hand in &game.hands {
                assert_eq!(hand.len(), HAND_SIZE, "seed {seed}");
                dealt.extend(hand.iter().copied());
            }
            let mut deck = standard_deck();
            dealt.sort_by_key(|c| (c.suit as u8, c.rank.value()));
            deck.sort_by_key(|c| (c.suit as u8, c.rank.value()));
            assert_eq!(dealt, deck, "seed {seed}: a hand is missing or doubled");
        }
    }

    #[test]
    fn the_same_seed_deals_the_same_hands() {
        let first = SpadesGame::with_seed(2024);
        let second = SpadesGame::with_seed(2024);
        assert_eq!(first.hands, second.hands);
    }

    #[test]
    fn different_seeds_deal_different_hands() {
        let first = SpadesGame::with_seed(1);
        let second = SpadesGame::with_seed(2);
        assert_ne!(first.hands, second.hands);
    }

    #[cfg(unix)]
    #[test]
    fn two_launches_do_not_play_the_same_hand() {
        // The whole point of the kernel seed. The game used to start from a
        // literal 42, so every launch on every machine dealt the identical
        // first hand — the same thirteen cards to South, every time.
        //
        // Host-only builds cannot run this: there is no kernel CSPRNG behind
        // the test toolchain, so `session_rng` takes its documented fallback
        // and both games really are identical there.
        let first = SpadesGame::new();
        let second = SpadesGame::new();
        assert_ne!(first.hands, second.hands);
    }

    #[test]
    fn starting_a_new_game_reshuffles() {
        let mut game = SpadesGame::with_seed(5);
        let opening = game.hands.clone();
        game.new_game();
        assert_ne!(game.hands, opening, "a new game is a new deal");
        // …and is still a legal deal.
        for hand in &game.hands {
            assert_eq!(hand.len(), HAND_SIZE);
        }
    }

    #[test]
    fn a_reseed_that_lands_on_zero_still_deals() {
        // `new_game` reseeds from the current generator's own next draw, which
        // can be any value including zero — the one state a xorshift can never
        // leave. The generator substitutes a non-zero constant for it, so the
        // deal that follows is a real one rather than fifty-two cards left in
        // deck order.
        let game = SpadesGame::with_seed(0);
        assert_eq!(game.hands[0].len(), HAND_SIZE);
        let deck_order = standard_deck();
        let dealt_in_order = game
            .hands
            .iter()
            .flat_map(|hand| hand.iter().copied())
            .eq(deck_order.iter().copied());
        assert!(!dealt_in_order, "a zero seed left the deck unshuffled");
    }

    // ── Card tests ──────────────────────────────────────────────────

    #[test]
    fn test_standard_deck_size() {
        let deck = standard_deck();
        assert_eq!(deck.len(), 52);
    }

    #[test]
    fn test_standard_deck_unique() {
        let deck = standard_deck();
        let mut seen = std::collections::HashSet::new();
        for card in &deck {
            assert!(seen.insert((card.suit, card.rank)));
        }
    }

    #[test]
    fn test_standard_deck_suits() {
        let deck = standard_deck();
        for suit in &Suit::ALL {
            let count = deck.iter().filter(|c| c.suit == *suit).count();
            assert_eq!(count, 13);
        }
    }

    #[test]
    fn test_card_beats_same_suit_higher_rank() {
        let ace_spades = Card::new(Suit::Spades, Rank::ACE);
        let king_spades = Card::new(Suit::Spades, Rank::KING);
        assert!(ace_spades.beats(king_spades, Suit::Spades));
        assert!(!king_spades.beats(ace_spades, Suit::Spades));
    }

    #[test]
    fn test_card_beats_trump_over_non_trump() {
        let two_spades = Card::new(Suit::Spades, Rank::TWO);
        let ace_hearts = Card::new(Suit::Hearts, Rank::ACE);
        assert!(two_spades.beats(ace_hearts, Suit::Hearts));
    }

    #[test]
    fn test_card_beats_non_trump_off_suit_loses() {
        let ace_clubs = Card::new(Suit::Clubs, Rank::ACE);
        let two_hearts = Card::new(Suit::Hearts, Rank::TWO);
        // Hearts led, clubs off suit: clubs lose
        assert!(!ace_clubs.beats(two_hearts, Suit::Hearts));
    }

    #[test]
    fn test_card_beats_led_suit_wins_over_off_suit() {
        let five_hearts = Card::new(Suit::Hearts, Rank::FIVE);
        let ace_clubs = Card::new(Suit::Clubs, Rank::ACE);
        // Hearts led
        assert!(five_hearts.beats(ace_clubs, Suit::Hearts));
    }

    #[test]
    fn test_card_sort_key_suit_ordering() {
        let two_clubs = Card::new(Suit::Clubs, Rank::TWO);
        let ace_clubs = Card::new(Suit::Clubs, Rank::ACE);
        assert!(two_clubs.sort_key_suit() < ace_clubs.sort_key_suit());
    }

    #[test]
    fn test_card_sort_key_suit_different_suits() {
        let ace_clubs = Card::new(Suit::Clubs, Rank::ACE);
        let two_diamonds = Card::new(Suit::Diamonds, Rank::TWO);
        assert!(ace_clubs.sort_key_suit() < two_diamonds.sort_key_suit());
    }

    #[test]
    fn test_suit_is_trump() {
        assert!(Suit::Spades.is_trump());
        assert!(!Suit::Hearts.is_trump());
        assert!(!Suit::Diamonds.is_trump());
        assert!(!Suit::Clubs.is_trump());
    }

    #[test]
    fn test_rank_labels() {
        assert_eq!(Rank::TWO.label(), "2");
        assert_eq!(Rank::TEN.label(), "10");
        assert_eq!(Rank::JACK.label(), "J");
        assert_eq!(Rank::QUEEN.label(), "Q");
        assert_eq!(Rank::KING.label(), "K");
        assert_eq!(Rank::ACE.label(), "A");
    }

    #[test]
    fn test_rank_ordering() {
        assert!(Rank::ACE > Rank::KING);
        assert!(Rank::KING > Rank::QUEEN);
        assert!(Rank::QUEEN > Rank::JACK);
        assert!(Rank::JACK > Rank::TEN);
        assert!(Rank::TEN > Rank::TWO);
    }

    // ── Player tests ────────────────────────────────────────────────

    #[test]
    fn test_player_next_wraps() {
        assert_eq!(PlayerId::SOUTH.next(), PlayerId::EAST);
        assert_eq!(PlayerId::EAST.next(), PlayerId::NORTH);
        assert_eq!(PlayerId::NORTH.next(), PlayerId::WEST);
        assert_eq!(PlayerId::WEST.next(), PlayerId::SOUTH);
    }

    #[test]
    fn test_player_teams() {
        assert_eq!(PlayerId::SOUTH.team(), 0);
        assert_eq!(PlayerId::NORTH.team(), 0);
        assert_eq!(PlayerId::EAST.team(), 1);
        assert_eq!(PlayerId::WEST.team(), 1);
    }

    #[test]
    fn test_player_is_human() {
        assert!(PlayerId::SOUTH.is_human());
        assert!(!PlayerId::EAST.is_human());
        assert!(!PlayerId::NORTH.is_human());
        assert!(!PlayerId::WEST.is_human());
    }

    #[test]
    fn test_player_index() {
        assert_eq!(PlayerId::SOUTH.index(), 0);
        assert_eq!(PlayerId::EAST.index(), 1);
        assert_eq!(PlayerId::NORTH.index(), 2);
        assert_eq!(PlayerId::WEST.index(), 3);
    }

    // ── Trick tests ─────────────────────────────────────────────────

    #[test]
    fn test_trick_new() {
        let trick = Trick::new();
        assert_eq!(trick.leader(), None);
        assert!(trick.cards.is_empty());
        assert!(!trick.is_complete());
    }

    #[test]
    fn test_trick_leader_is_whoever_played_first() {
        let mut trick = Trick::new();
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::TWO));
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        assert_eq!(trick.leader(), Some(PlayerId::WEST));
    }

    #[test]
    fn test_trick_led_suit() {
        let mut trick = Trick::new();
        assert_eq!(trick.led_suit(), None);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        assert_eq!(trick.led_suit(), Some(Suit::Hearts));
    }

    #[test]
    fn test_trick_is_complete() {
        let mut trick = Trick::new();
        for i in 0..4 {
            trick.add(PlayerId(i), Card::new(Suit::Hearts, Rank(2 + i)));
            if i < 3 {
                assert!(!trick.is_complete());
            }
        }
        assert!(trick.is_complete());
    }

    #[test]
    fn test_trick_winner_highest_of_led_suit() {
        let mut trick = Trick::new();
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::FIVE));
        trick.add(PlayerId::EAST, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::NORTH, Card::new(Suit::Hearts, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::TWO));
        assert_eq!(trick.winner(), Some(PlayerId::EAST));
    }

    #[test]
    fn test_trick_winner_trump_beats_all() {
        let mut trick = Trick::new();
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::EAST, Card::new(Suit::Spades, Rank::TWO));
        trick.add(PlayerId::NORTH, Card::new(Suit::Hearts, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::QUEEN));
        assert_eq!(trick.winner(), Some(PlayerId::EAST));
    }

    #[test]
    fn test_trick_winner_highest_trump_wins() {
        let mut trick = Trick::new();
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::EAST, Card::new(Suit::Spades, Rank::TWO));
        trick.add(PlayerId::NORTH, Card::new(Suit::Spades, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::QUEEN));
        assert_eq!(trick.winner(), Some(PlayerId::NORTH));
    }

    #[test]
    fn test_trick_winner_off_suit_loses_to_led() {
        let mut trick = Trick::new();
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::TWO));
        trick.add(PlayerId::EAST, Card::new(Suit::Clubs, Rank::ACE));
        trick.add(PlayerId::NORTH, Card::new(Suit::Diamonds, Rank::ACE));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::THREE));
        assert_eq!(trick.winner(), Some(PlayerId::WEST));
    }

    // `test_trick_contains_spade` went with the method it exercised.  What
    // it was really asking -- "does playing a spade break spades" -- is
    // covered by `test_play_spade_breaks_spades`, against the field the game
    // actually consults.

    #[test]
    fn test_trick_winner_empty() {
        let trick = Trick::new();
        assert_eq!(trick.winner(), None);
    }

    // ── Deal tests ──────────────────────────────────────────────────

    #[test]
    fn test_deal_gives_13_cards_each() {
        let game = SpadesGame::new();
        for i in 0..4 {
            assert_eq!(game.hands[i].len(), 13);
        }
    }

    #[test]
    fn test_deal_all_cards_unique() {
        let game = SpadesGame::new();
        let mut all_cards: Vec<Card> = Vec::new();
        for hand in &game.hands {
            all_cards.extend(hand);
        }
        assert_eq!(all_cards.len(), 52);
        let mut seen = std::collections::HashSet::new();
        for card in &all_cards {
            assert!(seen.insert((card.suit, card.rank)));
        }
    }

    #[test]
    fn test_deal_hands_sorted() {
        let game = SpadesGame::new();
        for hand in &game.hands {
            for w in hand.windows(2) {
                assert!(w[0].sort_key_suit() <= w[1].sort_key_suit());
            }
        }
    }

    // ── Bidding tests ───────────────────────────────────────────────

    #[test]
    fn test_initial_phase_is_bidding() {
        let game = SpadesGame::new();
        assert_eq!(game.phase, Phase::Bidding);
    }

    #[test]
    fn test_ai_bid_range() {
        let game = SpadesGame::new();
        for pid_val in 1..4u8 {
            let bid = game.ai_bid(PlayerId(pid_val));
            assert!(
                (1..=6).contains(&bid),
                "AI bid {} out of expected range",
                bid
            );
        }
    }

    #[test]
    fn test_submit_bid_advances_phase() {
        let mut game = SpadesGame::new();
        settle(&mut game);
        assert!(game.current_player.is_human(), "the human's turn to bid");
        game.bid_selection = 4;
        assert!(game.submit_human_bid());
        settle(&mut game);
        assert_eq!(game.phase, Phase::Playing);
    }

    #[test]
    fn test_all_bids_set_after_bidding() {
        let mut game = SpadesGame::new();
        settle(&mut game);
        game.bid_selection = 3;
        assert!(game.submit_human_bid());
        settle(&mut game);
        for pr in &game.player_rounds {
            assert!(pr.bid.is_some());
        }
    }

    #[test]
    fn test_nil_bid_value() {
        let mut pr = PlayerRound::new();
        pr.bid = Some(0);
        assert!(pr.is_nil());
        assert_eq!(pr.bid_value(), 0);
    }

    #[test]
    fn test_non_nil_bid() {
        let mut pr = PlayerRound::new();
        pr.bid = Some(5);
        assert!(!pr.is_nil());
        assert_eq!(pr.bid_value(), 5);
    }

    // ── Legal play tests ────────────────────────────────────────────

    #[test]
    fn test_legal_plays_must_follow_suit() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::ACE),
            Card::new(Suit::Hearts, Rank::KING),
            Card::new(Suit::Clubs, Rank::TWO),
            Card::new(Suit::Spades, Rank::THREE),
        ];
        game.current_trick = Trick::new();
        game.current_trick
            .add(PlayerId::EAST, Card::new(Suit::Hearts, Rank::FIVE));

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 2); // Only hearts
        for &idx in &legal {
            assert_eq!(game.hands[0][idx].suit, Suit::Hearts);
        }
    }

    #[test]
    fn test_legal_plays_any_card_when_void() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = vec![
            Card::new(Suit::Clubs, Rank::ACE),
            Card::new(Suit::Spades, Rank::TWO),
            Card::new(Suit::Diamonds, Rank::THREE),
        ];
        game.current_trick = Trick::new();
        game.current_trick
            .add(PlayerId::EAST, Card::new(Suit::Hearts, Rank::FIVE));

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 3); // Can play anything
    }

    #[test]
    fn test_legal_plays_cannot_lead_spades_unbroken() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.spades_broken = false;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::ACE),
            Card::new(Suit::Spades, Rank::TWO),
            Card::new(Suit::Spades, Rank::KING),
        ];
        game.current_trick = Trick::new();

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 1); // Only hearts
        assert_eq!(game.hands[0][legal[0]].suit, Suit::Hearts);
    }

    #[test]
    fn test_legal_plays_can_lead_spades_when_broken() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.spades_broken = true;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::ACE),
            Card::new(Suit::Spades, Rank::TWO),
        ];
        game.current_trick = Trick::new();

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 2);
    }

    #[test]
    fn test_legal_plays_all_spades_can_lead_spade() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.spades_broken = false;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = vec![
            Card::new(Suit::Spades, Rank::ACE),
            Card::new(Suit::Spades, Rank::KING),
        ];
        game.current_trick = Trick::new();

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 2); // Forced to lead spade
    }

    #[test]
    fn test_legal_plays_empty_hand() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = Vec::new();
        game.current_trick = Trick::new();

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert!(legal.is_empty());
    }

    // ── Play card tests ─────────────────────────────────────────────

    #[test]
    fn test_play_card_removes_from_hand() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        // Fill all bids so scoring logic works
        for i in 0..4 {
            game.player_rounds[i].bid = Some(3);
        }
        let initial_len = game.hands[0].len();
        let card = game.hands[0][0];
        game.current_trick = Trick::new();
        game.play_card(PlayerId::SOUTH, 0);
        assert_eq!(game.hands[0].len(), initial_len - 1);
        assert!(!game.hands[0].contains(&card));
    }

    #[test]
    fn test_play_spade_breaks_spades() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.spades_broken = false;
        game.current_player = PlayerId::SOUTH;
        for i in 0..4 {
            game.player_rounds[i].bid = Some(3);
        }
        game.hands[0] = vec![Card::new(Suit::Spades, Rank::ACE)];
        game.current_trick = Trick::new();
        game.current_trick
            .add(PlayerId::EAST, Card::new(Suit::Hearts, Rank::FIVE));
        game.play_card(PlayerId::SOUTH, 0);
        assert!(game.spades_broken);
    }

    // ── Scoring tests ───────────────────────────────────────────────

    #[test]
    fn test_scoring_made_bid() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        // NS team bids 5, makes exactly 5
        game.player_rounds[0].bid = Some(3); // South
        game.player_rounds[2].bid = Some(2); // North
        game.player_rounds[0].tricks_won = 3;
        game.player_rounds[2].tricks_won = 2;
        // EW team
        game.player_rounds[1].bid = Some(4);
        game.player_rounds[3].bid = Some(2);
        game.player_rounds[1].tricks_won = 4;
        game.player_rounds[3].tricks_won = 2;
        game.tricks_played = 13;
        game.score_round();
        // NS: 5*10 = 50 points
        assert_eq!(game.teams[0].score, 50);
        // EW: 6*10 = 60 points
        assert_eq!(game.teams[1].score, 60);
    }

    #[test]
    fn test_scoring_overtricks_become_bags() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.player_rounds[0].bid = Some(3);
        game.player_rounds[2].bid = Some(2);
        game.player_rounds[0].tricks_won = 4; // 1 overtrick
        game.player_rounds[2].tricks_won = 3; // 1 overtrick
        game.player_rounds[1].bid = Some(2);
        game.player_rounds[3].bid = Some(1);
        game.player_rounds[1].tricks_won = 2;
        game.player_rounds[3].tricks_won = 1;
        game.tricks_played = 13;
        game.score_round();
        // NS: 5*10 + 2 = 52, 2 bags
        assert_eq!(game.teams[0].score, 52);
        assert_eq!(game.teams[0].bags, 2);
    }

    #[test]
    fn test_scoring_set_penalty() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.player_rounds[0].bid = Some(5);
        game.player_rounds[2].bid = Some(5);
        game.player_rounds[0].tricks_won = 3;
        game.player_rounds[2].tricks_won = 3; // Only 6, bid 10 -> set
        game.player_rounds[1].bid = Some(1);
        game.player_rounds[3].bid = Some(1);
        game.player_rounds[1].tricks_won = 5;
        game.player_rounds[3].tricks_won = 2;
        game.tricks_played = 13;
        game.score_round();
        // NS: -10 * 10 = -100
        assert_eq!(game.teams[0].score, -100);
    }

    #[test]
    fn test_scoring_nil_success() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.player_rounds[0].bid = Some(0); // Nil
        game.player_rounds[2].bid = Some(5);
        game.player_rounds[0].tricks_won = 0; // Made nil!
        game.player_rounds[2].tricks_won = 5;
        game.player_rounds[1].bid = Some(4);
        game.player_rounds[3].bid = Some(4);
        game.player_rounds[1].tricks_won = 4;
        game.player_rounds[3].tricks_won = 4;
        game.tricks_played = 13;
        game.score_round();
        // NS: +100 (nil) + 5*10 = 150
        assert_eq!(game.teams[0].score, 150);
    }

    #[test]
    fn test_scoring_nil_failure() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.player_rounds[0].bid = Some(0); // Nil
        game.player_rounds[2].bid = Some(5);
        game.player_rounds[0].tricks_won = 2; // Failed nil
        game.player_rounds[2].tricks_won = 5;
        game.player_rounds[1].bid = Some(3);
        game.player_rounds[3].bid = Some(3);
        game.player_rounds[1].tricks_won = 3;
        game.player_rounds[3].tricks_won = 3;
        game.tricks_played = 13;
        game.score_round();
        // NS: -100 (nil fail) + 5*10 = -50
        assert_eq!(game.teams[0].score, -50);
    }

    #[test]
    fn test_scoring_bag_penalty() {
        let mut game = SpadesGame::new();
        game.teams[0].bags = 8; // Already have 8 bags
        game.phase = Phase::Playing;
        game.player_rounds[0].bid = Some(3);
        game.player_rounds[2].bid = Some(2);
        game.player_rounds[0].tricks_won = 4; // +1 overtrick
        game.player_rounds[2].tricks_won = 4; // +2 overtricks = 3 total
        // 8 + 3 = 11 bags -> 1 penalty (-100), 1 bag remaining
        game.player_rounds[1].bid = Some(2);
        game.player_rounds[3].bid = Some(1);
        game.player_rounds[1].tricks_won = 2;
        game.player_rounds[3].tricks_won = 1;
        game.tricks_played = 13;
        game.score_round();
        // NS: 5*10 + 3 - 100 = -47
        assert_eq!(game.teams[0].score, -47);
        assert_eq!(game.teams[0].bags, 1);
    }

    // ── Game over tests ─────────────────────────────────────────────

    #[test]
    fn test_game_over_ns_wins_at_500() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 500;
        game.teams[1].score = 300;
        game.check_game_over();
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.winner_message.contains("Your team wins"));
    }

    #[test]
    fn test_game_over_ew_wins_at_500() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 300;
        game.teams[1].score = 500;
        game.check_game_over();
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.winner_message.contains("East-West wins"));
    }

    #[test]
    fn test_game_over_both_500_higher_wins() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 520;
        game.teams[1].score = 500;
        game.check_game_over();
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.winner_message.contains("Your team wins"));
    }

    #[test]
    fn test_game_over_negative_200() {
        let mut game = SpadesGame::new();
        game.teams[0].score = -200;
        game.teams[1].score = 100;
        game.check_game_over();
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.winner_message.contains("East-West wins"));
    }

    #[test]
    fn test_game_over_ew_negative_200() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 100;
        game.teams[1].score = -200;
        game.check_game_over();
        assert_eq!(game.phase, Phase::GameOver);
        assert!(game.winner_message.contains("Your team wins"));
    }

    #[test]
    fn test_no_game_over_under_thresholds() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 300;
        game.teams[1].score = 400;
        game.check_game_over();
        // Should remain in current phase (Bidding from new)
        assert_ne!(game.phase, Phase::GameOver);
    }

    // ── Sort order tests ────────────────────────────────────────────

    #[test]
    fn test_sort_order_toggle() {
        assert_eq!(SortOrder::BySuit.toggle(), SortOrder::ByRank);
        assert_eq!(SortOrder::ByRank.toggle(), SortOrder::BySuit);
    }

    #[test]
    fn test_sort_by_suit() {
        let mut game = SpadesGame::new();
        game.sort_order = SortOrder::BySuit;
        game.sort_all_hands();
        for hand in &game.hands {
            for w in hand.windows(2) {
                assert!(w[0].sort_key_suit() <= w[1].sort_key_suit());
            }
        }
    }

    #[test]
    fn test_sort_by_rank() {
        let mut game = SpadesGame::new();
        game.sort_order = SortOrder::ByRank;
        game.sort_all_hands();
        for hand in &game.hands {
            for w in hand.windows(2) {
                assert!(w[0].sort_key_rank() <= w[1].sort_key_rank());
            }
        }
    }

    // ── Team calculation tests ──────────────────────────────────────

    #[test]
    fn test_team_bid_sum() {
        let mut game = SpadesGame::new();
        game.player_rounds[0].bid = Some(3);
        game.player_rounds[2].bid = Some(4);
        assert_eq!(game.team_bid(0), 7);
    }

    #[test]
    fn test_team_tricks_sum() {
        let mut game = SpadesGame::new();
        game.player_rounds[0].tricks_won = 2;
        game.player_rounds[2].tricks_won = 3;
        assert_eq!(game.team_tricks(0), 5);
    }

    #[test]
    fn test_team_bid_ew() {
        let mut game = SpadesGame::new();
        game.player_rounds[1].bid = Some(5);
        game.player_rounds[3].bid = Some(2);
        assert_eq!(game.team_bid(1), 7);
    }

    // ── New game tests ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_scores() {
        let mut game = SpadesGame::new();
        game.teams[0].score = 300;
        game.teams[1].score = 200;
        game.new_game();
        assert_eq!(game.teams[0].score, 0);
        assert_eq!(game.teams[1].score, 0);
    }

    #[test]
    fn test_new_game_resets_phase() {
        let mut game = SpadesGame::new();
        game.phase = Phase::GameOver;
        game.new_game();
        assert_eq!(game.phase, Phase::Bidding);
    }

    #[test]
    fn test_new_game_deals_fresh_hands() {
        let mut game = SpadesGame::new();
        game.new_game();
        for i in 0..4 {
            assert_eq!(game.hands[i].len(), 13);
        }
    }

    // ── Event handling tests ────────────────────────────────────────

    #[test]
    fn test_bid_up_increases() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 3;
        game.handle_key(&press(Key::Up));
        assert_eq!(game.bid_selection, 4);
    }

    #[test]
    fn test_bid_down_decreases() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 3;
        game.handle_key(&press(Key::Down));
        assert_eq!(game.bid_selection, 2);
    }

    #[test]
    fn test_bid_down_clamp_at_zero() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 0;
        game.handle_key(&press(Key::Down));
        assert_eq!(game.bid_selection, 0);
    }

    #[test]
    fn test_bid_up_clamp_at_13() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = MAX_BID;
        game.handle_key(&press(Key::Up));
        assert_eq!(game.bid_selection, MAX_BID);
    }

    #[test]
    fn test_key_not_pressed_ignored() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 5;
        let mut release = press(Key::Up);
        release.pressed = false;
        game.handle_key(&release);
        assert_eq!(game.bid_selection, 5);
    }

    #[test]
    fn test_card_navigation_right() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.selected_card = 0;
        game.handle_key(&press(Key::Right));
        assert_eq!(game.selected_card, 1);
    }

    #[test]
    fn test_card_navigation_left_at_zero() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.selected_card = 0;
        game.handle_key(&press(Key::Left));
        assert_eq!(game.selected_card, 0);
    }

    // ── Clamp selected card tests ───────────────────────────────────

    #[test]
    fn test_clamp_selected_card_empty() {
        let mut game = SpadesGame::new();
        game.hands[0] = Vec::new();
        game.selected_card = 5;
        game.clamp_selected_card();
        assert_eq!(game.selected_card, 0);
    }

    #[test]
    fn test_clamp_selected_card_within_range() {
        let mut game = SpadesGame::new();
        game.selected_card = 5;
        game.clamp_selected_card();
        assert_eq!(game.selected_card, 5);
    }

    #[test]
    fn test_clamp_selected_card_over_range() {
        let mut game = SpadesGame::new();
        game.hands[0] = vec![Card::new(Suit::Hearts, Rank::ACE)];
        game.selected_card = 5;
        game.clamp_selected_card();
        assert_eq!(game.selected_card, 0);
    }

    // ── Suit color tests ────────────────────────────────────────────

    #[test]
    fn test_suit_symbols() {
        assert_eq!(Suit::Clubs.symbol(), "\u{2663}");
        assert_eq!(Suit::Diamonds.symbol(), "\u{2666}");
        assert_eq!(Suit::Hearts.symbol(), "\u{2665}");
        assert_eq!(Suit::Spades.symbol(), "\u{2660}");
    }

    #[test]
    fn test_suit_names() {
        assert_eq!(Suit::Clubs.name(), "Clubs");
        assert_eq!(Suit::Diamonds.name(), "Diamonds");
        assert_eq!(Suit::Hearts.name(), "Hearts");
        assert_eq!(Suit::Spades.name(), "Spades");
    }

    // ── Player round tests ──────────────────────────────────────────

    #[test]
    fn test_player_round_new() {
        let pr = PlayerRound::new();
        assert_eq!(pr.bid, None);
        assert_eq!(pr.tricks_won, 0);
        assert!(!pr.is_nil());
    }

    #[test]
    fn test_player_names() {
        assert_eq!(PlayerId::SOUTH.name(), "You");
        assert_eq!(PlayerId::EAST.name(), "East");
        assert_eq!(PlayerId::NORTH.name(), "North");
        assert_eq!(PlayerId::WEST.name(), "West");
    }

    // `test_player_position_labels` went with `position_label`.  The three
    // seats it named the same way as `name()` are still covered by
    // `test_player_names` just above; the fourth, seat 0, is "You" there on
    // purpose.

    // ── Card face rendering tests ───────────────────────────────────

    #[test]
    fn test_render_card_face_produces_commands() {
        let mut f: Frame<Target> = Frame::new(200.0, 200.0);
        draw_card_face(
            &mut f,
            Rect::new(10.0, 10.0, 60.0, 84.0),
            Card::new(Suit::Spades, Rank::ACE),
            false,
        );
        // A face, a rank in the corner, the suit beside it, and the suit again
        // across the middle.
        assert!(f.commands().len() >= 4);
    }

    #[test]
    fn a_card_with_no_room_to_be_drawn_is_not_drawn() {
        let mut f: Frame<Target> = Frame::new(200.0, 200.0);
        draw_card_face(
            &mut f,
            Rect::EMPTY,
            Card::new(Suit::Spades, Rank::ACE),
            false,
        );
        assert!(f.commands().is_empty());
    }

    // ── AI card choice tests ────────────────────────────────────────

    #[test]
    fn test_ai_choose_single_legal() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::EAST;
        game.hands[1] = vec![
            Card::new(Suit::Hearts, Rank::ACE),
            Card::new(Suit::Clubs, Rank::TWO),
        ];
        game.current_trick = Trick::new();
        game.current_trick
            .add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::FIVE));
        let legal = vec![0]; // Only hearts
        let choice = game.ai_choose_card(PlayerId::EAST, &legal);
        assert_eq!(choice, 0);
    }

    #[test]
    fn test_advance_round_increments() {
        let mut game = SpadesGame::new();
        game.bid_selection = 3;
        game.submit_human_bid();
        game.round_number = 1;
        game.advance_round();
        assert_eq!(game.round_number, 2);
    }

    #[test]
    fn test_dealer_rotates() {
        let mut game = SpadesGame::new();
        let old_dealer = game.dealer;
        game.advance_round();
        assert_eq!(game.dealer, old_dealer.next());
    }

    // ── Card::beats edge cases ──────────────────────────────────────

    #[test]
    fn test_beats_same_card() {
        let card = Card::new(Suit::Hearts, Rank::ACE);
        assert!(!card.beats(card, Suit::Hearts));
    }

    #[test]
    fn test_beats_trump_vs_trump() {
        let ace_spades = Card::new(Suit::Spades, Rank::ACE);
        let king_spades = Card::new(Suit::Spades, Rank::KING);
        assert!(ace_spades.beats(king_spades, Suit::Hearts));
        assert!(!king_spades.beats(ace_spades, Suit::Hearts));
    }

    #[test]
    fn test_beats_neither_trump_neither_led() {
        let ace_clubs = Card::new(Suit::Clubs, Rank::ACE);
        let ace_diamonds = Card::new(Suit::Diamonds, Rank::ACE);
        // Hearts led, neither is trump
        // Clubs is not led suit either, diamonds is not led suit
        // The card matching led_suit wins; here neither matches, so
        // only self.suit == led matters: clubs != hearts so clubs doesn't beat
        assert!(!ace_clubs.beats(ace_diamonds, Suit::Hearts));
    }
}
