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

//! OurOS Solitaire — Klondike solitaire card game.
//!
//! Standard 52-card deck with 7 tableau columns, stock/waste piles,
//! and 4 foundation piles. Keyboard-driven with Tab navigation,
//! arrow keys, Enter/Space to select/move, undo (Z), and new game (N).
//! Cards rendered with rank + suit symbols in Catppuccin Mocha theme.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
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
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// ── Card colors ─────────────────────────────────────────────────────
const CARD_BG: Color = Color::from_hex(0xCDD6F4);
const CARD_BACK_BG: Color = Color::from_hex(0x45475A);
const CARD_BACK_PATTERN: Color = Color::from_hex(0x585B70);
const CARD_RED: Color = Color::from_hex(0xF38BA8);
const CARD_BLACK: Color = Color::from_hex(0x1E1E2E);
const SELECTED_HIGHLIGHT: Color = Color::from_hex(0x89B4FA);
const CURSOR_HIGHLIGHT: Color = Color::from_hex(0xF9E2AF);
const EMPTY_PILE: Color = Color::from_hex(0x313244);

// ── Layout constants ────────────────────────────────────────────────
const CARD_WIDTH: f32 = 70.0;
const CARD_HEIGHT: f32 = 100.0;
const CARD_CORNER: f32 = 6.0;
const CARD_GAP_X: f32 = 12.0;
const CARD_GAP_Y: f32 = 24.0;
const FACE_DOWN_OFFSET: f32 = 8.0;
const PADDING: f32 = 16.0;
const TOP_ROW_Y: f32 = 16.0;
const TABLEAU_Y: f32 = 140.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const CARD_FONT_SIZE: f32 = 16.0;
const CARD_SUIT_FONT_SIZE: f32 = 20.0;
const INFO_FONT_SIZE: f32 = 14.0;
const STATUS_FONT_SIZE: f32 = 16.0;
const OVERLAY_FONT_SIZE: f32 = 28.0;

