//! Slate OS Hearts -- four-player Hearts in a real window.
//!
//! One human at the south seat and three machine players. A round deals
//! thirteen cards each, passes three in a rotating direction, and then plays
//! thirteen tricks; hearts are worth a point each and the queen of spades
//! thirteen, a player who takes all twenty-six gives twenty-six to everyone
//! else instead, and the game ends when somebody reaches a hundred.
//!
//! Everything on the screen is derived from the live window size every frame,
//! and every card and button the pointer can reach is a hit box recorded by the
//! pass that paints it. What it replaced did none of that: `main` built the
//! game and dropped it, the picture was drawn from twenty-odd compile-time
//! coordinates, and the click handler re-derived the hand's geometry from its
//! own copies of those numbers.

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use randrange::{RandomSource, SeededRng, seed_from_system};
use std::process::ExitCode;

// ── Palette ─────────────────────────────────────────────────────────

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
const FELT: Color = Color::from_hex(0x1B3D2F);
const CARD_FACE: Color = Color::from_hex(0xEFF1F5);
const CARD_BACK: Color = Color::from_hex(0x3B4261);
const CARD_INK: Color = Color::from_hex(0x1E1E2E);
const CARD_INK_RED: Color = Color::from_hex(0xD20F39);

// ── Constants ───────────────────────────────────────────────────────

const TITLE: &str = "Hearts";
const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 620.0;

/// Players at the table, and so the length of every per-player array.
const SEATS: usize = 4;
/// Cards dealt to each player, and so the number of tricks in a round.
const HAND_SIZE: usize = 13;
/// Cards passed at the start of every round but the fourth.
const PASS_SIZE: usize = 3;
/// The score that ends the game.
const GAME_OVER_SCORE: u32 = 100;
/// Every point in a round: thirteen hearts and the queen of spades.
const MOON: u32 = 26;

/// How often the app asks to be woken.
const TICK_MS: u64 = 40;
/// How long a machine player appears to think before it plays.
const THINK_MS: u32 = 320;
/// How long a completed trick stays on the table before it is swept.
///
/// The old program had no such pause: `finish_trick` moved the fourth card
/// off the table in the same event that put it there, so the player never saw
/// the trick they had just played into. What was drawn in its place was four
/// blank grey rectangles -- `last_trick` painted as `FillRect` and nothing
/// else, with no rank and no suit on them.
const SWEEP_MS: u32 = 900;

/// The largest a card is allowed to get, however big the window is.
const MAX_CARD_W: f32 = 78.0;
/// A card is this many times as tall as it is wide.
const CARD_ASPECT: f32 = 1.4;
/// The narrowest scoreboard worth drawing; below this it is left out.
const MIN_SCORES_WIDTH: f32 = 110.0;

// ── Cards ───────────────────────────────────────────────────────────

/// Suit of a playing card, ordered as a sorted hand is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Suit {
    Clubs,
    Diamonds,
    Spades,
    Hearts,
}

impl Suit {
    const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Spades, Suit::Hearts];

    const fn symbol(self) -> &'static str {
        match self {
            Suit::Clubs => "\u{2663}",
            Suit::Diamonds => "\u{2666}",
            Suit::Spades => "\u{2660}",
            Suit::Hearts => "\u{2665}",
        }
    }

    /// Where this suit sorts, which is also its rank in the deck order.
    const fn index(self) -> u8 {
        match self {
            Suit::Clubs => 0,
            Suit::Diamonds => 1,
            Suit::Spades => 2,
            Suit::Hearts => 3,
        }
    }

    /// The colour the pips are printed in on a card face.
    const fn ink(self) -> Color {
        match self {
            Suit::Clubs | Suit::Spades => CARD_INK,
            Suit::Diamonds | Suit::Hearts => CARD_INK_RED,
        }
    }
}

/// Rank of a playing card, low to high.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

impl Rank {
    const ALL: [Rank; 13] = [
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
        Rank::Ace,
    ];

    const fn label(self) -> &'static str {
        match self {
            Rank::Two => "2",
            Rank::Three => "3",
            Rank::Four => "4",
            Rank::Five => "5",
            Rank::Six => "6",
            Rank::Seven => "7",
            Rank::Eight => "8",
            Rank::Nine => "9",
            Rank::Ten => "10",
            Rank::Jack => "J",
            Rank::Queen => "Q",
            Rank::King => "K",
            Rank::Ace => "A",
        }
    }

    /// How high this rank plays, which is the only comparison a trick makes.
    const fn value(self) -> u8 {
        match self {
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten => 10,
            Rank::Jack => 11,
            Rank::Queen => 12,
            Rank::King => 13,
            Rank::Ace => 14,
        }
    }
}

/// A playing card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Card {
    suit: Suit,
    rank: Rank,
}

impl Card {
    const QUEEN_OF_SPADES: Card = Card::new(Suit::Spades, Rank::Queen);
    const TWO_OF_CLUBS: Card = Card::new(Suit::Clubs, Rank::Two);

    const fn new(suit: Suit, rank: Rank) -> Self {
        Self { suit, rank }
    }

    const fn is_heart(self) -> bool {
        matches!(self.suit, Suit::Hearts)
    }

    /// What taking this card in a trick costs.
    const fn point_value(self) -> u32 {
        if matches!(self.suit, Suit::Spades) && matches!(self.rank, Rank::Queen) {
            13
        } else if self.is_heart() {
            1
        } else {
            0
        }
    }

    /// Suit first, then rank: the order a hand is held in.
    const fn sort_key(self) -> u8 {
        self.suit
            .index()
            .saturating_mul(13)
            .saturating_add(self.rank.value())
    }

    /// `"Q\u{2660}"` -- the shortest name that identifies the card.
    fn name(self) -> String {
        format!("{}{}", self.rank.label(), self.suit.symbol())
    }
}

impl PartialOrd for Card {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Card {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.sort_key().cmp(&other.sort_key())
    }
}

/// A standard 52-card deck, in suit-then-rank order.
fn new_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(SEATS * HAND_SIZE);
    for &suit in &Suit::ALL {
        for &rank in &Rank::ALL {
            deck.push(Card::new(suit, rank));
        }
    }
    deck
}

// ── Passing ─────────────────────────────────────────────────────────

/// Which way the three cards go at the start of a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassDirection {
    Left,
    Right,
    Across,
    Keep,
}

impl PassDirection {
    const fn label(self) -> &'static str {
        match self {
            PassDirection::Left => "Pass left",
            PassDirection::Right => "Pass right",
            PassDirection::Across => "Pass across",
            PassDirection::Keep => "No passing",
        }
    }

    /// The direction the round after this one uses.
    const fn next(self) -> PassDirection {
        match self {
            PassDirection::Left => PassDirection::Right,
            PassDirection::Right => PassDirection::Across,
            PassDirection::Across => PassDirection::Keep,
            PassDirection::Keep => PassDirection::Left,
        }
    }

    /// Who receives the cards `from` gives away.
    const fn target(self, from: usize) -> usize {
        match self {
            PassDirection::Left => from.saturating_add(1) % SEATS,
            PassDirection::Right => from.saturating_add(3) % SEATS,
            PassDirection::Across => from.saturating_add(2) % SEATS,
            PassDirection::Keep => from,
        }
    }
}

// ── Phases ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    /// Choosing three cards to give away.
    Passing,
    /// Playing tricks.
    Playing,
    /// The round is scored and the next one is a keypress away.
    RoundOver,
    /// Somebody reached a hundred.
    GameOver,
}

// ── Tricks ──────────────────────────────────────────────────────────

/// One card in a trick, and who put it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrickCard {
    player: usize,
    card: Card,
}

/// A trick, from the lead to the fourth card.
#[derive(Debug, Clone, Default)]
struct Trick {
    cards: Vec<TrickCard>,
}

impl Trick {
    fn new() -> Self {
        Self {
            cards: Vec::with_capacity(SEATS),
        }
    }

    /// The suit that was led.
    ///
    /// Derived from the first card rather than stored beside it. It used to be
    /// a field written by `play`, which is the same fact in two places: a trick
    /// could be constructed with cards and no lead suit, and `winner` answered
    /// `None` for it.
    fn lead_suit(&self) -> Option<Suit> {
        self.cards.first().map(|tc| tc.card.suit)
    }

    fn play(&mut self, player: usize, card: Card) {
        self.cards.push(TrickCard { player, card });
    }

    fn is_complete(&self) -> bool {
        self.cards.len() == SEATS
    }

    /// Who takes the trick: the highest card of the suit that was led.
    fn winner(&self) -> Option<usize> {
        let lead = self.lead_suit()?;
        self.cards
            .iter()
            .filter(|tc| tc.card.suit == lead)
            .max_by_key(|tc| tc.card.rank.value())
            .map(|tc| tc.player)
    }

    /// What taking this trick costs.
    fn points(&self) -> u32 {
        self.cards.iter().map(|tc| tc.card.point_value()).sum()
    }
}

// ── What a click can land on ────────────────────────────────────────

/// A verb with a key and a button that do the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// Whatever Enter means in this phase.
    Confirm,
    /// Put back the cards chosen for the pass.
    Clear,
    /// Abandon this game and deal a new one.
    New,
    /// Show or hide the help card.
    Help,
}

const BUTTONS: [Button; 4] = [Button::Confirm, Button::Clear, Button::New, Button::Help];

impl Button {
    /// The key that does the same thing.
    const fn key(self) -> Key {
        match self {
            Button::Confirm => Key::Enter,
            Button::Clear => Key::Escape,
            Button::New => Key::N,
            Button::Help => Key::H,
        }
    }

    /// The name a key is known by on the help card.
    const fn key_name(self) -> &'static str {
        match self {
            Button::Confirm => "Enter",
            Button::Clear => "Esc",
            Button::New => "N",
            Button::Help => "H",
        }
    }
}

/// What the `Confirm` button will do if it is pressed now.
///
/// One function, used by the button that carries the label, by the help card
/// that explains it and by the test that holds the two together. Enter means
/// four different things across the phases, and a label written out beside each
/// of them would be that rule written five times.
fn confirm_label(phase: GamePhase, chosen: usize) -> &'static str {
    match phase {
        GamePhase::Passing => {
            if chosen == PASS_SIZE {
                "Pass"
            } else {
                "Choose"
            }
        }
        GamePhase::Playing => "Play",
        GamePhase::RoundOver => "Next round",
        GamePhase::GameOver => "New game",
    }
}

/// Everything the pointer can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// The `i`th card of the human's hand, as the hand is currently sorted.
    Card(usize),
    Button(Button),
    /// The help card itself, which swallows the click that dismisses it.
    Help,
}

// ── Layout ──────────────────────────────────────────────────────────

/// Every rectangle in the picture, solved from the live window size.
///
/// What this replaced was twenty compile-time coordinates -- `HAND_Y = 480`,
/// `TRICK_CENTER_X = 400`, `SCORE_X = 700` -- and five `render_*` methods that
/// took no window size at all. `render_scores` took a `_width` and ignored it,
/// so in any window narrower than 860 the scoreboard was painted off the right
/// edge, and in a wider one it sat in the middle of the felt.
#[derive(Clone, Copy, Debug)]
struct Layout {
    window: Rect,
    /// The bar carrying the title, the round and the pass direction.
    header: Rect,
    /// The felt: the trick in the middle and the three machine seats round it.
    table: Rect,
    /// The strip the human's cards are fanned across.
    hand: Rect,
    /// The row of buttons.
    footer: Rect,
    /// The one line of prose under the buttons.
    status: Rect,
    /// The score panel. Empty when the window cannot pay for one.
    scores: Rect,
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
        let font = (h / 40.0).clamp(9.0, 17.0);
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

        let footer_h = (small * 2.4).min(free_h);
        let hand_h = card_h.min((free_h - footer_h).max(0.0));
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

