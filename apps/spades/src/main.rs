#![allow(dead_code)]
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
#![allow(clippy::match_same_arms)]
#![allow(unused_imports)]

//! OurOS Spades -- a 4-player trick-taking card game with AI opponents.
//!
//! The human plays as Player 0 (South), partnered with Player 2 (North)
//! against Player 1 (East) and Player 3 (West). Features bidding with nil
//! support, spades-broken tracking, bag penalties, and a Catppuccin Mocha UI.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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

// ── Layout constants ────────────────────────────────────────────────
const WINDOW_W: f32 = 900.0;
const WINDOW_H: f32 = 700.0;
const CARD_W: f32 = 60.0;
const CARD_H: f32 = 84.0;
const CARD_SPACING: f32 = 40.0;
const HAND_Y: f32 = 580.0;
const TRICK_CENTER_X: f32 = 390.0;
const TRICK_CENTER_Y: f32 = 300.0;
const SIDEBAR_X: f32 = 720.0;
const HEADER_Y: f32 = 10.0;
const FOOTER_Y: f32 = 675.0;

const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const CARD_FONT_SIZE: f32 = 18.0;
const CARD_SUIT_SIZE: f32 = 24.0;
const SMALL_FONT_SIZE: f32 = 12.0;
const BID_FONT_SIZE: f32 = 28.0;

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

    fn color(self) -> Color {
        match self {
            Suit::Clubs => GREEN,
            Suit::Diamonds => BLUE,
            Suit::Hearts => RED,
            Suit::Spades => LAVENDER,
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
        (self.suit as u16) * 100 + self.rank.0 as u16
    }

    /// Sort key: by rank first, then by suit.
    fn sort_key_rank(self) -> u16 {
        (self.rank.0 as u16) * 10 + self.suit as u16
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
        for r in 2..=14 {
            deck.push(Card::new(suit, Rank(r)));
        }
    }
    deck
}

// ── Seeded LCG RNG ─────────────────────────────────────────────────

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Random number in [0, n).
    fn next_range(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_u32() % n
    }

    /// Fisher-Yates shuffle.
    fn shuffle<T>(&mut self, slice: &mut [T]) {
        let len = slice.len();
        for i in (1..len).rev() {
            let j = self.next_range((i + 1) as u32) as usize;
            slice.swap(i, j);
        }
    }
}

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
        self.0 as usize
    }

    fn next(self) -> PlayerId {
        PlayerId((self.0 + 1) % 4)
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

    fn position_label(self) -> &'static str {
        match self.0 {
            0 => "South",
            1 => "East",
            2 => "North",
            3 => "West",
            _ => "?",
        }
    }

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
#[derive(Clone, Debug)]
struct Trick {
    cards: Vec<(PlayerId, Card)>,
    leader: PlayerId,
}

impl Trick {
    fn new(leader: PlayerId) -> Self {
        Self {
            cards: Vec::with_capacity(4),
            leader,
        }
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
        if self.cards.is_empty() {
            return None;
        }
        let led = self.led_suit()?;
        let mut best_player = self.cards[0].0;
        let mut best_card = self.cards[0].1;
        for &(player, card) in &self.cards[1..] {
            if card.beats(best_card, led) {
                best_card = card;
                best_player = player;
            }
        }
        Some(best_player)
    }

    /// Whether any card played is a spade.
    fn contains_spade(&self) -> bool {
        self.cards.iter().any(|(_, c)| c.suit == Suit::Spades)
    }
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
}

impl SpadesGame {
    fn new() -> Self {
        let mut game = Self {
            rng: Rng::new(42),
            phase: Phase::Bidding,
            hands: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            teams: [TeamState::new(), TeamState::new()],
            player_rounds: [
                PlayerRound::new(),
                PlayerRound::new(),
                PlayerRound::new(),
                PlayerRound::new(),
            ],
            current_trick: Trick::new(PlayerId::EAST),
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
        };
        game.deal();
        game.run_ai_bids_before_human();
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
        // Player to left of dealer leads bidding and first trick
        self.current_player = self.dealer.next();
        self.deal();
        self.run_ai_bids_before_human();
    }

    fn deal(&mut self) {
        let mut deck = standard_deck();
        self.rng.shuffle(&mut deck);
        for i in 0..4 {
            self.hands[i] = deck[i * 13..(i + 1) * 13].to_vec();
        }
        self.sort_hand(PlayerId::SOUTH);
        self.sort_hand(PlayerId::EAST);
        self.sort_hand(PlayerId::NORTH);
        self.sort_hand(PlayerId::WEST);
    }

    fn sort_hand(&mut self, player: PlayerId) {
        let idx = player.index();
        let order = self.sort_order;
        self.hands[idx].sort_by_key(|c| match order {
            SortOrder::BySuit => c.sort_key_suit(),
            SortOrder::ByRank => c.sort_key_rank(),
        });
    }

    fn sort_all_hands(&mut self) {
        for i in 0..4 {
            let order = self.sort_order;
            self.hands[i].sort_by_key(|c| match order {
                SortOrder::BySuit => c.sort_key_suit(),
                SortOrder::ByRank => c.sort_key_rank(),
            });
        }
    }

    // ── Bidding ─────────────────────────────────────────────────────

