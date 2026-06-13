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

//! Slate OS FreeCell -- classic FreeCell card game.
//!
//! Standard 52-card deck dealt across 8 tableau columns (7,7,7,7,6,6,6,6).
//! Four free cells for temporary single-card storage, four foundation piles
//! that build up by suit from Ace to King. Keyboard-driven with Tab/arrow
//! navigation, Enter/Space to select/place, Z for undo, N for new game.
//! Catppuccin Mocha themed.

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
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Card display colors ─────────────────────────────────────────────
const CARD_BG: Color = Color::from_hex(0xCDD6F4);
const CARD_RED: Color = Color::from_hex(0xF38BA8);
const CARD_BLACK: Color = Color::from_hex(0x1E1E2E);
const SELECTED_HIGHLIGHT: Color = Color::from_hex(0x89B4FA);
const CURSOR_HIGHLIGHT: Color = Color::from_hex(0xF9E2AF);
const EMPTY_PILE: Color = Color::from_hex(0x313244);

// ── Layout constants ────────────────────────────────────────────────
const CARD_WIDTH: f32 = 70.0;
const CARD_HEIGHT: f32 = 100.0;
const CARD_CORNER: f32 = 6.0;
const CARD_GAP_X: f32 = 10.0;
const CASCADE_OFFSET: f32 = 24.0;
const PADDING: f32 = 16.0;
const TOP_ROW_Y: f32 = 50.0;
const TABLEAU_Y: f32 = 175.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const CARD_FONT_SIZE: f32 = 16.0;
const CARD_SUIT_FONT_SIZE: f32 = 20.0;
const INFO_FONT_SIZE: f32 = 14.0;
const STATUS_FONT_SIZE: f32 = 16.0;
const OVERLAY_FONT_SIZE: f32 = 28.0;

