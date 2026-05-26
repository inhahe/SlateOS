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
#![allow(unused_imports)]

//! OurOS Hearts -- a classic four-player Hearts card game.
//!
//! Features:
//! - 4 players (1 human, 3 AI) playing standard Hearts rules
//! - Card passing phase (left, right, across, keep -- cycles each round)
//! - Trick-taking with suit-following and hearts-breaking rules
//! - 2 of clubs leads the first trick
//! - Queen of Spades worth 13 points, each heart worth 1 point
//! - Shooting the moon: collect all 26 points to give 26 to each opponent
//! - Game ends at 100 points, lowest score wins
//! - Keyboard and mouse controls
//! - Catppuccin Mocha themed UI

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// -- Catppuccin Mocha palette ------------------------------------------------
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// -- Layout constants --------------------------------------------------------
const CARD_WIDTH: f32 = 60.0;
const CARD_HEIGHT: f32 = 84.0;
const CARD_OVERLAP: f32 = 38.0;
const HAND_Y: f32 = 480.0;
const HAND_X_START: f32 = 60.0;
const TRICK_CENTER_X: f32 = 400.0;
const TRICK_CENTER_Y: f32 = 260.0;
const TRICK_SPREAD: f32 = 80.0;
const SCORE_X: f32 = 700.0;
const SCORE_Y: f32 = 60.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const CARD_FONT_SIZE: f32 = 16.0;
const CARD_SUIT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 14.0;
const SMALL_FONT_SIZE: f32 = 12.0;
const CARD_CORNER_RADIUS: f32 = 6.0;
const PASS_LABEL_Y: f32 = 440.0;
const STATUS_Y: f32 = 580.0;

// -- Card data types ---------------------------------------------------------

/// Suit of a playing card.
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

    const fn color(self) -> Color {
        match self {
            Suit::Clubs | Suit::Spades => TEXT_COLOR,
            Suit::Diamonds | Suit::Hearts => RED,
        }
    }

    const fn index(self) -> usize {
        match self {
            Suit::Clubs => 0,
            Suit::Diamonds => 1,
            Suit::Spades => 2,
            Suit::Hearts => 3,
        }
    }
}

/// Rank of a playing card (2 through Ace).
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

    /// Numeric comparison value for trick-winning (higher wins).
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

/// A playing card with a suit and rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Card {
    suit: Suit,
    rank: Rank,
}

impl Card {
    const fn new(suit: Suit, rank: Rank) -> Self {
        Self { suit, rank }
    }

    /// Whether this card is the Queen of Spades.
    const fn is_queen_of_spades(self) -> bool {
        matches!(self.suit, Suit::Spades) && matches!(self.rank, Rank::Queen)
    }

    /// Whether this card is the 2 of clubs.
    const fn is_two_of_clubs(self) -> bool {
        matches!(self.suit, Suit::Clubs) && matches!(self.rank, Rank::Two)
    }

    /// Whether this is a heart.
    const fn is_heart(self) -> bool {
        matches!(self.suit, Suit::Hearts)
    }

    /// Point value of this card in Hearts scoring.
    const fn point_value(self) -> u32 {
        if self.is_queen_of_spades() {
            13
        } else if self.is_heart() {
            1
        } else {
            0
        }
    }

    /// Sorting key: by suit then rank.
    const fn sort_key(self) -> u8 {
        self.suit.index() as u8 * 13 + self.rank.value()
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

// -- Deck --------------------------------------------------------------------

/// Create a standard 52-card deck.
fn new_deck() -> Vec<Card> {
    let mut deck = Vec::with_capacity(52);
    for &suit in &Suit::ALL {
        for &rank in &Rank::ALL {
            deck.push(Card::new(suit, rank));
        }
    }
    deck
}

// -- RNG ---------------------------------------------------------------------

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_range(&mut self, max: u64) -> u64 {
        self.next() % max
    }

    /// Fisher-Yates shuffle.
    fn shuffle<T>(&mut self, items: &mut [T]) {
        let len = items.len();
        for i in (1..len).rev() {
            let j = self.next_range((i + 1) as u64) as usize;
            items.swap(i, j);
        }
    }
}

// -- Pass direction ----------------------------------------------------------

/// Direction for card passing at the start of each round.
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
            PassDirection::Left => "Pass Left",
            PassDirection::Right => "Pass Right",
            PassDirection::Across => "Pass Across",
            PassDirection::Keep => "No Passing",
        }
    }

    /// Cycle through pass directions for successive rounds.
    const fn next(self) -> PassDirection {
        match self {
            PassDirection::Left => PassDirection::Right,
            PassDirection::Right => PassDirection::Across,
            PassDirection::Across => PassDirection::Keep,
            PassDirection::Keep => PassDirection::Left,
        }
    }

    /// Target player index given a source player index (0-3).
    const fn target(self, from: usize) -> usize {
        match self {
            PassDirection::Left => (from + 1) % 4,
            PassDirection::Right => (from + 3) % 4,
            PassDirection::Across => (from + 2) % 4,
            PassDirection::Keep => from,
        }
    }
}

// -- Game phase --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    /// Select 3 cards to pass.
    Passing,
    /// Playing tricks.
    Playing,
    /// Round over, showing scores before next round.
    RoundOver,
    /// Game over (someone reached 100 points).
    GameOver,
}

// -- Trick -------------------------------------------------------------------

/// One played card in a trick, tracking who played it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrickCard {
    player: usize,
    card: Card,
}

/// A trick being built up from 4 plays.
#[derive(Debug, Clone)]
struct Trick {
    cards: Vec<TrickCard>,
    lead_suit: Option<Suit>,
}

impl Trick {
    fn new() -> Self {
        Self {
            cards: Vec::with_capacity(4),
            lead_suit: None,
        }
    }

    fn play(&mut self, player: usize, card: Card) {
        if self.cards.is_empty() {
            self.lead_suit = Some(card.suit);
        }
        self.cards.push(TrickCard { player, card });
    }

    fn is_complete(&self) -> bool {
        self.cards.len() == 4
    }

    /// Determine the winner of a completed trick: highest card of the lead suit.
    fn winner(&self) -> Option<usize> {
        let lead = self.lead_suit?;
        self.cards
            .iter()
            .filter(|tc| tc.card.suit == lead)
            .max_by_key(|tc| tc.card.rank.value())
            .map(|tc| tc.player)
    }

    /// Total points in this trick.
    fn points(&self) -> u32 {
        self.cards.iter().map(|tc| tc.card.point_value()).sum()
    }
}

// -- Hearts game state -------------------------------------------------------

struct Hearts {
    /// Hands for each of the 4 players. Player 0 is the human.
    hands: [Vec<Card>; 4],
    /// Cards selected for passing (human only).
    pass_selections: Vec<usize>,
    /// Current trick being played.
    current_trick: Trick,
    /// Completed tricks this round (for scoring).
    completed_tricks: Vec<Trick>,
    /// Points taken this round by each player.
    round_points: [u32; 4],
    /// Cumulative scores across rounds.
    scores: [u32; 4],
    /// Which player leads / is currently to play.
    current_player: usize,
    /// Whether hearts have been broken this round.
    hearts_broken: bool,
    /// Current trick number (0-based, 0..13).
    trick_number: usize,
    /// Game phase.
    phase: GamePhase,
    /// Pass direction for the current round.
    pass_direction: PassDirection,
    /// Round number (0-based).
    round_number: usize,
    /// Selected card index in the human hand (for keyboard nav).
    selected_index: usize,
    /// Status message shown at bottom.
    status: String,
    /// RNG for shuffling.
    rng: Rng,
    /// Player names.
    names: [&'static str; 4],
    /// The last completed trick (for display).
    last_trick: Option<Trick>,
    /// Winner of the game (player index), if game is over.
    winner: Option<usize>,
}

impl Hearts {
    fn new() -> Self {
        let mut game = Self {
            hands: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            pass_selections: Vec::new(),
            current_trick: Trick::new(),
            completed_tricks: Vec::new(),
            round_points: [0; 4],
            scores: [0; 4],
            current_player: 0,
            hearts_broken: false,
            trick_number: 0,
            phase: GamePhase::Passing,
            pass_direction: PassDirection::Left,
            round_number: 0,
            selected_index: 0,
            status: String::new(),
            rng: Rng::new(42),
            names: ["You", "West", "North", "East"],
            last_trick: None,
            winner: None,
        };
        game.start_round();
        game
    }

    /// Start a new round: shuffle, deal, set up passing phase.
    fn start_round(&mut self) {
        let mut deck = new_deck();
        self.rng.shuffle(&mut deck);

        // Deal 13 cards to each player.
        for i in 0..4 {
            self.hands[i] = deck[i * 13..(i + 1) * 13].to_vec();
            sort_hand(&mut self.hands[i]);
        }

        self.pass_selections.clear();
        self.current_trick = Trick::new();
        self.completed_tricks.clear();
        self.round_points = [0; 4];
        self.hearts_broken = false;
        self.trick_number = 0;
        self.last_trick = None;

        if self.pass_direction == PassDirection::Keep {
            self.phase = GamePhase::Playing;
            self.current_player = self.find_two_of_clubs_holder();
            self.status = format!(
                "{} leads with 2\u{2663}",
                self.names[self.current_player]
            );
            if self.current_player != 0 {
                self.play_ai_turns();
            }
        } else {
            self.phase = GamePhase::Passing;
            self.status = format!(
                "Select 3 cards to pass ({})",
                self.pass_direction.label()
            );
        }

        self.clamp_selected_index();
    }