    /// AI bidding heuristic: count high cards and spades to estimate tricks.
    fn ai_bid(&self, player: PlayerId) -> u8 {
        let hand = &self.hands[player.index()];
        let mut estimate: u8 = 0;

        // Count aces and kings as likely tricks
        for card in hand {
            if card.rank == Rank::ACE {
                estimate += 1;
            } else if card.rank == Rank::KING {
                // King is usually good if you have 3+ cards in the suit
                let suit_count = hand.iter().filter(|c| c.suit == card.suit).count();
                if suit_count >= 3 {
                    estimate += 1;
                }
            }
        }

        // Count spades (trump) as partial tricks
        let spade_count = hand.iter().filter(|c| c.suit == Suit::Spades).count() as u8;
        if spade_count >= 3 {
            estimate += 1;
        }
        if spade_count >= 5 {
            estimate += 1;
        }

        // Queens in long suits
        for card in hand {
            if card.rank == Rank::QUEEN {
                let suit_count = hand.iter().filter(|c| c.suit == card.suit).count();
                if suit_count >= 4 {
                    estimate += 1;
                }
            }
        }

        // Clamp to reasonable range
        estimate.clamp(1, 6)
    }

    /// Run AI bids for players before the human (bidding starts left of dealer).
    fn run_ai_bids_before_human(&mut self) {
        while self.phase == Phase::Bidding && !self.current_player.is_human() {
            let bid = self.ai_bid(self.current_player);
            self.player_rounds[self.current_player.index()].bid = Some(bid);
            self.advance_bidder();
        }
        if self.phase == Phase::Bidding && self.current_player.is_human() {
            self.status_message =
                "Choose your bid (Up/Down to adjust, Enter to confirm)".to_string();
        }
    }

    fn advance_bidder(&mut self) {
        self.current_player = self.current_player.next();
        // Check if all 4 have bid
        if self.player_rounds.iter().all(|pr| pr.bid.is_some()) {
            self.phase = Phase::Playing;
            self.current_player = self.dealer.next();
            self.current_trick = Trick::new(self.current_player);
            self.status_message = format!("{} leads", self.current_player.name());
        }
    }

    fn submit_human_bid(&mut self) {
        if self.phase != Phase::Bidding || !self.current_player.is_human() {
            return;
        }
        self.player_rounds[0].bid = Some(self.bid_selection);
        self.advance_bidder();

        // Run remaining AI bids after human
        while self.phase == Phase::Bidding && !self.current_player.is_human() {
            let bid = self.ai_bid(self.current_player);
            self.player_rounds[self.current_player.index()].bid = Some(bid);
            self.advance_bidder();
        }

        if self.phase == Phase::Playing {
            if self.current_player.is_human() {
                self.status_message = "Your turn to lead".to_string();
            } else {
                self.run_ai_plays();
            }
        }
    }

    // ── Card play logic ─────────────────────────────────────────────

    /// Get legal cards the player can play from their hand.
    fn legal_plays(&self, player: PlayerId) -> Vec<usize> {
        let hand = &self.hands[player.index()];
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
        let card = self.hands[player.index()].remove(hand_index);
        self.current_trick.add(player, card);

        if card.suit == Suit::Spades {
            self.spades_broken = true;
        }

        if self.current_trick.is_complete() {
            self.resolve_trick();
        } else {
            self.current_player = self.current_player.next();
            if !self.current_player.is_human() {
                self.run_ai_plays();
            } else {
                self.status_message = "Your turn to play".to_string();
                self.clamp_selected_card();
            }
        }
    }

    fn resolve_trick(&mut self) {
        let winner = self.current_trick.winner().unwrap_or(PlayerId::SOUTH);
        self.player_rounds[winner.index()].tricks_won += 1;
        self.tricks_played += 1;
        self.last_trick = Some(self.current_trick.clone());
        self.phase = Phase::TrickDone;
        self.status_message = format!("{} wins the trick!", winner.name());
        self.current_player = winner;
    }

    /// Advance from TrickDone to the next trick or round.
    fn advance_after_trick(&mut self) {
        if self.tricks_played >= 13 {
            self.score_round();
            return;
        }
        self.phase = Phase::Playing;
        self.current_trick = Trick::new(self.current_player);

        if !self.current_player.is_human() {
            self.run_ai_plays();
        } else {
            self.status_message = "Your turn to lead".to_string();
            self.clamp_selected_card();
        }
    }

    // ── AI play ─────────────────────────────────────────────────────

    fn run_ai_plays(&mut self) {
        while self.phase == Phase::Playing && !self.current_player.is_human() {
            let legal = self.legal_plays(self.current_player);
            if legal.is_empty() {
                break;
            }
            let choice = self.ai_choose_card(self.current_player, &legal);
            self.play_card(self.current_player, choice);
            if self.phase != Phase::Playing {
                break;
            }
        }
    }