        // The scoreboard sits in the top-right of the felt, and is left out
        // rather than squeezed when what is left would not hold a name.
        let scores_w = (w * 0.22).clamp(0.0, 190.0);
        let scores_h = small.mul_add(6.0, pad * 3.0);
        let scores = if scores_w >= MIN_SCORES_WIDTH && table.h >= scores_h {
            Rect::new(table.right() - scores_w, table.y + pad, scores_w, scores_h)
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
            scores,
            card: (card_w, card_h),
            pad,
            title,
            font,
            small,
        }
    }

    /// How far apart the cards of an `n`-card hand are drawn.
    ///
    /// Cards overlap only as much as they must: a hand that fits at full width
    /// is drawn at full width, and one that does not is closed up until it
    /// fits. The old program used a fixed 38-pixel step whatever the window
    /// was, so thirteen cards were 516 pixels wide in every window there ever
    /// was -- off the right edge of a narrow one, and adrift in a wide one.
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
    /// This is the only place the hand's geometry exists. The click handler
    /// used to compute its own from `HAND_X_START`, `CARD_OVERLAP` and
    /// `CARD_WIDTH`, which is the picture and the hit test as two facts that
    /// can disagree -- and they did, because the drawing pass lifted a chosen
    /// card sixteen pixels and the hit test did not know.
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
    /// South is the human, and the seats run clockwise from there, which is
    /// also the order play passes in.
    fn trick_card(&self, seat: usize) -> Rect {
        let (cw, ch) = self.card;
        if cw <= 0.0 || ch <= 0.0 {
            return Rect::EMPTY;
        }
        let (cx, cy) = self.table.centre();
        let (dx, dy) = match seat % SEATS {
            0 => (0.0, ch * 0.55),
            1 => (-cw * 1.05, 0.0),
            2 => (0.0, -ch * 0.55),
            _ => (cw * 1.05, 0.0),
        };
        Rect::new(cx + dx - cw / 2.0, cy + dy - ch / 2.0, cw, ch)
    }

    /// Where a machine player's name, card count and round score are written.
    ///
    /// Empty for seat 0: the human's own hand is the strip along the bottom,
    /// and there is no label for it.
    fn seat_label(&self, seat: usize) -> Rect {
        let (cw, ch) = self.card;
        let w = (cw * 1.9).min(self.table.w / 3.0);
        let h = self.small * 2.4;
        if seat == 0 || w <= 0.0 || h > self.table.h {
            return Rect::EMPTY;
        }
        let (cx, cy) = self.table.centre();
        let label = match seat {
            1 => Rect::new((cx - cw * 1.6 - w).max(self.table.x), cy - h / 2.0, w, h),
            2 => Rect::new(cx - w / 2.0, (cy - ch * 1.15 - h).max(self.table.y), w, h),
            _ => Rect::new(
                (cx + cw * 1.6).min(self.table.right() - w),
                cy - h / 2.0,
                w,
                h,
            ),
        };
        // Clamping is what keeps a label on the felt, but a felt small enough
        // to need clamping is one where the clamp can push the label onto the
        // very card it describes. Left out rather than drawn over it.
        if label.intersect(self.trick_card(seat)).is_some() {
            return Rect::EMPTY;
        }
        label
    }
}

// ── The game ────────────────────────────────────────────────────────

/// Seat names, south first. Seat 0 is the human.
const NAMES: [&str; SEATS] = ["You", "West", "North", "East"];

struct Hearts {
    /// One hand per seat, each held in suit-then-rank order.
    hands: [Vec<Card>; SEATS],
    /// Indices into `hands[0]` of the cards the human is giving away.
    chosen: Vec<usize>,
    /// What is on the table: up to four cards, in the order they were played.
    trick: Trick,
    /// Points taken this round.
    round_points: [u32; SEATS],
    /// Points taken since the game began.
    scores: [u32; SEATS],
    /// Whose turn it is.
    turn: usize,
    /// Whether a heart has been played, which is what lets one be led.
    hearts_broken: bool,
    /// Which trick of the thirteen is being played, from zero.
    trick_number: usize,
    phase: GamePhase,
    pass_direction: PassDirection,
    /// Which round of the game, from zero.
    round_number: usize,
    /// The card of the human's hand the keyboard is pointing at.
    selected: usize,
    /// The line of prose along the bottom.
    status: String,
    rng: SeededRng,
    /// Who won, once somebody has reached a hundred.
    winner: Option<usize>,
    show_help: bool,
    /// The window as of the last frame, so a click can be resolved against the
    /// picture that was actually drawn.
    size: (f32, f32),
    /// Milliseconds until the machine player whose turn it is plays a card.
    think_ms: u32,
    /// Milliseconds until a finished trick is swept off the table.
    sweep_ms: u32,
    /// Who took the trick lying on the table, once it is complete.
    taker: Option<usize>,
}

impl Hearts {
    /// A game dealt from the system's randomness, so two launches are not the
    /// same game.
    fn new() -> Self {
        Self::with_seed(seed_from_system(0x4845_4152_5453))
    }

    /// The same, from a seed the caller chooses -- which is what the tests use:
    /// a deal that is a fact rather than a draw.
    fn with_seed(seed: u64) -> Self {
        let mut game = Self {
            hands: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            chosen: Vec::new(),
            trick: Trick::new(),
            round_points: [0; SEATS],
            scores: [0; SEATS],
            turn: 0,
            hearts_broken: false,
            trick_number: 0,
            phase: GamePhase::Passing,
            pass_direction: PassDirection::Left,
            round_number: 0,
            selected: 0,
            status: String::new(),
            // The seed is the caller's, and `new` takes it from the system
            // rather than from the literal 42 this used to carry. A fixed seed
            // shuffles a fixed deck into a fixed order, so every launch of the
            // program dealt the identical thirteen cards and the game was the
            // same game every time it was opened.
            rng: SeededRng::new(seed),
            winner: None,
            show_help: false,
            size: (WINDOW_WIDTH, WINDOW_HEIGHT),
            think_ms: 0,
            sweep_ms: 0,
            taker: None,
        };
        game.start_round();
        game
    }

    // ── Dealing and rounds ──────────────────────────────────────────

    /// Shuffle, deal thirteen each, and open either the pass or the play.
    fn start_round(&mut self) {
        let mut deck = new_deck();
        self.rng.shuffle(&mut deck);
        for (seat, hand) in self.hands.iter_mut().enumerate() {
            hand.clear();
            hand.extend(
                deck.iter()
                    .skip(seat.saturating_mul(HAND_SIZE))
                    .take(HAND_SIZE),
            );
            hand.sort_unstable();
        }

        self.chosen.clear();
        self.trick = Trick::new();
        self.round_points = [0; SEATS];
        self.hearts_broken = false;
        self.trick_number = 0;
        self.taker = None;
        self.sweep_ms = 0;
        self.think_ms = 0;

        if self.pass_direction == PassDirection::Keep {
            self.begin_play();
        } else {
            self.phase = GamePhase::Passing;
            self.status = format!(
                "Choose {PASS_SIZE} cards to give away ({})",
                self.pass_direction.label()
            );
        }
        self.clamp_selection();
    }

    /// Abandon the game in progress and deal a fresh one.
    fn new_game(&mut self) {
        self.scores = [0; SEATS];
        self.round_number = 0;
        self.pass_direction = PassDirection::Left;
        self.winner = None;
        self.start_round();
    }

    /// Hand the lead to whoever holds the two of clubs and start the tricks.
    fn begin_play(&mut self) {
        // A full deal always contains the two of clubs, which is what
        // `every_deal_has_a_two_of_clubs_to_lead_with` holds; the seat-0
        // fallback exists only so this is a total function.
        self.turn = self.holder_of(Card::TWO_OF_CLUBS).unwrap_or(0);
        self.phase = GamePhase::Playing;
        self.status = format!(
            "{} leads with {}",
            name(self.turn),
            Card::TWO_OF_CLUBS.name()
        );
        self.arm_think();
    }

    /// Which seat holds a given card, if any does.
    fn holder_of(&self, card: Card) -> Option<usize> {
        self.hands.iter().position(|hand| hand.contains(&card))
    }

    /// Score the round, and end the game if anybody has reached a hundred.
    fn end_round(&mut self) {
        if let Some(shooter) = self.moon_shooter() {
            for (seat, score) in self.scores.iter_mut().enumerate() {
                if seat != shooter {
                    *score = score.saturating_add(MOON);
                }
            }
            self.status = format!(
                "{} shot the moon -- {MOON} points to everyone else",
                name(shooter)
            );
        } else {
            for (score, taken) in self.scores.iter_mut().zip(self.round_points) {
                *score = score.saturating_add(taken);
            }
            self.status = String::from("Round over");
        }

        self.round_number = self.round_number.saturating_add(1);
        self.pass_direction = self.pass_direction.next();

        if self.scores.iter().copied().max().unwrap_or(0) >= GAME_OVER_SCORE {
            self.phase = GamePhase::GameOver;
            self.winner = self.lowest_scorer();
            if let Some(w) = self.winner {
                self.status = format!("{} wins the game", name(w));
            }
        } else {
            self.phase = GamePhase::RoundOver;
        }
    }

    /// Whoever took every point in the round, if anybody did.
    fn moon_shooter(&self) -> Option<usize> {
        self.round_points.iter().position(|&p| p == MOON)
    }

    /// The seat with the fewest points, which is the seat that wins.
    ///
    /// A tie goes to the earlier seat, which is the only rule this program has
    /// ever had for one; `a_tied_game_is_won_by_the_earlier_seat` holds it so
    /// that it is a decision rather than an accident of `position`.
    fn lowest_scorer(&self) -> Option<usize> {
        let low = self.scores.iter().copied().min()?;
        self.scores.iter().position(|&s| s == low)
    }

    // ── The rules of a trick ────────────────────────────────────────

    /// Which cards of `player`'s hand may legally be played right now.
    fn valid_plays(&self, player: usize) -> Vec<usize> {
        let Some(hand) = self.hands.get(player) else {
            return Vec::new();
        };
        if hand.is_empty() {
            return Vec::new();
        }
        let leading = self.trick.cards.is_empty();
        let first_trick = self.trick_number == 0;

        // The two of clubs leads the first trick, whoever holds it.
        if leading
            && first_trick
            && let Some(i) = hand.iter().position(|&c| c == Card::TWO_OF_CLUBS)
        {
            return vec![i];
        }

        let all = |f: &dyn Fn(&Card) -> bool| -> Vec<usize> {
            hand.iter()
                .enumerate()
                .filter(|(_, c)| f(c))
                .map(|(i, _)| i)
                .collect()
        };
        let everything = || -> Vec<usize> { (0..hand.len()).collect() };

        let candidates = match self.trick.lead_suit() {
            // Follow the suit that was led, if the suit is held at all.
            Some(lead) if hand.iter().any(|c| c.suit == lead) => all(&|c| c.suit == lead),
            // Leading: hearts are not led until a heart has been played,
            // unless hearts are all that is left to lead.
            None if !self.hearts_broken => {
                let others = all(&|c| !c.is_heart());
                if others.is_empty() {
                    everything()
                } else {
                    others
                }
            }
            _ => everything(),
        };

        // Nothing that scores goes on the first trick -- unless scoring cards
        // are the whole of what may be played, in which case the rule yields.
        // This used to be written only in the cannot-follow branch, so it was
        // a rule about one path rather than about the first trick.
        if first_trick {
            let clean: Vec<usize> = candidates
                .iter()
                .copied()
                .filter(|&i| hand.get(i).is_some_and(|c| c.point_value() == 0))
                .collect();
            if !clean.is_empty() {
                return clean;
            }
        }
        candidates
    }

    /// Put a card on the table for a seat, and settle the trick if it fills.
    fn play_card(&mut self, player: usize, index: usize) -> bool {
        let Some(hand) = self.hands.get_mut(player) else {
            return false;
        };
        if index >= hand.len() {
            return false;
        }
        let card = hand.remove(index);

        if card.is_heart() {
            self.hearts_broken = true;
        }
        self.trick.play(player, card);

        if self.trick.is_complete() {
            self.settle_trick();
        } else {
            // From the seat that played, not from `self.turn`: the two are the
            // same in every path that exists today, and taking it from the
            // argument is what keeps them so.
            self.turn = player.saturating_add(1) % SEATS;
            self.arm_think();
        }
        true
    }

    /// Award a full trick and leave it on the table for a moment.
    fn settle_trick(&mut self) {
        let Some(taker) = self.trick.winner() else {
            return;
        };
        let points = self.trick.points();
        if let Some(p) = self.round_points.get_mut(taker) {
            *p = p.saturating_add(points);
        }
        self.taker = Some(taker);
        self.sweep_ms = SWEEP_MS;
        self.think_ms = 0;
        self.status = format!(
            "{} takes the trick ({points} pt{})",
            name(taker),
            if points == 1 { "" } else { "s" }
        );
    }

    /// Clear the settled trick and hand the lead to whoever took it.
    fn sweep_trick(&mut self) {
        let Some(taker) = self.taker.take() else {
            return;
        };
        self.trick = Trick::new();
        self.sweep_ms = 0;
        self.turn = taker;
        self.trick_number = self.trick_number.saturating_add(1);
        if self.trick_number >= HAND_SIZE {
            self.end_round();
        } else {
            self.arm_think();
        }
        self.clamp_selection();
    }

    /// Whether the table is ready for another card.
    ///
    /// False while a settled trick is still being shown, which is the one
    /// window in which neither the human nor a machine player may play.
    fn ready(&self) -> bool {
        self.sweep_ms == 0
    }

    /// Set the machine player's pause, or clear it when the seat now to play
    /// is the human's.
    ///
    /// Called from the four places that hand the turn to a seat with a round
    /// in play -- the lead, a card played into an unfinished trick, the sweep
    /// of a finished one, and the clock's own retry -- so the phase and the
    /// settled-trick window are already decided by the caller. The one thing
    /// left to decide is whether the seat now to play answers on a clock.
    fn arm_think(&mut self) {
        self.think_ms = if self.turn == 0 { 0 } else { THINK_MS };
    }

    // ── Passing ─────────────────────────────────────────────────────

    /// Add or remove a card from the three the human is giving away.
    fn toggle_chosen(&mut self, index: usize) -> bool {
        if self.phase != GamePhase::Passing || index >= self.hands[0].len() {
            return false;
        }
        if let Some(pos) = self.chosen.iter().position(|&i| i == index) {
            self.chosen.remove(pos);
        } else if self.chosen.len() < PASS_SIZE {
            self.chosen.push(index);
        } else {
            return false;
        }
        self.status = self.pass_prompt();
        true
    }

