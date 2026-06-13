#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::similar_names)]
#![allow(clippy::struct_excessive_bools)]

//! SlateOS Flashcards -- spaced-repetition study application.
//!
//! Features:
//! - Multiple decks with create/edit/delete
//! - Card CRUD (front/back text, tags)
//! - Study mode with flip animation and SM-2 spaced repetition
//! - Scoring: Easy / Good / Hard / Again
//! - Per-deck and per-card statistics
//! - Tag filtering and search
//! - Deck shuffle
//! - Import/export (simple text format)
//! - Three sample decks pre-loaded

use guitk::color::Color;
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── SM-2 defaults ───────────────────────────────────────────────────
const SM2_INITIAL_EASE: f32 = 2.5;
const SM2_MIN_EASE: f32 = 1.3;

// ── Scoring ─────────────────────────────────────────────────────────
/// Quality ratings for SM-2 algorithm (0..5 scale mapped to our four buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rating {
    Again, // quality = 0
    Hard,  // quality = 2
    Good,  // quality = 3
    Easy,  // quality = 5
}

impl Rating {
    fn quality(self) -> u8 {
        match self {
            Self::Again => 0,
            Self::Hard => 2,
            Self::Good => 3,
            Self::Easy => 5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Again => "Again",
            Self::Hard => "Hard",
            Self::Good => "Good",
            Self::Easy => "Easy",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Again => RED,
            Self::Hard => PEACH,
            Self::Good => BLUE,
            Self::Easy => GREEN,
        }
    }
}

const ALL_RATINGS: [Rating; 4] = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy];

// ── SM-2 review data per card ───────────────────────────────────────
#[derive(Clone, Debug)]
struct ReviewData {
    /// Number of consecutive correct reviews (quality >= 3).
    repetitions: u32,
    /// Current ease factor (starts at 2.5).
    ease_factor: f32,
    /// Inter-repetition interval in days.
    interval_days: u32,
    /// Simulated day of last review (monotonic counter).
    last_review_day: u32,
    /// Total number of reviews this card has received.
    total_reviews: u32,
    /// Count of each rating received.
    rating_counts: [u32; 4], // Again, Hard, Good, Easy
}

impl ReviewData {
    fn new() -> Self {
        Self {
            repetitions: 0,
            ease_factor: SM2_INITIAL_EASE,
            interval_days: 0,
            last_review_day: 0,
            total_reviews: 0,
            rating_counts: [0; 4],
        }
    }

    /// Apply SM-2 algorithm after a review.
    fn apply_rating(&mut self, rating: Rating, current_day: u32) {
        let q = rating.quality();
        self.total_reviews += 1;
        let idx = match rating {
            Rating::Again => 0,
            Rating::Hard => 1,
            Rating::Good => 2,
            Rating::Easy => 3,
        };
        if let Some(count) = self.rating_counts.get_mut(idx) {
            *count += 1;
        }
        self.last_review_day = current_day;

        if q < 3 {
            // Failed: reset repetitions
            self.repetitions = 0;
            self.interval_days = 1;
        } else {
            self.repetitions += 1;
            match self.repetitions {
                1 => self.interval_days = 1,
                2 => self.interval_days = 6,
                _ => {
                    let new_interval = (self.interval_days as f32 * self.ease_factor) as u32;
                    self.interval_days = new_interval.max(1);
                }
            }
        }

        // Update ease factor: EF' = EF + (0.1 - (5-q)*(0.08 + (5-q)*0.02))
        let q_f = q as f32;
        let delta = 0.1 - (5.0 - q_f) * (0.08 + (5.0 - q_f) * 0.02);
        self.ease_factor += delta;
        if self.ease_factor < SM2_MIN_EASE {
            self.ease_factor = SM2_MIN_EASE;
        }
    }

    /// Is this card due for review on the given day?
    fn is_due(&self, current_day: u32) -> bool {
        if self.total_reviews == 0 {
            return true; // never reviewed
        }
        current_day >= self.last_review_day + self.interval_days
    }

    /// Accuracy as a percentage (0..100). Returns 0 if no reviews.
    fn accuracy_percent(&self) -> u32 {
        if self.total_reviews == 0 {
            return 0;
        }
        let good_and_easy = self.rating_counts[2] + self.rating_counts[3];
        (good_and_easy * 100) / self.total_reviews
    }
}

// ── Card ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Card {
    id: u32,
    front: String,
    back: String,
    tags: Vec<String>,
    review: ReviewData,
}

impl Card {
    fn new(id: u32, front: &str, back: &str) -> Self {
        Self {
            id,
            front: String::from(front),
            back: String::from(back),
            tags: Vec::new(),
            review: ReviewData::new(),
        }
    }

    fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|t| String::from(*t)).collect();
        self
    }

    fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_ascii_lowercase();
        self.front.to_ascii_lowercase().contains(&q)
            || self.back.to_ascii_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_ascii_lowercase().contains(&q))
    }

    fn has_tag(&self, tag: &str) -> bool {
        let t = tag.to_ascii_lowercase();
        self.tags.iter().any(|ct| ct.to_ascii_lowercase() == t)
    }
}

// ── Deck ────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Deck {
    name: String,
    description: String,
    cards: Vec<Card>,
    next_card_id: u32,
}

impl Deck {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: String::from(name),
            description: String::from(description),
            cards: Vec::new(),
            next_card_id: 1,
        }
    }

    fn add_card(&mut self, front: &str, back: &str) -> u32 {
        let id = self.next_card_id;
        self.next_card_id += 1;
        self.cards.push(Card::new(id, front, back));
        id
    }

    fn add_card_with_tags(&mut self, front: &str, back: &str, tags: &[&str]) -> u32 {
        let id = self.next_card_id;
        self.next_card_id += 1;
        self.cards.push(Card::new(id, front, back).with_tags(tags));
        id
    }

    fn remove_card(&mut self, card_id: u32) -> bool {
        if let Some(pos) = self.cards.iter().position(|c| c.id == card_id) {
            self.cards.remove(pos);
            true
        } else {
            false
        }
    }

    fn find_card(&self, card_id: u32) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == card_id)
    }

    fn find_card_mut(&mut self, card_id: u32) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.id == card_id)
    }

    fn due_cards(&self, current_day: u32) -> Vec<usize> {
        self.cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.review.is_due(current_day))
            .map(|(i, _)| i)
            .collect()
    }

    fn total_reviews(&self) -> u32 {
        self.cards.iter().map(|c| c.review.total_reviews).sum()
    }

    fn average_accuracy(&self) -> u32 {
        let reviewed: Vec<_> = self.cards.iter().filter(|c| c.review.total_reviews > 0).collect();
        if reviewed.is_empty() {
            return 0;
        }
        let sum: u32 = reviewed.iter().map(|c| c.review.accuracy_percent()).sum();
        sum / reviewed.len() as u32
    }

    fn mastered_count(&self) -> usize {
        self.cards
            .iter()
            .filter(|c| c.review.repetitions >= 3 && c.review.ease_factor >= 2.0)
            .count()
    }

    fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = Vec::new();
        for card in &self.cards {
            for tag in &card.tags {
                let lower = tag.to_ascii_lowercase();
                if !tags.iter().any(|t| t.to_ascii_lowercase() == lower) {
                    tags.push(tag.clone());
                }
            }
        }
        tags.sort();
        tags
    }

    fn cards_matching(&self, query: &str, tag_filter: Option<&str>) -> Vec<usize> {
        self.cards
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let search_ok = c.matches_search(query);
                let tag_ok = tag_filter.is_none_or(|t| c.has_tag(t));
                search_ok && tag_ok
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Shuffle the cards using a simple deterministic shuffle (seed-based).
    fn shuffle(&mut self, seed: u32) {
        let len = self.cards.len();
        if len <= 1 {
            return;
        }
        // Fisher-Yates using a simple LCG
        let mut state = seed.wrapping_add(1);
        for i in (1..len).rev() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let j = (state as usize) % (i + 1);
            self.cards.swap(i, j);
        }
    }

    /// Export deck to a simple text format.
    fn export_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n", self.name));
        out.push_str(&format!("## {}\n", self.description));
        for card in &self.cards {
            out.push_str(&format!("Q: {}\n", card.front));
            out.push_str(&format!("A: {}\n", card.back));
            if !card.tags.is_empty() {
                out.push_str(&format!("T: {}\n", card.tags.join(",")));
            }
            out.push('\n');
        }
        out
    }

    /// Import cards from a simple text format. Returns count of imported cards.
    fn import_text(&mut self, text: &str) -> u32 {
        let mut count = 0u32;
        let mut front: Option<String> = None;
        let mut back: Option<String> = None;
        let mut tags: Vec<String> = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // End of a card block
                if let (Some(f), Some(b)) = (front.take(), back.take()) {
                    let id = self.next_card_id;
                    self.next_card_id += 1;
                    let mut card = Card::new(id, &f, &b);
                    card.tags = tags.clone();
                    self.cards.push(card);
                    count += 1;
                    tags.clear();
                }
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("Q: ") {
                front = Some(String::from(rest));
            } else if let Some(rest) = trimmed.strip_prefix("A: ") {
                back = Some(String::from(rest));
            } else if let Some(rest) = trimmed.strip_prefix("T: ") {
                tags = rest.split(',').map(|s| String::from(s.trim())).collect();
            }
            // Skip lines starting with # or ##
        }
        // Handle last card if no trailing blank line
        if let (Some(f), Some(b)) = (front.take(), back.take()) {
            let id = self.next_card_id;
            self.next_card_id += 1;
            let mut card = Card::new(id, &f, &b);
            card.tags = tags;
            self.cards.push(card);
            count += 1;
        }
        count
    }
}