    /// Start a completely new game.
    fn new_game(&mut self) {
        self.scores = [0; 4];
        self.round_number = 0;
        self.pass_direction = PassDirection::Left;
        self.winner = None;
        self.phase = GamePhase::Passing;
        self.start_round();
    }

    /// Find which player holds the 2 of clubs.
    fn find_two_of_clubs_holder(&self) -> usize {
        for (i, hand) in self.hands.iter().enumerate() {
            if hand.iter().any(|c| c.is_two_of_clubs()) {
                return i;
            }
        }
        0
    }

    /// Get valid cards the given player can play.
    fn valid_plays(&self, player: usize) -> Vec<usize> {
        let hand = &self.hands[player];
        if hand.is_empty() {
            return Vec::new();
        }

        let mut valid = Vec::new();

        // First trick: must lead 2 of clubs
        if self.trick_number == 0 && self.current_trick.cards.is_empty() {
            for (i, card) in hand.iter().enumerate() {
                if card.is_two_of_clubs() {
                    return vec![i];
                }
            }
        }

        // Must follow lead suit if possible
        if let Some(lead_suit) = self.current_trick.lead_suit {
            let has_suit = hand.iter().any(|c| c.suit == lead_suit);
            if has_suit {
                for (i, card) in hand.iter().enumerate() {
                    if card.suit == lead_suit {
                        valid.push(i);
                    }
                }
                return valid;
            }
            // Can't follow suit: play anything, but on trick 0 no hearts or QoS
            if self.trick_number == 0 {
                // No point cards on first trick (unless hand is all point cards)
                let non_point: Vec<usize> = hand
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.point_value() == 0)
                    .map(|(i, _)| i)
                    .collect();
                if !non_point.is_empty() {
                    return non_point;
                }
            }
            // Play anything
            return (0..hand.len()).collect();
        }

        // Leading a trick
        if !self.hearts_broken {
            // Can't lead hearts unless hearts are broken or only hearts left
            let non_hearts: Vec<usize> = hand
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_heart())
                .map(|(i, _)| i)
                .collect();
            if !non_hearts.is_empty() {
                return non_hearts;
            }
        }