/// Number of tableau columns.
const TABLEAU_COLS: usize = 8;
/// Number of free cells.
const FREE_CELL_COUNT: usize = 4;
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

    /// Whether the suit is red (Hearts or Diamonds).
    fn is_red(self) -> bool {
        matches!(self, Self::Hearts | Self::Diamonds)
    }

    /// Display color for this suit.
    fn color(self) -> Color {
        if self.is_red() {
            CARD_RED
        } else {
            CARD_BLACK
        }
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

    /// Whether this card can stack on top of `below` in a tableau column.
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

/// Records one undoable action.
#[derive(Clone, Debug)]
enum UndoAction {
    /// Moved a card between locations.
    Move {
        from: MoveLocation,
        to: MoveLocation,
    },
    /// Auto-moved a card to foundation.
    AutoMove {
        from: MoveLocation,
        to: MoveLocation,
    },
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
            rng: Rng::new(seed),
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
        let mut idx = 0;
        for col in 0..TABLEAU_COLS {
            let count = if col < 4 { 7 } else { 6 };
            for _ in 0..count {
                self.tableau[col].push(deck[idx]);
                idx += 1;
            }
        }
    }

    /// Start a new game using the next RNG value as seed.
    fn new_game(&mut self) {
        let seed = self.rng.next();
        self.rng = Rng::new(seed);
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

    // ── Move logic ──────────────────────────────────────────────────

    /// Check if a card can be placed on a tableau column.
    fn can_place_on_tableau(&self, card: Card, col: usize) -> bool {
        if col >= TABLEAU_COLS {
            return false;
        }
        match self.tableau_top(col) {
            Some(top) => card.can_stack_on_tableau(top),
            // Any card can go on an empty column.
            None => true,
        }
    }

    /// Try to move a card from a free cell to a tableau column.
    fn try_freecell_to_tableau(&mut self, fc_idx: usize, col: usize) -> bool {
        let card = match self.free_cells.get(fc_idx).copied().flatten() {
            Some(c) => c,
            None => return false,
        };
        if !self.can_place_on_tableau(card, col) {
            return false;
        }
        self.free_cells[fc_idx] = None;
        self.tableau[col].push(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::FreeCell(fc_idx),
            to: MoveLocation::Tableau(col),
        });
        self.move_count += 1;
        true
    }

    /// Try to move a card from a free cell to its foundation.
    fn try_freecell_to_foundation(&mut self, fc_idx: usize) -> bool {
        let card = match self.free_cells.get(fc_idx).copied().flatten() {
            Some(c) => c,
            None => return false,
        };
        let fidx = card.suit.index();
        if !card.can_place_on_foundation(self.foundation_top_value(fidx)) {
            return false;
        }
        self.free_cells[fc_idx] = None;
        self.foundations[fidx].push(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::FreeCell(fc_idx),
            to: MoveLocation::Foundation(fidx),
        });
        self.move_count += 1;
        self.check_win();
        true
    }

    /// Try to move the top card from a tableau column to a free cell.
    fn try_tableau_to_freecell(&mut self, col: usize) -> bool {
        let card = match self.tableau_top(col) {
            Some(c) => c,
            None => return false,
        };
        let fc_idx = match self.first_empty_free_cell() {
            Some(i) => i,
            None => return false,
        };
        self.tableau[col].pop();
        self.free_cells[fc_idx] = Some(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::FreeCell(fc_idx),
        });
        self.move_count += 1;
        true
    }

    /// Try to move the top card from a tableau column to a specific free cell.
    fn try_tableau_to_specific_freecell(&mut self, col: usize, fc_idx: usize) -> bool {
        if fc_idx >= FREE_CELL_COUNT {
            return false;
        }
        if self.free_cells[fc_idx].is_some() {
            return false;
        }
        let card = match self.tableau_top(col) {
            Some(c) => c,
            None => return false,
        };
        self.tableau[col].pop();
        self.free_cells[fc_idx] = Some(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::FreeCell(fc_idx),
        });
        self.move_count += 1;
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
        self.tableau[col].pop();
        self.foundations[fidx].push(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::Tableau(col),
            to: MoveLocation::Foundation(fidx),
        });
        self.move_count += 1;
        self.check_win();
        true
    }

    /// Try to move the top card from one tableau column to another.
    fn try_tableau_to_tableau(&mut self, from_col: usize, to_col: usize) -> bool {
        if from_col == to_col || from_col >= TABLEAU_COLS || to_col >= TABLEAU_COLS {
            return false;
        }
        let card = match self.tableau_top(from_col) {
            Some(c) => c,
            None => return false,
        };
        if !self.can_place_on_tableau(card, to_col) {
            return false;
        }
        self.tableau[from_col].pop();
        self.tableau[to_col].push(card);
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::Tableau(from_col),
            to: MoveLocation::Tableau(to_col),
        });
        self.move_count += 1;
        true
    }

    /// Try to move a card from a free cell to a specific free cell (swap).
    fn try_freecell_to_freecell(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= FREE_CELL_COUNT || to >= FREE_CELL_COUNT {
            return false;
        }
        if self.free_cells[from].is_none() || self.free_cells[to].is_some() {
            return false;
        }
        let card = self.free_cells[from].take();
        self.free_cells[to] = card;
        self.undo_stack.push(UndoAction::Move {
            from: MoveLocation::FreeCell(from),
            to: MoveLocation::FreeCell(to),
        });
        self.move_count += 1;
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
        let needed = card.rank.value() - 1;
        let is_red = card.suit.is_red();
        for &s in &Suit::ALL {
            if s.is_red() != is_red && self.foundation_top_value(s.index()) < needed {
                return false;
            }
        }
        true
    }

    /// Auto-move eligible cards to foundations. Returns how many were moved.
    fn auto_move_to_foundations(&mut self) -> usize {
        let mut total = 0;
        loop {
            let mut moved_any = false;

            // Check free cells.
            for fc_idx in 0..FREE_CELL_COUNT {
                if let Some(card) = self.free_cells[fc_idx] {
                    let fidx = card.suit.index();
                    if card.can_place_on_foundation(self.foundation_top_value(fidx))
                        && self.is_safe_to_auto_move(card)
                    {
                        self.free_cells[fc_idx] = None;
                        self.foundations[fidx].push(card);
                        self.undo_stack.push(UndoAction::AutoMove {
                            from: MoveLocation::FreeCell(fc_idx),
                            to: MoveLocation::Foundation(fidx),
                        });
                        total += 1;
                        moved_any = true;
                    }
                }
            }

            // Check tableau tops.
            for col in 0..TABLEAU_COLS {
                if let Some(card) = self.tableau_top(col) {
                    let fidx = card.suit.index();
                    if card.can_place_on_foundation(self.foundation_top_value(fidx))
                        && self.is_safe_to_auto_move(card)
                    {
                        self.tableau[col].pop();
                        self.foundations[fidx].push(card);
                        self.undo_stack.push(UndoAction::AutoMove {
                            from: MoveLocation::Tableau(col),
                            to: MoveLocation::Foundation(fidx),
                        });
                        total += 1;
                        moved_any = true;
                    }
                }
            }

            if !moved_any {
                break;
            }
        }
        if total > 0 {
            self.check_win();
        }
        total
    }

    // ── Undo ────────────────────────────────────────────────────────

    /// Undo the last action.
    fn undo(&mut self) {
        let action = match self.undo_stack.pop() {
            Some(a) => a,
            None => return,
        };
        match action {
            UndoAction::Move { from, to } | UndoAction::AutoMove { from, to } => {
                // Reverse: take from `to`, put back at `from`.
                let card = self.take_card_from(to);
                if let Some(c) = card {
                    self.put_card_at(from, c);
                }
                // Undo auto-moves recursively (they chain).
                if matches!(action, UndoAction::AutoMove { .. }) {
                    // Keep undoing auto-moves.
                    if let Some(next) = self.undo_stack.last()
                        && matches!(next, UndoAction::AutoMove { .. }) {
                            self.undo();
                            return;
                        }
                }
                if self.move_count > 0 {
                    self.move_count -= 1;
                }
            }
        }
        self.won = false;
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

    /// Handle a key event.
    fn handle_key(&mut self, key: Key, _modifiers: Modifiers) {
        if self.won {
            if key == Key::N { self.new_game() }
            return;
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
                self.auto_move_to_foundations();
            }
            _ => {}
        }
    }

    /// Cycle focus between zones: free cells -> foundations -> tableau.
    fn navigate_next_zone(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(_) => FocusArea::Foundation(0),
            FocusArea::Foundation(_) => FocusArea::Tableau(0),
            FocusArea::Tableau(_) => FocusArea::FreeCell(0),
        };
    }

    /// Navigate left within the current zone.
    fn navigate_left(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => {
                if i > 0 {
                    FocusArea::FreeCell(i - 1)
                } else {
                    FocusArea::FreeCell(FREE_CELL_COUNT - 1)
                }
            }
            FocusArea::Foundation(i) => {
                if i > 0 {
                    FocusArea::Foundation(i - 1)
                } else {
                    FocusArea::Foundation(FOUNDATION_COUNT - 1)
                }
            }
            FocusArea::Tableau(i) => {
                if i > 0 {
                    FocusArea::Tableau(i - 1)
                } else {
                    FocusArea::Tableau(TABLEAU_COLS - 1)
                }
            }
        };
    }

    /// Navigate right within the current zone.
    fn navigate_right(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => FocusArea::FreeCell((i + 1) % FREE_CELL_COUNT),
            FocusArea::Foundation(i) => FocusArea::Foundation((i + 1) % FOUNDATION_COUNT),
            FocusArea::Tableau(i) => FocusArea::Tableau((i + 1) % TABLEAU_COLS),
        };
    }

    /// Navigate up to the top row from tableau.
    fn navigate_up(&mut self) {
        self.focus = match self.focus {
            FocusArea::Tableau(i) => {
                // Top row: left 4 = free cells, right 4 = foundations.
                if i < 4 {
                    FocusArea::FreeCell(i)
                } else {
                    FocusArea::Foundation(i - 4)
                }
            }
            FocusArea::Foundation(i) => FocusArea::FreeCell(i.min(FREE_CELL_COUNT - 1)),
            other => other,
        };
    }

    /// Navigate down to tableau from the top row.
    fn navigate_down(&mut self) {
        self.focus = match self.focus {
            FocusArea::FreeCell(i) => FocusArea::Tableau(i.min(TABLEAU_COLS - 1)),
            FocusArea::Foundation(i) => FocusArea::Tableau((i + 4).min(TABLEAU_COLS - 1)),
            other => other,
        };
    }

    /// Activate: select a card or place the selected card.
    fn activate(&mut self) {
        if let Some(sel) = self.selection {
            // We have a selection -- try to place it at the focused location.
            let placed = self.try_place_selection(sel);
            if placed {
                self.selection = None;
                self.auto_move_to_foundations();
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

    /// Calculate the x position of a column for the top row (free cells + foundations).
    fn top_row_x(slot: usize) -> f32 {
        PADDING + slot as f32 * (CARD_WIDTH + CARD_GAP_X)
    }

    /// Calculate the x position of a tableau column.
    fn tableau_col_x(col: usize) -> f32 {
        PADDING + col as f32 * (CARD_WIDTH + CARD_GAP_X)
    }

    /// Render the complete game.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(256);

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 900.0,
            height: 800.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 10.0,
            text: String::from("FreeCell"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Move counter.
        cmds.push(RenderCommand::Text {
            x: 200.0,
            y: 14.0,
            text: format!("Moves: {}", self.move_count),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Foundation total.
        cmds.push(RenderCommand::Text {
            x: 340.0,
            y: 14.0,
            text: format!("Foundation: {}/52", self.foundation_total()),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Help text.
        cmds.push(RenderCommand::Text {
            x: 520.0,
            y: 14.0,
            text: String::from("Tab:zone  Arrows:nav  Enter:act  Z:undo  N:new  A:auto"),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Top row labels.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: TOP_ROW_Y - 14.0,
            text: String::from("Free Cells"),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: Self::top_row_x(4),
            y: TOP_ROW_Y - 14.0,
            text: String::from("Foundations"),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Separator line between top row and tableau.
        cmds.push(RenderCommand::Line {
            x1: PADDING,
            y1: TABLEAU_Y - 12.0,
            x2: Self::tableau_col_x(TABLEAU_COLS - 1) + CARD_WIDTH,
            y2: TABLEAU_Y - 12.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Render free cells.
        for i in 0..FREE_CELL_COUNT {
            let x = Self::top_row_x(i);
            self.render_free_cell(&mut cmds, i, x, TOP_ROW_Y);
        }

        // Render foundations.
        for i in 0..FOUNDATION_COUNT {
            let x = Self::top_row_x(i + 4);
            self.render_foundation(&mut cmds, i, x, TOP_ROW_Y);
        }

        // Render tableau.
        for col in 0..TABLEAU_COLS {
            self.render_tableau_col(&mut cmds, col);
        }

        // Win overlay.
        if self.won {
            self.render_win_overlay(&mut cmds);
        }

        cmds
    }

    /// Render a free cell.
    fn render_free_cell(
        &self,
        cmds: &mut Vec<RenderCommand>,
        idx: usize,
        x: f32,
        y: f32,
    ) {
        let is_focused = self.focus == FocusArea::FreeCell(idx);
        let is_selected = self.selection == Some(Selection::FreeCell(idx));

        match self.free_cells[idx] {
            Some(card) => {
                self.render_card_face(cmds, card, x, y, is_focused, is_selected);
            }
            None => {
                self.render_empty_pile(cmds, x, y, is_focused);
            }
        }
    }

    /// Render a foundation pile.
    fn render_foundation(
        &self,
        cmds: &mut Vec<RenderCommand>,
        idx: usize,
        x: f32,
        y: f32,
    ) {
        let is_focused = self.focus == FocusArea::Foundation(idx);

        match self.foundation_top(idx) {
            Some(card) => {
                self.render_card_face(cmds, card, x, y, is_focused, false);
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
            let is_focused = self.focus == FocusArea::Tableau(col);
            self.render_empty_pile(cmds, x, TABLEAU_Y, is_focused);
            return;
        }

        let last_idx = pile.len() - 1;
        for (i, &card) in pile.iter().enumerate() {
            let y = TABLEAU_Y + i as f32 * CASCADE_OFFSET;
            let is_top = i == last_idx;
            let is_focused = is_top && self.focus == FocusArea::Tableau(col);
            let is_selected = is_top && self.selection == Some(Selection::Tableau(col));
            self.render_card_face(cmds, card, x, y, is_focused, is_selected);
        }
    }

    /// Render an empty pile placeholder.
    fn render_empty_pile(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        focused: bool,
    ) {
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

        // Bottom-right rank.
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
            width: 900.0,
            height: 800.0,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 280.0,
            y: 300.0,
            text: String::from("You Win!"),
            color: GREEN,
            font_size: OVERLAY_FONT_SIZE + 12.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 300.0,
            y: 350.0,
            text: format!("Moves: {}", self.move_count),
            color: SUBTEXT0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: 270.0,
            y: 390.0,
            text: String::from("Press N for a new game"),
            color: OVERLAY0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ── Application wrapper ─────────────────────────────────────────────

/// The FreeCell application.
struct FreeCell {
    state: GameState,
}

impl FreeCell {
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
    let _app = FreeCell::new();
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
        state.handle_key(key, Modifiers { shift: false, ctrl: false, alt: false, super_key: false });
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
            rng: Rng::new(99),
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
    fn test_rank_from_value() {
        for r in &Rank::ALL {
            assert_eq!(Rank::from_value(r.value()), Some(*r));
        }
        assert_eq!(Rank::from_value(0), None);
        assert_eq!(Rank::from_value(14), None);
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
            state.free_cells[i] = Some(card(Suit::Hearts, Rank::from_value(i as u8 + 1).unwrap()));
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
            state.free_cells[i] = Some(card(Suit::Hearts, Rank::from_value(i as u8 + 1).unwrap()));
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
        let moved = state.auto_move_to_foundations();
        assert_eq!(moved, 1);
        assert_eq!(state.foundations[0].len(), 1);
    }

    #[test]
    fn test_auto_move_ace_from_freecell() {
        let mut state = empty_game();
        state.free_cells[0] = Some(card(Suit::Clubs, Rank::Ace));
        let moved = state.auto_move_to_foundations();
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
        let moved = state.auto_move_to_foundations();
        assert_eq!(moved, 1);
        assert_eq!(state.foundations[0].len(), 2);
    }

    #[test]
    fn test_auto_move_chain() {
        let mut state = empty_game();
        // Set up ace, then two in different columns.
        state.tableau[0].push(card(Suit::Hearts, Rank::Ace));
        state.tableau[1].push(card(Suit::Hearts, Rank::Two));
        let moved = state.auto_move_to_foundations();
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
        let moved = state.auto_move_to_foundations();
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
        let moved = state.auto_move_to_foundations();
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
        let moved = state.auto_move_to_foundations();
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
        let moved = state.auto_move_to_foundations();
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

    // ── RNG tests ──────────────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        // Extremely unlikely to be equal.
        assert_ne!(r1.next(), r2.next());
    }

    #[test]
    fn test_rng_next_range() {
        let mut rng = Rng::new(42);
        for _ in 0..100 {
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
    fn test_rng_shuffle() {
        let mut rng = Rng::new(42);
        let mut v: Vec<i32> = (0..10).collect();
        let original: Vec<i32> = (0..10).collect();
        rng.shuffle(&mut v);
        // After shuffle, should contain same elements.
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, original);
        // Should be rearranged (extremely unlikely to stay the same).
        assert_ne!(v, original);
    }

    // ── Rendering tests ────────────────────────────────────────────

    #[test]
    fn test_render_not_empty() {
        let state = new_game();
        let cmds = state.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_win_overlay() {
        let mut state = empty_game();
        for &suit in &Suit::ALL {
            for &rank in &Rank::ALL {
                state.foundations[suit.index()].push(card(suit, rank));
            }
        }
        state.won = true;
        let cmds = state.render();
        // Should contain "You Win!" somewhere in text commands.
        let has_win = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("You Win!")
            } else {
                false
            }
        });
        assert!(has_win);
    }

    #[test]
    fn test_render_includes_title() {
        let state = new_game();
        let cmds = state.render();
        let has_title = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("FreeCell")
            } else {
                false
            }
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_includes_move_count() {
        let state = new_game();
        let cmds = state.render();
        let has_moves = cmds.iter().any(|cmd| {
            if let RenderCommand::Text { text, .. } = cmd {
                text.contains("Moves:")
            } else {
                false
            }
        });
        assert!(has_moves);
    }

    #[test]
    fn test_render_empty_game() {
        let state = empty_game();
        let cmds = state.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selection() {
        let mut state = new_game();
        state.selection = Some(Selection::Tableau(0));
        let cmds = state.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_freecell_focus() {
        let mut state = new_game();
        state.focus = FocusArea::FreeCell(0);
        let cmds = state.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_foundation_focus() {
        let mut state = new_game();
        state.focus = FocusArea::Foundation(0);
        let cmds = state.render();
        assert!(!cmds.is_empty());
    }

    // ── Layout position tests ──────────────────────────────────────

    #[test]
    fn test_top_row_x_positions() {
        let x0 = GameState::top_row_x(0);
        let x1 = GameState::top_row_x(1);
        assert!(x1 > x0);
        assert!((x1 - x0 - (CARD_WIDTH + CARD_GAP_X)).abs() < 0.01);
    }

    #[test]
    fn test_tableau_col_x_positions() {
        let x0 = GameState::tableau_col_x(0);
        let x1 = GameState::tableau_col_x(1);
        assert!(x1 > x0);
        assert!((x1 - x0 - (CARD_WIDTH + CARD_GAP_X)).abs() < 0.01);
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
    fn test_app_new() {
        let app = FreeCell::new();
        assert_eq!(app.state.move_count, 0);
        assert!(!app.state.won);
    }

    #[test]
    fn test_app_render() {
        let app = FreeCell::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_handle_event() {
        let mut app = FreeCell::new();
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers { shift: false, ctrl: false, alt: false, super_key: false },
            pressed: true,
            text: None,
        }));
        assert_eq!(app.state.focus, FocusArea::Tableau(1));
    }

    #[test]
    fn test_app_handle_event_not_pressed() {
        let mut app = FreeCell::new();
        // Key release events should be ignored.
        app.handle_event(Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers { shift: false, ctrl: false, alt: false, super_key: false },
            pressed: false,
            text: None,
        }));
        assert_eq!(app.state.focus, FocusArea::Tableau(0));
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
        state.auto_move_to_foundations();
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
}