// ── Application views ───────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppView {
    DeckList,
    DeckDetail,
    CardEditor,
    StudyMode,
    Statistics,
}

// ── Study session state ─────────────────────────────────────────────
#[derive(Clone, Debug)]
struct StudySession {
    /// Indices into the deck's card list, in study order.
    queue: Vec<usize>,
    /// Current position in the queue.
    current_pos: usize,
    /// Whether the current card is flipped (showing the back).
    flipped: bool,
    /// Count of cards reviewed this session.
    reviewed: u32,
    /// Ratings given this session (Again, Hard, Good, Easy).
    session_ratings: [u32; 4],
}

impl StudySession {
    fn new(queue: Vec<usize>) -> Self {
        Self {
            queue,
            current_pos: 0,
            flipped: false,
            reviewed: 0,
            session_ratings: [0; 4],
        }
    }

    fn current_card_idx(&self) -> Option<usize> {
        self.queue.get(self.current_pos).copied()
    }

    fn is_complete(&self) -> bool {
        self.queue.is_empty() || self.current_pos >= self.queue.len()
    }

    fn remaining(&self) -> usize {
        if self.current_pos >= self.queue.len() {
            0
        } else {
            self.queue.len() - self.current_pos
        }
    }

    fn record_rating(&mut self, rating: Rating) {
        let idx = match rating {
            Rating::Again => 0,
            Rating::Hard => 1,
            Rating::Good => 2,
            Rating::Easy => 3,
        };
        if let Some(count) = self.session_ratings.get_mut(idx) {
            *count += 1;
        }
        self.reviewed += 1;
    }

    fn session_accuracy(&self) -> u32 {
        if self.reviewed == 0 {
            return 0;
        }
        let good_easy = self.session_ratings[2] + self.session_ratings[3];
        (good_easy * 100) / self.reviewed
    }
}

// ── Main application ────────────────────────────────────────────────
struct FlashcardsApp {
    width: f32,
    height: f32,
    view: AppView,
    decks: Vec<Deck>,
    selected_deck: usize,
    /// Current simulated day (for spaced repetition scheduling).
    current_day: u32,
    /// Active study session.
    study_session: Option<StudySession>,
    /// Search query (used in deck detail view).
    search_query: String,
    /// Tag filter (used in deck detail view).
    tag_filter: Option<String>,
    /// Selected card index in deck detail view.
    selected_card: usize,
    /// Card editor state: editing which card ID (None = new card).
    editing_card_id: Option<u32>,
    editor_front: String,
    editor_back: String,
    editor_tags: String,
    /// Scroll offset for card lists.
    scroll_offset: usize,
    /// Status message displayed at the bottom.
    status_msg: String,
    /// Shuffle seed counter.
    shuffle_seed: u32,
}

impl FlashcardsApp {
    fn new() -> Self {
        let decks = vec![
            Self::sample_world_capitals(),
            Self::sample_programming(),
            Self::sample_science(),
        ];

        Self {
            width: 1000.0,
            height: 700.0,
            view: AppView::DeckList,
            decks,
            selected_deck: 0,
            current_day: 1,
            study_session: None,
            search_query: String::new(),
            tag_filter: None,
            selected_card: 0,
            editing_card_id: None,
            editor_front: String::new(),
            editor_back: String::new(),
            editor_tags: String::new(),
            scroll_offset: 0,
            status_msg: String::from("Welcome to Flashcards"),
            shuffle_seed: 42,
        }
    }

    // ── Sample decks ────────────────────────────────────────────────
    fn sample_world_capitals() -> Deck {
        let mut deck = Deck::new("World Capitals", "Capital cities of countries around the world");
        deck.add_card_with_tags("What is the capital of France?", "Paris", &["europe", "western"]);
        deck.add_card_with_tags("What is the capital of Japan?", "Tokyo", &["asia", "east-asia"]);
        deck.add_card_with_tags("What is the capital of Brazil?", "Brasilia", &["south-america"]);
        deck.add_card_with_tags("What is the capital of Australia?", "Canberra", &["oceania"]);
        deck.add_card_with_tags("What is the capital of Egypt?", "Cairo", &["africa"]);
        deck.add_card_with_tags("What is the capital of Canada?", "Ottawa", &["north-america"]);
        deck.add_card_with_tags("What is the capital of Germany?", "Berlin", &["europe", "western"]);
        deck.add_card_with_tags("What is the capital of South Korea?", "Seoul", &["asia", "east-asia"]);
        deck.add_card_with_tags("What is the capital of Argentina?", "Buenos Aires", &["south-america"]);
        deck.add_card_with_tags("What is the capital of India?", "New Delhi", &["asia", "south-asia"]);
        deck
    }

    fn sample_programming() -> Deck {
        let mut deck = Deck::new("Programming Concepts", "Fundamental CS and programming terms");
        deck.add_card_with_tags("What does RAII stand for?", "Resource Acquisition Is Initialization", &["rust", "cpp", "memory"]);
        deck.add_card_with_tags("What is a closure?", "A function that captures variables from its enclosing scope", &["functional", "rust"]);
        deck.add_card_with_tags("What is Big-O notation?", "A mathematical notation describing the upper bound of an algorithm's growth rate", &["algorithms", "complexity"]);
        deck.add_card_with_tags("What is a mutex?", "A synchronization primitive that provides mutual exclusion for shared data", &["concurrency", "rust"]);
        deck.add_card_with_tags("What is polymorphism?", "The ability to process objects differently based on their type or class", &["oop", "design"]);
        deck.add_card_with_tags("What is a hash map?", "A data structure mapping keys to values via a hash function, with O(1) average lookup", &["data-structures"]);
        deck.add_card_with_tags("What is recursion?", "A technique where a function calls itself to solve smaller sub-problems", &["algorithms"]);
        deck.add_card_with_tags("What is the stack vs the heap?", "Stack: LIFO, automatic, fast. Heap: dynamic, manual/GC, flexible size.", &["memory", "systems"]);
        deck.add_card_with_tags("What is a trait in Rust?", "A collection of methods defined for an unknown type, enabling polymorphism", &["rust", "oop"]);
        deck.add_card_with_tags("What is TCP vs UDP?", "TCP: reliable, ordered, connection-based. UDP: unreliable, fast, connectionless.", &["networking"]);
        deck
    }

    fn sample_science() -> Deck {
        let mut deck = Deck::new("Science Basics", "Elementary science facts and concepts");
        deck.add_card_with_tags("What is the chemical symbol for water?", "H2O", &["chemistry"]);
        deck.add_card_with_tags("What is the speed of light?", "Approximately 299,792,458 m/s", &["physics"]);
        deck.add_card_with_tags("What is DNA?", "Deoxyribonucleic acid, the molecule carrying genetic instructions", &["biology"]);
        deck.add_card_with_tags("What is Newton's first law?", "An object at rest stays at rest; an object in motion stays in motion unless acted upon by a force", &["physics"]);
        deck.add_card_with_tags("What is photosynthesis?", "The process by which plants convert sunlight, CO2, and water into glucose and oxygen", &["biology", "chemistry"]);
        deck.add_card_with_tags("What is the periodic table?", "A tabular arrangement of chemical elements ordered by atomic number", &["chemistry"]);
        deck.add_card_with_tags("What is mitosis?", "Cell division producing two genetically identical daughter cells", &["biology"]);
        deck.add_card_with_tags("What is E = mc^2?", "Einstein's mass-energy equivalence: energy equals mass times the speed of light squared", &["physics"]);
        deck.add_card_with_tags("What is an atom?", "The smallest unit of matter that retains the properties of an element", &["chemistry", "physics"]);
        deck.add_card_with_tags("What is evolution?", "Change in heritable characteristics of populations over successive generations", &["biology"]);
        deck
    }

    // ── Deck operations ─────────────────────────────────────────────
    fn current_deck(&self) -> Option<&Deck> {
        self.decks.get(self.selected_deck)
    }

    fn current_deck_mut(&mut self) -> Option<&mut Deck> {
        self.decks.get_mut(self.selected_deck)
    }

    fn add_deck(&mut self, name: &str, description: &str) {
        self.decks.push(Deck::new(name, description));
        self.status_msg = format!("Created deck: {name}");
    }

    fn remove_deck(&mut self, idx: usize) {
        if idx < self.decks.len() && self.decks.len() > 1 {
            let name = self.decks[idx].name.clone();
            self.decks.remove(idx);
            if self.selected_deck >= self.decks.len() {
                self.selected_deck = self.decks.len().saturating_sub(1);
            }
            self.status_msg = format!("Deleted deck: {name}");
        }
    }

    fn select_deck(&mut self, idx: usize) {
        if idx < self.decks.len() {
            self.selected_deck = idx;
            self.selected_card = 0;
            self.scroll_offset = 0;
            self.search_query.clear();
            self.tag_filter = None;
            self.view = AppView::DeckDetail;
        }
    }