        // All cards are valid
        (0..hand.len()).collect()
    }

    /// Play a card for the given player.
    fn play_card(&mut self, player: usize, card_index: usize) {
        let card = self.hands[player].remove(card_index);

        // Break hearts if a heart is played
        if card.is_heart() {
            self.hearts_broken = true;
        }

        self.current_trick.play(player, card);

        if self.current_trick.is_complete() {
            self.finish_trick();
        } else {
            self.current_player = (self.current_player + 1) % 4;
        }
    }

    /// Finish the current trick: assign points, set up next trick or end round.
    fn finish_trick(&mut self) {
        if let Some(winner) = self.current_trick.winner() {
            let pts = self.current_trick.points();
            self.round_points[winner] += pts;
            self.current_player = winner;

            self.status = format!(
                "{} wins the trick ({} pts)",
                self.names[winner], pts
            );

            self.last_trick = Some(core::mem::replace(
                &mut self.current_trick,
                Trick::new(),
            ));
            let last = self.last_trick.as_ref().unwrap();
            self.completed_tricks.push(last.clone());

            self.trick_number += 1;

            if self.trick_number >= 13 {
                self.end_round();
            }
        }
    }

    /// End the current round: tally scores, check for game over.
    fn end_round(&mut self) {
        // Check for shooting the moon
        let moon_shooter = self.check_shoot_the_moon();
        if let Some(shooter) = moon_shooter {
            // Give 26 points to everyone else
            for i in 0..4 {
                if i != shooter {
                    self.scores[i] += 26;
                }
            }
            self.status = format!(
                "{} shot the moon! 26 points to everyone else!",
                self.names[shooter]
            );
        } else {
            for i in 0..4 {
                self.scores[i] += self.round_points[i];
            }
            self.status = "Round over!".to_string();
        }

        // Check for game over
        let max_score = self.scores.iter().copied().max().unwrap_or(0);
        if max_score >= 100 {
            self.phase = GamePhase::GameOver;
            let min_score = self.scores.iter().copied().min().unwrap_or(0);
            self.winner = self.scores.iter().position(|&s| s == min_score);
            if let Some(w) = self.winner {
                self.status = format!("{} wins the game!", self.names[w]);
            }
        } else {
            self.phase = GamePhase::RoundOver;
        }

        self.round_number += 1;
        self.pass_direction = self.pass_direction.next();
    }

    /// Check if any player took all 26 points (shot the moon).
    fn check_shoot_the_moon(&self) -> Option<usize> {
        for i in 0..4 {
            if self.round_points[i] == 26 {
                return Some(i);
            }
        }
        None
    }

    /// Handle card passing for the human player.
    fn toggle_pass_selection(&mut self, card_index: usize) {
        if card_index >= self.hands[0].len() {
            return;
        }
        if let Some(pos) = self.pass_selections.iter().position(|&i| i == card_index) {
            self.pass_selections.remove(pos);
        } else if self.pass_selections.len() < 3 {
            self.pass_selections.push(card_index);
        }

        let count = self.pass_selections.len();
        if count < 3 {
            self.status = format!(
                "Select {} more card{} to pass ({})",
                3 - count,
                if count == 2 { "" } else { "s" },
                self.pass_direction.label()
            );
        } else {
            self.status = "Press Enter to confirm pass".to_string();
        }
    }

    /// Execute the passing phase.
    fn execute_pass(&mut self) {
        if self.pass_selections.len() != 3 {
            return;
        }
        if self.pass_direction == PassDirection::Keep {
            self.phase = GamePhase::Playing;
            return;
        }

        // Collect cards to pass from all players
        let mut pass_cards: [Vec<Card>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

        // Human passes selected cards
        let mut human_indices: Vec<usize> = self.pass_selections.clone();
        human_indices.sort_unstable();
        human_indices.reverse();
        for &idx in &human_indices {
            pass_cards[0].push(self.hands[0].remove(idx));
        }

        // AI selects cards to pass (simple strategy: pass highest point cards first)
        for p in 1..4 {
            let ai_pass = self.ai_select_pass_cards(p);
            let mut indices = ai_pass;
            indices.sort_unstable();
            indices.reverse();
            for &idx in &indices {
                pass_cards[p].push(self.hands[p].remove(idx));
            }
        }

        // Distribute passed cards to targets
        for from in 0..4 {
            let target = self.pass_direction.target(from);
            let cards = pass_cards[from].clone();
            for card in cards {
                self.hands[target].push(card);
            }
        }

        // Sort all hands
        for hand in &mut self.hands {
            sort_hand(hand);
        }

        self.pass_selections.clear();
        self.phase = GamePhase::Playing;
        self.current_player = self.find_two_of_clubs_holder();
        self.status = format!(
            "{} leads with 2\u{2663}",
            self.names[self.current_player]
        );

        self.clamp_selected_index();

        // Let AI play if they lead
        if self.current_player != 0 {
            self.play_ai_turns();
        }
    }

    /// AI selects 3 cards to pass. Strategy: pass high spades (especially QoS),
    /// high hearts, then highest cards of any suit.
    fn ai_select_pass_cards(&mut self, player: usize) -> Vec<usize> {
        let hand = &self.hands[player];
        let mut indices: Vec<usize> = (0..hand.len()).collect();

        // Sort by "desire to pass" -- high-point, high-rank cards first
        indices.sort_by(|&a, &b| {
            let ca = hand[a];
            let cb = hand[b];
            // Prefer passing QoS
            let qa = if ca.is_queen_of_spades() { 100 } else { 0 };
            let qb = if cb.is_queen_of_spades() { 100 } else { 0 };
            // Prefer passing high spades (K, A of spades)
            let sa = if ca.suit == Suit::Spades && ca.rank.value() >= 13 {
                50
            } else {
                0
            };
            let sb = if cb.suit == Suit::Spades && cb.rank.value() >= 13 {
                50
            } else {
                0
            };
            // Prefer passing hearts
            let ha = if ca.is_heart() { ca.rank.value() as i32 } else { 0 };
            let hb = if cb.is_heart() { cb.rank.value() as i32 } else { 0 };
            let score_a = qa + sa + ha;
            let score_b = qb + sb + hb;
            score_b.cmp(&score_a)
        });

        indices.truncate(3);
        indices
    }

    /// Play AI turns until it's the human's turn or the trick/round ends.
    fn play_ai_turns(&mut self) {
        while self.phase == GamePhase::Playing
            && self.current_player != 0
            && !self.hands[self.current_player].is_empty()
        {
            let player = self.current_player;
            let card_idx = self.ai_choose_card(player);
            self.play_card(player, card_idx);
        }
        self.clamp_selected_index();
    }

    /// AI card selection strategy.
    fn ai_choose_card(&mut self, player: usize) -> usize {
        let valid = self.valid_plays(player);
        if valid.len() == 1 {
            return valid[0];
        }

        let hand = &self.hands[player];
        let is_leading = self.current_trick.cards.is_empty();

        if is_leading {
            // When leading: play lowest non-heart, non-spade card if possible
            let mut best = valid[0];
            let mut best_val = u16::MAX;
            for &i in &valid {
                let c = hand[i];
                let penalty = if c.is_heart() || c.suit == Suit::Spades {
                    100
                } else {
                    0
                };
                let val = penalty as u16 + c.rank.value() as u16;
                if val < best_val {
                    best_val = val;
                    best = i;
                }
            }
            return best;
        }

        let lead_suit = self.current_trick.lead_suit.unwrap_or(Suit::Clubs);
        let following_suit = hand.get(valid[0]).map_or(false, |c| c.suit == lead_suit);

        if following_suit {
            // Following suit: try to play just below the current highest
            let current_high = self
                .current_trick
                .cards
                .iter()
                .filter(|tc| tc.card.suit == lead_suit)
                .map(|tc| tc.card.rank.value())
                .max()
                .unwrap_or(0);

            // Play highest card that's still below the current winner
            let mut best = valid[0];
            let mut best_val = 0u8;
            let mut found_below = false;
            for &i in &valid {
                let v = hand[i].rank.value();
                if v < current_high {
                    if !found_below || v > best_val {
                        best_val = v;
                        best = i;
                        found_below = true;
                    }
                }
            }
            if found_below {
                return best;
            }
            // Must go higher; play lowest
            let mut lowest_i = valid[0];
            let mut lowest_v = u8::MAX;
            for &i in &valid {
                if hand[i].rank.value() < lowest_v {
                    lowest_v = hand[i].rank.value();
                    lowest_i = i;
                }
            }
            return lowest_i;
        }

        // Not following suit: dump QoS if possible, then highest heart, then highest card
        // Try QoS first
        for &i in &valid {
            if hand[i].is_queen_of_spades() {
                return i;
            }
        }
        // Try highest heart
        let mut best_heart: Option<usize> = None;
        let mut best_heart_val = 0u8;
        for &i in &valid {
            if hand[i].is_heart() && hand[i].rank.value() > best_heart_val {
                best_heart_val = hand[i].rank.value();
                best_heart = Some(i);
            }
        }
        if let Some(i) = best_heart {
            return i;
        }
        // Play highest card to void a suit
        let mut highest_i = valid[0];
        let mut highest_v = 0u8;
        for &i in &valid {
            if hand[i].rank.value() > highest_v {
                highest_v = hand[i].rank.value();
                highest_i = i;
            }
        }
        highest_i
    }

    /// Human plays the currently selected card (if valid).
    fn human_play_selected(&mut self) {
        if self.phase != GamePhase::Playing || self.current_player != 0 {
            return;
        }
        let valid = self.valid_plays(0);
        if valid.contains(&self.selected_index) {
            self.play_card(0, self.selected_index);
            self.clamp_selected_index();

            // Continue with AI turns
            if self.phase == GamePhase::Playing && self.current_player != 0 {
                self.play_ai_turns();
            }
        } else {
            self.status = "Invalid card. Must follow suit or play a valid card.".to_string();
        }
    }

    /// Start the next round (called from RoundOver phase).
    fn advance_to_next_round(&mut self) {
        self.start_round();
    }

    /// Ensure selected_index is within bounds.
    fn clamp_selected_index(&mut self) {
        let len = self.hands[0].len();
        if len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= len {
            self.selected_index = len - 1;
        }
    }

    // -- Event handling ------------------------------------------------------

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

        match event.key {
            Key::N => {
                self.new_game();
            }
            Key::Left => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
            Key::Right => {
                let max = if self.hands[0].is_empty() {
                    0
                } else {
                    self.hands[0].len() - 1
                };
                if self.selected_index < max {
                    self.selected_index += 1;
                }
            }
            Key::Enter | Key::Space => {
                match self.phase {
                    GamePhase::Passing => {
                        if self.pass_selections.len() == 3 {
                            self.execute_pass();
                        } else {
                            self.toggle_pass_selection(self.selected_index);
                        }
                    }
                    GamePhase::Playing => {
                        self.human_play_selected();
                    }
                    GamePhase::RoundOver => {
                        self.advance_to_next_round();
                    }
                    GamePhase::GameOver => {
                        self.new_game();
                    }
                }
            }
            Key::Escape => {
                if self.phase == GamePhase::Passing && !self.pass_selections.is_empty() {
                    self.pass_selections.clear();
                    self.status = format!(
                        "Select 3 cards to pass ({})",
                        self.pass_direction.label()
                    );
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind {
            // Check if clicking on a card in the human hand
            let hand_len = self.hands[0].len();
            if hand_len == 0 {
                return;
            }

            let mx = event.x;
            let my = event.y;

            // Check card area
            if my >= HAND_Y && my <= HAND_Y + CARD_HEIGHT {
                for i in (0..hand_len).rev() {
                    let cx = HAND_X_START + i as f32 * CARD_OVERLAP;
                    let cw = if i == hand_len - 1 {
                        CARD_WIDTH
                    } else {
                        CARD_OVERLAP
                    };
                    if mx >= cx && mx < cx + cw {
                        self.selected_index = i;
                        match self.phase {
                            GamePhase::Passing => {
                                self.toggle_pass_selection(i);
                            }
                            GamePhase::Playing => {
                                self.human_play_selected();
                            }
                            _ => {}
                        }
                        break;
                    }
                }
            }
        }
    }

    // -- Rendering -----------------------------------------------------------

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: 24.0,
            text: "Hearts".to_string(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Round/phase info
        let phase_label = match self.phase {
            GamePhase::Passing => self.pass_direction.label().to_string(),
            GamePhase::Playing => format!("Trick {}/13", self.trick_number + 1),
            GamePhase::RoundOver => "Round Over".to_string(),
            GamePhase::GameOver => "Game Over".to_string(),
        };
        cmds.push(RenderCommand::Text {
            x: 160.0,
            y: 28.0,
            text: format!("Round {} - {}", self.round_number + 1, phase_label),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Render table (trick area)
        self.render_trick_area(&mut cmds);

        // Render human hand
        self.render_hand(&mut cmds);

        // Render opponent labels
        self.render_opponent_labels(&mut cmds);

        // Render scoreboard
        self.render_scores(&mut cmds, width);

        // Status bar
        self.render_status(&mut cmds, width, height);

        // Hearts broken indicator
        if self.hearts_broken {
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: 52.0,
                text: "\u{2665} Hearts Broken".to_string(),
                color: RED,
                font_size: SMALL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Controls hint
        let hint = match self.phase {
            GamePhase::Passing => "\u{2190}\u{2192} Select | Enter Toggle/Confirm | Esc Clear | N New Game",
            GamePhase::Playing => "\u{2190}\u{2192} Select | Enter Play | N New Game",
            GamePhase::RoundOver => "Enter Next Round | N New Game",
            GamePhase::GameOver => "Enter/N New Game",
        };
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: height - 14.0,
            text: hint.to_string(),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }

    /// Render the trick area in the center of the table.
    fn render_trick_area(&self, cmds: &mut Vec<RenderCommand>) {
        // Green felt background for the play area
        cmds.push(RenderCommand::FillRect {
            x: TRICK_CENTER_X - 130.0,
            y: TRICK_CENTER_Y - 100.0,
            width: 260.0,
            height: 240.0,
            color: Color::from_hex(0x1B3D2F),
            corner_radii: CornerRadii::all(12.0),
        });

        // Render cards in the current trick
        let offsets: [(f32, f32); 4] = [
            (0.0, 50.0),    // South (human)
            (-80.0, 0.0),   // West
            (0.0, -50.0),   // North
            (80.0, 0.0),    // East
        ];

        for tc in &self.current_trick.cards {
            let (ox, oy) = offsets[tc.player];
            let cx = TRICK_CENTER_X + ox - CARD_WIDTH / 2.0;
            let cy = TRICK_CENTER_Y + oy - CARD_HEIGHT / 2.0;
            render_card(cmds, cx, cy, tc.card, false, false);
        }

        // If trick is waiting to be cleared, show last trick dimmed
        if self.current_trick.cards.is_empty() {
            if let Some(ref last) = self.last_trick {
                if self.trick_number > 0 && self.trick_number <= 13 {
                    for tc in &last.cards {
                        let (ox, oy) = offsets[tc.player];
                        let cx = TRICK_CENTER_X + ox - CARD_WIDTH / 2.0;
                        let cy = TRICK_CENTER_Y + oy - CARD_HEIGHT / 2.0;
                        // Dim the last trick cards
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: CARD_WIDTH,
                            height: CARD_HEIGHT,
                            color: Color::rgba(49, 50, 68, 180),
                            corner_radii: CornerRadii::all(CARD_CORNER_RADIUS),
                        });
                    }
                }
            }
        }
    }

    /// Render the human player's hand at the bottom.
    fn render_hand(&self, cmds: &mut Vec<RenderCommand>) {
        let hand = &self.hands[0];
        let valid_plays = if self.phase == GamePhase::Playing && self.current_player == 0 {
            self.valid_plays(0)
        } else {
            Vec::new()
        };

        for (i, &card) in hand.iter().enumerate() {
            let x = HAND_X_START + i as f32 * CARD_OVERLAP;
            let is_selected = i == self.selected_index;
            let is_pass_selected = self.phase == GamePhase::Passing
                && self.pass_selections.contains(&i);
            let is_valid = self.phase == GamePhase::Playing && valid_plays.contains(&i);

            let y = if is_pass_selected {
                HAND_Y - 16.0
            } else {
                HAND_Y
            };

            render_card(cmds, x, y, card, is_selected, is_pass_selected);

            // Dim invalid cards during play phase
            if self.phase == GamePhase::Playing && self.current_player == 0 && !is_valid {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: CARD_WIDTH,
                    height: CARD_HEIGHT,
                    color: Color::rgba(30, 30, 46, 150),
                    corner_radii: CornerRadii::all(CARD_CORNER_RADIUS),
                });
            }

            // Selection highlight for keyboard nav
            if is_selected && !is_pass_selected {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: y + CARD_HEIGHT - 4.0,
                    width: CARD_WIDTH,
                    height: 4.0,
                    color: BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }
    }

    /// Render labels for AI opponents.
    fn render_opponent_labels(&self, cmds: &mut Vec<RenderCommand>) {
        // West (player 1)
        let west_cards = self.hands[1].len();
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X - 200.0,
            y: TRICK_CENTER_Y - 10.0,
            text: format!("{} ({})", self.names[1], west_cards),
            color: if self.current_player == 1 { GREEN } else { SUBTEXT0 },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // North (player 2)
        let north_cards = self.hands[2].len();
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X - 30.0,
            y: TRICK_CENTER_Y - 120.0,
            text: format!("{} ({})", self.names[2], north_cards),
            color: if self.current_player == 2 { GREEN } else { SUBTEXT0 },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // East (player 3)
        let east_cards = self.hands[3].len();
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X + 140.0,
            y: TRICK_CENTER_Y - 10.0,
            text: format!("{} ({})", self.names[3], east_cards),
            color: if self.current_player == 3 { GREEN } else { SUBTEXT0 },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Round points display
        let rp_y = TRICK_CENTER_Y + 8.0;
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X - 200.0,
            y: rp_y,
            text: format!("{} pts", self.round_points[1]),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X - 30.0,
            y: TRICK_CENTER_Y - 106.0,
            text: format!("{} pts", self.round_points[2]),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: TRICK_CENTER_X + 140.0,
            y: rp_y,
            text: format!("{} pts", self.round_points[3]),
            color: OVERLAY0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the scoreboard.
    fn render_scores(&self, cmds: &mut Vec<RenderCommand>, _width: f32) {
        cmds.push(RenderCommand::FillRect {
            x: SCORE_X,
            y: SCORE_Y,
            width: 160.0,
            height: 130.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: SCORE_X + 10.0,
            y: SCORE_Y + 20.0,
            text: "Scores".to_string(),
            color: LAVENDER,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        for (i, name) in self.names.iter().enumerate() {
            let y = SCORE_Y + 44.0 + i as f32 * 22.0;
            let color = if Some(i) == self.winner {
                GREEN
            } else if self.scores[i] >= 100 {
                RED
            } else {
                TEXT_COLOR
            };
            cmds.push(RenderCommand::Text {
                x: SCORE_X + 10.0,
                y,
                text: format!("{}: {}", name, self.scores[i]),
                color,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render the status bar.
    fn render_status(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: height - 36.0,
            width,
            height: 36.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: height - 18.0,
            text: self.status.clone(),
            color: YELLOW,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// -- Card rendering helper ---------------------------------------------------

fn render_card(
    cmds: &mut Vec<RenderCommand>,
    x: f32,
    y: f32,
    card: Card,
    selected: bool,
    pass_selected: bool,
) {
    // Card background
    let bg = if pass_selected {
        Color::from_hex(0x45475A)
    } else {
        Color::from_hex(0xEFF1F5)
    };

    // Border/shadow
    cmds.push(RenderCommand::FillRect {
        x: x - 1.0,
        y: y - 1.0,
        width: CARD_WIDTH + 2.0,
        height: CARD_HEIGHT + 2.0,
        color: if selected {
            BLUE
        } else if pass_selected {
            GREEN
        } else {
            OVERLAY0
        },
        corner_radii: CornerRadii::all(CARD_CORNER_RADIUS + 1.0),
    });

    cmds.push(RenderCommand::FillRect {
        x,
        y,
        width: CARD_WIDTH,
        height: CARD_HEIGHT,
        color: bg,
        corner_radii: CornerRadii::all(CARD_CORNER_RADIUS),
    });

    let text_color = if pass_selected {
        // Brighten for contrast against dark bg
        match card.suit {
            Suit::Hearts | Suit::Diamonds => RED,
            _ => TEXT_COLOR,
        }
    } else {
        // Darken for light card bg
        match card.suit {
            Suit::Hearts | Suit::Diamonds => Color::from_hex(0xD20F39),
            _ => Color::from_hex(0x1E1E2E),
        }
    };

    // Rank in top-left
    cmds.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 16.0,
        text: card.rank.label().to_string(),
        color: text_color,
        font_size: CARD_FONT_SIZE,
        font_weight: FontWeightHint::Bold,
        max_width: None,
    });

    // Suit symbol below rank
    cmds.push(RenderCommand::Text {
        x: x + 4.0,
        y: y + 34.0,
        text: card.suit.symbol().to_string(),
        color: text_color,
        font_size: CARD_FONT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });

    // Large suit in center
    cmds.push(RenderCommand::Text {
        x: x + CARD_WIDTH / 2.0 - 8.0,
        y: y + CARD_HEIGHT / 2.0 + 6.0,
        text: card.suit.symbol().to_string(),
        color: text_color,
        font_size: CARD_SUIT_SIZE,
        font_weight: FontWeightHint::Regular,
        max_width: None,
    });
}

// -- Sort hand by suit then rank ---------------------------------------------

fn sort_hand(hand: &mut Vec<Card>) {
    hand.sort();
}

// -- Main --------------------------------------------------------------------

fn main() {
    let _app = Hearts::new();
}

// -- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Card basics ---------------------------------------------------------

    #[test]
    fn test_card_new() {
        let c = Card::new(Suit::Hearts, Rank::Ace);
        assert_eq!(c.suit, Suit::Hearts);
        assert_eq!(c.rank, Rank::Ace);
    }

    #[test]
    fn test_queen_of_spades_detection() {
        let qs = Card::new(Suit::Spades, Rank::Queen);
        assert!(qs.is_queen_of_spades());
        let qh = Card::new(Suit::Hearts, Rank::Queen);
        assert!(!qh.is_queen_of_spades());
    }

    #[test]
    fn test_two_of_clubs_detection() {
        let tc = Card::new(Suit::Clubs, Rank::Two);
        assert!(tc.is_two_of_clubs());
        let td = Card::new(Suit::Diamonds, Rank::Two);
        assert!(!td.is_two_of_clubs());
    }

    #[test]
    fn test_heart_detection() {
        let h = Card::new(Suit::Hearts, Rank::Five);
        assert!(h.is_heart());
        let s = Card::new(Suit::Spades, Rank::Five);
        assert!(!s.is_heart());
    }

    #[test]
    fn test_point_value_heart() {
        let h = Card::new(Suit::Hearts, Rank::Seven);
        assert_eq!(h.point_value(), 1);
    }

    #[test]
    fn test_point_value_queen_of_spades() {
        let qs = Card::new(Suit::Spades, Rank::Queen);
        assert_eq!(qs.point_value(), 13);
    }

    #[test]
    fn test_point_value_no_points() {
        let c = Card::new(Suit::Clubs, Rank::Ace);
        assert_eq!(c.point_value(), 0);
        let d = Card::new(Suit::Diamonds, Rank::King);
        assert_eq!(d.point_value(), 0);
    }

    #[test]
    fn test_card_sort_key() {
        let c2 = Card::new(Suit::Clubs, Rank::Two);
        let ha = Card::new(Suit::Hearts, Rank::Ace);
        assert!(c2.sort_key() < ha.sort_key());
    }

    #[test]
    fn test_card_ordering() {
        let c2 = Card::new(Suit::Clubs, Rank::Two);
        let c3 = Card::new(Suit::Clubs, Rank::Three);
        let h2 = Card::new(Suit::Hearts, Rank::Two);
        assert!(c2 < c3);
        assert!(c3 < h2);
    }

    // -- Suit tests ----------------------------------------------------------

    #[test]
    fn test_suit_symbol() {
        assert_eq!(Suit::Hearts.symbol(), "\u{2665}");
        assert_eq!(Suit::Spades.symbol(), "\u{2660}");
        assert_eq!(Suit::Diamonds.symbol(), "\u{2666}");
        assert_eq!(Suit::Clubs.symbol(), "\u{2663}");
    }

    #[test]
    fn test_suit_index() {
        assert_eq!(Suit::Clubs.index(), 0);
        assert_eq!(Suit::Diamonds.index(), 1);
        assert_eq!(Suit::Spades.index(), 2);
        assert_eq!(Suit::Hearts.index(), 3);
    }

    #[test]
    fn test_suit_all_count() {
        assert_eq!(Suit::ALL.len(), 4);
    }

    // -- Rank tests ----------------------------------------------------------

    #[test]
    fn test_rank_values() {
        assert_eq!(Rank::Two.value(), 2);
        assert_eq!(Rank::Ten.value(), 10);
        assert_eq!(Rank::Jack.value(), 11);
        assert_eq!(Rank::Queen.value(), 12);
        assert_eq!(Rank::King.value(), 13);
        assert_eq!(Rank::Ace.value(), 14);
    }

    #[test]
    fn test_rank_labels() {
        assert_eq!(Rank::Two.label(), "2");
        assert_eq!(Rank::Jack.label(), "J");
        assert_eq!(Rank::Queen.label(), "Q");
        assert_eq!(Rank::King.label(), "K");
        assert_eq!(Rank::Ace.label(), "A");
    }

    #[test]
    fn test_rank_all_count() {
        assert_eq!(Rank::ALL.len(), 13);
    }

    // -- Deck tests ----------------------------------------------------------

    #[test]
    fn test_deck_has_52_cards() {
        let deck = new_deck();
        assert_eq!(deck.len(), 52);
    }

    #[test]
    fn test_deck_unique_cards() {
        let deck = new_deck();
        let mut seen = std::collections::HashSet::new();
        for card in &deck {
            assert!(seen.insert(*card), "Duplicate card found");
        }
    }

    #[test]
    fn test_deck_contains_all_suits() {
        let deck = new_deck();
        for suit in &Suit::ALL {
            let count = deck.iter().filter(|c| c.suit == *suit).count();
            assert_eq!(count, 13, "Suit {:?} should have 13 cards", suit);
        }
    }

    #[test]
    fn test_deck_contains_two_of_clubs() {
        let deck = new_deck();
        assert!(deck.iter().any(|c| c.is_two_of_clubs()));
    }

    #[test]
    fn test_deck_contains_queen_of_spades() {
        let deck = new_deck();
        assert!(deck.iter().any(|c| c.is_queen_of_spades()));
    }

    // -- RNG tests -----------------------------------------------------------

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..10 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        // They should produce different sequences
        let v1 = r1.next();
        let v2 = r2.next();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_rng_range() {
        let mut rng = Rng::new(99);
        for _ in 0..100 {
            let val = rng.next_range(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_rng_shuffle() {
        let mut rng = Rng::new(42);
        let mut items: Vec<u32> = (0..10).collect();
        let original = items.clone();
        rng.shuffle(&mut items);
        // Should still contain same elements
        items.sort();
        let mut sorted_original = original;
        sorted_original.sort();
        assert_eq!(items, sorted_original);
    }

    #[test]
    fn test_rng_shuffle_changes_order() {
        let mut rng = Rng::new(42);
        let mut items: Vec<u32> = (0..52).collect();
        let original = items.clone();
        rng.shuffle(&mut items);
        assert_ne!(items, original, "Shuffle should change order");
    }

    // -- Pass direction tests ------------------------------------------------

    #[test]
    fn test_pass_direction_cycle() {
        let d = PassDirection::Left;
        assert_eq!(d.next(), PassDirection::Right);
        assert_eq!(d.next().next(), PassDirection::Across);
        assert_eq!(d.next().next().next(), PassDirection::Keep);
        assert_eq!(d.next().next().next().next(), PassDirection::Left);
    }

    #[test]
    fn test_pass_direction_target_left() {
        assert_eq!(PassDirection::Left.target(0), 1);
        assert_eq!(PassDirection::Left.target(1), 2);
        assert_eq!(PassDirection::Left.target(2), 3);
        assert_eq!(PassDirection::Left.target(3), 0);
    }

    #[test]
    fn test_pass_direction_target_right() {
        assert_eq!(PassDirection::Right.target(0), 3);
        assert_eq!(PassDirection::Right.target(1), 0);
        assert_eq!(PassDirection::Right.target(2), 1);
        assert_eq!(PassDirection::Right.target(3), 2);
    }

    #[test]
    fn test_pass_direction_target_across() {
        assert_eq!(PassDirection::Across.target(0), 2);
        assert_eq!(PassDirection::Across.target(1), 3);
        assert_eq!(PassDirection::Across.target(2), 0);
        assert_eq!(PassDirection::Across.target(3), 1);
    }

    #[test]
    fn test_pass_direction_target_keep() {
        for i in 0..4 {
            assert_eq!(PassDirection::Keep.target(i), i);
        }
    }

    #[test]
    fn test_pass_direction_labels() {
        assert_eq!(PassDirection::Left.label(), "Pass Left");
        assert_eq!(PassDirection::Right.label(), "Pass Right");
        assert_eq!(PassDirection::Across.label(), "Pass Across");
        assert_eq!(PassDirection::Keep.label(), "No Passing");
    }

    // -- Trick tests ---------------------------------------------------------

    #[test]
    fn test_trick_new_is_empty() {
        let t = Trick::new();
        assert!(t.cards.is_empty());
        assert!(t.lead_suit.is_none());
        assert!(!t.is_complete());
    }

    #[test]
    fn test_trick_lead_sets_suit() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Clubs, Rank::Five));
        assert_eq!(t.lead_suit, Some(Suit::Clubs));
    }

    #[test]
    fn test_trick_complete_after_four() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Clubs, Rank::Five));
        t.play(1, Card::new(Suit::Clubs, Rank::Seven));
        t.play(2, Card::new(Suit::Clubs, Rank::Jack));
        assert!(!t.is_complete());
        t.play(3, Card::new(Suit::Clubs, Rank::Two));
        assert!(t.is_complete());
    }

    #[test]
    fn test_trick_winner_highest_lead_suit() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Clubs, Rank::Five));
        t.play(1, Card::new(Suit::Clubs, Rank::King));
        t.play(2, Card::new(Suit::Hearts, Rank::Ace)); // off-suit, does not win
        t.play(3, Card::new(Suit::Clubs, Rank::Ten));
        assert_eq!(t.winner(), Some(1));
    }

    #[test]
    fn test_trick_winner_off_suit_doesnt_win() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Diamonds, Rank::Three));
        t.play(1, Card::new(Suit::Spades, Rank::Ace));
        t.play(2, Card::new(Suit::Hearts, Rank::Ace));
        t.play(3, Card::new(Suit::Diamonds, Rank::Five));
        assert_eq!(t.winner(), Some(3));
    }

    #[test]
    fn test_trick_points_no_hearts() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Clubs, Rank::Five));
        t.play(1, Card::new(Suit::Clubs, Rank::Seven));
        t.play(2, Card::new(Suit::Clubs, Rank::Jack));
        t.play(3, Card::new(Suit::Clubs, Rank::Two));
        assert_eq!(t.points(), 0);
    }

    #[test]
    fn test_trick_points_with_hearts() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Hearts, Rank::Five));
        t.play(1, Card::new(Suit::Hearts, Rank::Seven));
        t.play(2, Card::new(Suit::Clubs, Rank::Jack));
        t.play(3, Card::new(Suit::Hearts, Rank::Two));
        assert_eq!(t.points(), 3);
    }

    #[test]
    fn test_trick_points_with_queen_of_spades() {
        let mut t = Trick::new();
        t.play(0, Card::new(Suit::Spades, Rank::Queen));
        t.play(1, Card::new(Suit::Spades, Rank::Seven));
        t.play(2, Card::new(Suit::Hearts, Rank::Jack));
        t.play(3, Card::new(Suit::Spades, Rank::Two));
        assert_eq!(t.points(), 14); // 13 + 1
    }

    // -- Game initialization tests -------------------------------------------

    #[test]
    fn test_new_game_deals_13_cards_each() {
        let game = Hearts::new();
        for i in 0..4 {
            assert_eq!(game.hands[i].len(), 13, "Player {} should have 13 cards", i);
        }
    }

    #[test]
    fn test_new_game_all_cards_unique() {
        let game = Hearts::new();
        let mut all_cards: Vec<&Card> = Vec::new();
        for hand in &game.hands {
            all_cards.extend(hand.iter());
        }
        assert_eq!(all_cards.len(), 52);
        let mut seen = std::collections::HashSet::new();
        for &card in &all_cards {
            assert!(seen.insert(card));
        }
    }

    #[test]
    fn test_new_game_scores_zero() {
        let game = Hearts::new();
        assert_eq!(game.scores, [0; 4]);
    }

    #[test]
    fn test_new_game_starts_passing() {
        let game = Hearts::new();
        assert_eq!(game.phase, GamePhase::Passing);
    }

    #[test]
    fn test_new_game_pass_direction_left() {
        let game = Hearts::new();
        assert_eq!(game.pass_direction, PassDirection::Left);
    }

    #[test]
    fn test_new_game_hearts_not_broken() {
        let game = Hearts::new();
        assert!(!game.hearts_broken);
    }

    #[test]
    fn test_new_game_no_winner() {
        let game = Hearts::new();
        assert!(game.winner.is_none());
    }

    // -- Card sorting tests --------------------------------------------------

    #[test]
    fn test_sort_hand() {
        let mut hand = vec![
            Card::new(Suit::Hearts, Rank::Ace),
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Diamonds, Rank::King),
            Card::new(Suit::Clubs, Rank::Ace),
        ];
        sort_hand(&mut hand);
        assert_eq!(hand[0], Card::new(Suit::Clubs, Rank::Two));
        assert_eq!(hand[1], Card::new(Suit::Clubs, Rank::Ace));
        assert_eq!(hand[2], Card::new(Suit::Diamonds, Rank::King));
        assert_eq!(hand[3], Card::new(Suit::Hearts, Rank::Ace));
    }

    #[test]
    fn test_sort_hand_same_suit() {
        let mut hand = vec![
            Card::new(Suit::Spades, Rank::King),
            Card::new(Suit::Spades, Rank::Two),
            Card::new(Suit::Spades, Rank::Ace),
            Card::new(Suit::Spades, Rank::Five),
        ];
        sort_hand(&mut hand);
        assert_eq!(hand[0].rank, Rank::Two);
        assert_eq!(hand[1].rank, Rank::Five);
        assert_eq!(hand[2].rank, Rank::King);
        assert_eq!(hand[3].rank, Rank::Ace);
    }

    // -- Valid plays tests ---------------------------------------------------

    fn make_test_game() -> Hearts {
        let mut game = Hearts::new();
        // Clear and set up a controlled hand for player 0
        game.hands[0] = vec![
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Clubs, Rank::Five),
            Card::new(Suit::Diamonds, Rank::King),
            Card::new(Suit::Hearts, Rank::Three),
            Card::new(Suit::Spades, Rank::Queen),
        ];
        game.phase = GamePhase::Playing;
        game.current_player = 0;
        game
    }

    #[test]
    fn test_valid_plays_must_lead_two_of_clubs_first_trick() {
        let mut game = make_test_game();
        game.trick_number = 0;
        game.current_trick = Trick::new();
        let valid = game.valid_plays(0);
        assert_eq!(valid, vec![0]); // Only 2 of clubs
    }

    #[test]
    fn test_valid_plays_follow_suit() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Seven));
        let valid = game.valid_plays(0);
        // Should only include clubs
        assert_eq!(valid, vec![0, 1]);
    }

    #[test]
    fn test_valid_plays_no_suit_play_anything() {
        let mut game = make_test_game();
        game.trick_number = 2;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Spades, Rank::Seven));
        // Player 0 has clubs, diamonds, hearts, spades
        let valid = game.valid_plays(0);
        // Has spades: Q of spades -> must follow suit
        assert!(valid.contains(&4));
    }

    #[test]
    fn test_valid_plays_cant_lead_hearts_until_broken() {
        let mut game = make_test_game();
        game.trick_number = 3;
        game.hearts_broken = false;
        game.current_trick = Trick::new();
        let valid = game.valid_plays(0);
        // Should not include hearts (index 3) unless only hearts
        assert!(!valid.contains(&3));
    }

    #[test]
    fn test_valid_plays_can_lead_hearts_after_broken() {
        let mut game = make_test_game();
        game.trick_number = 3;
        game.hearts_broken = true;
        game.current_trick = Trick::new();
        let valid = game.valid_plays(0);
        assert!(valid.contains(&3));
    }

    #[test]
    fn test_valid_plays_only_hearts_can_lead_hearts() {
        let mut game = Hearts::new();
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::Three),
            Card::new(Suit::Hearts, Rank::Seven),
            Card::new(Suit::Hearts, Rank::Ace),
        ];
        game.phase = GamePhase::Playing;
        game.current_player = 0;
        game.trick_number = 3;
        game.hearts_broken = false;
        game.current_trick = Trick::new();
        let valid = game.valid_plays(0);
        // All hearts, so must be able to lead them
        assert_eq!(valid.len(), 3);
    }

    #[test]
    fn test_valid_plays_first_trick_no_points_off_suit() {
        let mut game = Hearts::new();
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::Three),
            Card::new(Suit::Hearts, Rank::Seven),
            Card::new(Suit::Diamonds, Rank::Five),
            Card::new(Suit::Spades, Rank::Queen),
        ];
        game.phase = GamePhase::Playing;
        game.current_player = 0;
        game.trick_number = 0;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Two));
        let valid = game.valid_plays(0);
        // Can't follow clubs, but first trick no point cards if possible
        // Only the diamond 5 has no points
        assert_eq!(valid, vec![2]);
    }

    #[test]
    fn test_valid_plays_first_trick_all_point_cards() {
        let mut game = Hearts::new();
        game.hands[0] = vec![
            Card::new(Suit::Hearts, Rank::Three),
            Card::new(Suit::Hearts, Rank::Seven),
            Card::new(Suit::Spades, Rank::Queen),
        ];
        game.phase = GamePhase::Playing;
        game.current_player = 0;
        game.trick_number = 0;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Two));
        let valid = game.valid_plays(0);
        // All point cards, must play one
        assert_eq!(valid.len(), 3);
    }

    // -- Play card tests -----------------------------------------------------

    #[test]
    fn test_play_card_removes_from_hand() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.current_trick = Trick::new();
        let hand_len = game.hands[0].len();
        game.play_card(0, 0);
        assert_eq!(game.hands[0].len(), hand_len - 1);
    }

    #[test]
    fn test_play_card_adds_to_trick() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.play_card(0, 0);
        assert_eq!(game.current_trick.cards.len(), 1);
    }

    #[test]
    fn test_play_card_advances_player() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.play_card(0, 0);
        assert_eq!(game.current_player, 1);
    }

    #[test]
    fn test_play_heart_breaks_hearts() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.hearts_broken = false;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Diamonds, Rank::Five));
        // Play the heart (index 3 in make_test_game hand)
        game.play_card(0, 3);
        assert!(game.hearts_broken);
    }

    // -- Trick completion tests ----------------------------------------------

    #[test]
    fn test_finish_trick_assigns_points() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.current_trick.play(0, Card::new(Suit::Hearts, Rank::Five));
        game.current_trick.play(1, Card::new(Suit::Hearts, Rank::King));
        game.current_trick.play(2, Card::new(Suit::Hearts, Rank::Two));
        game.current_trick.play(3, Card::new(Suit::Hearts, Rank::Three));
        // Manually finish the trick
        game.finish_trick();
        // Player 1 should win (King of hearts is highest)
        assert_eq!(game.round_points[1], 4); // 4 hearts = 4 points
    }

    #[test]
    fn test_finish_trick_increments_trick_number() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.trick_number = 3;
        game.current_trick = Trick::new();
        game.current_trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        game.current_trick.play(1, Card::new(Suit::Clubs, Rank::Seven));
        game.current_trick.play(2, Card::new(Suit::Clubs, Rank::Jack));
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Two));
        game.finish_trick();
        assert_eq!(game.trick_number, 4);
    }

    #[test]
    fn test_finish_trick_winner_leads_next() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.current_trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        game.current_trick.play(1, Card::new(Suit::Clubs, Rank::Ace));
        game.current_trick.play(2, Card::new(Suit::Clubs, Rank::Jack));
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Two));
        game.finish_trick();
        assert_eq!(game.current_player, 1); // Ace wins
    }

    // -- Shooting the moon tests ---------------------------------------------

    #[test]
    fn test_shoot_the_moon_detection() {
        let mut game = Hearts::new();
        game.round_points = [0, 26, 0, 0];
        assert_eq!(game.check_shoot_the_moon(), Some(1));
    }

    #[test]
    fn test_shoot_the_moon_no_one() {
        let mut game = Hearts::new();
        game.round_points = [5, 8, 10, 3];
        assert_eq!(game.check_shoot_the_moon(), None);
    }

    #[test]
    fn test_shoot_the_moon_scoring() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.round_points = [26, 0, 0, 0];
        game.trick_number = 13;
        // Fill in enough empty hands so end_round works
        for i in 0..4 {
            game.hands[i].clear();
        }
        game.end_round();
        // Player 0 shot the moon: everyone else gets 26
        assert_eq!(game.scores[0], 0);
        assert_eq!(game.scores[1], 26);
        assert_eq!(game.scores[2], 26);
        assert_eq!(game.scores[3], 26);
    }

    // -- Round end tests -----------------------------------------------------

    #[test]
    fn test_end_round_adds_to_scores() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.round_points = [5, 8, 10, 3];
        game.trick_number = 13;
        game.end_round();
        assert_eq!(game.scores, [5, 8, 10, 3]);
    }

    #[test]
    fn test_end_round_game_over_at_100() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.scores = [20, 95, 30, 40];
        game.round_points = [5, 8, 10, 3];
        game.trick_number = 13;
        game.end_round();
        assert_eq!(game.phase, GamePhase::GameOver);
        assert_eq!(game.scores[1], 103);
    }

    #[test]
    fn test_end_round_winner_is_lowest_score() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.scores = [20, 95, 30, 40];
        game.round_points = [5, 8, 10, 3];
        game.trick_number = 13;
        game.end_round();
        assert_eq!(game.winner, Some(0)); // 25 is lowest
    }

    #[test]
    fn test_end_round_not_game_over_under_100() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.round_points = [5, 8, 10, 3];
        game.trick_number = 13;
        game.end_round();
        assert_eq!(game.phase, GamePhase::RoundOver);
    }

    #[test]
    fn test_pass_direction_advances_after_round() {
        let mut game = Hearts::new();
        assert_eq!(game.pass_direction, PassDirection::Left);
        game.phase = GamePhase::Playing;
        game.round_points = [5, 8, 10, 3];
        game.trick_number = 13;
        game.end_round();
        assert_eq!(game.pass_direction, PassDirection::Right);
    }

    // -- Pass selection tests ------------------------------------------------

    #[test]
    fn test_toggle_pass_selection_add() {
        let mut game = Hearts::new();
        game.toggle_pass_selection(0);
        assert_eq!(game.pass_selections, vec![0]);
    }

    #[test]
    fn test_toggle_pass_selection_remove() {
        let mut game = Hearts::new();
        game.toggle_pass_selection(0);
        game.toggle_pass_selection(0);
        assert!(game.pass_selections.is_empty());
    }

    #[test]
    fn test_toggle_pass_selection_max_three() {
        let mut game = Hearts::new();
        game.toggle_pass_selection(0);
        game.toggle_pass_selection(1);
        game.toggle_pass_selection(2);
        game.toggle_pass_selection(3); // Should not add a 4th
        assert_eq!(game.pass_selections.len(), 3);
    }

    #[test]
    fn test_toggle_pass_selection_out_of_bounds() {
        let mut game = Hearts::new();
        game.toggle_pass_selection(100); // Out of bounds
        assert!(game.pass_selections.is_empty());
    }

    // -- Pass execution tests ------------------------------------------------

    #[test]
    fn test_execute_pass_needs_three_cards() {
        let mut game = Hearts::new();
        game.pass_selections = vec![0, 1];
        game.execute_pass();
        // Should not change phase since only 2 selected
        assert_eq!(game.phase, GamePhase::Passing);
    }

    #[test]
    fn test_execute_pass_transitions_to_playing() {
        let mut game = Hearts::new();
        game.pass_selections = vec![0, 1, 2];
        game.execute_pass();
        assert_eq!(game.phase, GamePhase::Playing);
    }

    #[test]
    fn test_execute_pass_preserves_52_cards() {
        let mut game = Hearts::new();
        game.pass_selections = vec![0, 1, 2];
        game.execute_pass();
        // After passing, AI may have started playing, so count cards in
        // hands plus any cards already played in the current trick.
        let hand_total: usize = game.hands.iter().map(|h| h.len()).sum();
        let trick_cards = game.current_trick.cards.len();
        assert_eq!(hand_total + trick_cards, 52,
            "Total cards (hands={} + trick={}) should be 52", hand_total, trick_cards);
    }

    // -- Keyboard navigation tests -------------------------------------------

    #[test]
    fn test_key_right_increments_selection() {
        let mut game = Hearts::new();
        game.selected_index = 0;
        game.handle_key(&KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_index, 1);
    }

    #[test]
    fn test_key_left_decrements_selection() {
        let mut game = Hearts::new();
        game.selected_index = 5;
        game.handle_key(&KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_index, 4);
    }

    #[test]
    fn test_key_left_at_zero_stays() {
        let mut game = Hearts::new();
        game.selected_index = 0;
        game.handle_key(&KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_index, 0);
    }

    #[test]
    fn test_key_right_at_end_stays() {
        let mut game = Hearts::new();
        game.selected_index = game.hands[0].len() - 1;
        let max = game.selected_index;
        game.handle_key(&KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.selected_index, max);
    }

    #[test]
    fn test_key_n_starts_new_game() {
        let mut game = Hearts::new();
        game.scores = [50, 60, 70, 80];
        game.handle_key(&KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.scores, [0; 4]);
        assert_eq!(game.phase, GamePhase::Passing);
    }

    #[test]
    fn test_key_not_pressed_ignored() {
        let mut game = Hearts::new();
        game.selected_index = 5;
        game.handle_key(&KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: false,
            text: None,
        });
        assert_eq!(game.selected_index, 5); // Unchanged
    }

    #[test]
    fn test_key_escape_clears_pass_selections() {
        let mut game = Hearts::new();
        game.toggle_pass_selection(0);
        game.toggle_pass_selection(1);
        assert_eq!(game.pass_selections.len(), 2);
        game.handle_key(&KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert!(game.pass_selections.is_empty());
    }

    #[test]
    fn test_key_enter_during_round_over_advances() {
        let mut game = Hearts::new();
        game.phase = GamePhase::RoundOver;
        game.pass_direction = PassDirection::Right;
        // Need hands to be populated for the next round
        game.handle_key(&KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        // Should have started a new round
        assert!(game.phase == GamePhase::Passing || game.phase == GamePhase::Playing);
    }

    #[test]
    fn test_key_enter_during_game_over_restarts() {
        let mut game = Hearts::new();
        game.phase = GamePhase::GameOver;
        game.scores = [100, 50, 60, 70];
        game.handle_key(&KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        assert_eq!(game.scores, [0; 4]);
    }

    // -- Event dispatch tests ------------------------------------------------

    #[test]
    fn test_handle_event_key() {
        let mut game = Hearts::new();
        game.selected_index = 0;
        let event = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        game.handle_event(&event);
        assert_eq!(game.selected_index, 1);
    }

    #[test]
    fn test_handle_event_mouse_click_card() {
        let mut game = Hearts::new();
        // Click on the second card position
        let event = Event::Mouse(MouseEvent {
            x: HAND_X_START + CARD_OVERLAP + 5.0,
            y: HAND_Y + 10.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        game.handle_event(&event);
        // During passing, clicking toggles pass selection
        // The exact index depends on the click position
        assert!(!game.pass_selections.is_empty() || game.selected_index > 0);
    }

    // -- Clamp selection tests -----------------------------------------------

    #[test]
    fn test_clamp_selected_index_empty_hand() {
        let mut game = Hearts::new();
        game.hands[0].clear();
        game.selected_index = 5;
        game.clamp_selected_index();
        assert_eq!(game.selected_index, 0);
    }

    #[test]
    fn test_clamp_selected_index_out_of_bounds() {
        let mut game = Hearts::new();
        game.hands[0] = vec![Card::new(Suit::Clubs, Rank::Two)];
        game.selected_index = 10;
        game.clamp_selected_index();
        assert_eq!(game.selected_index, 0);
    }

    #[test]
    fn test_clamp_selected_index_within_bounds() {
        let mut game = Hearts::new();
        game.selected_index = 5;
        game.clamp_selected_index();
        assert_eq!(game.selected_index, 5);
    }

    // -- Find two of clubs tests ---------------------------------------------

    #[test]
    fn test_find_two_of_clubs_holder() {
        let mut game = Hearts::new();
        // Clear all hands and place 2 of clubs specifically
        for hand in &mut game.hands {
            hand.retain(|c| !c.is_two_of_clubs());
        }
        game.hands[2].push(Card::new(Suit::Clubs, Rank::Two));
        assert_eq!(game.find_two_of_clubs_holder(), 2);
    }

    // -- AI tests ------------------------------------------------------------

    #[test]
    fn test_ai_choose_card_returns_valid_index() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.trick_number = 1;
        game.current_trick = Trick::new();
        for player in 1..4 {
            game.current_player = player;
            let valid = game.valid_plays(player);
            let choice = game.ai_choose_card(player);
            assert!(valid.contains(&choice), "AI chose invalid card");
        }
    }

    #[test]
    fn test_ai_choose_card_only_one_valid() {
        let mut game = Hearts::new();
        game.hands[1] = vec![Card::new(Suit::Clubs, Rank::Two)];
        game.phase = GamePhase::Playing;
        game.current_player = 1;
        game.trick_number = 0;
        game.current_trick = Trick::new();
        let choice = game.ai_choose_card(1);
        assert_eq!(choice, 0);
    }

    #[test]
    fn test_ai_dumps_queen_when_void() {
        let mut game = Hearts::new();
        game.hands[1] = vec![
            Card::new(Suit::Spades, Rank::Queen),
            Card::new(Suit::Hearts, Rank::Three),
            Card::new(Suit::Diamonds, Rank::Five),
        ];
        game.phase = GamePhase::Playing;
        game.current_player = 1;
        game.trick_number = 2;
        game.current_trick = Trick::new();
        game.current_trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        let choice = game.ai_choose_card(1);
        // AI should dump the Queen of Spades
        assert_eq!(choice, 0);
    }

    #[test]
    fn test_ai_select_pass_cards_returns_three() {
        let mut game = Hearts::new();
        let pass = game.ai_select_pass_cards(1);
        assert_eq!(pass.len(), 3);
    }

    #[test]
    fn test_ai_select_pass_cards_valid_indices() {
        let mut game = Hearts::new();
        let hand_len = game.hands[1].len();
        let pass = game.ai_select_pass_cards(1);
        for &idx in &pass {
            assert!(idx < hand_len);
        }
    }

    #[test]
    fn test_ai_select_pass_prefers_queen() {
        let mut game = Hearts::new();
        game.hands[1] = vec![
            Card::new(Suit::Clubs, Rank::Two),
            Card::new(Suit::Clubs, Rank::Three),
            Card::new(Suit::Clubs, Rank::Four),
            Card::new(Suit::Clubs, Rank::Five),
            Card::new(Suit::Spades, Rank::Queen),
        ];
        let pass = game.ai_select_pass_cards(1);
        // Queen of Spades should be in the pass
        let passed_cards: Vec<Card> = pass.iter().map(|&i| game.hands[1][i]).collect();
        assert!(passed_cards.iter().any(|c| c.is_queen_of_spades()));
    }

    // -- New game reset tests ------------------------------------------------

    #[test]
    fn test_new_game_resets_scores() {
        let mut game = Hearts::new();
        game.scores = [50, 60, 70, 80];
        game.new_game();
        assert_eq!(game.scores, [0; 4]);
    }

    #[test]
    fn test_new_game_resets_round_number() {
        let mut game = Hearts::new();
        game.round_number = 5;
        game.new_game();
        assert_eq!(game.round_number, 0);
    }

    #[test]
    fn test_new_game_resets_pass_direction() {
        let mut game = Hearts::new();
        game.pass_direction = PassDirection::Across;
        game.new_game();
        assert_eq!(game.pass_direction, PassDirection::Left);
    }

    #[test]
    fn test_new_game_clears_winner() {
        let mut game = Hearts::new();
        game.winner = Some(2);
        game.new_game();
        assert!(game.winner.is_none());
    }

    // -- Rendering tests (ensure no panics) ----------------------------------

    #[test]
    fn test_render_does_not_panic() {
        let game = Hearts::new();
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_phase() {
        let mut game = Hearts::new();
        game.pass_selections = vec![0, 1, 2];
        game.execute_pass();
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over() {
        let mut game = Hearts::new();
        game.phase = GamePhase::GameOver;
        game.winner = Some(0);
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_round_over() {
        let mut game = Hearts::new();
        game.phase = GamePhase::RoundOver;
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_trick_cards() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Playing;
        game.current_trick.play(0, Card::new(Suit::Clubs, Rank::Five));
        game.current_trick.play(1, Card::new(Suit::Clubs, Rank::Seven));
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_hearts_broken_indicator() {
        let mut game = Hearts::new();
        game.hearts_broken = true;
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_empty_hand() {
        let mut game = Hearts::new();
        game.hands[0].clear();
        game.phase = GamePhase::Playing;
        let cmds = game.render(900.0, 650.0);
        assert!(!cmds.is_empty());
    }

    // -- Integration: full round simulation ----------------------------------

    #[test]
    fn test_full_round_points_sum_to_26() {
        let mut game = Hearts::new();
        // Skip passing
        game.pass_direction = PassDirection::Keep;
        game.start_round();

        // Play all 13 tricks with AI making all decisions
        for _ in 0..200 {
            if game.phase != GamePhase::Playing {
                break;
            }
            if game.current_player == 0 {
                // Human plays first valid card
                let valid = game.valid_plays(0);
                if !valid.is_empty() {
                    game.play_card(0, valid[0]);
                }
            }
            if game.phase == GamePhase::Playing && game.current_player != 0 {
                game.play_ai_turns();
            }
        }

        let total: u32 = game.round_points.iter().sum();
        assert_eq!(total, 26, "Total points in a round should be 26, got {}", total);
    }

    #[test]
    fn test_all_cards_played_after_round() {
        let mut game = Hearts::new();
        game.pass_direction = PassDirection::Keep;
        game.start_round();

        for _ in 0..200 {
            if game.phase != GamePhase::Playing {
                break;
            }
            if game.current_player == 0 {
                let valid = game.valid_plays(0);
                if !valid.is_empty() {
                    game.play_card(0, valid[0]);
                }
            }
            if game.phase == GamePhase::Playing && game.current_player != 0 {
                game.play_ai_turns();
            }
        }

        for i in 0..4 {
            assert!(game.hands[i].is_empty(), "Player {} should have no cards after round", i);
        }
    }

    #[test]
    fn test_keep_pass_direction_skips_passing() {
        let mut game = Hearts::new();
        game.pass_direction = PassDirection::Keep;
        game.start_round();
        assert_eq!(game.phase, GamePhase::Playing);
    }

    // -- Card render helper test ---------------------------------------------

    #[test]
    fn test_render_card_produces_commands() {
        let mut cmds = Vec::new();
        let card = Card::new(Suit::Hearts, Rank::Ace);
        render_card(&mut cmds, 10.0, 20.0, card, false, false);
        assert!(cmds.len() >= 4); // bg border, bg card, rank text, suit texts
    }

    #[test]
    fn test_render_card_selected() {
        let mut cmds = Vec::new();
        render_card(&mut cmds, 10.0, 20.0, Card::new(Suit::Clubs, Rank::King), true, false);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_card_pass_selected() {
        let mut cmds = Vec::new();
        render_card(&mut cmds, 10.0, 20.0, Card::new(Suit::Diamonds, Rank::Five), false, true);
        assert!(!cmds.is_empty());
    }

    // -- Point value exhaustive test -----------------------------------------

    #[test]
    fn test_total_point_value_of_deck() {
        let deck = new_deck();
        let total: u32 = deck.iter().map(|c| c.point_value()).sum();
        assert_eq!(total, 26); // 13 hearts + QoS
    }

    #[test]
    fn test_hearts_count_in_deck() {
        let deck = new_deck();
        let hearts = deck.iter().filter(|c| c.is_heart()).count();
        assert_eq!(hearts, 13);
    }

    // -- Multiple round pass direction cycling --------------------------------

    #[test]
    fn test_pass_direction_full_cycle_over_rounds() {
        let mut game = Hearts::new();
        let directions = [
            PassDirection::Left,
            PassDirection::Right,
            PassDirection::Across,
            PassDirection::Keep,
        ];
        for expected in &directions {
            assert_eq!(game.pass_direction, *expected);
            game.phase = GamePhase::Playing;
            game.round_points = [3, 5, 10, 8];
            game.trick_number = 13;
            game.end_round();
        }
        // Should cycle back to Left
        assert_eq!(game.pass_direction, PassDirection::Left);
    }

    // -- AI play never panics ------------------------------------------------

    #[test]
    fn test_ai_play_turns_no_panic() {
        let mut game = Hearts::new();
        game.pass_direction = PassDirection::Keep;
        game.start_round();

        // Ensure we can get through an entire round without panics
        let mut iterations = 0;
        while game.phase == GamePhase::Playing && iterations < 300 {
            if game.current_player == 0 {
                let valid = game.valid_plays(0);
                if valid.is_empty() {
                    break;
                }
                game.play_card(0, valid[0]);
            } else {
                let choice = game.ai_choose_card(game.current_player);
                game.play_card(game.current_player, choice);
            }
            iterations += 1;
        }
    }

    // -- Human play invalid card test ----------------------------------------

    #[test]
    fn test_human_play_invalid_card_does_nothing() {
        let mut game = make_test_game();
        game.trick_number = 1;
        game.current_trick = Trick::new();
        game.current_trick.play(3, Card::new(Suit::Clubs, Rank::Seven));
        // Select a diamond (not valid when clubs is led and we have clubs)
        game.selected_index = 2; // Diamonds King
        let hand_before = game.hands[0].len();
        game.human_play_selected();
        assert_eq!(game.hands[0].len(), hand_before); // Nothing changed
    }

    #[test]
    fn test_human_play_not_during_playing_phase() {
        let mut game = Hearts::new();
        game.phase = GamePhase::Passing;
        let hand_before = game.hands[0].len();
        game.human_play_selected();
        assert_eq!(game.hands[0].len(), hand_before);
    }

    // -- Multiple seeds produce different deals --------------------------------

    #[test]
    fn test_different_seeds_different_deals() {
        let mut g1 = Hearts::new();
        g1.rng = Rng::new(1);
        g1.start_round();

        let mut g2 = Hearts::new();
        g2.rng = Rng::new(999);
        g2.start_round();

        // Hands should (almost certainly) differ
        assert_ne!(g1.hands[0], g2.hands[0]);
    }
}