/// Number of tableau columns.
const TABLEAU_COLS: usize = 7;
/// Number of foundation piles.
const FOUNDATION_COUNT: usize = 4;

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
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

    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 {
            0
        } else {
            (self.next() % max as u64) as usize
        }
    }

    fn shuffle<T>(&mut self, s: &mut [T]) {
        for i in (1..s.len()).rev() {
            let j = self.next_range(i + 1);
            s.swap(i, j);
        }
    }
}

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

    /// Build a Rank from a numeric value (1..=13).
    fn from_value(v: u8) -> Option<Rank> {
        match v {
            1 => Some(Self::Ace),
            2 => Some(Self::Two),
            3 => Some(Self::Three),
            4 => Some(Self::Four),
            5 => Some(Self::Five),
            6 => Some(Self::Six),
            7 => Some(Self::Seven),
            8 => Some(Self::Eight),
            9 => Some(Self::Nine),
            10 => Some(Self::Ten),
            11 => Some(Self::Jack),
            12 => Some(Self::Queen),
            13 => Some(Self::King),
            _ => None,
        }
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

    /// Whether this card can stack on top of `below` in a tableau pile.
    /// Must be opposite color and one rank lower.
    fn can_stack_on_tableau(self, below: Card) -> bool {
        self.suit.is_red() != below.suit.is_red() && self.rank.value() + 1 == below.rank.value()
    }

    /// Whether this card can be placed on a foundation pile whose
    /// current top card has value `foundation_top_value` (0 if empty).
    fn can_place_on_foundation(self, foundation_top_value: u8) -> bool {
        self.rank.value() == foundation_top_value + 1
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
    rng: Rng,
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
            rng: Rng::new(seed),
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
        let mut idx = 0;
        for col in 0..TABLEAU_COLS {
            for row in 0..=col {
                let face_up = row == col;
                self.tableau[col].push(PileCard::new(deck[idx], face_up));
                idx += 1;
            }
        }

        // Remaining cards go to stock.
        for i in idx..deck.len() {
            self.stock.push(deck[i]);
        }
    }

    /// Start a new game using the next RNG value as seed.
    fn new_game(&mut self) {
        let seed = self.rng.next();
        self.rng = Rng::new(seed);
        self.deal();
    }

    /// Draw one card from stock to waste.
    fn draw_from_stock(&mut self) {
        if let Some(card) = self.stock.pop() {
            self.waste.push(card);
            self.undo_stack.push(UndoAction::Draw);
            self.move_count += 1;
        } else if !self.waste.is_empty() {
            // Recycle waste back to stock (reversed).
            while let Some(card) = self.waste.pop() {
                self.stock.push(card);
            }
            self.undo_stack.push(UndoAction::Recycle);
            self.move_count += 1;
        }
    }

    /// Get the number of face-up cards in a tableau column.
    fn tableau_face_up_count(&self, col: usize) -> usize {
        if col >= TABLEAU_COLS {
            return 0;
        }
        self.tableau[col].iter().filter(|c| c.face_up).count()
    }

    /// Get the number of face-down cards in a tableau column.
    fn tableau_face_down_count(&self, col: usize) -> usize {
        if col >= TABLEAU_COLS {
            return 0;
        }
        self.tableau[col].iter().filter(|c| !c.face_up).count()
    }

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

    /// Get the top card of the waste pile.
    fn waste_top(&self) -> Option<Card> {
        self.waste.last().copied()
    }

    /// Get the face-up cards in a tableau column.
    fn tableau_face_up_cards(&self, col: usize) -> Vec<Card> {
        if col >= TABLEAU_COLS {
            return Vec::new();
        }
        self.tableau[col]
            .iter()
            .filter(|c| c.face_up)
            .map(|c| c.card)
            .collect()
    }

    /// Get the bottom-most face-up card in a tableau column.
    fn tableau_top_card(&self, col: usize) -> Option<Card> {
        if col >= TABLEAU_COLS {
            return None;
        }
        self.tableau[col]
            .last()
            .filter(|c| c.face_up)
            .map(|c| c.card)
    }

    /// Try to move the waste top card to a foundation.
    fn try_waste_to_foundation(&mut self) -> bool {
        let card = match self.waste_top() {
            Some(c) => c,
            None => return false,
        };
        let fidx = card.suit.index();
        if card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            self.waste.pop();
            self.foundations[fidx].push(card);
            self.undo_stack.push(UndoAction::Move {
                from: MoveSource::Waste,
                to: MoveDest::Foundation(fidx),
                count: 1,
                flipped: false,
            });
            self.move_count += 1;
            self.check_win();
            true
        } else {
            false
        }
    }

    /// Try to move the waste top card to a tableau column.
    fn try_waste_to_tableau(&mut self, col: usize) -> bool {
        let card = match self.waste_top() {
            Some(c) => c,
            None => return false,
        };
        if col >= TABLEAU_COLS {
            return false;
        }
        if self.can_place_on_tableau(card, col) {
            self.waste.pop();
            self.tableau[col].push(PileCard::new(card, true));
            self.undo_stack.push(UndoAction::Move {
                from: MoveSource::Waste,
                to: MoveDest::Tableau(col),
                count: 1,
                flipped: false,
            });
            self.move_count += 1;
            true
        } else {
            false
        }
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
        if from_col >= TABLEAU_COLS || to_col >= TABLEAU_COLS || from_col == to_col {
            return false;
        }

        let face_down = self.tableau_face_down_count(from_col);
        let abs_idx = face_down + face_up_idx;
        if abs_idx >= self.tableau[from_col].len() {
            return false;
        }

        // The card at the start of the run we want to move.
        let moving_card = self.tableau[from_col][abs_idx].card;
        if !self.can_place_on_tableau(moving_card, to_col) {
            return false;
        }

        let count = self.tableau[from_col].len() - abs_idx;
        let cards: Vec<PileCard> = self.tableau[from_col].drain(abs_idx..).collect();
        self.tableau[to_col].extend(cards);

        // Flip the new top card if it was face-down.
        let flipped = self.flip_top_if_needed(from_col);

        self.undo_stack.push(UndoAction::Move {
            from: MoveSource::Tableau(from_col),
            to: MoveDest::Tableau(to_col),
            count,
            flipped,
        });
        self.move_count += 1;
        true
    }

    /// Try to move the top card of a tableau column to its foundation.
    fn try_tableau_to_foundation(&mut self, col: usize) -> bool {
        if col >= TABLEAU_COLS {
            return false;
        }
        let card = match self.tableau_top_card(col) {
            Some(c) => c,
            None => return false,
        };
        let fidx = card.suit.index();
        if card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            self.tableau[col].pop();
            self.foundations[fidx].push(card);
            let flipped = self.flip_top_if_needed(col);
            self.undo_stack.push(UndoAction::Move {
                from: MoveSource::Tableau(col),
                to: MoveDest::Foundation(fidx),
                count: 1,
                flipped,
            });
            self.move_count += 1;
            self.check_win();
            true
        } else {
            false
        }
    }

    /// Try to move the top card of a foundation pile to a tableau column.
    fn try_foundation_to_tableau(&mut self, fidx: usize, col: usize) -> bool {
        if fidx >= FOUNDATION_COUNT || col >= TABLEAU_COLS {
            return false;
        }
        let card = match self.foundation_top(fidx) {
            Some(c) => c,
            None => return false,
        };
        if self.can_place_on_tableau(card, col) {
            self.foundations[fidx].pop();
            self.tableau[col].push(PileCard::new(card, true));
            self.undo_stack.push(UndoAction::Move {
                from: MoveSource::Foundation(fidx),
                to: MoveDest::Tableau(col),
                count: 1,
                flipped: false,
            });
            self.move_count += 1;
            true
        } else {
            false
        }
    }

    /// Flip the top card of a tableau column face-up if it is face-down.
    /// Returns true if a flip occurred.
    fn flip_top_if_needed(&mut self, col: usize) -> bool {
        if col >= TABLEAU_COLS {
            return false;
        }
        if let Some(top) = self.tableau[col].last_mut()
            && !top.face_up {
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
                    && let Some(top) = self.tableau[col].last_mut()
                {
                    top.face_up = false;
                }
                // Move cards back.
                let cards: Vec<PileCard> = match to {
                    MoveDest::Foundation(fidx) => {
                        let mut result = Vec::new();
                        for _ in 0..count {
                            if let Some(c) = self.foundations[fidx].pop() {
                                result.push(PileCard::new(c, true));
                            }
                        }
                        result.reverse();
                        result
                    }
                    MoveDest::Tableau(col) => {
                        let len = self.tableau[col].len();
                        let start = len.saturating_sub(count);
                        self.tableau[col].drain(start..).collect()
                    }
                };
                match from {
                    MoveSource::Waste => {
                        for pc in cards {
                            self.waste.push(pc.card);
                        }
                    }
                    MoveSource::Foundation(fidx) => {
                        for pc in cards {
                            self.foundations[fidx].push(pc.card);
                        }
                    }
                    MoveSource::Tableau(col) => {
                        self.tableau[col].extend(cards);
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
                            if from_idx + 1 == fu {
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
                if i + 1 < FOUNDATION_COUNT {
                    FocusArea::Foundation(i + 1)
                } else {
                    FocusArea::Tableau(0, 0)
                }
            }
            FocusArea::Tableau(col, _) => {
                if col + 1 < TABLEAU_COLS {
                    FocusArea::Tableau(col + 1, 0)
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
            FocusArea::Foundation(i) => FocusArea::Foundation(i - 1),
            FocusArea::Tableau(0, _) => FocusArea::Foundation(FOUNDATION_COUNT - 1),
            FocusArea::Tableau(col, _) => {
                let prev = col - 1;
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
            let new_offset = if delta < 0 {
                offset.saturating_sub((-delta) as usize)
            } else {
                (offset + delta as usize).min(max_idx)
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
            FocusArea::Foundation(i) => {
                let new_i = i as i32 + delta;
                if new_i < 0 {
                    self.focus = FocusArea::Waste;
                } else if (new_i as usize) < FOUNDATION_COUNT {
                    self.focus = FocusArea::Foundation(new_i as usize);
                }
            }
            FocusArea::Tableau(col, offset) => {
                let new_col = col as i32 + delta;
                if new_col >= 0 && (new_col as usize) < TABLEAU_COLS {
                    let new_c = new_col as usize;
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
                    FocusArea::Foundation(i) => (i + 3).min(TABLEAU_COLS - 1),
                    _ => 0,
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
                    self.focus = FocusArea::Foundation(col - 3);
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

    /// Handle a key event.
    fn handle_key(&mut self, key: Key, modifiers: Modifiers) {
        if self.won {
            if key == Key::N { self.new_game() }
            return;
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
            _ => {}
        }
    }

    /// Compute the x position for a top-row item (stock, waste, foundations).
    fn top_row_x(index: usize) -> f32 {
        PADDING + (CARD_WIDTH + CARD_GAP_X) * index as f32
    }

    /// Compute the x position for a tableau column.
    fn tableau_col_x(col: usize) -> f32 {
        PADDING + (CARD_WIDTH + CARD_GAP_X) * col as f32
    }

    /// Compute the y position for a card in a tableau column.
    fn tableau_card_y(face_down_count: usize, face_up_idx: usize) -> f32 {
        TABLEAU_Y + face_down_count as f32 * FACE_DOWN_OFFSET + face_up_idx as f32 * CARD_GAP_Y
    }

    /// Generate render commands for the entire game.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 700.0,
            height: 700.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: TOP_ROW_Y - 2.0,
            text: String::from("Solitaire"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Move counter and help text.
        cmds.push(RenderCommand::Text {
            x: 400.0,
            y: TOP_ROW_Y - 2.0,
            text: format!("Moves: {}", self.move_count),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: TOP_ROW_Y - 2.0,
            text: String::from("N:New Z:Undo A:Auto"),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let top_y = TOP_ROW_Y + 24.0;

        // Stock pile (index 0 in top row).
        self.render_stock(&mut cmds, Self::top_row_x(0), top_y);

        // Waste pile (index 1).
        self.render_waste(&mut cmds, Self::top_row_x(1), top_y);

        // Foundation piles (indices 3..6, gap after waste).
        for i in 0..FOUNDATION_COUNT {
            self.render_foundation(&mut cmds, i, Self::top_row_x(i + 3), top_y);
        }

        // Tableau columns.
        for col in 0..TABLEAU_COLS {
            self.render_tableau_col(&mut cmds, col);
        }

        // Win overlay.
        if self.won {
            self.render_win_overlay(&mut cmds);
        }

        cmds
    }

    /// Render the stock pile.
    fn render_stock(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let is_focused = self.focus == FocusArea::Stock;

        if self.stock.is_empty() {
            // Empty stock — show recycle indicator.
            self.render_empty_pile(cmds, x, y, is_focused);
            if !self.waste.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + CARD_WIDTH / 2.0 - 8.0,
                    y: y + CARD_HEIGHT / 2.0 - 10.0,
                    text: String::from("\u{21BB}"),
                    color: OVERLAY0,
                    font_size: CARD_SUIT_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
        } else {
            // Card back.
            self.render_card_back(cmds, x, y, is_focused);
            // Count.
            cmds.push(RenderCommand::Text {
                x: x + 2.0,
                y: y + CARD_HEIGHT + 2.0,
                text: format!("{}", self.stock.len()),
                color: SUBTEXT0,
                font_size: INFO_FONT_SIZE - 2.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render the waste pile.
    fn render_waste(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32) {
        let is_focused = self.focus == FocusArea::Waste;
        let is_selected = self.selection == Some(Selection::Waste);

        match self.waste_top() {
            Some(card) => {
                self.render_card_face(cmds, card, x, y, is_focused, is_selected);
            }
            None => {
                self.render_empty_pile(cmds, x, y, is_focused);
            }
        }
    }

    /// Render a foundation pile.
    fn render_foundation(&self, cmds: &mut Vec<RenderCommand>, idx: usize, x: f32, y: f32) {
        let is_focused = self.focus == FocusArea::Foundation(idx);
        let is_selected = self.selection == Some(Selection::Foundation(idx));

        match self.foundation_top(idx) {
            Some(card) => {
                self.render_card_face(cmds, card, x, y, is_focused, is_selected);
                // Show count.
                cmds.push(RenderCommand::Text {
                    x: x + 2.0,
                    y: y + CARD_HEIGHT + 2.0,
                    text: format!("{}/13", self.foundations[idx].len()),
                    color: SUBTEXT0,
                    font_size: INFO_FONT_SIZE - 2.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            None => {
                self.render_empty_pile(cmds, x, y, is_focused);
                // Show suit target.
                let suit = Suit::ALL[idx];
                cmds.push(RenderCommand::Text {
                    x: x + CARD_WIDTH / 2.0 - 8.0,
                    y: y + CARD_HEIGHT / 2.0 - 10.0,
                    text: String::from(suit.symbol()),
                    color: OVERLAY0,
                    font_size: CARD_SUIT_FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    /// Render a single tableau column.
    fn render_tableau_col(&self, cmds: &mut Vec<RenderCommand>, col: usize) {
        let x = Self::tableau_col_x(col);
        let pile = &self.tableau[col];

        if pile.is_empty() {
            let is_focused = matches!(self.focus, FocusArea::Tableau(c, _) if c == col);
            self.render_empty_pile(cmds, x, TABLEAU_Y, is_focused);
            return;
        }

        let face_down_count = pile.iter().filter(|c| !c.face_up).count();
        let mut face_up_idx = 0;

        for (i, pc) in pile.iter().enumerate() {
            if !pc.face_up {
                let y = TABLEAU_Y + i as f32 * FACE_DOWN_OFFSET;
                self.render_card_back(cmds, x, y, false);
            } else {
                let y = Self::tableau_card_y(face_down_count, face_up_idx);

                let is_focused = self.focus == FocusArea::Tableau(col, face_up_idx);

                let is_selected = match self.selection {
                    Some(Selection::Tableau(sel_col, sel_idx)) => {
                        sel_col == col && face_up_idx >= sel_idx
                    }
                    _ => false,
                };

                self.render_card_face(cmds, pc.card, x, y, is_focused, is_selected);
                face_up_idx += 1;
            }
        }
    }

    /// Render an empty pile placeholder.
    fn render_empty_pile(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, focused: bool) {
        let border_color = if focused { CURSOR_HIGHLIGHT } else { OVERLAY0 };
        cmds.push(RenderCommand::StrokeRect {
            x,
            y,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
            color: border_color,
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(CARD_CORNER),
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 1.0,
            y: y + 1.0,
            width: CARD_WIDTH - 2.0,
            height: CARD_HEIGHT - 2.0,
            color: EMPTY_PILE,
            corner_radii: CornerRadii::all(CARD_CORNER),
        });
    }

    /// Render a face-down card back.
    fn render_card_back(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, focused: bool) {
        // Border.
        if focused {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 1.0,
                y: y - 1.0,
                width: CARD_WIDTH + 2.0,
                height: CARD_HEIGHT + 2.0,
                color: CURSOR_HIGHLIGHT,
                line_width: 2.0,
                corner_radii: CornerRadii::all(CARD_CORNER + 1.0),
            });
        }

        // Card body.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
            color: CARD_BACK_BG,
            corner_radii: CornerRadii::all(CARD_CORNER),
        });

        // Pattern: cross-hatch lines.
        let inset = 6.0;
        let spacing = 10.0;
        let left = x + inset;
        let right = x + CARD_WIDTH - inset;
        let top_edge = y + inset;
        let bottom = y + CARD_HEIGHT - inset;

        // Draw diagonal lines for the card back pattern.
        let mut lx = left;
        while lx <= right {
            cmds.push(RenderCommand::Line {
                x1: lx,
                y1: top_edge,
                x2: lx.min(right),
                y2: bottom.min(top_edge + (lx - left) + spacing),
                color: CARD_BACK_PATTERN,
                width: 1.0,
            });
            lx += spacing;
        }

        // Inner border.
        cmds.push(RenderCommand::StrokeRect {
            x: x + inset - 1.0,
            y: y + inset - 1.0,
            width: CARD_WIDTH - 2.0 * inset + 2.0,
            height: CARD_HEIGHT - 2.0 * inset + 2.0,
            color: CARD_BACK_PATTERN,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
    }

    /// Render a face-up card.
    fn render_card_face(
        &self,
        cmds: &mut Vec<RenderCommand>,
        card: Card,
        x: f32,
        y: f32,
        focused: bool,
        selected: bool,
    ) {
        // Selection highlight.
        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 2.0,
                y: y - 2.0,
                width: CARD_WIDTH + 4.0,
                height: CARD_HEIGHT + 4.0,
                color: SELECTED_HIGHLIGHT,
                line_width: 2.5,
                corner_radii: CornerRadii::all(CARD_CORNER + 2.0),
            });
        } else if focused {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 1.0,
                y: y - 1.0,
                width: CARD_WIDTH + 2.0,
                height: CARD_HEIGHT + 2.0,
                color: CURSOR_HIGHLIGHT,
                line_width: 2.0,
                corner_radii: CornerRadii::all(CARD_CORNER + 1.0),
            });
        }

        // Card body.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: CARD_WIDTH,
            height: CARD_HEIGHT,
            color: CARD_BG,
            corner_radii: CornerRadii::all(CARD_CORNER),
        });

        let text_color = card.suit.color();

        // Top-left rank.
        cmds.push(RenderCommand::Text {
            x: x + 5.0,
            y: y + 4.0,
            text: String::from(card.rank.label()),
            color: text_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Top-left suit.
        cmds.push(RenderCommand::Text {
            x: x + 5.0,
            y: y + 20.0,
            text: String::from(card.suit.symbol()),
            color: text_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Center suit (larger).
        cmds.push(RenderCommand::Text {
            x: x + CARD_WIDTH / 2.0 - 8.0,
            y: y + CARD_HEIGHT / 2.0 - 10.0,
            text: String::from(card.suit.symbol()),
            color: text_color,
            font_size: CARD_SUIT_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom-right rank (offset for inverted).
        cmds.push(RenderCommand::Text {
            x: x + CARD_WIDTH - 22.0,
            y: y + CARD_HEIGHT - 22.0,
            text: String::from(card.rank.label()),
            color: text_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Bottom-right suit.
        cmds.push(RenderCommand::Text {
            x: x + CARD_WIDTH - 22.0,
            y: y + CARD_HEIGHT - 38.0,
            text: String::from(card.suit.symbol()),
            color: text_color,
            font_size: CARD_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render win overlay.
    fn render_win_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 700.0,
            height: 700.0,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 200.0,
            y: 280.0,
            text: String::from("You Win!"),
            color: GREEN,
            font_size: OVERLAY_FONT_SIZE + 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 210.0,
            y: 330.0,
            text: format!("Moves: {}", self.move_count),
            color: SUBTEXT0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 180.0,
            y: 370.0,
            text: String::from("Press N for a new game"),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ── Application wrapper ─────────────────────────────────────────────

/// The solitaire application.
struct SolitaireApp {
    state: GameState,
}

impl SolitaireApp {
    fn new() -> Self {
        Self {
            state: GameState::new(42),
        }
    }

    fn handle_event(&mut self, event: Event) {
        if let Event::Key(KeyEvent {
                key,
                modifiers,
                pressed: true,
                ..
            }) = event {
            self.state.handle_key(key, modifiers);
        }
    }

    fn render(&self) -> Vec<RenderCommand> {
        self.state.render()
    }
}

fn main() {
    let _app = SolitaireApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_rank_from_value() {
        for r in &Rank::ALL {
            assert_eq!(Rank::from_value(r.value()), Some(*r));
        }
        assert_eq!(Rank::from_value(0), None);
        assert_eq!(Rank::from_value(14), None);
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

    // ── RNG tests ──────────────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(123);
        let mut r2 = Rng::new(123);
        for _ in 0..10 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        // At least one value should differ in 10 draws.
        let mut differ = false;
        for _ in 0..10 {
            if r1.next() != r2.next() {
                differ = true;
            }
        }
        assert!(differ);
    }

    #[test]
    fn test_rng_next_range() {
        let mut rng = Rng::new(77);
        for _ in 0..100 {
            let v = rng.next_range(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn test_rng_next_range_zero() {
        let mut rng = Rng::new(42);
        assert_eq!(rng.next_range(0), 0);
    }

    #[test]
    fn test_rng_shuffle_preserves_elements() {
        let mut rng = Rng::new(99);
        let mut v: Vec<i32> = (0..20).collect();
        let original = v.clone();
        rng.shuffle(&mut v);
        v.sort();
        assert_eq!(v, original);
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

    // ── Rendering tests ────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let gs = new_game();
        let cmds = gs.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_has_background() {
        let gs = new_game();
        let cmds = gs.render();
        // First command should be the background fill.
        match &cmds[0] {
            RenderCommand::FillRect { width, height, .. } => {
                assert_eq!(*width, 700.0);
                assert_eq!(*height, 700.0);
            }
            _ => panic!("First render command should be FillRect background"),
        }
    }

    #[test]
    fn test_render_has_title() {
        let gs = new_game();
        let cmds = gs.render();
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Solitaire"));
        assert!(has_title);
    }

    #[test]
    fn test_render_win_overlay() {
        let mut gs = new_game();
        gs.won = true;
        let cmds = gs.render();
        let has_win = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "You Win!"));
        assert!(has_win);
    }

    #[test]
    fn test_render_move_counter() {
        let mut gs = new_game();
        gs.move_count = 42;
        let cmds = gs.render();
        let has_count = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Moves: 42"));
        assert!(has_count);
    }

    // ── Layout helper tests ────────────────────────────────────────

    #[test]
    fn test_top_row_x_positions() {
        let x0 = GameState::top_row_x(0);
        let x1 = GameState::top_row_x(1);
        assert!(x1 > x0);
        assert!((x1 - x0 - CARD_WIDTH - CARD_GAP_X).abs() < 0.01);
    }

    #[test]
    fn test_tableau_col_x_positions() {
        let x0 = GameState::tableau_col_x(0);
        let x1 = GameState::tableau_col_x(1);
        assert!(x1 > x0);
        assert!((x1 - x0 - CARD_WIDTH - CARD_GAP_X).abs() < 0.01);
    }

    #[test]
    fn test_tableau_card_y_positions() {
        let y0 = GameState::tableau_card_y(0, 0);
        let y1 = GameState::tableau_card_y(0, 1);
        assert!((y1 - y0 - CARD_GAP_Y).abs() < 0.01);
    }

    #[test]
    fn test_tableau_card_y_with_face_down() {
        let y_no_fd = GameState::tableau_card_y(0, 0);
        let y_with_fd = GameState::tableau_card_y(3, 0);
        assert!((y_with_fd - y_no_fd - 3.0 * FACE_DOWN_OFFSET).abs() < 0.01);
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

    // ── SolitaireApp tests ─────────────────────────────────────────

    #[test]
    fn test_app_creation() {
        let app = SolitaireApp::new();
        assert_eq!(app.state.stock.len(), 24);
        assert!(!app.state.won);
    }

    #[test]
    fn test_app_render() {
        let app = SolitaireApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_handle_event() {
        let mut app = SolitaireApp::new();
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Tab,
            modifiers: Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                super_key: false,
            },
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state.focus, FocusArea::Waste);
    }

    #[test]
    fn test_app_key_release_ignored() {
        let mut app = SolitaireApp::new();
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Tab,
            modifiers: Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
                super_key: false,
            },
            pressed: false,
            text: None,
        }));
        // Focus should not change on key release.
        assert_eq!(app.state.focus, FocusArea::Stock);
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