    // ── Study session ───────────────────────────────────────────────
    fn start_study(&mut self) {
        if let Some(deck) = self.decks.get(self.selected_deck) {
            let due = deck.due_cards(self.current_day);
            if due.is_empty() {
                self.status_msg = String::from("No cards due for review!");
                return;
            }
            self.study_session = Some(StudySession::new(due));
            self.view = AppView::StudyMode;
            self.status_msg = String::from("Study session started");
        }
    }

    fn start_study_all(&mut self) {
        if let Some(deck) = self.decks.get(self.selected_deck) {
            if deck.cards.is_empty() {
                self.status_msg = String::from("Deck is empty!");
                return;
            }
            let all: Vec<usize> = (0..deck.cards.len()).collect();
            self.study_session = Some(StudySession::new(all));
            self.view = AppView::StudyMode;
            self.status_msg = String::from("Studying all cards");
        }
    }

    fn flip_card(&mut self) {
        if let Some(session) = &mut self.study_session {
            session.flipped = true;
        }
    }

    fn rate_card(&mut self, rating: Rating) {
        let day = self.current_day;
        let deck_idx = self.selected_deck;

        // Get the card index from the session, record rating in session
        let card_idx = {
            let session = match &mut self.study_session {
                Some(s) => s,
                None => return,
            };
            if !session.flipped {
                return; // must flip first
            }
            let idx = match session.current_card_idx() {
                Some(i) => i,
                None => return,
            };
            session.record_rating(rating);
            idx
        };

        // Apply SM-2 to the card in the deck
        if let Some(deck) = self.decks.get_mut(deck_idx)
            && let Some(card) = deck.cards.get_mut(card_idx) {
                card.review.apply_rating(rating, day);
            }

        // Advance to next card
        if let Some(session) = &mut self.study_session {
            session.current_pos += 1;
            session.flipped = false;
        }
    }

    fn end_study(&mut self) {
        self.study_session = None;
        self.view = AppView::DeckDetail;
        self.status_msg = String::from("Study session ended");
    }

    fn advance_day(&mut self) {
        self.current_day += 1;
        self.status_msg = format!("Day {} -- cards may be due for review", self.current_day);
    }

    // ── Card editor ─────────────────────────────────────────────────
    fn open_new_card_editor(&mut self) {
        self.editing_card_id = None;
        self.editor_front.clear();
        self.editor_back.clear();
        self.editor_tags.clear();
        self.view = AppView::CardEditor;
    }

    fn open_edit_card(&mut self, card_id: u32) {
        let card_data = self
            .current_deck()
            .and_then(|deck| deck.find_card(card_id))
            .map(|card| (card.front.clone(), card.back.clone(), card.tags.join(", ")));
        if let Some((front, back, tags)) = card_data {
            self.editing_card_id = Some(card_id);
            self.editor_front = front;
            self.editor_back = back;
            self.editor_tags = tags;
            self.view = AppView::CardEditor;
        }
    }

    fn save_card(&mut self) -> bool {
        if self.editor_front.trim().is_empty() || self.editor_back.trim().is_empty() {
            self.status_msg = String::from("Front and back text are required");
            return false;
        }
        let front = self.editor_front.trim().to_string();
        let back = self.editor_back.trim().to_string();
        let tags: Vec<String> = self
            .editor_tags
            .split(',')
            .map(|s| String::from(s.trim()))
            .filter(|s| !s.is_empty())
            .collect();

        if let Some(card_id) = self.editing_card_id {
            // Update existing card
            if let Some(deck) = self.current_deck_mut()
                && let Some(card) = deck.find_card_mut(card_id) {
                    card.front = front;
                    card.back = back;
                    card.tags = tags;
                    self.status_msg = String::from("Card updated");
                    self.view = AppView::DeckDetail;
                    return true;
                }
        } else {
            // Create new card
            if let Some(deck) = self.current_deck_mut() {
                let id = deck.next_card_id;
                deck.next_card_id += 1;
                let mut card = Card::new(id, &front, &back);
                card.tags = tags;
                deck.cards.push(card);
                self.status_msg = String::from("Card added");
                self.view = AppView::DeckDetail;
                return true;
            }
        }
        false
    }

    fn delete_selected_card(&mut self) {
        let matching = self.matching_card_indices();
        if let Some(&card_list_idx) = matching.get(self.selected_card)
            && let Some(deck) = self.current_deck_mut()
                && card_list_idx < deck.cards.len() {
                    deck.cards.remove(card_list_idx);
                    self.status_msg = String::from("Card deleted");
                    if self.selected_card > 0 && self.selected_card >= matching.len().saturating_sub(1) {
                        self.selected_card = self.selected_card.saturating_sub(1);
                    }
                }
    }

    fn matching_card_indices(&self) -> Vec<usize> {
        if let Some(deck) = self.current_deck() {
            deck.cards_matching(&self.search_query, self.tag_filter.as_deref())
        } else {
            Vec::new()
        }
    }

    // ── Key handling ────────────────────────────────────────────────
    fn handle_key(&mut self, key: &str, ctrl: bool, _shift: bool) {
        match self.view {
            AppView::DeckList => self.handle_key_deck_list(key, ctrl),
            AppView::DeckDetail => self.handle_key_deck_detail(key, ctrl),
            AppView::CardEditor => self.handle_key_card_editor(key),
            AppView::StudyMode => self.handle_key_study(key),
            AppView::Statistics => self.handle_key_statistics(key),
        }
    }

    fn handle_key_deck_list(&mut self, key: &str, _ctrl: bool) {
        match key {
            "Up" | "k"
                if self.selected_deck > 0 => {
                    self.selected_deck -= 1;
                }
            "Down" | "j"
                if self.selected_deck + 1 < self.decks.len() => {
                    self.selected_deck += 1;
                }
            "Enter" => self.select_deck(self.selected_deck),
            "n" => self.add_deck("New Deck", ""),
            "Delete" | "x" => {
                let idx = self.selected_deck;
                self.remove_deck(idx);
            }
            _ => {}
        }
    }

    fn handle_key_deck_detail(&mut self, key: &str, _ctrl: bool) {
        match key {
            "Escape" => {
                self.view = AppView::DeckList;
                self.search_query.clear();
                self.tag_filter = None;
            }
            "Up" | "k"
                if self.selected_card > 0 => {
                    self.selected_card -= 1;
                    self.ensure_card_visible();
                }
            "Down" | "j" => {
                let count = self.matching_card_indices().len();
                if self.selected_card + 1 < count {
                    self.selected_card += 1;
                    self.ensure_card_visible();
                }
            }
            "s" => self.start_study(),
            "S" => self.start_study_all(),
            "n" => self.open_new_card_editor(),
            "e" => {
                let matching = self.matching_card_indices();
                if let Some(&idx) = matching.get(self.selected_card)
                    && let Some(deck) = self.current_deck()
                        && let Some(card) = deck.cards.get(idx) {
                            let cid = card.id;
                            self.open_edit_card(cid);
                        }
            }
            "Delete" | "x" => self.delete_selected_card(),
            "d" => self.advance_day(),
            "r" => {
                self.shuffle_seed = self.shuffle_seed.wrapping_add(7);
                let seed = self.shuffle_seed;
                if let Some(deck) = self.current_deck_mut() {
                    deck.shuffle(seed);
                }
                self.status_msg = String::from("Deck shuffled");
            }
            "t" => {
                // Cycle through tag filters
                if let Some(deck) = self.current_deck() {
                    let tags = deck.all_tags();
                    if tags.is_empty() {
                        return;
                    }
                    let next = match &self.tag_filter {
                        None => Some(tags[0].clone()),
                        Some(current) => {
                            let pos = tags.iter().position(|t| t == current);
                            match pos {
                                Some(i) if i + 1 < tags.len() => Some(tags[i + 1].clone()),
                                _ => None,
                            }
                        }
                    };
                    self.tag_filter = next.clone();
                    self.selected_card = 0;
                    self.scroll_offset = 0;
                    match next {
                        Some(tag) => self.status_msg = format!("Filter: {tag}"),
                        None => self.status_msg = String::from("Filter cleared"),
                    }
                }
            }
            "i" => self.view = AppView::Statistics,
            _ => {}
        }
    }

    fn handle_key_card_editor(&mut self, key: &str) {
        match key {
            "Escape" => self.view = AppView::DeckDetail,
            "Enter" => { self.save_card(); }
            _ => {}
        }
    }

    fn handle_key_study(&mut self, key: &str) {
        match key {
            "Escape" => self.end_study(),
            "Space" => self.flip_card(),
            "1" => self.rate_card(Rating::Again),
            "2" => self.rate_card(Rating::Hard),
            "3" => self.rate_card(Rating::Good),
            "4" => self.rate_card(Rating::Easy),
            _ => {}
        }
    }

    fn handle_key_statistics(&mut self, key: &str) {
        if key == "Escape" {
            self.view = AppView::DeckDetail;
        }
    }

    fn handle_search_text(&mut self, text: &str) {
        if self.view == AppView::DeckDetail {
            self.search_query.push_str(text);
            self.selected_card = 0;
            self.scroll_offset = 0;
        }
    }

    fn handle_search_backspace(&mut self) {
        if self.view == AppView::DeckDetail && !self.search_query.is_empty() {
            self.search_query.pop();
            self.selected_card = 0;
            self.scroll_offset = 0;
        }
    }