    /// AI card selection strategy.
    fn ai_choose_card(&mut self, player: PlayerId, legal: &[usize]) -> usize {
        if legal.is_empty() {
            return 0;
        }
        if legal.len() == 1 {
            return legal[0];
        }

        let hand = &self.hands[player.index()];
        let is_nil = self.player_rounds[player.index()].is_nil();

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

    fn pick_lowest(&self, hand: &[Card], legal: &[usize]) -> usize {
        let mut best = legal[0];
        let mut best_rank = hand[legal[0]].rank;
        for &i in &legal[1..] {
            if hand[i].rank < best_rank {
                best_rank = hand[i].rank;
                best = i;
            }
        }
        best
    }

    fn pick_lead(&self, hand: &[Card], legal: &[usize]) -> usize {
        // Prefer leading a low card from a non-trump suit
        let mut best = legal[0];
        let mut best_score: i32 = 1000;
        for &i in legal {
            let card = hand[i];
            let mut score = card.rank.0 as i32;
            if card.suit.is_trump() {
                score += 50; // Avoid leading trump
            }
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
                let card = hand[i];
                if !card.beats(winner_card, led_suit)
                    && (best_losing.is_none() || card.rank > best_losing_rank) {
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
                let card = hand[i];
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

    fn score_round(&mut self) {
        self.phase = Phase::RoundOver;

        for team_idx in 0..2 {
            let (p1, p2) = if team_idx == 0 {
                (PlayerId::SOUTH, PlayerId::NORTH)
            } else {
                (PlayerId::EAST, PlayerId::WEST)
            };

            let p1r = &self.player_rounds[p1.index()];
            let p2r = &self.player_rounds[p2.index()];

            let mut round_score: i32 = 0;

            // Handle nil bids individually
            let p1_nil = p1r.is_nil();
            let p2_nil = p2r.is_nil();

            if p1_nil {
                if p1r.tricks_won == 0 {
                    round_score += 100;
                } else {
                    round_score -= 100;
                }
            }
            if p2_nil {
                if p2r.tricks_won == 0 {
                    round_score += 100;
                } else {
                    round_score -= 100;
                }
            }

            // Team bid (excluding nil bidders)
            let team_bid = if p1_nil { 0 } else { p1r.bid_value() as i32 }
                + if p2_nil { 0 } else { p2r.bid_value() as i32 };
            let non_nil_tricks = if p1_nil { 0 } else { p1r.tricks_won as i32 }
                + if p2_nil { 0 } else { p2r.tricks_won as i32 };

            if team_bid > 0 {
                if non_nil_tricks >= team_bid {
                    // Made bid
                    let overtricks = non_nil_tricks - team_bid;
                    round_score += team_bid * 10 + overtricks;
                    // Add bags
                    let new_bags = self.teams[team_idx].bags + overtricks as u32;
                    let bag_penalties = new_bags / 10;
                    let remaining_bags = new_bags % 10;
                    round_score -= (bag_penalties as i32) * 100;
                    self.teams[team_idx].bags = remaining_bags;
                } else {
                    // Set: lose 10 * bid
                    round_score -= team_bid * 10;
                }
            }

            self.teams[team_idx].score += round_score;
        }

        self.check_game_over();
        if self.phase != Phase::GameOver {
            let ns_score = self.teams[0].score;
            let ew_score = self.teams[1].score;
            self.status_message = format!(
                "Round {} over! NS: {} EW: {} (Enter to continue)",
                self.round_number, ns_score, ew_score
            );
        }
    }

    fn check_game_over(&mut self) {
        let ns = self.teams[0].score;
        let ew = self.teams[1].score;

        // Both reach 500: higher score wins
        if ns >= 500 || ew >= 500 {
            if ns >= 500 && ew >= 500 {
                if ns > ew {
                    self.phase = Phase::GameOver;
                    self.winner_message = format!("Your team wins! {} to {}", ns, ew);
                } else if ew > ns {
                    self.phase = Phase::GameOver;
                    self.winner_message = format!("East-West wins! {} to {}", ew, ns);
                } else {
                    // Tie: keep playing
                    return;
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
        self.round_number += 1;
        self.dealer = self.dealer.next();
        self.start_round();
    }

    // ── Human input helpers ─────────────────────────────────────────

    fn clamp_selected_card(&mut self) {
        let hand_len = self.hands[0].len();
        if hand_len == 0 {
            self.selected_card = 0;
        } else if self.selected_card >= hand_len {
            self.selected_card = hand_len - 1;
        }
    }

    fn try_play_selected(&mut self) {
        if self.phase != Phase::Playing || !self.current_player.is_human() {
            return;
        }
        let legal = self.legal_plays(PlayerId::SOUTH);
        if legal.contains(&self.selected_card) {
            self.play_card(PlayerId::SOUTH, self.selected_card);
            self.clamp_selected_card();
        } else {
            self.status_message = "Illegal play! Must follow suit.".to_string();
        }
    }

    // ── Team bid total for a team ───────────────────────────────────

    fn team_bid(&self, team: usize) -> u8 {
        let (p1, p2) = if team == 0 {
            (PlayerId::SOUTH, PlayerId::NORTH)
        } else {
            (PlayerId::EAST, PlayerId::WEST)
        };
        self.player_rounds[p1.index()].bid_value() + self.player_rounds[p2.index()].bid_value()
    }

    fn team_tricks(&self, team: usize) -> u8 {
        let (p1, p2) = if team == 0 {
            (PlayerId::SOUTH, PlayerId::NORTH)
        } else {
            (PlayerId::EAST, PlayerId::WEST)
        };
        self.player_rounds[p1.index()].tricks_won + self.player_rounds[p2.index()].tricks_won
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match self.phase {
            Phase::Bidding => self.handle_key_bidding(event),
            Phase::Playing => self.handle_key_playing(event),
            Phase::TrickDone => self.handle_key_trick_done(event),
            Phase::RoundOver => self.handle_key_round_over(event),
            Phase::GameOver => self.handle_key_game_over(event),
        }
    }

    fn handle_key_bidding(&mut self, event: &KeyEvent) {
        if !self.current_player.is_human() {
            return;
        }
        match event.key {
            Key::Up
                if self.bid_selection < 13 => {
                    self.bid_selection += 1;
                }
            Key::Down
                if self.bid_selection > 0 => {
                    self.bid_selection -= 1;
                }
            Key::Enter | Key::Space => {
                self.submit_human_bid();
            }
            Key::N => {
                self.new_game();
            }
            _ => {}
        }
    }

    fn handle_key_playing(&mut self, event: &KeyEvent) {
        if !self.current_player.is_human() {
            return;
        }
        match event.key {
            Key::Left
                if self.selected_card > 0 => {
                    self.selected_card -= 1;
                }
            Key::Right => {
                let max = self.hands[0].len().saturating_sub(1);
                if self.selected_card < max {
                    self.selected_card += 1;
                }
            }
            Key::Enter | Key::Space => {
                self.try_play_selected();
            }
            Key::H => {
                self.sort_order = self.sort_order.toggle();
                self.sort_all_hands();
                self.clamp_selected_card();
            }
            Key::N => {
                self.new_game();
            }
            _ => {}
        }
    }

    fn handle_key_trick_done(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Enter | Key::Space => {
                self.advance_after_trick();
            }
            Key::N => {
                self.new_game();
            }
            _ => {}
        }
    }

    fn handle_key_round_over(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Enter | Key::Space => {
                self.advance_round();
            }
            Key::N => {
                self.new_game();
            }
            _ => {}
        }
    }

    fn handle_key_game_over(&mut self, event: &KeyEvent) {
        if event.key == Key::N {
            self.new_game();
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            match self.phase {
                Phase::Bidding => self.handle_mouse_bidding(event),
                Phase::Playing => self.handle_mouse_playing(event),
                Phase::TrickDone => {
                    self.advance_after_trick();
                }
                Phase::RoundOver => {
                    self.advance_round();
                }
                Phase::GameOver => {}
            }
        }
    }

    fn handle_mouse_bidding(&mut self, event: &MouseEvent) {
        if !self.current_player.is_human() {
            return;
        }
        // Check if click is on a bid button in the overlay
        let overlay_x = TRICK_CENTER_X - 120.0;
        let overlay_y = TRICK_CENTER_Y - 100.0;

        // Bid number grid: 7 columns x 2 rows (0-6, 7-13)
        let btn_w: f32 = 32.0;
        let btn_h: f32 = 32.0;
        let btn_gap: f32 = 4.0;

        for bid_val in 0..=13u8 {
            let col = (bid_val % 7) as f32;
            let row = (bid_val / 7) as f32;
            let bx = overlay_x + 10.0 + col * (btn_w + btn_gap);
            let by = overlay_y + 50.0 + row * (btn_h + btn_gap);

            if event.x >= bx && event.x < bx + btn_w && event.y >= by && event.y < by + btn_h {
                self.bid_selection = bid_val;
                self.submit_human_bid();
                return;
            }
        }
    }

    fn handle_mouse_playing(&mut self, event: &MouseEvent) {
        if !self.current_player.is_human() {
            return;
        }
        // Check if click is on a card in the hand
        let hand_len = self.hands[0].len();
        if hand_len == 0 {
            return;
        }
        let total_width = (hand_len - 1) as f32 * CARD_SPACING + CARD_W;
        let start_x = TRICK_CENTER_X - total_width / 2.0;

        for i in 0..hand_len {
            let cx = start_x + i as f32 * CARD_SPACING;
            if event.x >= cx
                && event.x < cx + CARD_W
                && event.y >= HAND_Y
                && event.y < HAND_Y + CARD_H
            {
                self.selected_card = i;
                self.try_play_selected();
                return;
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut commands = Vec::with_capacity(512);

        // Background
        commands.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_W,
            height: WINDOW_H,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut commands);
        self.render_trick_area(&mut commands);
        self.render_hand(&mut commands);
        self.render_sidebar(&mut commands);
        self.render_footer(&mut commands);

        if self.phase == Phase::Bidding && self.current_player.is_human() {
            self.render_bid_overlay(&mut commands);
        }

        if self.phase == Phase::GameOver {
            self.render_game_over_overlay(&mut commands);
        }

        commands
    }

    fn render_header(&self, commands: &mut Vec<RenderCommand>) {
        // Title
        commands.push(RenderCommand::Text {
            x: 20.0,
            y: HEADER_Y,
            text: "Spades".to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Round number
        commands.push(RenderCommand::Text {
            x: 120.0,
            y: HEADER_Y + 4.0,
            text: format!("Round {}", self.round_number),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Team scores
        let ns_score = self.teams[0].score;
        let ns_bags = self.teams[0].bags;
        let ew_score = self.teams[1].score;
        let ew_bags = self.teams[1].bags;

        commands.push(RenderCommand::FillRect {
            x: 230.0,
            y: HEADER_Y - 2.0,
            width: 220.0,
            height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        commands.push(RenderCommand::Text {
            x: 240.0,
            y: HEADER_Y + 3.0,
            text: format!("NS: {} (bags: {})", ns_score, ns_bags),
            color: GREEN,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        commands.push(RenderCommand::FillRect {
            x: 470.0,
            y: HEADER_Y - 2.0,
            width: 220.0,
            height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        commands.push(RenderCommand::Text {
            x: 480.0,
            y: HEADER_Y + 3.0,
            text: format!("EW: {} (bags: {})", ew_score, ew_bags),
            color: PEACH,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Trick count vs bid for this round (below scores)
        if self.phase == Phase::Playing || self.phase == Phase::TrickDone {
            let ns_tricks = self.team_tricks(0);
            let ns_bid = self.team_bid(0);
            let ew_tricks = self.team_tricks(1);
            let ew_bid = self.team_bid(1);

            commands.push(RenderCommand::Text {
                x: 240.0,
                y: HEADER_Y + 26.0,
                text: format!("Tricks: {}/{}", ns_tricks, ns_bid),
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            commands.push(RenderCommand::Text {
                x: 480.0,
                y: HEADER_Y + 26.0,
                text: format!("Tricks: {}/{}", ew_tricks, ew_bid),
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_trick_area(&self, commands: &mut Vec<RenderCommand>) {
        // Central trick display area background
        commands.push(RenderCommand::FillRect {
            x: TRICK_CENTER_X - 130.0,
            y: TRICK_CENTER_Y - 100.0,
            width: 260.0,
            height: 200.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Player position labels
        let positions = [
            (TRICK_CENTER_X - 15.0, TRICK_CENTER_Y + 60.0, "S"),
            (TRICK_CENTER_X + 80.0, TRICK_CENTER_Y - 15.0, "E"),
            (TRICK_CENTER_X - 15.0, TRICK_CENTER_Y - 85.0, "N"),
            (TRICK_CENTER_X - 100.0, TRICK_CENTER_Y - 15.0, "W"),
        ];
        for &(px, py, label) in &positions {
            commands.push(RenderCommand::Text {
                x: px,
                y: py,
                text: label.to_string(),
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Render played cards in trick
        let trick_ref = if self.phase == Phase::TrickDone {
            self.last_trick.as_ref().unwrap_or(&self.current_trick)
        } else {
            &self.current_trick
        };

        for &(player, card) in &trick_ref.cards {
            let (cx, cy) = self.trick_card_position(player);
            self.render_card_face(commands, cx, cy, card, false, false);
        }

        // Spades broken indicator
        if self.spades_broken {
            commands.push(RenderCommand::Text {
                x: TRICK_CENTER_X - 125.0,
                y: TRICK_CENTER_Y + 80.0,
                text: "\u{2660} Broken".to_string(),
                color: LAVENDER,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn trick_card_position(&self, player: PlayerId) -> (f32, f32) {
        match player.0 {
            0 => (TRICK_CENTER_X - CARD_W / 2.0, TRICK_CENTER_Y + 10.0), // South
            1 => (TRICK_CENTER_X + 30.0, TRICK_CENTER_Y - CARD_H / 2.0), // East
            2 => (
                TRICK_CENTER_X - CARD_W / 2.0,
                TRICK_CENTER_Y - CARD_H - 10.0,
            ), // North
            3 => (
                TRICK_CENTER_X - CARD_W - 30.0,
                TRICK_CENTER_Y - CARD_H / 2.0,
            ), // West
            _ => (0.0, 0.0),
        }
    }

    fn render_card_face(
        &self,
        commands: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        card: Card,
        selected: bool,
        dimmed: bool,
    ) {
        let bg = if selected {
            SURFACE1
        } else {
            Color::from_hex(0xEEEEEE)
        };
        let border_color = if selected { BLUE } else { OVERLAY0 };

        // Card border
        commands.push(RenderCommand::FillRect {
            x: x - 1.0,
            y: y - 1.0,
            width: CARD_W + 2.0,
            height: CARD_H + 2.0,
            color: border_color,
            corner_radii: CornerRadii::all(5.0),
        });

        // Card background
        let card_bg = if dimmed {
            Color::rgba(bg.r, bg.g, bg.b, 160)
        } else {
            bg
        };
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width: CARD_W,
            height: CARD_H,
            color: card_bg,
            corner_radii: CornerRadii::all(4.0),
        });

        let suit_color = if dimmed { OVERLAY0 } else { card.suit.color() };

        // Rank in top-left
        commands.push(RenderCommand::Text {
            x: x + 4.0,
            y: y + 4.0,
            text: card.rank.label().to_string(),
            color: suit_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Suit symbol center
        commands.push(RenderCommand::Text {
            x: x + CARD_W / 2.0 - 8.0,
            y: y + CARD_H / 2.0 - 8.0,
            text: card.suit.symbol().to_string(),
            color: suit_color,
            font_size: CARD_SUIT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Rank in bottom-right
        commands.push(RenderCommand::Text {
            x: x + CARD_W - 20.0,
            y: y + CARD_H - 22.0,
            text: card.rank.label().to_string(),
            color: suit_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_hand(&self, commands: &mut Vec<RenderCommand>) {
        let hand = &self.hands[0];
        if hand.is_empty() {
            return;
        }

        let legal = if self.phase == Phase::Playing && self.current_player.is_human() {
            self.legal_plays(PlayerId::SOUTH)
        } else {
            Vec::new()
        };

        let total_width = (hand.len() - 1) as f32 * CARD_SPACING + CARD_W;
        let start_x = TRICK_CENTER_X - total_width / 2.0;

        // "Your hand" label
        commands.push(RenderCommand::Text {
            x: start_x,
            y: HAND_Y - 20.0,
            text: "Your Hand".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        for (i, &card) in hand.iter().enumerate() {
            let cx = start_x + i as f32 * CARD_SPACING;
            let is_selected = i == self.selected_card
                && self.phase == Phase::Playing
                && self.current_player.is_human();
            let is_legal = legal.contains(&i);
            let dimmed =
                self.phase == Phase::Playing && self.current_player.is_human() && !is_legal;

            let card_y = if is_selected { HAND_Y - 10.0 } else { HAND_Y };
            self.render_card_face(commands, cx, card_y, card, is_selected, dimmed);
        }
    }

    fn render_sidebar(&self, commands: &mut Vec<RenderCommand>) {
        // Sidebar background
        commands.push(RenderCommand::FillRect {
            x: SIDEBAR_X,
            y: 50.0,
            width: 170.0,
            height: 400.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        commands.push(RenderCommand::Text {
            x: SIDEBAR_X + 10.0,
            y: 60.0,
            text: "Players".to_string(),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Separator
        commands.push(RenderCommand::Line {
            x1: SIDEBAR_X + 10.0,
            y1: 82.0,
            x2: SIDEBAR_X + 160.0,
            y2: 82.0,
            color: SURFACE1,
            width: 1.0,
        });

        let player_order = [
            PlayerId::SOUTH,
            PlayerId::EAST,
            PlayerId::NORTH,
            PlayerId::WEST,
        ];
        for (idx, &pid) in player_order.iter().enumerate() {
            let py = 90.0 + idx as f32 * 90.0;
            let pr = &self.player_rounds[pid.index()];

            // Player name
            let name_color = if pid == self.current_player {
                YELLOW
            } else {
                TEXT_COLOR
            };
            let team_marker = if pid.team() == 0 { " (NS)" } else { " (EW)" };
            commands.push(RenderCommand::Text {
                x: SIDEBAR_X + 10.0,
                y: py,
                text: format!("{}{}", pid.name(), team_marker),
                color: name_color,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Bid
            let bid_text = if let Some(b) = pr.bid {
                if b == 0 {
                    "Nil".to_string()
                } else {
                    format!("Bid: {}", b)
                }
            } else {
                "...".to_string()
            };
            commands.push(RenderCommand::Text {
                x: SIDEBAR_X + 10.0,
                y: py + 18.0,
                text: bid_text,
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Tricks won
            commands.push(RenderCommand::Text {
                x: SIDEBAR_X + 10.0,
                y: py + 34.0,
                text: format!("Tricks: {}", pr.tricks_won),
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Cards remaining
            let cards_left = self.hands[pid.index()].len();
            commands.push(RenderCommand::Text {
                x: SIDEBAR_X + 10.0,
                y: py + 50.0,
                text: format!("Cards: {}", cards_left),
                color: OVERLAY0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_footer(&self, commands: &mut Vec<RenderCommand>) {
        // Status message
        let status_color = match self.phase {
            Phase::GameOver => RED,
            Phase::RoundOver => YELLOW,
            Phase::TrickDone => TEAL,
            _ => SUBTEXT0,
        };
        commands.push(RenderCommand::Text {
            x: 20.0,
            y: FOOTER_Y - 25.0,
            text: self.status_message.clone(),
            color: status_color,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls help
        let controls = match self.phase {
            Phase::Bidding => "\u{2191}/\u{2193}: Adjust bid  Enter: Confirm  N: New game",
            Phase::Playing => "\u{2190}/\u{2192}: Select card  Enter: Play  H: Sort  N: New game",
            Phase::TrickDone => "Enter: Next trick  N: New game",
            Phase::RoundOver => "Enter: Next round  N: New game",
            Phase::GameOver => "N: New game",
        };
        commands.push(RenderCommand::Text {
            x: 20.0,
            y: FOOTER_Y,
            text: controls.to_string(),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_bid_overlay(&self, commands: &mut Vec<RenderCommand>) {
        let overlay_x = TRICK_CENTER_X - 140.0;
        let overlay_y = TRICK_CENTER_Y - 120.0;
        let overlay_w = 280.0;
        let overlay_h = 200.0;

        // Overlay background
        commands.push(RenderCommand::FillRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: Color::rgba(30, 30, 46, 240),
            corner_radii: CornerRadii::all(12.0),
        });

        // Border
        commands.push(RenderCommand::FillRect {
            x: overlay_x - 2.0,
            y: overlay_y - 2.0,
            width: overlay_w + 4.0,
            height: overlay_h + 4.0,
            color: LAVENDER,
            corner_radii: CornerRadii::all(14.0),
        });
        // Inner fill again on top of border
        commands.push(RenderCommand::FillRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: Color::rgba(30, 30, 46, 245),
            corner_radii: CornerRadii::all(12.0),
        });

        // Title
        commands.push(RenderCommand::Text {
            x: overlay_x + 80.0,
            y: overlay_y + 10.0,
            text: "Your Bid".to_string(),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Current selection
        let bid_label = if self.bid_selection == 0 {
            "Nil".to_string()
        } else {
            format!("{}", self.bid_selection)
        };
        commands.push(RenderCommand::Text {
            x: overlay_x + overlay_w / 2.0 - 15.0,
            y: overlay_y + 35.0,
            text: bid_label,
            color: YELLOW,
            font_size: BID_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Bid button grid (0-13)
        let btn_w: f32 = 32.0;
        let btn_h: f32 = 32.0;
        let btn_gap: f32 = 4.0;
        let grid_start_x = overlay_x + 10.0;
        let grid_start_y = overlay_y + 75.0;

        for bid_val in 0..=13u8 {
            let col = (bid_val % 7) as f32;
            let row = (bid_val / 7) as f32;
            let bx = grid_start_x + col * (btn_w + btn_gap);
            let by = grid_start_y + row * (btn_h + btn_gap);

            let bg = if bid_val == self.bid_selection {
                BLUE
            } else {
                SURFACE1
            };
            let fg = if bid_val == self.bid_selection {
                BASE
            } else {
                TEXT_COLOR
            };

            commands.push(RenderCommand::FillRect {
                x: bx,
                y: by,
                width: btn_w,
                height: btn_h,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            let label = if bid_val == 0 {
                "N".to_string()
            } else {
                format!("{}", bid_val)
            };
            commands.push(RenderCommand::Text {
                x: bx + 8.0,
                y: by + 8.0,
                text: label,
                color: fg,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Instructions
        commands.push(RenderCommand::Text {
            x: overlay_x + 20.0,
            y: overlay_y + overlay_h - 30.0,
            text: "Click or \u{2191}/\u{2193} then Enter".to_string(),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Show other players' bids if they've already bid
        let mut already_bid = Vec::new();
        for pid_val in 0..4u8 {
            let pid = PlayerId(pid_val);
            if let Some(b) = self.player_rounds[pid.index()].bid {
                let bid_str = if b == 0 {
                    "Nil".to_string()
                } else {
                    format!("{}", b)
                };
                already_bid.push(format!("{}: {}", pid.name(), bid_str));
            }
        }
        if !already_bid.is_empty() {
            commands.push(RenderCommand::Text {
                x: overlay_x + 20.0,
                y: overlay_y + overlay_h - 15.0,
                text: already_bid.join("  "),
                color: SUBTEXT0,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_game_over_overlay(&self, commands: &mut Vec<RenderCommand>) {
        let overlay_x = TRICK_CENTER_X - 160.0;
        let overlay_y = TRICK_CENTER_Y - 80.0;
        let overlay_w = 320.0;
        let overlay_h = 160.0;

        // Background
        commands.push(RenderCommand::FillRect {
            x: overlay_x - 2.0,
            y: overlay_y - 2.0,
            width: overlay_w + 4.0,
            height: overlay_h + 4.0,
            color: MAUVE,
            corner_radii: CornerRadii::all(14.0),
        });
        commands.push(RenderCommand::FillRect {
            x: overlay_x,
            y: overlay_y,
            width: overlay_w,
            height: overlay_h,
            color: Color::rgba(30, 30, 46, 250),
            corner_radii: CornerRadii::all(12.0),
        });

        // Title
        commands.push(RenderCommand::Text {
            x: overlay_x + 100.0,
            y: overlay_y + 20.0,
            text: "Game Over".to_string(),
            color: MAUVE,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Winner
        commands.push(RenderCommand::Text {
            x: overlay_x + 30.0,
            y: overlay_y + 60.0,
            text: self.winner_message.clone(),
            color: YELLOW,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Instructions
        commands.push(RenderCommand::Text {
            x: overlay_x + 80.0,
            y: overlay_y + 110.0,
            text: "Press N for new game".to_string(),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn main() {
    let _app = SpadesGame::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RNG tests ───────────────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = Rng::new(42);
        let mut rng2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = Rng::new(1);
        let mut rng2 = Rng::new(2);
        // Should differ within first few values
        let differ = (0..10).any(|_| rng1.next_u64() != rng2.next_u64());
        assert!(differ);
    }

    #[test]
    fn test_rng_next_range() {
        let mut rng = Rng::new(99);
        for _ in 0..1000 {
            let val = rng.next_range(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_rng_next_range_zero() {
        let mut rng = Rng::new(42);
        assert_eq!(rng.next_range(0), 0);
    }

    #[test]
    fn test_rng_shuffle_preserves_elements() {
        let mut rng = Rng::new(42);
        let mut arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        rng.shuffle(&mut arr);
        arr.sort();
        assert_eq!(arr, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_rng_shuffle_actually_shuffles() {
        let mut rng = Rng::new(42);
        let original = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut arr = original;
        rng.shuffle(&mut arr);
        // Extremely unlikely to remain in order with 10 elements
        assert_ne!(arr, original);
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
        let trick = Trick::new(PlayerId::SOUTH);
        assert_eq!(trick.leader, PlayerId::SOUTH);
        assert!(trick.cards.is_empty());
        assert!(!trick.is_complete());
    }

    #[test]
    fn test_trick_led_suit() {
        let mut trick = Trick::new(PlayerId::SOUTH);
        assert_eq!(trick.led_suit(), None);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        assert_eq!(trick.led_suit(), Some(Suit::Hearts));
    }

    #[test]
    fn test_trick_is_complete() {
        let mut trick = Trick::new(PlayerId::SOUTH);
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
        let mut trick = Trick::new(PlayerId::SOUTH);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::FIVE));
        trick.add(PlayerId::EAST, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::NORTH, Card::new(Suit::Hearts, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::TWO));
        assert_eq!(trick.winner(), Some(PlayerId::EAST));
    }

    #[test]
    fn test_trick_winner_trump_beats_all() {
        let mut trick = Trick::new(PlayerId::SOUTH);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::EAST, Card::new(Suit::Spades, Rank::TWO));
        trick.add(PlayerId::NORTH, Card::new(Suit::Hearts, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::QUEEN));
        assert_eq!(trick.winner(), Some(PlayerId::EAST));
    }

    #[test]
    fn test_trick_winner_highest_trump_wins() {
        let mut trick = Trick::new(PlayerId::SOUTH);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        trick.add(PlayerId::EAST, Card::new(Suit::Spades, Rank::TWO));
        trick.add(PlayerId::NORTH, Card::new(Suit::Spades, Rank::KING));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::QUEEN));
        assert_eq!(trick.winner(), Some(PlayerId::NORTH));
    }

    #[test]
    fn test_trick_winner_off_suit_loses_to_led() {
        let mut trick = Trick::new(PlayerId::SOUTH);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::TWO));
        trick.add(PlayerId::EAST, Card::new(Suit::Clubs, Rank::ACE));
        trick.add(PlayerId::NORTH, Card::new(Suit::Diamonds, Rank::ACE));
        trick.add(PlayerId::WEST, Card::new(Suit::Hearts, Rank::THREE));
        assert_eq!(trick.winner(), Some(PlayerId::WEST));
    }

    #[test]
    fn test_trick_contains_spade() {
        let mut trick = Trick::new(PlayerId::SOUTH);
        trick.add(PlayerId::SOUTH, Card::new(Suit::Hearts, Rank::ACE));
        assert!(!trick.contains_spade());
        trick.add(PlayerId::EAST, Card::new(Suit::Spades, Rank::TWO));
        assert!(trick.contains_spade());
    }

    #[test]
    fn test_trick_winner_empty() {
        let trick = Trick::new(PlayerId::SOUTH);
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
            assert!((1..=6).contains(&bid), "AI bid {} out of expected range", bid);
        }
    }

    #[test]
    fn test_submit_bid_advances_phase() {
        let mut game = SpadesGame::new();
        // AI bids should have run for players before human
        game.bid_selection = 4;
        game.submit_human_bid();
        // After all bids submitted, should be Playing phase
        assert_eq!(game.phase, Phase::Playing);
    }

    #[test]
    fn test_all_bids_set_after_bidding() {
        let mut game = SpadesGame::new();
        game.bid_selection = 3;
        game.submit_human_bid();
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
        game.current_trick = Trick::new(PlayerId::EAST);
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
        game.current_trick = Trick::new(PlayerId::EAST);
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
        game.current_trick = Trick::new(PlayerId::SOUTH);

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
        game.current_trick = Trick::new(PlayerId::SOUTH);

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
        game.current_trick = Trick::new(PlayerId::SOUTH);

        let legal = game.legal_plays(PlayerId::SOUTH);
        assert_eq!(legal.len(), 2); // Forced to lead spade
    }

    #[test]
    fn test_legal_plays_empty_hand() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.hands[0] = Vec::new();
        game.current_trick = Trick::new(PlayerId::SOUTH);

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
        game.current_trick = Trick::new(PlayerId::SOUTH);
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
        game.current_trick = Trick::new(PlayerId::EAST);
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
        game.handle_key(&KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.bid_selection, 4);
    }

    #[test]
    fn test_bid_down_decreases() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 3;
        game.handle_key(&KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.bid_selection, 2);
    }

    #[test]
    fn test_bid_down_clamp_at_zero() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 0;
        game.handle_key(&KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.bid_selection, 0);
    }

    #[test]
    fn test_bid_up_clamp_at_13() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 13;
        game.handle_key(&KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.bid_selection, 13);
    }

    #[test]
    fn test_key_not_pressed_ignored() {
        let mut game = SpadesGame::new();
        game.current_player = PlayerId::SOUTH;
        game.bid_selection = 5;
        game.handle_key(&KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: false,
            text: None,
        });
        assert_eq!(game.bid_selection, 5);
    }

    #[test]
    fn test_card_navigation_right() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.selected_card = 0;
        game.handle_key(&KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_card, 1);
    }

    #[test]
    fn test_card_navigation_left_at_zero() {
        let mut game = SpadesGame::new();
        game.phase = Phase::Playing;
        game.current_player = PlayerId::SOUTH;
        game.selected_card = 0;
        game.handle_key(&KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_card, 0);
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_returns_commands() {
        let game = SpadesGame::new();
        let commands = game.render();
        assert!(!commands.is_empty());
    }

    #[test]
    fn test_render_bidding_overlay() {
        let game = SpadesGame::new();
        assert_eq!(game.phase, Phase::Bidding);
        let commands = game.render();
        // Should have bid overlay elements
        assert!(commands.len() > 20);
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut game = SpadesGame::new();
        game.phase = Phase::GameOver;
        game.winner_message = "Test winner".to_string();
        let commands = game.render();
        assert!(commands.len() > 10);
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

    #[test]
    fn test_player_position_labels() {
        assert_eq!(PlayerId::SOUTH.position_label(), "South");
        assert_eq!(PlayerId::EAST.position_label(), "East");
        assert_eq!(PlayerId::NORTH.position_label(), "North");
        assert_eq!(PlayerId::WEST.position_label(), "West");
    }

    // ── Card face rendering tests ───────────────────────────────────

    #[test]
    fn test_render_card_face_produces_commands() {
        let game = SpadesGame::new();
        let mut commands = Vec::new();
        game.render_card_face(
            &mut commands,
            0.0,
            0.0,
            Card::new(Suit::Spades, Rank::ACE),
            false,
            false,
        );
        // Should produce: border rect, bg rect, rank text, suit text, bottom rank text
        assert!(commands.len() >= 5);
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
        game.current_trick = Trick::new(PlayerId::SOUTH);
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