    /// What to say about a part-chosen pass.
    fn pass_prompt(&self) -> String {
        let left = PASS_SIZE.saturating_sub(self.chosen.len());
        if left == 0 {
            format!("Press {} to pass them", Button::Confirm.key_name())
        } else {
            format!(
                "Choose {left} more card{} to give away ({})",
                if left == 1 { "" } else { "s" },
                self.pass_direction.label()
            )
        }
    }

    /// Give three cards from every seat to the seat the direction names.
    fn execute_pass(&mut self) -> bool {
        if self.phase != GamePhase::Passing || self.chosen.len() != PASS_SIZE {
            return false;
        }

        let mut given: [Vec<Card>; SEATS] = [const { Vec::new() }; SEATS];
        for seat in 0..SEATS {
            let mut indices = if seat == 0 {
                self.chosen.clone()
            } else {
                self.ai_pass_choice(seat)
            };
            // Highest index first, so removing one does not move the next.
            indices.sort_unstable();
            indices.reverse();
            let (Some(hand), Some(pile)) = (self.hands.get_mut(seat), given.get_mut(seat)) else {
                continue;
            };
            for index in indices {
                if index < hand.len() {
                    let card = hand.remove(index);
                    pile.push(card);
                }
            }
        }

        for (from, cards) in given.into_iter().enumerate() {
            let to = self.pass_direction.target(from);
            if let Some(hand) = self.hands.get_mut(to) {
                hand.extend(cards);
            }
        }
        for hand in &mut self.hands {
            hand.sort_unstable();
        }

        self.chosen.clear();
        self.begin_play();
        self.clamp_selection();
        true
    }

    /// The three cards a machine player gives away: the queen of spades
    /// first, then the high spades that might be forced to take her, then
    /// hearts high to low.
    fn ai_pass_choice(&self, seat: usize) -> Vec<usize> {
        let Some(hand) = self.hands.get(seat) else {
            return Vec::new();
        };
        let mut indices: Vec<usize> = (0..hand.len()).collect();
        indices.sort_by_key(|&i| {
            let Some(&card) = hand.get(i) else {
                return 0i32;
            };
            let want = if card == Card::QUEEN_OF_SPADES {
                100
            } else if card.suit == Suit::Spades && card.rank.value() >= Rank::King.value() {
                50
            } else if card.is_heart() {
                i32::from(card.rank.value())
            } else {
                0
            };
            // Sorted ascending, so the most-wanted card must sort first.
            want.saturating_neg()
        });
        indices.truncate(PASS_SIZE);
        indices
    }

    // ── The machine players ─────────────────────────────────────────

    /// Which card a machine player puts down, given what it may play.
    fn ai_choice(&self, seat: usize) -> Option<usize> {
        let hand = self.hands.get(seat)?;
        let valid = self.valid_plays(seat);
        let first = *valid.first()?;
        if valid.len() == 1 {
            return Some(first);
        }
        let rank_of = |i: usize| hand.get(i).map_or(0, |c| c.rank.value());

        // Leading: the lowest card of a suit that cannot cost anything.
        if self.trick.cards.is_empty() {
            return valid.iter().copied().min_by_key(|&i| {
                let risky = hand
                    .get(i)
                    .is_some_and(|c| c.is_heart() || c.suit == Suit::Spades);
                u16::from(risky)
                    .saturating_mul(100)
                    .saturating_add(u16::from(rank_of(i)))
            });
        }

        let following = self
            .trick
            .lead_suit()
            .is_some_and(|lead| hand.get(first).is_some_and(|c| c.suit == lead));
        if following {
            // Duck if ducking is possible: the highest card that still loses.
            let high = self
                .trick
                .cards
                .iter()
                .filter(|tc| Some(tc.card.suit) == self.trick.lead_suit())
                .map(|tc| tc.card.rank.value())
                .max()
                .unwrap_or(0);
            let duck = valid
                .iter()
                .copied()
                .filter(|&i| rank_of(i) < high)
                .max_by_key(|&i| rank_of(i));
            return duck.or_else(|| valid.iter().copied().min_by_key(|&i| rank_of(i)));
        }

        // Void in the led suit: get rid of the queen, then the highest heart,
        // then the highest card of anything, which shortens a suit.
        valid
            .iter()
            .copied()
            .find(|&i| hand.get(i) == Some(&Card::QUEEN_OF_SPADES))
            .or_else(|| {
                valid
                    .iter()
                    .copied()
                    .filter(|&i| hand.get(i).is_some_and(|c| c.is_heart()))
                    .max_by_key(|&i| rank_of(i))
            })
            .or_else(|| valid.iter().copied().max_by_key(|&i| rank_of(i)))
    }

    // ── The clock ───────────────────────────────────────────────────

    /// Advance the two timers. Answers whether anything changed.
    fn tick(&mut self, elapsed_ms: u64) -> bool {
        let ms = u32::try_from(elapsed_ms).unwrap_or(u32::MAX);
        if self.sweep_ms > 0 {
            self.sweep_ms = self.sweep_ms.saturating_sub(ms);
            if self.sweep_ms == 0 {
                self.sweep_trick();
            }
            return true;
        }
        // `arm_think` is the only writer of the pause, and it clears it to
        // zero whenever one is not owed. Repeating its phase and seat tests
        // here would be the same fact written in two places -- the shape of
        // fault that let `Trick::lead_suit` disagree with its own cards.
        if self.think_ms > 0 {
            self.think_ms = self.think_ms.saturating_sub(ms);
            if self.think_ms == 0 {
                if let Some(index) = self.ai_choice(self.turn) {
                    self.play_card(self.turn, index);
                } else {
                    // A machine player with no legal card would otherwise hold
                    // the game for ever; there is no such hand, and
                    // `every_seat_always_has_a_legal_card` says so.
                    self.arm_think();
                }
            }
            return true;
        }
        false
    }

    // ── What the human can do ───────────────────────────────────────

    /// Move the keyboard's pointer along the hand.
    ///
    /// One function for both directions. `Key::Left` used to carry its bound
    /// in the match arm's guard and `Key::Right` its own in the arm's body, so
    /// the two directions were not the same code in any sense a reader could
    /// check.
    fn move_selection(&mut self, step: isize) -> bool {
        let len = self.hands[0].len();
        if len == 0 {
            return false;
        }
        let last = len.saturating_sub(1);
        let next = if step < 0 {
            self.selected.saturating_sub(step.unsigned_abs())
        } else {
            self.selected.saturating_add(step.unsigned_abs()).min(last)
        };
        if next == self.selected {
            return false;
        }
        self.selected = next;
        true
    }