    fn ensure_card_visible(&mut self) {
        let visible_rows = 8usize;
        if self.selected_card < self.scroll_offset {
            self.scroll_offset = self.selected_card;
        } else if self.selected_card >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_card + 1 - visible_rows;
        }
    }

    // ── Layout constants ────────────────────────────────────────────
    const HEADER_H: f32 = 50.0;
    const STATUS_H: f32 = 28.0;
    const SIDEBAR_W: f32 = 240.0;
    const CARD_ROW_H: f32 = 48.0;
    const PADDING: f32 = 16.0;

    // ── Rendering ───────────────────────────────────────────────────
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);

        match self.view {
            AppView::DeckList => self.render_deck_list(&mut cmds),
            AppView::DeckDetail => self.render_deck_detail(&mut cmds),
            AppView::CardEditor => self.render_card_editor(&mut cmds),
            AppView::StudyMode => self.render_study_mode(&mut cmds),
            AppView::Statistics => self.render_statistics(&mut cmds),
        }

        self.render_status_bar(&mut cmds);
        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: Self::HEADER_H,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: 14.0,
            text: String::from("Flashcards"),
            font_size: 20.0,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
        });

        // View indicator
        let view_label = match self.view {
            AppView::DeckList => "Decks",
            AppView::DeckDetail => "Cards",
            AppView::CardEditor => "Editor",
            AppView::StudyMode => "Study",
            AppView::Statistics => "Stats",
        };
        cmds.push(RenderCommand::Text {
            x: 180.0,
            y: 18.0,
            text: String::from(view_label),
            font_size: 14.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Day counter
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: 18.0,
            text: format!("Day {}", self.current_day),
            font_size: 14.0,
            color: TEAL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Header separator
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: Self::HEADER_H,
            x2: self.width,
            y2: Self::HEADER_H,
            color: SURFACE0,
            width: 1.0,
        });
    }

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.height - Self::STATUS_H;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.width,
            height: Self::STATUS_H,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: y + 7.0,
            text: self.status_msg.clone(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.width - 32.0),
        });
    }

    fn render_deck_list(&self, cmds: &mut Vec<RenderCommand>) {
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: String::from("Your Decks"),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 26.0,
            text: String::from("[N]ew deck  [Enter] open  [X] delete  [Up/Down] navigate"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
        });

        let list_top = top + 52.0;
        let row_h = 72.0;

        for (i, deck) in self.decks.iter().enumerate() {
            let y = list_top + (i as f32) * (row_h + 8.0);
            let is_selected = i == self.selected_deck;

            let bg = if is_selected { SURFACE1 } else { SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x: Self::PADDING,
                y,
                width: content_w,
                height: row_h,
                color: bg,
                corner_radii: CornerRadii::all(8.0),
            });

            if is_selected {
                cmds.push(RenderCommand::StrokeRect {
                    x: Self::PADDING,
                    y,
                    width: content_w,
                    height: row_h,
                    color: BLUE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(8.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: Self::PADDING + 16.0,
                y: y + 12.0,
                text: deck.name.clone(),
                font_size: 16.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(content_w - 200.0),
            });

            cmds.push(RenderCommand::Text {
                x: Self::PADDING + 16.0,
                y: y + 36.0,
                text: format!(
                    "{} cards | {} reviews | {}% accuracy",
                    deck.cards.len(),
                    deck.total_reviews(),
                    deck.average_accuracy()
                ),
                font_size: 12.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 48.0),
            });

            // Due count badge
            let due_count = deck.due_cards(self.current_day).len();
            if due_count > 0 {
                let badge_x = self.width - Self::PADDING - 90.0;
                cmds.push(RenderCommand::FillRect {
                    x: badge_x,
                    y: y + 14.0,
                    width: 72.0,
                    height: 24.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(12.0),
                });
                cmds.push(RenderCommand::Text {
                    x: badge_x + 8.0,
                    y: y + 18.0,
                    text: format!("{due_count} due"),
                    font_size: 12.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(60.0),
                });
            }

            if !deck.description.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: Self::PADDING + 16.0,
                    y: y + 52.0,
                    text: deck.description.clone(),
                    font_size: 11.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(content_w - 48.0),
                });
            }
        }
    }

    fn render_deck_detail(&self, cmds: &mut Vec<RenderCommand>) {
        let deck = match self.current_deck() {
            Some(d) => d,
            None => return,
        };

        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        // Deck title
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: deck.name.clone(),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
        });

        // Shortcuts
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 24.0,
            text: String::from("[S]tudy due  [Shift+S] all  [N]ew  [E]dit  [X] delete  [R]andom  [T]ag  [I]nfo  [D]ay+  [Esc] back"),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
        });

        // Search bar
        let search_y = top + 44.0;
        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING,
            y: search_y,
            width: content_w * 0.6,
            height: 28.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        let search_display = if self.search_query.is_empty() {
            String::from("Search cards...")
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 8.0,
            y: search_y + 7.0,
            text: search_display,
            font_size: 12.0,
            color: search_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w * 0.6 - 16.0),
        });

        // Tag filter display
        if let Some(tag) = &self.tag_filter {
            let tag_x = Self::PADDING + content_w * 0.6 + 8.0;
            cmds.push(RenderCommand::FillRect {
                x: tag_x,
                y: search_y + 2.0,
                width: 100.0,
                height: 24.0,
                color: MAUVE,
                corner_radii: CornerRadii::all(12.0),
            });
            cmds.push(RenderCommand::Text {
                x: tag_x + 8.0,
                y: search_y + 6.0,
                text: tag.clone(),
                font_size: 11.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(84.0),
            });
        }

        // Card list
        let list_top = search_y + 40.0;
        let matching = self.matching_card_indices();

        if matching.is_empty() {
            cmds.push(RenderCommand::Text {
                x: Self::PADDING + 8.0,
                y: list_top + 20.0,
                text: String::from("No cards match your search."),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w),
            });
            return;
        }

        // Column headers
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 8.0,
            y: list_top,
            text: String::from("Front"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });
        cmds.push(RenderCommand::Text {
            x: self.width * 0.5,
            y: list_top,
            text: String::from("Tags"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });
        cmds.push(RenderCommand::Text {
            x: self.width - 120.0,
            y: list_top,
            text: String::from("Status"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });

        let rows_top = list_top + 20.0;
        let visible_count = 8usize;

        let end = (self.scroll_offset + visible_count).min(matching.len());
        for (vis_i, list_i) in (self.scroll_offset..end).enumerate() {
            if let Some(&card_idx) = matching.get(list_i)
                && let Some(card) = deck.cards.get(card_idx) {
                    let y = rows_top + (vis_i as f32) * Self::CARD_ROW_H;
                    let is_selected = list_i == self.selected_card;
                    let bg = if is_selected { SURFACE1 } else { SURFACE0 };

                    cmds.push(RenderCommand::FillRect {
                        x: Self::PADDING,
                        y,
                        width: content_w,
                        height: Self::CARD_ROW_H - 4.0,
                        color: bg,
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Truncate front text for display
                    let front_display = if card.front.len() > 50 {
                        format!("{}...", &card.front[..47])
                    } else {
                        card.front.clone()
                    };
                    cmds.push(RenderCommand::Text {
                        x: Self::PADDING + 8.0,
                        y: y + 8.0,
                        text: front_display,
                        font_size: 13.0,
                        color: TEXT_COLOR,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(self.width * 0.45),
                    });

                    // Tags
                    let tags_str = card.tags.join(", ");
                    cmds.push(RenderCommand::Text {
                        x: self.width * 0.5,
                        y: y + 8.0,
                        text: if tags_str.is_empty() {
                            String::from("-")
                        } else {
                            tags_str
                        },
                        font_size: 11.0,
                        color: MAUVE,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(200.0),
                    });

                    // Review status
                    let status_text = if card.review.total_reviews == 0 {
                        String::from("New")
                    } else if card.review.repetitions >= 3 {
                        String::from("Mastered")
                    } else {
                        format!("{}d", card.review.interval_days)
                    };
                    let status_color = if card.review.total_reviews == 0 {
                        YELLOW
                    } else if card.review.repetitions >= 3 {
                        GREEN
                    } else {
                        SUBTEXT0
                    };
                    cmds.push(RenderCommand::Text {
                        x: self.width - 120.0,
                        y: y + 8.0,
                        text: status_text,
                        font_size: 12.0,
                        color: status_color,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(100.0),
                    });

                    // Back text preview on second line
                    let back_display = if card.back.len() > 60 {
                        format!("{}...", &card.back[..57])
                    } else {
                        card.back.clone()
                    };
                    cmds.push(RenderCommand::Text {
                        x: Self::PADDING + 8.0,
                        y: y + 26.0,
                        text: back_display,
                        font_size: 11.0,
                        color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(self.width * 0.45),
                    });
                }
        }

        // Scroll indicator
        if matching.len() > visible_count {
            cmds.push(RenderCommand::Text {
                x: self.width - 160.0,
                y: rows_top + (visible_count as f32) * Self::CARD_ROW_H + 4.0,
                text: format!(
                    "Showing {}-{} of {}",
                    self.scroll_offset + 1,
                    end,
                    matching.len()
                ),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });
        }
    }

    fn render_card_editor(&self, cmds: &mut Vec<RenderCommand>) {
        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;
        let field_w = content_w - 32.0;

        let title = if self.editing_card_id.is_some() {
            "Edit Card"
        } else {
            "New Card"
        };
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: String::from(title),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 26.0,
            text: String::from("[Enter] save  [Esc] cancel"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Front field
        let field_y = top + 56.0;
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 16.0,
            y: field_y,
            text: String::from("Front (Question):"),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING + 16.0,
            y: field_y + 20.0,
            width: field_w,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 24.0,
            y: field_y + 30.0,
            text: if self.editor_front.is_empty() {
                String::from("Enter question text...")
            } else {
                self.editor_front.clone()
            },
            font_size: 13.0,
            color: if self.editor_front.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 16.0),
        });

        // Back field
        let back_y = field_y + 72.0;
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 16.0,
            y: back_y,
            text: String::from("Back (Answer):"),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING + 16.0,
            y: back_y + 20.0,
            width: field_w,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 24.0,
            y: back_y + 30.0,
            text: if self.editor_back.is_empty() {
                String::from("Enter answer text...")
            } else {
                self.editor_back.clone()
            },
            font_size: 13.0,
            color: if self.editor_back.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 16.0),
        });

        // Tags field
        let tags_y = back_y + 72.0;
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 16.0,
            y: tags_y,
            text: String::from("Tags (comma-separated):"),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING + 16.0,
            y: tags_y + 20.0,
            width: field_w,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 24.0,
            y: tags_y + 30.0,
            text: if self.editor_tags.is_empty() {
                String::from("e.g. math, algebra")
            } else {
                self.editor_tags.clone()
            },
            font_size: 13.0,
            color: if self.editor_tags.is_empty() {
                OVERLAY0
            } else {
                TEXT_COLOR
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 16.0),
        });
    }

    fn render_study_mode(&self, cmds: &mut Vec<RenderCommand>) {
        let session = match &self.study_session {
            Some(s) => s,
            None => return,
        };

        let deck = match self.current_deck() {
            Some(d) => d,
            None => return,
        };

        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        // Progress bar
        let total = session.queue.len() as f32;
        let done = session.current_pos as f32;
        let progress_w = content_w;
        let progress_h = 8.0;
        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING,
            y: top,
            width: progress_w,
            height: progress_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        if total > 0.0 {
            let filled = (done / total) * progress_w;
            if filled > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: Self::PADDING,
                    y: top,
                    width: filled,
                    height: progress_h,
                    color: BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // Progress text
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 14.0,
            text: format!(
                "{} / {} cards  |  {} remaining  |  [Esc] quit",
                session.current_pos,
                session.queue.len(),
                session.remaining()
            ),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w),
        });

        if session.is_complete() {
            // Session complete summary
            self.render_session_summary(cmds, session, top + 50.0, content_w);
            return;
        }

        let card_idx = match session.current_card_idx() {
            Some(i) => i,
            None => return,
        };
        let card = match deck.cards.get(card_idx) {
            Some(c) => c,
            None => return,
        };

        // Card display
        let card_y = top + 50.0;
        let card_h = 260.0;

        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING + 40.0,
            y: card_y,
            width: content_w - 80.0,
            height: card_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: Self::PADDING + 40.0,
            y: card_y,
            width: content_w - 80.0,
            height: card_h,
            color: if session.flipped { GREEN } else { BLUE },
            line_width: 2.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Side label
        let side_label = if session.flipped { "ANSWER" } else { "QUESTION" };
        let side_color = if session.flipped { GREEN } else { BLUE };
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 60.0,
            y: card_y + 16.0,
            text: String::from(side_label),
            font_size: 11.0,
            color: side_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });

        // Card text
        let display_text = if session.flipped {
            card.back.clone()
        } else {
            card.front.clone()
        };
        cmds.push(RenderCommand::Text {
            x: Self::PADDING + 60.0,
            y: card_y + 60.0,
            text: display_text,
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(content_w - 160.0),
        });

        // Tags on card
        if !card.tags.is_empty() {
            cmds.push(RenderCommand::Text {
                x: Self::PADDING + 60.0,
                y: card_y + card_h - 30.0,
                text: card.tags.join(" | "),
                font_size: 10.0,
                color: MAUVE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(content_w - 160.0),
            });
        }

        // Action buttons
        let btn_y = card_y + card_h + 20.0;
        if session.flipped {
            // Rating buttons
            let btn_w = 120.0;
            let gap = 16.0;
            let total_btn_w = btn_w * 4.0 + gap * 3.0;
            let start_x = (self.width - total_btn_w) / 2.0;

            for (i, rating) in ALL_RATINGS.iter().enumerate() {
                let x = start_x + (i as f32) * (btn_w + gap);
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: btn_y,
                    width: btn_w,
                    height: 40.0,
                    color: rating.color(),
                    corner_radii: CornerRadii::all(8.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: btn_y + 10.0,
                    text: format!("[{}] {}", i + 1, rating.label()),
                    font_size: 14.0,
                    color: CRUST,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(btn_w - 20.0),
                });
            }
        } else {
            // Flip prompt
            let prompt_w = 200.0;
            let prompt_x = (self.width - prompt_w) / 2.0;
            cmds.push(RenderCommand::FillRect {
                x: prompt_x,
                y: btn_y,
                width: prompt_w,
                height: 40.0,
                color: BLUE,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: prompt_x + 20.0,
                y: btn_y + 10.0,
                text: String::from("[Space] Flip Card"),
                font_size: 14.0,
                color: CRUST,
                font_weight: FontWeightHint::Bold,
                max_width: Some(prompt_w - 40.0),
            });
        }
    }

    fn render_session_summary(
        &self,
        cmds: &mut Vec<RenderCommand>,
        session: &StudySession,
        top: f32,
        content_w: f32,
    ) {
        let cx = self.width / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: Self::PADDING + 60.0,
            y: top,
            width: content_w - 120.0,
            height: 280.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: cx - 80.0,
            y: top + 20.0,
            text: String::from("Session Complete!"),
            font_size: 20.0,
            color: GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: cx - 100.0,
            y: top + 60.0,
            text: format!("Cards reviewed: {}", session.reviewed),
            font_size: 14.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Text {
            x: cx - 100.0,
            y: top + 84.0,
            text: format!("Accuracy: {}%", session.session_accuracy()),
            font_size: 14.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Rating breakdown
        let labels = ["Again", "Hard", "Good", "Easy"];
        let colors = [RED, PEACH, BLUE, GREEN];
        for (i, (label, color)) in labels.iter().zip(colors.iter()).enumerate() {
            let y = top + 120.0 + (i as f32) * 24.0;
            cmds.push(RenderCommand::Text {
                x: cx - 80.0,
                y,
                text: format!("{}: {}", label, session.session_ratings[i]),
                font_size: 13.0,
                color: *color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(150.0),
            });
        }

        cmds.push(RenderCommand::Text {
            x: cx - 60.0,
            y: top + 240.0,
            text: String::from("[Esc] Back to deck"),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });
    }

    fn render_statistics(&self, cmds: &mut Vec<RenderCommand>) {
        let deck = match self.current_deck() {
            Some(d) => d,
            None => return,
        };

        let top = Self::HEADER_H + Self::PADDING;
        let content_w = self.width - Self::PADDING * 2.0;

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top,
            text: format!("Statistics: {}", deck.name),
            font_size: 18.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(400.0),
        });

        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: top + 26.0,
            text: String::from("[Esc] back"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        let stats_y = top + 56.0;
        let col_w = (content_w - 32.0) / 3.0;

        // Stat boxes
        let stats = [
            ("Total Cards", format!("{}", deck.cards.len()), BLUE),
            ("Total Reviews", format!("{}", deck.total_reviews()), TEAL),
            ("Avg Accuracy", format!("{}%", deck.average_accuracy()), GREEN),
        ];

        for (i, (label, value, color)) in stats.iter().enumerate() {
            let x = Self::PADDING + (i as f32) * (col_w + 16.0);
            cmds.push(RenderCommand::FillRect {
                x,
                y: stats_y,
                width: col_w,
                height: 70.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: stats_y + 10.0,
                text: String::from(*label),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_w - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: stats_y + 32.0,
                text: value.clone(),
                font_size: 24.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w - 24.0),
            });
        }

        // Second row
        let row2_y = stats_y + 90.0;
        let stats2 = [
            ("Due Today", format!("{}", deck.due_cards(self.current_day).len()), YELLOW),
            ("Mastered", format!("{}", deck.mastered_count()), GREEN),
            ("New", format!("{}", deck.cards.iter().filter(|c| c.review.total_reviews == 0).count()), PEACH),
        ];

        for (i, (label, value, color)) in stats2.iter().enumerate() {
            let x = Self::PADDING + (i as f32) * (col_w + 16.0);
            cmds.push(RenderCommand::FillRect {
                x,
                y: row2_y,
                width: col_w,
                height: 70.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row2_y + 10.0,
                text: String::from(*label),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_w - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0,
                y: row2_y + 32.0,
                text: value.clone(),
                font_size: 24.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: Some(col_w - 24.0),
            });
        }

        // Per-card breakdown
        let breakdown_y = row2_y + 90.0;
        cmds.push(RenderCommand::Text {
            x: Self::PADDING,
            y: breakdown_y,
            text: String::from("Card Breakdown"),
            font_size: 14.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        cmds.push(RenderCommand::Line {
            x1: Self::PADDING,
            y1: breakdown_y + 20.0,
            x2: self.width - Self::PADDING,
            y2: breakdown_y + 20.0,
            color: SURFACE1,
            width: 1.0,
        });

        // Show ease factor distribution as a bar chart approximation
        let bar_y = breakdown_y + 30.0;
        let bar_h = 20.0;
        let bar_max_w = content_w - 200.0;

        let ease_ranges = [
            ("Ease < 1.8 (difficult)", RED),
            ("Ease 1.8-2.2 (moderate)", YELLOW),
            ("Ease 2.2-2.5 (good)", BLUE),
            ("Ease > 2.5 (easy)", GREEN),
        ];

        let counts: [usize; 4] = [
            deck.cards.iter().filter(|c| c.review.ease_factor < 1.8).count(),
            deck.cards.iter().filter(|c| c.review.ease_factor >= 1.8 && c.review.ease_factor < 2.2).count(),
            deck.cards.iter().filter(|c| c.review.ease_factor >= 2.2 && c.review.ease_factor < 2.5).count(),
            deck.cards.iter().filter(|c| c.review.ease_factor >= 2.5).count(),
        ];
        let max_count = counts.iter().copied().max().unwrap_or(1).max(1);

        for (i, ((label, color), count)) in ease_ranges.iter().zip(counts.iter()).enumerate() {
            let y = bar_y + (i as f32) * (bar_h + 8.0);
            cmds.push(RenderCommand::Text {
                x: Self::PADDING,
                y: y + 3.0,
                text: String::from(*label),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(180.0),
            });

            let w = (*count as f32 / max_count as f32) * bar_max_w;
            if w > 0.0 {
                cmds.push(RenderCommand::FillRect {
                    x: Self::PADDING + 190.0,
                    y,
                    width: w,
                    height: bar_h,
                    color: *color,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: Self::PADDING + 196.0 + w,
                y: y + 3.0,
                text: format!("{count}"),
                font_size: 11.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: Some(40.0),
            });
        }
    }
}

fn main() {
    let _app = FlashcardsApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── ReviewData / SM-2 tests ─────────────────────────────────────

    #[test]
    fn test_review_data_initial() {
        let rd = ReviewData::new();
        assert_eq!(rd.repetitions, 0);
        assert!((rd.ease_factor - SM2_INITIAL_EASE).abs() < f32::EPSILON);
        assert_eq!(rd.interval_days, 0);
        assert_eq!(rd.total_reviews, 0);
    }

    #[test]
    fn test_review_data_is_due_when_new() {
        let rd = ReviewData::new();
        assert!(rd.is_due(1));
    }

    #[test]
    fn test_review_good_first_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        assert_eq!(rd.repetitions, 1);
        assert_eq!(rd.interval_days, 1);
        assert_eq!(rd.total_reviews, 1);
    }

    #[test]
    fn test_review_good_second_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        assert_eq!(rd.repetitions, 2);
        assert_eq!(rd.interval_days, 6);
    }

    #[test]
    fn test_review_good_third_time() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Good, 8);
        assert_eq!(rd.repetitions, 3);
        assert!(rd.interval_days > 6);
    }

    #[test]
    fn test_review_again_resets_repetitions() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Again, 8);
        assert_eq!(rd.repetitions, 0);
        assert_eq!(rd.interval_days, 1);
    }

    #[test]
    fn test_review_hard_resets_repetitions() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Hard, 2);
        assert_eq!(rd.repetitions, 0);
        assert_eq!(rd.interval_days, 1);
    }

    #[test]
    fn test_review_easy_increases_ease() {
        let mut rd = ReviewData::new();
        let initial = rd.ease_factor;
        rd.apply_rating(Rating::Easy, 1);
        assert!(rd.ease_factor > initial);
    }

    #[test]
    fn test_review_again_decreases_ease() {
        let mut rd = ReviewData::new();
        let initial = rd.ease_factor;
        rd.apply_rating(Rating::Again, 1);
        assert!(rd.ease_factor < initial);
    }

    #[test]
    fn test_ease_never_below_minimum() {
        let mut rd = ReviewData::new();
        for i in 0..20 {
            rd.apply_rating(Rating::Again, i);
        }
        assert!(rd.ease_factor >= SM2_MIN_EASE);
    }

    #[test]
    fn test_is_due_after_interval() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        // interval_days = 1, so due on day 2
        assert!(!rd.is_due(1)); // same day
        assert!(rd.is_due(2));  // next day
    }

    #[test]
    fn test_is_due_not_before_interval() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        // interval_days = 6, so due on day 8
        assert!(!rd.is_due(5));
        assert!(rd.is_due(8));
    }

    #[test]
    fn test_accuracy_percent_no_reviews() {
        let rd = ReviewData::new();
        assert_eq!(rd.accuracy_percent(), 0);
    }

    #[test]
    fn test_accuracy_percent_all_good() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Good, 2);
        assert_eq!(rd.accuracy_percent(), 100);
    }

    #[test]
    fn test_accuracy_percent_mixed() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Good, 1);
        rd.apply_rating(Rating::Again, 2);
        // 1 good + 1 again = 50%
        assert_eq!(rd.accuracy_percent(), 50);
    }

    #[test]
    fn test_rating_counts_tracked() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Again, 1);
        rd.apply_rating(Rating::Hard, 2);
        rd.apply_rating(Rating::Good, 3);
        rd.apply_rating(Rating::Easy, 4);
        assert_eq!(rd.rating_counts, [1, 1, 1, 1]);
    }

    // ── Card tests ──────────────────────────────────────────────────

    #[test]
    fn test_card_new() {
        let card = Card::new(1, "Q?", "A!");
        assert_eq!(card.id, 1);
        assert_eq!(card.front, "Q?");
        assert_eq!(card.back, "A!");
        assert!(card.tags.is_empty());
    }

    #[test]
    fn test_card_with_tags() {
        let card = Card::new(1, "Q", "A").with_tags(&["math", "algebra"]);
        assert_eq!(card.tags.len(), 2);
        assert_eq!(card.tags[0], "math");
    }

    #[test]
    fn test_card_matches_search_empty() {
        let card = Card::new(1, "Hello", "World");
        assert!(card.matches_search(""));
    }

    #[test]
    fn test_card_matches_search_front() {
        let card = Card::new(1, "What is Rust?", "A language");
        assert!(card.matches_search("rust"));
    }

    #[test]
    fn test_card_matches_search_back() {
        let card = Card::new(1, "What?", "Answer here");
        assert!(card.matches_search("answer"));
    }

    #[test]
    fn test_card_matches_search_tag() {
        let card = Card::new(1, "Q", "A").with_tags(&["biology"]);
        assert!(card.matches_search("bio"));
    }

    #[test]
    fn test_card_matches_search_no_match() {
        let card = Card::new(1, "Hello", "World");
        assert!(!card.matches_search("xyz"));
    }

    #[test]
    fn test_card_has_tag() {
        let card = Card::new(1, "Q", "A").with_tags(&["Math", "Science"]);
        assert!(card.has_tag("math"));   // case insensitive
        assert!(card.has_tag("SCIENCE"));
        assert!(!card.has_tag("history"));
    }

    // ── Deck tests ──────────────────────────────────────────────────

    #[test]
    fn test_deck_new() {
        let deck = Deck::new("Test", "A test deck");
        assert_eq!(deck.name, "Test");
        assert!(deck.cards.is_empty());
        assert_eq!(deck.next_card_id, 1);
    }

    #[test]
    fn test_deck_add_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Front", "Back");
        assert_eq!(id, 1);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.next_card_id, 2);
    }

    #[test]
    fn test_deck_add_card_with_tags() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card_with_tags("Q", "A", &["tag1", "tag2"]);
        assert_eq!(id, 1);
        assert_eq!(deck.cards[0].tags.len(), 2);
    }

    #[test]
    fn test_deck_remove_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        assert!(deck.remove_card(id));
        assert!(deck.cards.is_empty());
    }

    #[test]
    fn test_deck_remove_nonexistent() {
        let mut deck = Deck::new("Test", "");
        assert!(!deck.remove_card(999));
    }

    #[test]
    fn test_deck_find_card() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        assert!(deck.find_card(id).is_some());
        assert!(deck.find_card(999).is_none());
    }

    #[test]
    fn test_deck_find_card_mut() {
        let mut deck = Deck::new("Test", "");
        let id = deck.add_card("Q", "A");
        if let Some(card) = deck.find_card_mut(id) {
            card.front = String::from("Updated");
        }
        assert_eq!(deck.cards[0].front, "Updated");
    }

    #[test]
    fn test_deck_due_cards_all_new() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        let due = deck.due_cards(1);
        assert_eq!(due.len(), 2);
    }

    #[test]
    fn test_deck_due_cards_after_review() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        // Card 0 is not due on day 1, card 1 is new so due
        let due = deck.due_cards(1);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0], 1);
    }

    #[test]
    fn test_deck_total_reviews() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[1].review.apply_rating(Rating::Easy, 1);
        deck.cards[1].review.apply_rating(Rating::Good, 2);
        assert_eq!(deck.total_reviews(), 3);
    }

    #[test]
    fn test_deck_average_accuracy() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[0].review.apply_rating(Rating::Good, 2);
        deck.cards[1].review.apply_rating(Rating::Again, 1);
        deck.cards[1].review.apply_rating(Rating::Again, 2);
        // Card 0: 100%, Card 1: 0% -> average 50%
        assert_eq!(deck.average_accuracy(), 50);
    }

    #[test]
    fn test_deck_mastered_count() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.cards[0].review.apply_rating(Rating::Good, 1);
        deck.cards[0].review.apply_rating(Rating::Good, 2);
        deck.cards[0].review.apply_rating(Rating::Good, 8);
        assert_eq!(deck.mastered_count(), 1);
    }

    #[test]
    fn test_deck_all_tags() {
        let mut deck = Deck::new("Test", "");
        deck.add_card_with_tags("Q1", "A1", &["alpha", "beta"]);
        deck.add_card_with_tags("Q2", "A2", &["Beta", "gamma"]); // Beta dups beta
        let tags = deck.all_tags();
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_deck_cards_matching_all() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.add_card("Q2", "A2");
        let m = deck.cards_matching("", None);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn test_deck_cards_matching_search() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Rust question", "Answer");
        deck.add_card("Python question", "Answer");
        let m = deck.cards_matching("rust", None);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_deck_cards_matching_tag() {
        let mut deck = Deck::new("Test", "");
        deck.add_card_with_tags("Q1", "A1", &["alpha"]);
        deck.add_card_with_tags("Q2", "A2", &["beta"]);
        let m = deck.cards_matching("", Some("alpha"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_deck_shuffle() {
        let mut deck = Deck::new("Test", "");
        for i in 0..20 {
            deck.add_card(&format!("Q{i}"), &format!("A{i}"));
        }
        let original_ids: Vec<u32> = deck.cards.iter().map(|c| c.id).collect();
        deck.shuffle(42);
        let shuffled_ids: Vec<u32> = deck.cards.iter().map(|c| c.id).collect();
        // Very unlikely that 20 cards stay in the same order
        assert_ne!(original_ids, shuffled_ids);
    }

    #[test]
    fn test_deck_shuffle_single_card() {
        let mut deck = Deck::new("Test", "");
        deck.add_card("Q1", "A1");
        deck.shuffle(42);
        assert_eq!(deck.cards.len(), 1);
    }

    #[test]
    fn test_deck_shuffle_empty() {
        let mut deck = Deck::new("Test", "");
        deck.shuffle(42); // should not panic
        assert!(deck.cards.is_empty());
    }

    // ── Export/Import tests ─────────────────────────────────────────

    #[test]
    fn test_export_basic() {
        let mut deck = Deck::new("Test", "Description");
        deck.add_card("Q1", "A1");
        let text = deck.export_text();
        assert!(text.contains("# Test"));
        assert!(text.contains("## Description"));
        assert!(text.contains("Q: Q1"));
        assert!(text.contains("A: A1"));
    }

    #[test]
    fn test_export_with_tags() {
        let mut deck = Deck::new("T", "D");
        deck.add_card_with_tags("Q1", "A1", &["tag1", "tag2"]);
        let text = deck.export_text();
        assert!(text.contains("T: tag1,tag2"));
    }

    #[test]
    fn test_import_basic() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: What is 1+1?\nA: 2\n\nQ: What is 2+2?\nA: 4\n";
        let count = deck.import_text(text);
        assert_eq!(count, 2);
        assert_eq!(deck.cards.len(), 2);
        assert_eq!(deck.cards[0].front, "What is 1+1?");
        assert_eq!(deck.cards[0].back, "2");
    }

    #[test]
    fn test_import_with_tags() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: Question\nA: Answer\nT: math, algebra\n";
        let count = deck.import_text(text);
        assert_eq!(count, 1);
        assert_eq!(deck.cards[0].tags.len(), 2);
        assert_eq!(deck.cards[0].tags[0], "math");
    }

    #[test]
    fn test_import_no_trailing_newline() {
        let mut deck = Deck::new("T", "D");
        let text = "Q: Question\nA: Answer";
        let count = deck.import_text(text);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_roundtrip_export_import() {
        let mut original = Deck::new("Test", "Desc");
        original.add_card_with_tags("Q1", "A1", &["t1"]);
        original.add_card("Q2", "A2");
        let exported = original.export_text();
        let mut imported = Deck::new("Imported", "");
        let count = imported.import_text(&exported);
        assert_eq!(count, 2);
        assert_eq!(imported.cards[0].front, "Q1");
        assert_eq!(imported.cards[0].back, "A1");
        assert_eq!(imported.cards[0].tags.len(), 1);
        assert_eq!(imported.cards[1].front, "Q2");
    }

    // ── StudySession tests ──────────────────────────────────────────

    #[test]
    fn test_study_session_new() {
        let session = StudySession::new(vec![0, 1, 2]);
        assert_eq!(session.current_pos, 0);
        assert!(!session.flipped);
        assert_eq!(session.reviewed, 0);
        assert_eq!(session.remaining(), 3);
    }

    #[test]
    fn test_study_session_current_card_idx() {
        let session = StudySession::new(vec![5, 10, 15]);
        assert_eq!(session.current_card_idx(), Some(5));
    }

    #[test]
    fn test_study_session_empty() {
        let session = StudySession::new(vec![]);
        assert!(session.is_complete());
        assert_eq!(session.remaining(), 0);
        assert_eq!(session.current_card_idx(), None);
    }

    #[test]
    fn test_study_session_complete() {
        let mut session = StudySession::new(vec![0]);
        session.record_rating(Rating::Good);
        session.current_pos = 1;
        assert!(session.is_complete());
    }

    #[test]
    fn test_study_session_accuracy() {
        let mut session = StudySession::new(vec![0, 1, 2, 3]);
        session.record_rating(Rating::Good);
        session.record_rating(Rating::Easy);
        session.record_rating(Rating::Again);
        session.record_rating(Rating::Hard);
        assert_eq!(session.session_accuracy(), 50); // 2 good/easy out of 4
    }

    #[test]
    fn test_study_session_accuracy_empty() {
        let session = StudySession::new(vec![0]);
        assert_eq!(session.session_accuracy(), 0);
    }

    // ── FlashcardsApp tests ─────────────────────────────────────────

    #[test]
    fn test_app_new() {
        let app = FlashcardsApp::new();
        assert_eq!(app.decks.len(), 3);
        assert_eq!(app.selected_deck, 0);
        assert_eq!(app.view, AppView::DeckList);
        assert_eq!(app.current_day, 1);
    }

    #[test]
    fn test_sample_decks_non_empty() {
        let app = FlashcardsApp::new();
        for deck in &app.decks {
            assert!(!deck.cards.is_empty());
            assert!(deck.cards.len() >= 10);
        }
    }

    #[test]
    fn test_sample_decks_have_tags() {
        let app = FlashcardsApp::new();
        for deck in &app.decks {
            let has_tags = deck.cards.iter().any(|c| !c.tags.is_empty());
            assert!(has_tags);
        }
    }

    #[test]
    fn test_add_deck() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.add_deck("My Deck", "Desc");
        assert_eq!(app.decks.len(), n + 1);
        assert_eq!(app.decks.last().unwrap().name, "My Deck");
    }

    #[test]
    fn test_remove_deck() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.remove_deck(0);
        assert_eq!(app.decks.len(), n - 1);
    }

    #[test]
    fn test_remove_last_deck_prevented() {
        let mut app = FlashcardsApp::new();
        app.decks.truncate(1);
        app.remove_deck(0);
        assert_eq!(app.decks.len(), 1); // cannot remove last
    }

    #[test]
    fn test_select_deck() {
        let mut app = FlashcardsApp::new();
        app.select_deck(1);
        assert_eq!(app.selected_deck, 1);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_select_deck_out_of_bounds() {
        let mut app = FlashcardsApp::new();
        app.select_deck(999);
        assert_eq!(app.selected_deck, 0); // unchanged
    }

    #[test]
    fn test_navigate_decks() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, 1);
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, 2);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_deck, 1);
    }

    #[test]
    fn test_navigate_deck_boundary_top() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_deck, 0);
    }

    #[test]
    fn test_navigate_deck_boundary_bottom() {
        let mut app = FlashcardsApp::new();
        let last = app.decks.len() - 1;
        app.selected_deck = last;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_deck, last);
    }

    #[test]
    fn test_open_deck_enter() {
        let mut app = FlashcardsApp::new();
        app.handle_key("Enter", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_new_deck_key() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.handle_key("n", false, false);
        assert_eq!(app.decks.len(), n + 1);
    }

    #[test]
    fn test_delete_deck_key() {
        let mut app = FlashcardsApp::new();
        let n = app.decks.len();
        app.handle_key("Delete", false, false);
        assert_eq!(app.decks.len(), n - 1);
    }

    #[test]
    fn test_deck_detail_back() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckList);
    }

    #[test]
    fn test_deck_detail_navigate_cards() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("Down", false, false);
        assert_eq!(app.selected_card, 1);
        app.handle_key("Up", false, false);
        assert_eq!(app.selected_card, 0);
    }

    #[test]
    fn test_deck_detail_new_card() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("n", false, false);
        assert_eq!(app.view, AppView::CardEditor);
        assert!(app.editing_card_id.is_none());
    }

    #[test]
    fn test_deck_detail_statistics() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("i", false, false);
        assert_eq!(app.view, AppView::Statistics);
    }

    #[test]
    fn test_statistics_back() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::Statistics;
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_study_start_due() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        assert_eq!(app.view, AppView::StudyMode);
        assert!(app.study_session.is_some());
    }

    #[test]
    fn test_study_start_all() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        assert_eq!(app.view, AppView::StudyMode);
        let session = app.study_session.as_ref().unwrap();
        assert_eq!(session.queue.len(), app.decks[0].cards.len());
    }

    #[test]
    fn test_study_start_empty_deck() {
        let mut app = FlashcardsApp::new();
        app.decks.push(Deck::new("Empty", ""));
        app.selected_deck = app.decks.len() - 1;
        app.start_study_all();
        assert_ne!(app.view, AppView::StudyMode);
    }

    #[test]
    fn test_study_flip() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        assert!(!app.study_session.as_ref().unwrap().flipped);
        app.flip_card();
        assert!(app.study_session.as_ref().unwrap().flipped);
    }

    #[test]
    fn test_study_rate_requires_flip() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        let pos_before = app.study_session.as_ref().unwrap().current_pos;
        app.rate_card(Rating::Good); // should do nothing without flip
        assert_eq!(app.study_session.as_ref().unwrap().current_pos, pos_before);
    }

    #[test]
    fn test_study_rate_advances() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        app.rate_card(Rating::Good);
        let session = app.study_session.as_ref().unwrap();
        assert_eq!(session.current_pos, 1);
        assert!(!session.flipped);
        assert_eq!(session.reviewed, 1);
    }

    #[test]
    fn test_study_full_session() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        let total = app.study_session.as_ref().unwrap().queue.len();
        for _ in 0..total {
            app.flip_card();
            app.rate_card(Rating::Good);
        }
        assert!(app.study_session.as_ref().unwrap().is_complete());
    }

    #[test]
    fn test_study_escape() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.handle_key("Escape", false, false);
        assert!(app.study_session.is_none());
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_study_space_flips() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.handle_key("Space", false, false);
        assert!(app.study_session.as_ref().unwrap().flipped);
    }

    #[test]
    fn test_study_number_keys_rate() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        app.handle_key("3", false, false); // Good
        assert_eq!(app.study_session.as_ref().unwrap().reviewed, 1);
    }

    #[test]
    fn test_advance_day() {
        let mut app = FlashcardsApp::new();
        let d = app.current_day;
        app.advance_day();
        assert_eq!(app.current_day, d + 1);
    }

    #[test]
    fn test_card_editor_save_new() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        app.editor_front = String::from("New Q");
        app.editor_back = String::from("New A");
        app.editor_tags = String::from("tag1, tag2");
        assert!(app.save_card());
        let deck = &app.decks[app.selected_deck];
        let last = deck.cards.last().unwrap();
        assert_eq!(last.front, "New Q");
        assert_eq!(last.back, "New A");
        assert_eq!(last.tags.len(), 2);
    }

    #[test]
    fn test_card_editor_save_empty_rejected() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        assert!(!app.save_card()); // empty front/back
    }

    #[test]
    fn test_card_editor_edit_existing() {
        let mut app = FlashcardsApp::new();
        let card_id = app.decks[0].cards[0].id;
        app.open_edit_card(card_id);
        assert_eq!(app.editing_card_id, Some(card_id));
        app.editor_front = String::from("Updated question");
        app.editor_back = String::from("Updated answer");
        assert!(app.save_card());
        assert_eq!(app.decks[0].cards[0].front, "Updated question");
    }

    #[test]
    fn test_delete_selected_card() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let n = app.decks[0].cards.len();
        app.delete_selected_card();
        assert_eq!(app.decks[0].cards.len(), n - 1);
    }

    #[test]
    fn test_search_text() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_search_text("cap");
        assert_eq!(app.search_query, "cap");
    }

    #[test]
    fn test_search_backspace() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("test");
        app.handle_search_backspace();
        assert_eq!(app.search_query, "tes");
    }

    #[test]
    fn test_shuffle_key() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let ids_before: Vec<u32> = app.decks[0].cards.iter().map(|c| c.id).collect();
        app.handle_key("r", false, false);
        let ids_after: Vec<u32> = app.decks[0].cards.iter().map(|c| c.id).collect();
        assert_ne!(ids_before, ids_after);
    }

    #[test]
    fn test_tag_filter_cycle() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.handle_key("t", false, false);
        assert!(app.tag_filter.is_some());
    }

    #[test]
    fn test_tag_filter_cleared() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        // Cycle through all tags until cleared
        let tags = app.decks[0].all_tags();
        for _ in 0..=tags.len() {
            app.handle_key("t", false, false);
        }
        assert!(app.tag_filter.is_none());
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_deck_list() {
        let app = FlashcardsApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_deck_detail() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_card_editor() {
        let mut app = FlashcardsApp::new();
        app.open_new_card_editor();
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_mode() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_flipped() {
        let mut app = FlashcardsApp::new();
        app.start_study();
        app.flip_card();
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_study_complete() {
        let mut app = FlashcardsApp::new();
        app.start_study_all();
        let total = app.study_session.as_ref().unwrap().queue.len();
        for _ in 0..total {
            app.flip_card();
            app.rate_card(Rating::Good);
        }
        let cmds = app.render();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_statistics() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::Statistics;
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_with_search() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("capital");
        let cmds = app.render();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_with_tag_filter() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.tag_filter = Some(String::from("europe"));
        let cmds = app.render();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_empty_search_results() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.search_query = String::from("zzzznonexistent");
        let cmds = app.render();
        assert!(cmds.len() > 5);
    }

    // ── Rating tests ────────────────────────────────────────────────

    #[test]
    fn test_rating_quality_values() {
        assert_eq!(Rating::Again.quality(), 0);
        assert_eq!(Rating::Hard.quality(), 2);
        assert_eq!(Rating::Good.quality(), 3);
        assert_eq!(Rating::Easy.quality(), 5);
    }

    #[test]
    fn test_rating_labels() {
        assert_eq!(Rating::Again.label(), "Again");
        assert_eq!(Rating::Hard.label(), "Hard");
        assert_eq!(Rating::Good.label(), "Good");
        assert_eq!(Rating::Easy.label(), "Easy");
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_remove_deck_adjusts_selection() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = app.decks.len() - 1;
        let idx = app.selected_deck;
        app.remove_deck(idx);
        assert!(app.selected_deck < app.decks.len());
    }

    #[test]
    fn test_card_editor_cancel() {
        let mut app = FlashcardsApp::new();
        app.view = AppView::DeckDetail;
        app.open_new_card_editor();
        assert_eq!(app.view, AppView::CardEditor);
        app.handle_key("Escape", false, false);
        assert_eq!(app.view, AppView::DeckDetail);
    }

    #[test]
    fn test_ensure_card_visible_scroll_down() {
        let mut app = FlashcardsApp::new();
        app.selected_card = 10;
        app.scroll_offset = 0;
        app.ensure_card_visible();
        assert!(app.scroll_offset > 0);
    }

    #[test]
    fn test_ensure_card_visible_scroll_up() {
        let mut app = FlashcardsApp::new();
        app.scroll_offset = 5;
        app.selected_card = 2;
        app.ensure_card_visible();
        assert_eq!(app.scroll_offset, 2);
    }

    #[test]
    fn test_current_deck_none() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = 999;
        assert!(app.current_deck().is_none());
    }

    #[test]
    fn test_matching_card_indices_no_deck() {
        let mut app = FlashcardsApp::new();
        app.selected_deck = 999;
        assert!(app.matching_card_indices().is_empty());
    }

    #[test]
    fn test_sm2_long_sequence() {
        let mut rd = ReviewData::new();
        // Simulate a long study sequence
        let mut day = 1u32;
        for _ in 0..10 {
            rd.apply_rating(Rating::Good, day);
            day += rd.interval_days;
        }
        assert!(rd.repetitions > 5);
        assert!(rd.interval_days > 10);
    }

    #[test]
    fn test_sm2_mixed_ratings() {
        let mut rd = ReviewData::new();
        rd.apply_rating(Rating::Easy, 1);
        rd.apply_rating(Rating::Good, 2);
        rd.apply_rating(Rating::Hard, 8);
        rd.apply_rating(Rating::Good, 9);
        rd.apply_rating(Rating::Easy, 10);
        assert_eq!(rd.total_reviews, 5);
        assert!(rd.ease_factor >= SM2_MIN_EASE);
    }

    #[test]
    fn test_import_skips_header_lines() {
        let mut deck = Deck::new("T", "D");
        let text = "# Deck Name\n## Description\n\nQ: Question\nA: Answer\n";
        let count = deck.import_text(text);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_import_empty_text() {
        let mut deck = Deck::new("T", "D");
        let count = deck.import_text("");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_study_no_due_cards() {
        let mut app = FlashcardsApp::new();
        // Review all cards so none are due
        for card in &mut app.decks[0].cards {
            card.review.apply_rating(Rating::Good, 1);
        }
        app.start_study();
        // Should not enter study mode since no cards are due on day 1
        assert!(app.study_session.is_none());
    }
}