    /// Keep the pointer on a card that exists.
    fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.hands[0].len().saturating_sub(1));
    }

    /// Whatever Enter means in this phase.
    ///
    /// The button in the footer and the key call this same function, which is
    /// why `confirm_label` can promise what pressing it will do.
    fn confirm(&mut self) -> bool {
        match self.phase {
            GamePhase::Passing => {
                if self.chosen.len() == PASS_SIZE {
                    self.execute_pass()
                } else {
                    self.toggle_chosen(self.selected)
                }
            }
            GamePhase::Playing => self.play_selected(),
            GamePhase::RoundOver => {
                self.start_round();
                true
            }
            GamePhase::GameOver => {
                self.new_game();
                true
            }
        }
    }

    /// Play the card the pointer is on, if the rules allow it.
    fn play_selected(&mut self) -> bool {
        if self.phase != GamePhase::Playing || self.turn != 0 || !self.ready() {
            return false;
        }
        if !self.valid_plays(0).contains(&self.selected) {
            self.status = String::from("That card cannot be played on this trick");
            return true;
        }
        self.play_card(0, self.selected);
        self.clamp_selection();
        true
    }

    /// Put back the cards chosen for the pass.
    fn clear_choice(&mut self) -> bool {
        if self.show_help {
            self.show_help = false;
            return true;
        }
        if self.phase != GamePhase::Passing || self.chosen.is_empty() {
            return false;
        }
        self.chosen.clear();
        self.status = self.pass_prompt();
        true
    }

    /// Do what a button says, whether it was clicked or its key was pressed.
    fn press(&mut self, button: Button) -> bool {
        match button {
            Button::Confirm => self.confirm(),
            Button::Clear => self.clear_choice(),
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
        self.selected = index;
        match self.phase {
            GamePhase::Passing => {
                self.toggle_chosen(index);
                true
            }
            GamePhase::Playing => {
                self.play_selected();
                true
            }
            _ => true,
        }
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
        // Ctrl+N is the compositor's new window and must not deal a new hand.
        if event.modifiers.ctrl || event.modifiers.alt || event.modifiers.super_key {
            return EventResult::Ignored;
        }
        let changed = match event.key {
            Key::Left => self.move_selection(-1),
            Key::Right => self.move_selection(1),
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
        // Against the frame the renderer last drew, so a click lands on the
        // card the player is looking at. The old handler recomputed the hand's
        // geometry from its own copies of three constants.
        let (w, h) = self.size;
        let changed = match self.frame(w, h).hit_test(event.x, event.y) {
            Some(Target::Card(i)) => self.touch_card(i),
            Some(Target::Button(b)) => self.press(b),
            // The help card covers the table; a click on it dismisses it
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

impl Hearts {
    /// The whole window: every rectangle solved from the size handed in, and
    /// every card and button the pointer can reach recorded as a hit box.
    ///
    /// The old renderer took a width and a height and passed neither to four
    /// of its five sub-renderers; the fifth took a width and ignored it. The
    /// picture was therefore drawn at one fixed size in a window of any other.
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
            TEXT_COLOR,
            l.title,
            FontWeightHint::Bold,
        );

        let right = format!(
            "Round {} \u{2014} {}",
            self.round_number.saturating_add(1),
            self.pass_direction.label()
        );
        let w = text::measure(&right, l.small, FontWeightHint::Regular);
        let x = l.header.right() - l.pad - w;
        // Left out rather than drawn over the title: the two are measured
        // against each other, not assumed to fit.
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
        self.draw_scores(f, l);
    }

    /// The cards in play, and a ring round the one that took them.
    ///
    /// A settled trick stays here for `SWEEP_MS`, which is the whole reason
    /// `taker` exists. The old program cleared the trick in the same event that
    /// completed it and drew four blank grey rectangles in its place -- four
    /// `FillRect`s with no rank and no suit on them -- so the player never saw
    /// the trick they had just played into.
    fn draw_trick(&self, f: &mut Frame<Target>, l: &Layout) {
        for tc in &self.trick.cards {
            let r = l.trick_card(tc.player);
            draw_card_face(f, r, tc.card, false);
            if self.taker == Some(tc.player) {
                outline(f, r, GREEN, (l.card.0 * 0.07).clamp(1.0, 4.0));
            }
        }
        if self.trick.cards.is_empty()
            && let Some(message) = self.table_message()
        {
            let (_, cy) = l.table.centre();
            centred(
                f,
                l.table.x,
                l.table.w,
                cy - l.font / 2.0,
                &message,
                LAVENDER,
                l.font,
                FontWeightHint::Bold,
            );
        }
    }

    /// What is written across the middle of an empty table.
    ///
    /// Deliberately not the status line: the two say different things, and a
    /// message that merely repeated the status would make every test that
    /// searched the frame for a string pass for the wrong reason.
    fn table_message(&self) -> Option<String> {
        match self.phase {
            GamePhase::Passing => Some(String::from(self.pass_direction.label())),
            GamePhase::Playing => None,
            GamePhase::RoundOver => Some(format!(
                "Press {} for the next round",
                Button::Confirm.key_name()
            )),
            GamePhase::GameOver => self.winner.map(|w| {
                let total = self.scores.get(w).copied().unwrap_or(0);
                format!("{} wins with {total}", name(w))
            }),
        }
    }

    /// The three machine players: who they are, what they hold, what they have
    /// taken, and whose turn it is.
    ///
    /// The old program wrote `West (13)` at `TRICK_CENTER_X - 200.0`, `North
    /// (13)` at `TRICK_CENTER_X - 30.0` and `East (13)` at `TRICK_CENTER_X +
    /// 140.0`: three literal offsets from a literal centre, so the three sat
    /// where a 900x620 window put them and nowhere else. They said how many
    /// cards a seat held but not what it had taken, and the seat to play was
    /// marked only by the colour of its own text.
    fn draw_seats(&self, f: &mut Frame<Target>, l: &Layout) {
        for seat in 1..SEATS {
            let label = l.seat_label(seat);
            if label.is_empty() {
                continue;
            }
            let live = self.phase == GamePhase::Playing && self.turn == seat && self.ready();
            fill(
                f,
                label,
                if live { SURFACE1 } else { SURFACE0 },
                l.pad * 0.4,
            );
            let inset = l.pad * 0.4;
            text_at(
                f,
                label.x + inset,
                label.y + l.small * 0.2,
                name(seat),
                if live { YELLOW } else { TEXT_COLOR },
                l.small,
                FontWeightHint::Bold,
            );
            let held = self.hands.get(seat).map_or(0, Vec::len);
            let taken = self.round_points.get(seat).copied().unwrap_or(0);
            let line = format!("{held} left \u{00b7} {taken} pt");
            f.push(RenderCommand::Text {
                x: label.x + inset,
                y: label.y + l.small * 1.3,
                text: line,
                font_size: l.small * 0.9,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((label.w - inset * 2.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            self.draw_backs(f, l, seat, label);
        }
    }

    /// A machine player's hand, face down, fanned under their label.
    ///
    /// Decoration, and so carries no hit box: there is nothing a player can do
    /// to somebody else's hand. It is left out entirely rather than drawn over
    /// the trick when the felt is too short to hold it.
    fn draw_backs(&self, f: &mut Frame<Target>, l: &Layout, seat: usize, label: Rect) {
        let held = self.hands.get(seat).map_or(0, Vec::len);
        if held == 0 || label.is_empty() {
            return;
        }
        let (cw, ch) = (l.card.0 * 0.42, l.card.1 * 0.42);
        let y = label.bottom() + l.pad * 0.3;
        if cw <= 0.0 || ch <= 0.0 || y + ch > l.table.bottom() {
            return;
        }
        // The step is cut from a full thirteen-card hand, so the fan visibly
        // shrinks as the round is played rather than staying the same width.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand is at most thirteen cards; exact in f32"
        )]
        let gaps = (held.saturating_sub(1)) as f32;
        let step = ((label.w - cw) / 12.0).clamp(0.0, cw * 0.6);
        let span = step.mul_add(gaps, cw);
        let x0 = label.x + (label.w - span) / 2.0;
        for i in 0..held {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a hand is at most thirteen cards; exact in f32"
            )]
            let fi = i as f32;
            draw_card_back(f, Rect::new(step.mul_add(fi, x0), y, cw, ch));
        }
    }

    /// The scoreboard: the game score, and what each seat has taken this round.
    ///
    /// It used to be pinned at `SCORE_X = 700.0` by a method that took the
    /// window width and ignored it, so it fell off the right-hand edge of any
    /// window narrower than 860 and floated in the middle of the felt in a
    /// wider one. It is now in the top-right of the table, or left out when
    /// the window cannot pay for one.
    fn draw_scores(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.scores.is_empty() {
            return;
        }
        fill(f, l.scores, MANTLE, l.pad * 0.5);
        let inset = l.pad * 0.6;
        let mut y = l.scores.y + inset;
        text_at(
            f,
            l.scores.x + inset,
            y,
            "Scores",
            SUBTEXT0,
            l.small,
            FontWeightHint::Bold,
        );
        y += l.small * 1.6;

        for seat in 0..SEATS {
            if y + l.small > l.scores.bottom() {
                break;
            }
            let total = self.scores.get(seat).copied().unwrap_or(0);
            let taken = self.round_points.get(seat).copied().unwrap_or(0);
            let value = format!("{total} (+{taken})");
            let w = text::measure(&value, l.small, FontWeightHint::Regular);
            text_at(
                f,
                l.scores.x + inset,
                y,
                name(seat),
                if seat == 0 { BLUE } else { TEXT_COLOR },
                l.small,
                FontWeightHint::Regular,
            );
            text_at(
                f,
                l.scores.right() - inset - w,
                y,
                &value,
                if total >= GAME_OVER_SCORE {
                    RED
                } else {
                    SUBTEXT0
                },
                l.small,
                FontWeightHint::Regular,
            );
            y += l.small * 1.25;
        }
    }

    /// The human's hand, and the only cards in the window a click can reach.
    ///
    /// The hit box is the rectangle that was just painted, lift and all. That
    /// is the repair for the fault this program was built around: the drawing
    /// pass raised a chosen card sixteen pixels (`HAND_Y - 16.0`) and the click
    /// handler, which re-derived the row from its own copies of `HAND_Y`,
    /// `HAND_X_START`, `CARD_OVERLAP` and `CARD_WIDTH`, did not know.
    fn draw_hand(&self, f: &mut Frame<Target>, l: &Layout) {
        let hand = &self.hands[0];
        let n = hand.len();
        // Only asked for when the answer means something: outside the human's
        // turn every card is drawn plainly, because none of them is playable
        // and greying the lot says nothing.
        let legal = (self.phase == GamePhase::Playing && self.turn == 0 && self.ready())
            .then(|| self.valid_plays(0));
        let lift = l.card.1 * 0.12;
        let ring = (l.card.0 * 0.07).clamp(1.0, 4.0);

        for (i, &card) in hand.iter().enumerate() {
            let mut r = l.hand_card(i, n);
            if r.is_empty() {
                continue;
            }
            let chosen = self.chosen.contains(&i);
            if chosen {
                r = Rect::new(r.x, r.y - lift, r.w, r.h);
            }
            let dim = legal.as_ref().is_some_and(|valid| !valid.contains(&i));
            draw_card_face(f, r, card, dim);
            if i == self.selected {
                outline(f, r, YELLOW, ring);
            } else if chosen {
                outline(f, r, BLUE, ring);
            }
            // Recorded after the card is painted and in the order the cards are
            // painted, so where two overlap the hit map resolves to the one
            // drawn on top -- which is the one the player is pointing at.
            f.hit(Target::Card(i), r);
        }
    }

    /// The buttons. Every verb the keyboard has, the pointer has too.
    ///
    /// The old program had no buttons at all: the only clickable thing in the
    /// window was the hand, and every other verb was a key you had to already
    /// know about.
    fn draw_footer(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.footer, MANTLE, 0.0);
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
    /// The old program drew the status at `height - 18.0` and a controls hint
    /// at `height - 14.0`, four pixels apart in the same font: the hint was
    /// painted through the status line it overlapped.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.status, MANTLE, 0.0);
        if l.status.is_empty() {
            return;
        }
        let y = l.status.y + (l.status.h - l.font) / 2.0;
        let mut budget = (l.status.w - l.pad * 2.0).max(0.0);

        if self.phase == GamePhase::Playing {
            let counter = format!(
                "Trick {}/{HAND_SIZE}",
                self.trick_number.saturating_add(1).min(HAND_SIZE)
            );
            let w = text::measure(&counter, l.small, FontWeightHint::Regular);
            // The counter is only drawn if the status still has room to say
            // something after it; the prose is what the player is reading.
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

        // Bounded and elided: the status carries seat names and card names, and
        // unbounded it ran off the edge of a narrow window.
        f.push(RenderCommand::Text {
            x: l.pad,
            y,
            text: self.status.clone(),
            font_size: l.font,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(budget),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The help card, sized from the rows it holds.
    ///
    /// The keys it lists are `Button::key_name` and the labels are
    /// `button_label` -- the same two functions the footer draws from, so a
    /// button whose label changes cannot leave the help card describing the
    /// old one.
    fn draw_help(&self, f: &mut Frame<Target>, l: &Layout) {
        fill(f, l.window, Color::rgba(0, 0, 0, 180), 0.0);

        let mut rows: Vec<(String, String)> = vec![
            (
                String::from("\u{2190} \u{2192}"),
                String::from("Move along your hand"),
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
        #[expect(
            clippy::cast_precision_loss,
            reason = "half a dozen rows; exact in f32"
        )]
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
    /// `Confirm` says four different things across the phases, which is why it
    /// asks `confirm_label`: the footer, the help card and the test that holds
    /// the two together all read the one function.
    fn button_label(&self, button: Button) -> &'static str {
        match button {
            Button::Confirm => confirm_label(self.phase, self.chosen.len()),
            Button::Clear => "Clear",
            Button::New => "New game",
            Button::Help => "Help",
        }
    }
}

// ── Drawing helpers ─────────────────────────────────────────────────

/// The name of a seat.
///
/// Total, so that no caller has to index `NAMES` and no caller has to carry a
/// bound. Seats are produced by `% SEATS` and by `0..SEATS`, so the fallback is
/// unreachable; it exists so that this is a function and not an assertion.
fn name(seat: usize) -> &'static str {
    NAMES.get(seat).copied().unwrap_or("?")
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

/// A ring round a rectangle: the selection, the pass and the trick's taker.
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

/// A run of text centred in `[x, x + w)`, by measuring it.
///
/// The old program centred by subtracting a literal -- `x + CARD_WIDTH / 2.0 -
/// 8.0` for the suit in the middle of a card -- which is half of one particular
/// string at one particular size, in a program that links `guitk::text`.
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
/// `dim` is a card the rules will not allow. The old program drew every card in
/// the hand identically, so the only way to find out that a card could not be
/// played was to play it and read the complaint.
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

/// A card, face down.
fn draw_card_back(f: &mut Frame<Target>, r: Rect) {
    if r.is_empty() {
        return;
    }
    fill(f, r, CARD_BACK, r.w * 0.16);
    let inset = r.w * 0.18;
    let inner = Rect::new(
        r.x + inset,
        r.y + inset,
        (r.w - inset * 2.0).max(0.0),
        (r.h - inset * 2.0).max(0.0),
    );
    outline(f, inner, LAVENDER, (r.w * 0.05).clamp(0.5, 2.0));
}

// ── The window ──────────────────────────────────────────────────────

impl App for Hearts {
    fn title(&self) -> String {
        String::from(TITLE)
    }

    fn app_id(&self) -> String {
        String::from("hearts")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<std::time::Duration> {
        // Without a tick the machine players never play: the old program moved
        // them along inside the human's click handler, so the game only
        // advanced when the human touched it.
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

impl Probe for Hearts {
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
    let mut app = Hearts::new();
    app::launch("hearts", &mut app)
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![expect(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "test code: a panic is a diagnosis"
    )]

    use super::*;
    use guitk::probe::{click_sized, ctrl, is_visible_sized, press, rect_of_sized};

    // ── Fixtures ───────────────────────────────────────────────────

    /// The windows every geometric claim is checked at.
    ///
    /// A tall narrow one and a wide short one are both here because the two
    /// break a layout in opposite directions, and the old program -- which drew
    /// at one size whatever the window was -- was broken in both.
    const SIZES: [(f32, f32); 6] = [
        (WINDOW_WIDTH, WINDOW_HEIGHT),
        (640.0, 420.0),
        (1400.0, 900.0),
        (360.0, 280.0),
        (1200.0, 380.0),
        (480.0, 820.0),
    ];

    /// Widths and heights swept by the claims that are about the layout
    /// itself rather than about one window.
    ///
    /// A pane ordering or a containment rule is a property of `Layout::solve`
    /// at *every* size, and six sampled points are not a sweep: the sizes that
    /// break such a rule are the awkward ones, and none of them is a size
    /// anybody would choose. A window 57 pixels tall is the one where a card
    /// grows taller than what is left of the felt once the buttons have been
    /// paid for; a window 60 wide is the one where the fourth button does not
    /// fit; a window 20 wide is the one where a seat's label cannot be both on
    /// the felt and clear of its own card. Each of those was a mutation that
    /// survived the six-point sample.
    const GRID_W: [f32; 7] = [0.0, 20.0, 60.0, 120.0, 360.0, 900.0, 1400.0];
    const GRID_H: [f32; 6] = [0.0, 57.0, 120.0, 280.0, 620.0, 900.0];

    /// A game whose deal is a fact rather than a draw.
    fn game() -> Hearts {
        Hearts::with_seed(42)
    }

    /// The same game, past the pass and playing tricks.
    fn playing() -> Hearts {
        let mut g = game();
        g.chosen = vec![0, 1, 2];
        assert!(g.execute_pass(), "the pass was refused");
        assert_eq!(g.phase, GamePhase::Playing);
        g
    }

    /// A game in which it is the human's turn, with the clock stopped.
    fn our_turn() -> Hearts {
        let mut g = playing();
        run_for(&mut g, 60_000, |g| g.turn == 0 && g.ready());
        assert_eq!(g.turn, 0, "the game never came round to the human");
        g
    }

    /// Tick the clock until `done`, or until `budget_ms` has been spent.
    ///
    /// Answers whether `done` came true, so a test can say so itself rather
    /// than hanging: a game that stops advancing is a bug this suite must
    /// report, not one it must wait for.
    fn run_for(g: &mut Hearts, budget_ms: u64, done: impl Fn(&Hearts) -> bool) -> bool {
        let mut spent = 0u64;
        while spent < budget_ms {
            if done(g) {
                return true;
            }
            g.tick(TICK_MS);
            spent = spent.saturating_add(TICK_MS);
        }
        done(g)
    }

    /// Whether `inner` is wholly within `outer`, to within a pixel of rounding.
    fn inside(outer: Rect, inner: Rect) -> bool {
        inner.is_empty()
            || (inner.x >= outer.x - 0.01
                && inner.y >= outer.y - 0.01
                && inner.right() <= outer.right() + 0.01
                && inner.bottom() <= outer.bottom() + 0.01)
    }

    /// Every hit box the frame recorded for a card of the human's hand.
    fn card_boxes(g: &Hearts, size: (f32, f32)) -> Vec<(usize, Rect)> {
        g.frame(size.0, size.1)
            .hits()
            .iter()
            .filter_map(|(t, r)| match *t {
                Target::Card(i) => Some((i, *r)),
                _ => None,
            })
            .collect()
    }

    // ── The window ─────────────────────────────────────────────────

    #[test]
    fn the_program_asks_for_a_window_and_a_clock() {
        // `main` was `let _app = Hearts::new();`: the game dealt itself a hand,
        // built a renderer it never called, and dropped the lot. Nothing in the
        // file mentioned a window.
        let g = game();
        assert_eq!(g.title(), TITLE);
        assert_eq!(g.app_id(), "hearts");
        assert_eq!(
            g.initial_size(),
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        );
        assert_eq!(
            g.tick_interval(),
            Some(std::time::Duration::from_millis(TICK_MS)),
            "without a clock the machine players never play"
        );
    }

    #[test]
    fn closing_the_window_exits() {
        let mut g = game();
        assert!(matches!(g.on_event(&Event::CloseRequested), Response::Exit));
    }

    #[test]
    fn a_frame_is_drawn_at_the_size_it_is_given_not_the_size_it_remembers() {
        // The first frame a real window submits goes out before any resize
        // event arrives, so a renderer that trusted a remembered size would
        // draw that frame at the wrong one.
        let mut g = game();
        g.size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let f = g.draw((400.0, 300.0));
        assert_eq!((f.width, f.height), (400.0, 300.0));
        let hand = card_boxes(&g, (400.0, 300.0));
        assert!(
            hand.iter().all(|(_, r)| r.right() <= 400.5),
            "the hand was drawn for a 900-pixel window in a 400-pixel one"
        );
    }

    #[test]
    fn a_move_asks_for_a_redraw_and_a_dead_key_does_not() {
        // A window that answered `Idle` to a move would show the selection
        // where it used to be until something else happened to repaint.
        let mut g = game();
        assert!(matches!(
            g.on_event(&Event::Key(press(Key::Right))),
            Response::Redraw
        ));
        assert!(matches!(
            g.on_event(&Event::Key(press(Key::Left))),
            Response::Redraw
        ));
        assert!(
            matches!(g.on_event(&Event::Key(press(Key::Left))), Response::Idle),
            "the left end of the hand asked for a repaint that changes nothing"
        );
    }

    #[test]
    fn render_draws_at_the_window_it_was_given_and_records_it() {
        // Two claims, because the renderer is the only place the window's size
        // is learned. It must draw at the size the compositor hands it, and it
        // must remember that size: the click handler resolves a pointer against
        // the frame the renderer last drew, so a renderer that forgot would
        // test every click against a picture that is not on the screen.
        let mut g = game();
        let small = (520.0, 700.0);
        let tree = g.render(small.0, small.1);
        assert!(
            tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::FillRect { width, height, .. }
                    if (*width - small.0).abs() < 0.01 && (*height - small.1).abs() < 0.01
            )),
            "nothing was painted at the size render was given"
        );
        assert_eq!(g.size, small, "the renderer did not record its window");

        let boxes = card_boxes(&g, small);
        let (i, r) = *boxes.last().unwrap();
        let (cx, cy) = r.centre();
        assert_eq!(
            g.handle_event(&Event::Mouse(MouseEvent {
                x: cx,
                y: cy,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            EventResult::Consumed
        );
        assert_eq!(
            g.selected, i,
            "the click was resolved against another window"
        );
    }

    // ── The panes ──────────────────────────────────────────────────

    #[test]
    fn every_pane_is_inside_the_window_at_every_size() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            for (name, r) in [
                ("header", l.header),
                ("table", l.table),
                ("hand", l.hand),
                ("footer", l.footer),
                ("status", l.status),
                ("scores", l.scores),
            ] {
                assert!(
                    inside(l.window, r),
                    "{name} {r:?} left the {w}x{h} window {:?}",
                    l.window
                );
            }
        }
    }

    #[test]
    fn the_panes_are_stacked_and_do_not_overlap() {
        // This is the repair for a program that drew its status line at
        // `height - 18.0` and its controls hint at `height - 14.0`: four pixels
        // apart, in the same font, one painted through the other.
        for w in GRID_W {
            for h in GRID_H {
                let l = Layout::solve(w, h);
                assert!(
                    l.header.bottom() <= l.table.y + 0.01,
                    "header over table at {w}x{h}"
                );
                assert!(
                    l.table.bottom() <= l.hand.y + 0.01,
                    "table over hand at {w}x{h}"
                );
                assert!(
                    l.hand.bottom() <= l.footer.y + 0.01,
                    "the hand was drawn over the buttons at {w}x{h}"
                );
                assert!(
                    l.footer.bottom() <= l.status.y + 0.01,
                    "the buttons were drawn over the status line at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn the_buttons_are_inside_the_footer_and_clear_of_the_status() {
        for (w, h) in GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h))) {
            let l = Layout::solve(w, h);
            let g = game();
            let f = g.frame(w, h);
            for (target, r) in f.hits() {
                if let Target::Button(b) = target {
                    assert!(
                        inside(l.footer, *r),
                        "{b:?} at {r:?} left the footer {:?} at {w}x{h}",
                        l.footer
                    );
                    assert!(
                        r.bottom() <= l.status.y + 0.01,
                        "{b:?} overlapped the status line at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_degenerate_window_draws_without_panicking() {
        // A compositor can hand out a zero-sized window while a resize is in
        // flight, and a layout that divided by the window would take the whole
        // program down with it.
        for (w, h) in [(0.0, 0.0), (1.0, 1.0), (900.0, 1.0), (1.0, 620.0)] {
            let g = game();
            let f = g.frame(w, h);
            assert!(
                f.commands().iter().all(|c| match c {
                    RenderCommand::FillRect { x, y, .. } => x.is_finite() && y.is_finite(),
                    RenderCommand::Text { x, y, .. } => x.is_finite() && y.is_finite(),
                    _ => true,
                }),
                "a {w}x{h} window drew a command at a coordinate that is not a number"
            );
        }
    }

    // ── The hand ───────────────────────────────────────────────────

    #[test]
    fn every_card_of_the_hand_is_drawn_and_reachable_at_every_size() {
        for (w, h) in SIZES {
            let g = game();
            let boxes = card_boxes(&g, (w, h));
            assert_eq!(
                boxes.len(),
                HAND_SIZE,
                "{} of {HAND_SIZE} cards had a hit box at {w}x{h}",
                boxes.len()
            );
            for (i, r) in &boxes {
                assert!(
                    inside(Rect::new(0.0, 0.0, w, h), *r),
                    "card {i} at {r:?} left the {w}x{h} window"
                );
            }
        }
    }

    #[test]
    fn the_cards_of_the_hand_run_left_to_right_in_order() {
        for (w, h) in SIZES {
            let boxes = card_boxes(&game(), (w, h));
            for pair in boxes.windows(2) {
                assert!(
                    pair[1].1.x > pair[0].1.x,
                    "cards {} and {} are out of order at {w}x{h}",
                    pair[0].0,
                    pair[1].0
                );
            }
        }
    }

    #[test]
    fn the_hand_is_centred_in_its_strip() {
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            let boxes = card_boxes(&game(), (w, h));
            let first = boxes.first().unwrap().1;
            let last = boxes.last().unwrap().1;
            let left = first.x - l.hand.x;
            let right = l.hand.right() - last.right();
            assert!(
                (left - right).abs() < 0.5,
                "the hand sits {left} from the left and {right} from the right at {w}x{h}"
            );
        }
    }

    #[test]
    fn a_narrow_window_closes_the_hand_up_rather_than_running_off_the_edge() {
        // The old hand was thirteen cards at a fixed 38-pixel step from a fixed
        // `HAND_X_START`: 516 pixels wide in every window there has ever been.
        let narrow = (360.0, 280.0);
        let wide = (1400.0, 900.0);
        let step_at = |size: (f32, f32)| {
            let b = card_boxes(&game(), size);
            b[1].1.x - b[0].1.x
        };
        let l = Layout::solve(narrow.0, narrow.1);
        assert!(
            step_at(narrow) < l.card.0,
            "a narrow window did not overlap the cards"
        );
        assert!(
            step_at(wide) > step_at(narrow),
            "the hand did not open out in a wider window"
        );
    }

    #[test]
    fn a_chosen_card_lifts_and_its_hit_box_lifts_with_it() {
        // The fault this program was built around: the drawing pass raised a
        // chosen card sixteen pixels and the click handler, which re-derived
        // the row from its own copies of `HAND_Y`, `HAND_X_START`,
        // `CARD_OVERLAP` and `CARD_WIDTH`, went on hit-testing the old row.
        let size = (WINDOW_WIDTH, WINDOW_HEIGHT);
        let mut g = game();
        let resting = card_boxes(&g, size)[4].1;
        assert!(g.toggle_chosen(4));
        let lifted = card_boxes(&g, size)[4].1;
        assert!(
            lifted.y < resting.y,
            "a chosen card was not lifted out of the hand"
        );

        let (cx, _) = lifted.centre();
        assert_eq!(
            g.frame(size.0, size.1).hit_test(cx, lifted.y + 2.0),
            Some(Target::Card(4)),
            "the top of the lifted card is not clickable"
        );
    }

    #[test]
    fn a_click_where_two_cards_overlap_reaches_the_one_on_top() {
        // Cards are drawn left to right and overlap, so the card a player sees
        // in the overlap is the right-hand one. A hit map recorded in any other
        // order would hand the click to the card underneath.
        let size = (360.0, 280.0);
        let g = game();
        let boxes = card_boxes(&g, size);
        let (left, right) = (boxes[3].1, boxes[4].1);
        assert!(right.x < left.right(), "these two cards do not overlap");
        let x = f32::midpoint(right.x, left.right());
        assert_eq!(
            g.frame(size.0, size.1).hit_test(x, left.centre().1),
            Some(Target::Card(4)),
            "the click reached the card underneath"
        );
    }

    // ── The table ──────────────────────────────────────────────────

    #[test]
    fn every_card_played_is_drawn_inside_the_table() {
        for (w, h) in SIZES {
            let mut g = playing();
            fill_a_trick(&mut g);
            assert_eq!(g.trick.cards.len(), SEATS, "no trick ever filled up");
            let l = Layout::solve(w, h);
            for tc in &g.trick.cards {
                assert!(
                    inside(l.table, l.trick_card(tc.player)),
                    "seat {}'s card left the felt at {w}x{h}",
                    tc.player
                );
            }
        }
    }

    #[test]
    fn the_scoreboard_is_inside_the_table_or_is_left_out() {
        // It was pinned at `SCORE_X = 700.0` by a method that took the window
        // width and ignored it: off the right-hand edge of any window narrower
        // than 860, and adrift in the middle of the felt in a wider one.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            assert!(
                inside(l.table, l.scores),
                "the scoreboard {:?} left the felt {:?} at {w}x{h}",
                l.scores,
                l.table
            );
        }
        let (w, h) = (300.0, 620.0);
        assert!(
            Layout::solve(w, h).scores.is_empty(),
            "a {w}-pixel window drew a scoreboard it cannot pay for"
        );
    }

    #[test]
    fn the_seat_labels_stay_on_the_felt_and_clear_of_the_trick() {
        for (w, h) in GRID_W.into_iter().flat_map(|w| GRID_H.map(move |h| (w, h))) {
            let l = Layout::solve(w, h);
            for seat in 1..SEATS {
                let label = l.seat_label(seat);
                assert!(
                    inside(l.table, label),
                    "seat {seat}'s label {label:?} left the felt at {w}x{h}"
                );
                assert!(
                    label.intersect(l.trick_card(seat)).is_none(),
                    "seat {seat}'s label sits under its own card at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn each_seat_plays_its_card_in_its_own_place() {
        // Four cards on one square of felt is not a trick anybody can read.
        for (w, h) in SIZES {
            let l = Layout::solve(w, h);
            for seat in 0..SEATS {
                for other in (seat + 1)..SEATS {
                    assert!(
                        l.trick_card(seat).intersect(l.trick_card(other)).is_none(),
                        "seats {seat} and {other} play on the same square at {w}x{h}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_wide_window_lays_the_hand_out_without_hiding_any_card() {
        // A card is capped at `MAX_CARD_W`, and the cap is the reason a large
        // window can show all thirteen at once: uncapped, the card grows with
        // the window faster than the strip does, and the fan closes back up
        // until every card but the last is partly behind its neighbour.
        let g = game();
        let boxes = card_boxes(&g, (1200.0, 900.0));
        assert_eq!(boxes.len(), HAND_SIZE);
        for pair in boxes.windows(2) {
            assert!(
                pair[0].1.intersect(pair[1].1).is_none(),
                "card {} hides behind card {} in a window with room for both",
                pair[0].0,
                pair[1].0
            );
        }
    }

    // ── The deal ───────────────────────────────────────────────────

    #[test]
    fn a_deal_gives_every_seat_thirteen_distinct_cards() {
        let g = game();
        let mut seen: Vec<Card> = Vec::new();
        for hand in &g.hands {
            assert_eq!(hand.len(), HAND_SIZE);
            seen.extend(hand.iter().copied());
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), SEATS * HAND_SIZE, "a card was dealt twice");
    }

    #[test]
    fn a_hand_is_held_in_suit_then_rank_order() {
        let g = game();
        for (seat, hand) in g.hands.iter().enumerate() {
            for pair in hand.windows(2) {
                assert!(pair[0] < pair[1], "seat {seat}'s hand is out of order");
            }
        }
    }

    #[test]
    fn every_deal_has_a_two_of_clubs_to_lead_with() {
        // `begin_play` falls back to seat 0 when nobody holds it, and this is
        // what says the fallback is unreachable rather than a rule.
        for seed in 0..64u64 {
            let g = Hearts::with_seed(seed);
            assert!(
                g.holder_of(Card::TWO_OF_CLUBS).is_some(),
                "deal {seed} contains no two of clubs"
            );
        }
    }

    #[test]
    fn a_launch_does_not_deal_the_one_hand_that_was_hardcoded() {
        // The deal used to be `SeededRng::new(42)`, so every launch on every
        // machine got the same thirteen cards for ever.
        //
        // Two `new()` games cannot be compared with each other here: off
        // Slate OS there is no kernel randomness to open, so `seed_from_system`
        // answers with its fallback and the two would agree for a reason that
        // has nothing to do with this program. What can be checked is that the
        // seed is no longer the literal 42.
        let dealt = Hearts::new();
        let old = Hearts::with_seed(42);
        assert!(
            dealt.hands != old.hands,
            "a fresh game still deals the hardcoded hand"
        );
    }

    #[test]
    fn two_seeds_deal_two_different_games() {
        assert!(Hearts::with_seed(1).hands != Hearts::with_seed(2).hands);
    }

    // ── The rules of a trick ───────────────────────────────────────

    #[test]
    fn the_first_trick_is_led_with_the_two_of_clubs() {
        let g = playing();
        let leader = g.holder_of(Card::TWO_OF_CLUBS).unwrap();
        assert_eq!(g.turn, leader, "the lead did not go to the two of clubs");
        let valid = g.valid_plays(leader);
        assert_eq!(valid.len(), 1, "the leader was offered a choice");
        assert_eq!(g.hands[leader][valid[0]], Card::TWO_OF_CLUBS);
    }

    #[test]
    fn following_suit_is_forced_when_the_suit_is_held() {
        let mut g = playing();
        g.trick_number = 1;
        g.trick = Trick::new();
        g.trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        g.hands[1] = vec![
            Card::new(Suit::Clubs, Rank::Three),
            Card::new(Suit::Diamonds, Rank::King),
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        assert_eq!(
            g.valid_plays(1),
            vec![0],
            "a seat holding a club was allowed to play something else"
        );
    }

    #[test]
    fn a_seat_void_in_the_led_suit_may_play_anything() {
        let mut g = playing();
        g.trick_number = 1;
        g.trick = Trick::new();
        g.trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        g.hands[1] = vec![
            Card::new(Suit::Diamonds, Rank::Two),
            Card::QUEEN_OF_SPADES,
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        assert_eq!(g.valid_plays(1), vec![0, 1, 2]);
    }

    #[test]
    fn no_point_card_is_discarded_on_the_first_trick() {
        // A seat void in clubs on the first trick could otherwise unload the
        // queen of spades on somebody in the opening seconds of the round.
        let mut g = playing();
        g.trick_number = 0;
        g.trick = Trick::new();
        g.trick.play(0, Card::TWO_OF_CLUBS);
        g.hands[1] = vec![
            Card::new(Suit::Diamonds, Rank::Two),
            Card::QUEEN_OF_SPADES,
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        assert_eq!(
            g.valid_plays(1),
            vec![0],
            "a point card was playable on the first trick"
        );
    }

    #[test]
    fn the_first_trick_rule_yields_when_points_are_all_that_is_left() {
        // A seat holding nothing but hearts and the queen has to play one of
        // them; a rule with no escape would deadlock the round.
        let mut g = playing();
        g.trick_number = 0;
        g.trick = Trick::new();
        g.trick.play(0, Card::TWO_OF_CLUBS);
        g.hands[1] = vec![Card::QUEEN_OF_SPADES, Card::new(Suit::Hearts, Rank::Ace)];
        assert_eq!(g.valid_plays(1), vec![0, 1]);
    }

    #[test]
    fn hearts_are_not_led_until_one_has_been_played() {
        let mut g = playing();
        g.trick_number = 4;
        g.trick = Trick::new();
        g.hearts_broken = false;
        g.hands[2] = vec![
            Card::new(Suit::Clubs, Rank::Nine),
            Card::new(Suit::Hearts, Rank::Two),
        ];
        assert_eq!(g.valid_plays(2), vec![0], "a heart was led unbroken");
        g.hearts_broken = true;
        assert_eq!(
            g.valid_plays(2),
            vec![0, 1],
            "a broken heart was still held"
        );
    }

    #[test]
    fn a_hand_of_nothing_but_hearts_may_lead_one() {
        let mut g = playing();
        g.trick_number = 4;
        g.trick = Trick::new();
        g.hearts_broken = false;
        g.hands[2] = vec![
            Card::new(Suit::Hearts, Rank::Two),
            Card::new(Suit::Hearts, Rank::King),
        ];
        assert_eq!(g.valid_plays(2), vec![0, 1]);
    }

    #[test]
    fn playing_a_heart_breaks_hearts() {
        let mut g = playing();
        g.trick_number = 3;
        g.trick = Trick::new();
        g.trick.play(1, Card::new(Suit::Clubs, Rank::Four));
        g.hands[2] = vec![Card::new(Suit::Hearts, Rank::Six)];
        assert!(!g.hearts_broken);
        assert!(g.play_card(2, 0));
        assert!(
            g.hearts_broken,
            "a heart was played and hearts stayed whole"
        );
    }

    #[test]
    fn a_trick_names_its_lead_suit_from_the_card_that_was_led() {
        // The lead suit used to be a field written by `play`, which is the same
        // fact in two places: a trick built any other way carried cards and no
        // lead suit, and `winner` answered `None` for it.
        let mut trick = Trick::new();
        assert_eq!(trick.lead_suit(), None);
        trick.play(2, Card::new(Suit::Diamonds, Rank::Nine));
        trick.play(3, Card::new(Suit::Spades, Rank::Ace));
        assert_eq!(trick.lead_suit(), Some(Suit::Diamonds));
        assert_eq!(trick.winner(), Some(2), "a spade took a diamond trick");
    }

    #[test]
    fn the_highest_card_of_the_led_suit_takes_the_trick() {
        let mut trick = Trick::new();
        trick.play(0, Card::new(Suit::Clubs, Rank::Four));
        trick.play(1, Card::new(Suit::Clubs, Rank::King));
        trick.play(2, Card::new(Suit::Clubs, Rank::Seven));
        trick.play(3, Card::new(Suit::Hearts, Rank::Ace));
        assert_eq!(trick.winner(), Some(1));
        assert!(trick.is_complete());
    }

    #[test]
    fn a_trick_is_worth_a_point_a_heart_and_thirteen_for_the_queen() {
        let mut trick = Trick::new();
        trick.play(0, Card::new(Suit::Clubs, Rank::Four));
        assert_eq!(trick.points(), 0);
        trick.play(1, Card::new(Suit::Hearts, Rank::Two));
        assert_eq!(trick.points(), 1);
        trick.play(2, Card::QUEEN_OF_SPADES);
        assert_eq!(trick.points(), 14);
        trick.play(3, Card::new(Suit::Spades, Rank::King));
        assert_eq!(trick.points(), 14, "an ordinary spade scored");
    }

    #[test]
    fn the_taker_of_a_trick_takes_its_points_and_leads_the_next() {
        let mut g = playing();
        g.trick_number = 5;
        g.trick = Trick::new();
        g.trick.play(0, Card::new(Suit::Clubs, Rank::Four));
        g.trick.play(1, Card::new(Suit::Clubs, Rank::King));
        g.trick.play(2, Card::new(Suit::Hearts, Rank::Two));
        g.hands[3] = vec![Card::new(Suit::Clubs, Rank::Five)];
        g.turn = 3;
        assert!(g.play_card(3, 0));

        assert_eq!(g.taker, Some(1), "the king did not take the trick");
        assert_eq!(g.round_points[1], 1);
        g.sweep_trick();
        assert_eq!(g.turn, 1, "the taker did not lead the next trick");
        assert_eq!(g.trick_number, 6);
        assert!(g.trick.cards.is_empty());
    }

    // ── Rounds and scores ──────────────────────────────────────────

    #[test]
    fn a_round_scores_what_each_seat_took() {
        let mut g = playing();
        g.round_points = [3, 0, 10, 13];
        g.scores = [5, 5, 5, 5];
        g.end_round();
        assert_eq!(g.scores, [8, 5, 15, 18]);
        assert_eq!(g.phase, GamePhase::RoundOver);
    }

    #[test]
    fn shooting_the_moon_scores_everyone_else_instead() {
        let mut g = playing();
        g.round_points = [0, MOON, 0, 0];
        g.scores = [1, 1, 1, 1];
        g.end_round();
        assert_eq!(
            g.scores,
            [27, 1, 27, 27],
            "the shooter was scored and the table was not"
        );
        assert!(g.status.contains(name(1)));
    }

    #[test]
    fn twenty_five_points_is_not_the_moon() {
        // The moon is all twenty-six, and a seat one heart short of it takes
        // the twenty-five in the ordinary way.
        let mut g = playing();
        g.round_points = [1, 25, 0, 0];
        g.scores = [0; SEATS];
        g.end_round();
        assert_eq!(g.scores, [1, 25, 0, 0]);
    }

    #[test]
    fn the_pass_direction_rotates_round_the_table() {
        let mut g = playing();
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push(g.pass_direction);
            g.round_points = [1, 0, 0, 0];
            g.end_round();
        }
        assert_eq!(
            seen,
            vec![
                PassDirection::Left,
                PassDirection::Right,
                PassDirection::Across,
                PassDirection::Keep,
                PassDirection::Left,
            ]
        );
    }

    #[test]
    fn the_game_ends_when_a_seat_reaches_a_hundred() {
        let mut g = playing();
        g.scores = [10, 20, 95, 30];
        g.round_points = [0, 0, 5, 0];
        g.end_round();
        assert_eq!(g.phase, GamePhase::GameOver);
        assert_eq!(g.winner, Some(0), "the lowest score did not win");
        assert!(g.status.contains(name(0)));
    }

    #[test]
    fn a_game_short_of_a_hundred_carries_on() {
        let mut g = playing();
        g.scores = [10, 20, 90, 30];
        g.round_points = [0, 0, 9, 0];
        g.end_round();
        assert_eq!(g.phase, GamePhase::RoundOver);
        assert_eq!(g.winner, None);
    }

    #[test]
    fn a_tied_game_is_won_by_the_earlier_seat() {
        // The only tie-break this game has ever had. It is held here so that it
        // is a decision rather than an accident of `position`.
        let mut g = playing();
        g.scores = [20, 20, 100, 20];
        g.round_points = [0; SEATS];
        g.end_round();
        assert_eq!(g.winner, Some(0));
    }

    // ── The game plays itself ──────────────────────────────────────

    /// Drive a whole game: tick the clock, and play the first legal card
    /// whenever it is the human's turn.
    ///
    /// Every step asserts that the seat to play has something it may play,
    /// which is what holds `every_seat_always_has_a_legal_card`: a hand with no
    /// legal card would stop the game for ever rather than fail a test.
    fn drive_round(g: &mut Hearts, steps: usize) {
        for _ in 0..steps {
            match g.phase {
                GamePhase::Passing => {
                    g.chosen = vec![0, 1, 2];
                    assert!(g.execute_pass());
                }
                GamePhase::Playing => {
                    if g.ready() {
                        let valid = g.valid_plays(g.turn);
                        assert!(
                            !valid.is_empty(),
                            "seat {} has thirteen cards and no legal play",
                            g.turn
                        );
                    }
                    if g.turn == 0 && g.ready() {
                        let valid = g.valid_plays(0);
                        g.selected = valid[0];
                        assert!(g.play_selected());
                    } else {
                        g.tick(u64::from(THINK_MS).saturating_add(u64::from(SWEEP_MS)));
                    }
                }
                GamePhase::RoundOver | GamePhase::GameOver => return,
            }
        }
    }

    /// Play rounds until the game is over or the budget of rounds is spent.
    fn drive_game(g: &mut Hearts, rounds: usize) {
        for _ in 0..rounds {
            drive_round(g, 400);
            if g.phase == GamePhase::GameOver {
                return;
            }
            assert_eq!(g.phase, GamePhase::RoundOver, "a round did not finish");
            g.start_round();
        }
    }

    /// Put four cards on the table, playing the human's for them.
    ///
    /// Ticking alone is not enough: the clock moves the machine players, and a
    /// trick whose fourth card is the human's waits for the human.
    fn fill_a_trick(g: &mut Hearts) {
        for _ in 0..400 {
            if g.trick.cards.len() == SEATS {
                return;
            }
            if g.phase == GamePhase::Playing && g.turn == 0 && g.ready() {
                let valid = g.valid_plays(0);
                g.selected = valid[0];
                assert!(g.play_selected());
            } else {
                g.tick(u64::from(THINK_MS));
            }
        }
    }

    #[test]
    fn a_round_is_thirteen_tricks_and_then_it_is_scored() {
        let mut g = playing();
        drive_round(&mut g, 400);
        assert!(
            matches!(g.phase, GamePhase::RoundOver | GamePhase::GameOver),
            "the round never finished: phase {:?} at trick {}",
            g.phase,
            g.trick_number
        );
        assert_eq!(g.trick_number, HAND_SIZE);
        assert_eq!(
            g.round_points.iter().sum::<u32>(),
            MOON,
            "the round's points do not add up to the twenty-six that were dealt"
        );
        assert!(
            g.hands.iter().all(Vec::is_empty),
            "a card was left in a hand at the end of the round"
        );
    }

    #[test]
    fn a_whole_game_plays_itself_to_a_hundred() {
        let mut g = game();
        drive_game(&mut g, 60);
        assert_eq!(g.phase, GamePhase::GameOver, "the game never ended");
        assert!(g.scores.iter().copied().max().unwrap() >= GAME_OVER_SCORE);
        assert!(g.winner.is_some());
    }

    #[test]
    fn every_seat_always_has_a_legal_card() {
        // Driven from four different deals, because a rule that deadlocks does
        // so on a hand and not on a program.
        for seed in [1u64, 7, 99, 12_345] {
            let mut g = Hearts::with_seed(seed);
            drive_game(&mut g, 20);
        }
    }

    // ── Passing ────────────────────────────────────────────────────

    #[test]
    fn three_cards_are_chosen_and_no_more() {
        let mut g = game();
        assert!(g.toggle_chosen(0));
        assert!(g.toggle_chosen(1));
        assert!(g.toggle_chosen(2));
        assert!(!g.toggle_chosen(3), "a fourth card was accepted");
        assert_eq!(g.chosen, vec![0, 1, 2]);
    }

    #[test]
    fn choosing_a_chosen_card_puts_it_back() {
        let mut g = game();
        assert!(g.toggle_chosen(4));
        assert!(g.toggle_chosen(4));
        assert!(g.chosen.is_empty());
        assert!(g.status.contains(&PASS_SIZE.to_string()));
    }

    #[test]
    fn clearing_puts_every_chosen_card_back() {
        let mut g = game();
        g.chosen = vec![0, 1, 2];
        assert!(g.press(Button::Clear));
        assert!(g.chosen.is_empty());
        assert!(
            !g.press(Button::Clear),
            "clearing an empty choice reported a change"
        );
    }

    #[test]
    fn the_pass_is_refused_until_three_cards_are_chosen() {
        let mut g = game();
        g.chosen = vec![0];
        assert!(!g.execute_pass());
        assert_eq!(g.phase, GamePhase::Passing);
        assert_eq!(g.hands[0].len(), HAND_SIZE);
    }

    #[test]
    fn passing_gives_the_cards_to_the_seat_the_direction_names() {
        for (direction, seat) in [
            (PassDirection::Left, 1),
            (PassDirection::Across, 2),
            (PassDirection::Right, 3),
        ] {
            let mut g = game();
            g.pass_direction = direction;
            g.chosen = vec![0, 1, 2];
            let given: Vec<Card> = g.chosen.iter().map(|&i| g.hands[0][i]).collect();
            assert!(g.execute_pass());
            for card in given {
                assert!(
                    g.hands[seat].contains(&card),
                    "{:?} did not send {} to seat {seat}",
                    direction,
                    card.name()
                );
                assert!(
                    !g.hands[0].contains(&card),
                    "{} was given away and kept",
                    card.name()
                );
            }
        }
    }

    #[test]
    fn every_seat_still_holds_thirteen_after_the_pass() {
        let mut g = game();
        g.chosen = vec![0, 5, 9];
        assert!(g.execute_pass());
        for (seat, hand) in g.hands.iter().enumerate() {
            assert_eq!(hand.len(), HAND_SIZE, "seat {seat} holds {}", hand.len());
        }
        assert!(g.chosen.is_empty());
        assert_eq!(g.phase, GamePhase::Playing);
    }

    #[test]
    fn the_round_with_no_pass_is_dealt_straight_into_play() {
        let mut g = game();
        g.pass_direction = PassDirection::Keep;
        g.start_round();
        assert_eq!(
            g.phase,
            GamePhase::Playing,
            "the no-pass round stopped to ask for three cards"
        );
        assert!(g.chosen.is_empty());
    }

    #[test]
    fn a_machine_player_gives_away_the_queen_of_spades() {
        // The three it picks are the queen first, then the spades that might be
        // forced to take her, then hearts high to low.
        let mut g = game();
        g.hands[1] = vec![
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Spades, Rank::Ace),
            Card::QUEEN_OF_SPADES,
            Card::new(Suit::Hearts, Rank::King),
        ];
        let chosen: Vec<Card> = g.ai_pass_choice(1).iter().map(|&i| g.hands[1][i]).collect();
        assert!(chosen.contains(&Card::QUEEN_OF_SPADES));
        assert!(chosen.contains(&Card::new(Suit::Spades, Rank::Ace)));
        assert!(!chosen.contains(&Card::new(Suit::Clubs, Rank::Two)));
    }

    // ── The clock ──────────────────────────────────────────────────

    #[test]
    fn a_machine_player_plays_on_the_clock_and_not_before() {
        // The old program moved the machine players inside the human's click
        // handler, so the game only advanced when the human touched it.
        let mut g = playing();
        if g.turn == 0 {
            let valid = g.valid_plays(0);
            g.selected = valid[0];
            assert!(g.play_selected());
        }
        assert_ne!(g.turn, 0);
        let before = g.trick.cards.len();
        g.tick(1);
        assert_eq!(
            g.trick.cards.len(),
            before,
            "a machine player answered instantly"
        );
        g.tick(u64::from(THINK_MS));
        assert_eq!(
            g.trick.cards.len(),
            before.saturating_add(1),
            "a machine player never played"
        );
    }

    #[test]
    fn a_finished_trick_stays_on_the_table_before_it_is_swept() {
        // `finish_trick` used to move the fourth card off the table in the same
        // event that put it there, so the player never saw the trick they had
        // just played into.
        let mut g = playing();
        fill_a_trick(&mut g);
        assert_eq!(g.trick.cards.len(), SEATS);
        let taker = g.taker.expect("a full trick named nobody");

        g.tick(u64::from(SWEEP_MS).saturating_sub(1));
        assert_eq!(
            g.trick.cards.len(),
            SEATS,
            "the trick was swept before it could be seen"
        );
        g.tick(u64::from(SWEEP_MS));
        assert!(g.trick.cards.is_empty(), "the trick was never swept");
        assert_eq!(g.turn, taker);
        assert_eq!(g.taker, None);
    }

    #[test]
    fn nobody_plays_while_a_settled_trick_is_being_shown() {
        let mut g = playing();
        fill_a_trick(&mut g);
        assert!(!g.ready());
        g.turn = 0;
        g.selected = 0;
        assert!(
            !g.play_selected(),
            "the human played into a trick that was still on the table"
        );
        assert_eq!(g.trick.cards.len(), SEATS);
    }

    #[test]
    fn the_clock_does_nothing_while_it_is_the_humans_turn() {
        let mut g = our_turn();
        let before = g.trick.cards.len();
        assert!(!g.tick(10_000), "the clock reported work it had not done");
        assert_eq!(g.trick.cards.len(), before);
        assert_eq!(g.hands[0].len(), HAND_SIZE);
    }

    // ── The keyboard and the pointer ───────────────────────────────

    #[test]
    fn the_arrows_move_the_selection_and_stop_at_both_ends() {
        // The two directions used to be written differently -- `Key::Left`
        // carried its bound in the match guard and `Key::Right` carried its own
        // in the arm body -- so they were not the same code in any sense a
        // reader could check.
        let mut g = game();
        assert_eq!(g.selected, 0);
        assert_eq!(
            g.key_at(&press(Key::Left), Hearts::SIZE),
            EventResult::Ignored,
            "the selection moved off the left end"
        );
        for i in 1..HAND_SIZE {
            assert_eq!(
                g.key_at(&press(Key::Right), Hearts::SIZE),
                EventResult::Consumed
            );
            assert_eq!(g.selected, i);
        }
        assert_eq!(
            g.key_at(&press(Key::Right), Hearts::SIZE),
            EventResult::Ignored,
            "the selection moved off the right end"
        );
        assert_eq!(g.selected, HAND_SIZE.saturating_sub(1));
    }

    #[test]
    fn a_key_release_does_nothing() {
        // A release that moved the selection would move it twice for every
        // press, because a key that goes down also comes back up.
        let mut g = game();
        let mut release = press(Key::Right);
        release.pressed = false;
        assert_eq!(g.key_at(&release, Hearts::SIZE), EventResult::Ignored);
        assert_eq!(g.selected, 0);
    }

    #[test]
    fn a_key_with_a_modifier_is_left_to_the_window() {
        // Ctrl+N is the compositor's new window; a game that dealt a fresh hand
        // on it would throw the player's game away as they opened a terminal.
        let mut g = playing();
        g.scores = [11; SEATS];
        assert_eq!(g.key_at(&ctrl(Key::N), Hearts::SIZE), EventResult::Ignored);
        assert_eq!(g.scores, [11; SEATS], "Ctrl+N dealt a new game");
    }

    #[test]
    fn a_click_on_a_card_plays_it_when_the_rules_allow() {
        let mut g = our_turn();
        let playable = g.valid_plays(0)[0];
        let card = g.hands[0][playable];
        assert_eq!(
            click_sized(
                &mut g,
                Target::Card(playable),
                MouseButton::Left,
                Hearts::SIZE
            ),
            EventResult::Consumed
        );
        assert!(!g.hands[0].contains(&card), "the card stayed in the hand");
        assert!(
            g.trick.cards.iter().any(|tc| tc.card == card),
            "the card never reached the table"
        );
    }

    #[test]
    fn a_click_on_a_card_the_rules_forbid_says_so_and_plays_nothing() {
        let mut g = playing();
        g.trick_number = 3;
        g.trick = Trick::new();
        g.trick.play(3, Card::new(Suit::Clubs, Rank::Nine));
        g.turn = 0;
        g.hands[0] = vec![
            Card::new(Suit::Clubs, Rank::Three),
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        g.selected = 0;
        assert_eq!(
            click_sized(&mut g, Target::Card(1), MouseButton::Left, Hearts::SIZE),
            EventResult::Consumed
        );
        assert_eq!(g.hands[0].len(), 2, "an illegal card was played");
        assert!(
            g.status.contains("cannot"),
            "the player was not told why: {:?}",
            g.status
        );
    }

    #[test]
    fn a_click_on_a_card_out_of_turn_plays_nothing() {
        let mut g = playing();
        g.turn = 2;
        g.trick = Trick::new();
        g.trick_number = 3;
        let held = g.hands[0].len();
        click_sized(&mut g, Target::Card(0), MouseButton::Left, Hearts::SIZE);
        assert_eq!(g.hands[0].len(), held, "the human played out of turn");
    }

    #[test]
    fn a_click_on_a_card_chooses_it_for_the_pass() {
        let mut g = game();
        assert_eq!(g.phase, GamePhase::Passing);
        click_sized(&mut g, Target::Card(6), MouseButton::Left, Hearts::SIZE);
        assert_eq!(g.chosen, vec![6]);
        assert_eq!(g.selected, 6, "the keyboard did not follow the pointer");
    }

    #[test]
    fn every_button_does_what_its_key_does() {
        // The pointer and the keyboard reach the same function, so a verb
        // cannot exist on one and not the other.
        fn snapshot(g: &Hearts) -> (GamePhase, Vec<usize>, bool, [u32; SEATS], Vec<Card>, usize) {
            (
                g.phase,
                g.chosen.clone(),
                g.show_help,
                g.scores,
                g.hands[0].clone(),
                g.turn,
            )
        }
        for button in BUTTONS {
            let mut by_key = game();
            let mut by_click = game();
            by_key.chosen = vec![0, 1, 2];
            by_click.chosen = vec![0, 1, 2];
            by_key.key_at(&press(button.key()), Hearts::SIZE);
            click_sized(
                &mut by_click,
                Target::Button(button),
                MouseButton::Left,
                Hearts::SIZE,
            );
            assert_eq!(
                snapshot(&by_key),
                snapshot(&by_click),
                "{button:?} does one thing from the keyboard and another from the pointer"
            );
        }
    }

    #[test]
    fn new_game_clears_the_scores_and_deals_again() {
        let mut g = playing();
        g.scores = [40, 50, 60, 70];
        g.round_number = 3;
        g.pass_direction = PassDirection::Across;
        assert!(g.press(Button::New));
        assert_eq!(g.scores, [0; SEATS]);
        assert_eq!(g.round_number, 0);
        assert_eq!(g.pass_direction, PassDirection::Left);
        assert_eq!(g.phase, GamePhase::Passing);
        assert_eq!(g.hands[0].len(), HAND_SIZE);
    }

    #[test]
    fn enter_deals_the_next_round_and_then_the_next_game() {
        let mut g = playing();
        g.phase = GamePhase::RoundOver;
        g.pass_direction = PassDirection::Right;
        assert!(g.confirm());
        assert_eq!(g.phase, GamePhase::Passing);
        assert_eq!(g.hands[0].len(), HAND_SIZE);

        g.phase = GamePhase::GameOver;
        g.scores = [100, 3, 4, 5];
        assert!(g.confirm());
        assert_eq!(g.scores, [0; SEATS], "a new game kept the old scores");
    }

    #[test]
    fn the_selection_stays_on_a_card_that_exists() {
        let mut g = playing();
        g.trick_number = 6;
        g.trick = Trick::new();
        g.turn = 0;
        // Two cards, and the *last* one played: a one-card hand cannot tell a
        // clamp from the absence of one, because `0.min(len - 1)` is zero
        // either way.
        g.hands[0] = vec![
            Card::new(Suit::Clubs, Rank::Three),
            Card::new(Suit::Clubs, Rank::Four),
        ];
        g.selected = 1;
        assert!(g.play_selected());
        assert_eq!(g.hands[0].len(), 1);
        assert_eq!(
            g.selected, 0,
            "the selection points past the end of the hand"
        );

        // Said in the window as well as in the field: an index that names no
        // card leaves the hand with nothing ringed, and the human with no
        // sign of what Enter would play.
        let boxes = card_boxes(&g, Hearts::SIZE);
        assert_eq!(boxes.len(), 1);
        assert!(
            ringed(&g, Hearts::SIZE, boxes[0].1, YELLOW),
            "the hand it left behind has no card ringed"
        );

        // An empty hand still draws: a round ends with four of them.
        g.trick = Trick::new();
        g.turn = 0;
        assert!(g.play_selected());
        assert!(g.hands[0].is_empty());
        assert!(card_boxes(&g, Hearts::SIZE).is_empty());
    }

    #[test]
    fn the_help_card_covers_the_table_and_a_click_dismisses_it() {
        let mut g = our_turn();
        let held = g.hands[0].len();
        assert!(!is_visible_sized(&g, Target::Help, Hearts::SIZE));
        assert!(g.press(Button::Help));

        let card = rect_of_sized(&g, Target::Help, Hearts::SIZE).expect("no help card was drawn");
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let (cx, cy) = l.table.centre();
        assert!(
            card.contains(cx, cy),
            "the help card {card:?} does not cover the middle of the table"
        );
        assert_eq!(
            click_sized(&mut g, Target::Help, MouseButton::Left, Hearts::SIZE),
            EventResult::Consumed
        );
        assert!(!g.show_help, "the help card would not close");
        assert_eq!(g.hands[0].len(), held, "the click fell through to the game");
    }

    #[test]
    fn escape_closes_the_help_card_before_it_clears_a_choice() {
        let mut g = game();
        g.chosen = vec![0, 1];
        g.show_help = true;
        assert!(g.press(Button::Clear));
        assert!(!g.show_help);
        assert_eq!(g.chosen, vec![0, 1], "the choice was cleared as well");
    }

    // ── What the window actually says ──────────────────────────────

    /// Every string painted inside `area`.
    ///
    /// Scoped to a rectangle, because a search of the whole frame is not a test
    /// of the widget you meant: the rank "3" appears on a card of the hand
    /// whether or not the table drew anything at all.
    fn texts_in(g: &Hearts, size: (f32, f32), area: Rect) -> Vec<String> {
        g.frame(size.0, size.1)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { x, y, text, .. } if area.contains(*x, *y) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The colour of the rectangle filled at exactly `r`, if one was.
    fn fill_color_at(g: &Hearts, size: (f32, f32), r: Rect) -> Option<Color> {
        g.frame(size.0, size.1)
            .commands()
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (x - r.x).abs() < 0.01
                    && (y - r.y).abs() < 0.01
                    && (width - r.w).abs() < 0.01
                    && (height - r.h).abs() < 0.01 =>
                {
                    Some(*color)
                }
                _ => None,
            })
    }

    /// Whether a ring of `color` was drawn round exactly `r`.
    fn ringed(g: &Hearts, size: (f32, f32), r: Rect, want: Color) -> bool {
        g.frame(size.0, size.1).commands().iter().any(|c| match c {
            RenderCommand::StrokeRect {
                x, y, width, color, ..
            } => {
                *color == want
                    && (x - r.x).abs() < 0.01
                    && (y - r.y).abs() < 0.01
                    && (width - r.w).abs() < 0.01
            }
            _ => false,
        })
    }

    #[test]
    fn a_card_on_the_table_is_drawn_with_its_rank_and_its_suit() {
        // What the old program drew for a finished trick was four `FillRect`s
        // and nothing else: blank grey rectangles with no rank and no suit on
        // them.
        let mut g = playing();
        fill_a_trick(&mut g);
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        for tc in &g.trick.cards {
            let words = texts_in(&g, Hearts::SIZE, l.trick_card(tc.player));
            assert!(
                words.iter().any(|s| s == tc.card.rank.label()),
                "seat {}'s {} was drawn without its rank: {words:?}",
                tc.player,
                tc.card.name()
            );
            assert!(
                words.iter().any(|s| s == tc.card.suit.symbol()),
                "seat {}'s {} was drawn without its suit: {words:?}",
                tc.player,
                tc.card.name()
            );
        }
    }

    #[test]
    fn the_taker_of_the_trick_on_the_table_is_ringed() {
        let mut g = playing();
        fill_a_trick(&mut g);
        let taker = g.taker.expect("a full trick named nobody");
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert!(
            ringed(&g, Hearts::SIZE, l.trick_card(taker), GREEN),
            "the trick was taken and the table did not say by whom"
        );
        for seat in 0..SEATS {
            if seat != taker {
                assert!(
                    !ringed(&g, Hearts::SIZE, l.trick_card(seat), GREEN),
                    "seat {seat} was ringed as well as the taker"
                );
            }
        }
    }

    #[test]
    fn a_card_the_rules_forbid_is_drawn_dimmed() {
        // The old hand was drawn one way whatever the rules said, so the only
        // way to find out that a card could not be played was to play it and
        // read the complaint.
        let mut g = playing();
        g.trick_number = 3;
        g.trick = Trick::new();
        g.trick.play(3, Card::new(Suit::Clubs, Rank::Nine));
        g.turn = 0;
        g.hands[0] = vec![
            Card::new(Suit::Clubs, Rank::Three),
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        assert_eq!(g.valid_plays(0), vec![0]);

        let boxes = card_boxes(&g, Hearts::SIZE);
        assert_eq!(
            fill_color_at(&g, Hearts::SIZE, boxes[0].1),
            Some(CARD_FACE),
            "the legal card was dimmed"
        );
        assert_eq!(
            fill_color_at(&g, Hearts::SIZE, boxes[1].1),
            Some(SUBTEXT0),
            "the illegal card was drawn as though it could be played"
        );
    }

    #[test]
    fn no_card_is_dimmed_when_it_is_not_the_humans_turn() {
        // Greying the whole hand says nothing, and a hand that greys itself
        // between turns reads as a hand that has gone dead.
        let mut g = playing();
        g.turn = 2;
        g.trick_number = 3;
        g.trick = Trick::new();
        for (_, r) in card_boxes(&g, Hearts::SIZE) {
            assert_eq!(fill_color_at(&g, Hearts::SIZE, r), Some(CARD_FACE));
        }
    }

    #[test]
    fn the_selected_card_is_ringed_and_only_that_one() {
        let mut g = game();
        g.selected = 5;
        let boxes = card_boxes(&g, Hearts::SIZE);
        for (i, r) in boxes {
            assert_eq!(
                ringed(&g, Hearts::SIZE, r, YELLOW),
                i == 5,
                "card {i} is ringed and should not be, or is not and should be"
            );
        }
    }

    #[test]
    fn the_confirm_button_says_what_pressing_it_will_do() {
        // Enter means four different things across the phases. The footer, the
        // help card and this test all read `confirm_label`, so a button whose
        // meaning changes cannot leave its label behind.
        for (phase, chosen) in [
            (GamePhase::Passing, vec![]),
            (GamePhase::Passing, vec![0, 1, 2]),
            (GamePhase::Playing, vec![]),
            (GamePhase::RoundOver, vec![]),
            (GamePhase::GameOver, vec![]),
        ] {
            let mut g = game();
            g.phase = phase;
            g.chosen = chosen.clone();
            let r = rect_of_sized(&g, Target::Button(Button::Confirm), Hearts::SIZE)
                .expect("no confirm button was drawn");
            let words = texts_in(&g, Hearts::SIZE, r);
            let want = confirm_label(phase, chosen.len());
            assert!(
                words.iter().any(|s| s == want),
                "in {phase:?} with {} chosen the button says {words:?}, not {want:?}",
                chosen.len()
            );
        }
    }

    #[test]
    fn the_help_card_lists_every_button_by_key_and_by_label() {
        let mut g = game();
        assert!(g.press(Button::Help));
        let card = rect_of_sized(&g, Target::Help, Hearts::SIZE).expect("no help card");
        let words = texts_in(&g, Hearts::SIZE, card);
        for button in BUTTONS {
            assert!(
                words.iter().any(|s| s == button.key_name()),
                "{button:?} has no key on the help card: {words:?}"
            );
            assert!(
                words.iter().any(|s| s == g.button_label(button)),
                "{button:?} has no label on the help card: {words:?}"
            );
        }
    }

    #[test]
    fn the_scoreboard_names_every_seat_and_shows_what_it_has() {
        let mut g = playing();
        g.scores = [7, 19, 0, 45];
        g.round_points = [1, 2, 3, 4];
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let words = texts_in(&g, Hearts::SIZE, l.scores);
        for seat in 0..SEATS {
            assert!(
                words.iter().any(|s| s == name(seat)),
                "{} is missing from the scoreboard: {words:?}",
                name(seat)
            );
            let want = format!("{} (+{})", g.scores[seat], g.round_points[seat]);
            assert!(
                words.contains(&want),
                "seat {seat}'s score {want:?} is missing: {words:?}"
            );
        }
    }

    #[test]
    fn a_seat_label_says_who_it_is_and_how_much_is_left() {
        // The old table wrote `West (13)` and its two fellows at three literal
        // offsets from a literal centre, and said nothing about what a seat had
        // taken -- which is the number that decides the round.
        let mut g = playing();
        g.turn = 2;
        g.round_points = [0, 5, 0, 0];
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        for seat in 1..SEATS {
            let words = texts_in(&g, Hearts::SIZE, l.seat_label(seat));
            assert!(
                words.iter().any(|s| s == name(seat)),
                "seat {seat} is unnamed: {words:?}"
            );
            let want = format!(
                "{} left \u{00b7} {} pt",
                g.hands[seat].len(),
                g.round_points[seat]
            );
            assert!(
                words.contains(&want),
                "seat {seat} does not say {want:?}: {words:?}"
            );
        }
    }

    #[test]
    fn the_seat_whose_turn_it_is_is_marked() {
        let mut g = playing();
        g.turn = 2;
        g.sweep_ms = 0;
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(
            fill_color_at(&g, Hearts::SIZE, l.seat_label(2)),
            Some(SURFACE1),
            "the seat to play is not marked"
        );
        assert_eq!(
            fill_color_at(&g, Hearts::SIZE, l.seat_label(1)),
            Some(SURFACE0),
            "a seat that is not to play is marked as though it were"
        );
    }

    #[test]
    fn the_table_says_which_way_the_pass_goes() {
        for direction in [
            PassDirection::Left,
            PassDirection::Right,
            PassDirection::Across,
        ] {
            let mut g = game();
            g.pass_direction = direction;
            let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
            let words = texts_in(&g, Hearts::SIZE, l.table);
            assert!(
                words.iter().any(|s| s == direction.label()),
                "the felt does not say {:?}: {words:?}",
                direction.label()
            );
        }
    }

    #[test]
    fn the_header_says_which_round_is_being_played() {
        let mut g = playing();
        g.round_number = 2;
        let l = Layout::solve(WINDOW_WIDTH, WINDOW_HEIGHT);
        let words = texts_in(&g, Hearts::SIZE, l.header);
        assert!(words.iter().any(|s| s == TITLE));
        assert!(
            words.iter().any(|s| s.contains("Round 3")),
            "the header does not name the round: {words:?}"
        );
    }

    #[test]
    fn the_status_line_is_bounded_by_the_window_it_is_drawn_in() {
        // The status carries seat names and card names, and unbounded it ran
        // straight off the right-hand edge of a narrow window.
        for (w, h) in SIZES {
            let g = playing();
            let bound = g
                .frame(w, h)
                .commands()
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text {
                        x,
                        text,
                        max_width,
                        overflow,
                        ..
                    } if *text == g.status => Some((*x, *max_width, *overflow)),
                    _ => None,
                })
                .expect("the status line was not drawn");
            let (x, max_width, overflow) = bound;
            let width = max_width.expect("the status line was drawn unbounded");
            assert!(
                x + width <= w + 0.01,
                "the status line runs {} past the right edge at {w}x{h}",
                x + width - w
            );
            assert_eq!(overflow, TextOverflow::Ellipsis);
        }
    }

    #[test]
    fn the_status_line_says_what_just_happened() {
        let mut g = playing();
        fill_a_trick(&mut g);
        let taker = g.taker.unwrap();
        assert!(
            g.status.contains(name(taker)),
            "the status does not name the taker: {:?}",
            g.status
        );
    }
}
