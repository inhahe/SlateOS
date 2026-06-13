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
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

//! Slate OS Hangman -- classic word guessing game.
//!
//! Features 100+ words across 5 categories (animals, fruits, countries,
//! sports, technology), 3 difficulty levels, progressive hangman figure
//! drawing, hint system (1 per game), win/loss detection, persistent
//! stats (wins, losses, current streak, best streak), and category
//! selection. Uses an LCG pseudo-random number generator seeded at
//! construction (no external rand crate).

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// -- Catppuccin Mocha palette -------------------------------------------
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// -- Layout constants ---------------------------------------------------
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 54.0;
const GALLOWS_SIZE: f32 = 220.0;
const WORD_AREA_HEIGHT: f32 = 60.0;
const KEYBOARD_HEIGHT: f32 = 120.0;
const STATS_PANEL_WIDTH: f32 = 180.0;
const WINDOW_WIDTH: f32 = 740.0;
const WINDOW_HEIGHT: f32 = 560.0;

const HEADER_FONT: f32 = 18.0;
const TITLE_FONT: f32 = 24.0;
const WORD_FONT: f32 = 28.0;
const KEY_FONT: f32 = 16.0;
const STATS_FONT: f32 = 13.0;
const OVERLAY_FONT: f32 = 16.0;
const CATEGORY_FONT: f32 = 14.0;
const HINT_FONT: f32 = 13.0;

/// Maximum wrong guesses before the game is lost.
const MAX_WRONG: usize = 6;

// -- LCG random number generator ---------------------------------------
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Returns a value in `0..bound` (exclusive upper bound).
    fn next_bounded(&mut self, bound: usize) -> usize {
        let val = self.next_u64();
        (val % bound as u64) as usize
    }
}

// -- Category -----------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Animals,
    Fruits,
    Countries,
    Sports,
    Technology,
}

impl Category {
    const ALL: [Category; 5] = [
        Category::Animals,
        Category::Fruits,
        Category::Countries,
        Category::Sports,
        Category::Technology,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Animals => "Animals",
            Self::Fruits => "Fruits",
            Self::Countries => "Countries",
            Self::Sports => "Sports",
            Self::Technology => "Technology",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Animals => PEACH,
            Self::Fruits => GREEN,
            Self::Countries => BLUE,
            Self::Sports => YELLOW,
            Self::Technology => MAUVE,
        }
    }

    fn words(self) -> &'static [&'static str] {
        match self {
            Self::Animals => &ANIMALS,
            Self::Fruits => &FRUITS,
            Self::Countries => &COUNTRIES,
            Self::Sports => &SPORTS,
            Self::Technology => &TECHNOLOGY,
        }
    }

    fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::Animals),
            1 => Some(Self::Fruits),
            2 => Some(Self::Countries),
            3 => Some(Self::Sports),
            4 => Some(Self::Technology),
            _ => None,
        }
    }
}

// -- Word lists (100+ total) --------------------------------------------
const ANIMALS: [&str; 24] = [
    "elephant", "giraffe", "penguin", "dolphin", "kangaroo",
    "cheetah", "octopus", "flamingo", "buffalo", "panther",
    "leopard", "hamster", "gazelle", "toucan", "walrus",
    "pelican", "lobster", "sparrow", "raccoon", "vulture",
    "gorilla", "seahorse", "parrot", "falcon",
];

const FRUITS: [&str; 22] = [
    "banana", "strawberry", "pineapple", "blueberry", "raspberry",
    "watermelon", "tangerine", "coconut", "avocado", "apricot",
    "pomegranate", "cranberry", "nectarine", "dragonfruit", "mulberry",
    "blackberry", "mandarin", "papaya", "guava", "lychee",
    "mango", "cherry",
];

const COUNTRIES: [&str; 22] = [
    "australia", "argentina", "brazil", "canada", "denmark",
    "ethiopia", "finland", "germany", "hungary", "iceland",
    "jamaica", "kenya", "malaysia", "norway", "portugal",
    "romania", "singapore", "thailand", "ukraine", "vietnam",
    "colombia", "morocco",
];

const SPORTS: [&str; 20] = [
    "basketball", "football", "baseball", "swimming", "wrestling",
    "volleyball", "badminton", "archery", "fencing", "lacrosse",
    "kayaking", "climbing", "cycling", "handball", "softball",
    "triathlon", "sprinting", "javelin", "hurdles", "canoeing",
];

const TECHNOLOGY: [&str; 20] = [
    "algorithm", "bluetooth", "compiler", "database", "ethernet",
    "firmware", "graphics", "hardware", "internet", "javascript",
    "keyboard", "terminal", "microchip", "notebook", "software",
    "protocol", "robotics", "transistor", "wireless", "processor",
];

/// Total number of words across all categories.
fn total_word_count() -> usize {
    ANIMALS.len() + FRUITS.len() + COUNTRIES.len() + SPORTS.len() + TECHNOLOGY.len()
}

// -- Difficulty ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    /// Minimum word length for this difficulty.
    fn min_length(self) -> usize {
        match self {
            Self::Easy => 3,
            Self::Medium => 6,
            Self::Hard => 8,
        }
    }

    /// Maximum word length for this difficulty.
    fn max_length(self) -> usize {
        match self {
            Self::Easy => 6,
            Self::Medium => 8,
            Self::Hard => 20,
        }
    }

    /// Number of letters revealed at start as a free hint.
    fn free_reveals(self) -> usize {
        match self {
            Self::Easy => 2,
            Self::Medium => 1,
            Self::Hard => 0,
        }
    }
}

// -- Game state ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    /// Player is choosing a category.
    CategorySelect,
    /// Actively playing a round.
    Playing,
    /// Round won.
    Won,
    /// Round lost.
    Lost,
}

// -- Stats --------------------------------------------------------------
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stats {
    wins: u32,
    losses: u32,
    current_streak: u32,
    best_streak: u32,
}

impl Stats {
    fn new() -> Self {
        Self {
            wins: 0,
            losses: 0,
            current_streak: 0,
            best_streak: 0,
        }
    }

    fn record_win(&mut self) {
        self.wins += 1;
        self.current_streak += 1;
        if self.current_streak > self.best_streak {
            self.best_streak = self.current_streak;
        }
    }

    fn record_loss(&mut self) {
        self.losses += 1;
        self.current_streak = 0;
    }

    fn total_games(&self) -> u32 {
        self.wins + self.losses
    }

    fn win_rate_percent(&self) -> u32 {
        let total = self.total_games();
        if total == 0 {
            return 0;
        }
        (self.wins * 100) / total
    }
}

// -- Main app struct ----------------------------------------------------
struct HangmanApp {
    /// The secret word (lowercase ASCII).
    word: Vec<u8>,
    /// Which letters (a-z) have been guessed. Index 0 = 'a'.
    guessed: [bool; 26],
    /// Number of wrong guesses so far.
    wrong_count: usize,
    /// Current game phase.
    phase: GamePhase,
    /// Selected category.
    category: Category,
    /// Difficulty level.
    difficulty: Difficulty,
    /// Whether the hint has been used this round.
    hint_used: bool,
    /// Persistent stats.
    stats: Stats,
    /// RNG state.
    rng: Lcg,
    /// Index of the highlighted category in selection screen.
    category_cursor: usize,
}

impl HangmanApp {
    /// Create a new Hangman game with default settings.
    fn new() -> Self {
        Self::with_seed(42)
    }

    /// Create a new Hangman game with a specific RNG seed.
    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            word: Vec::new(),
            guessed: [false; 26],
            wrong_count: 0,
            phase: GamePhase::CategorySelect,
            category: Category::Animals,
            difficulty: Difficulty::Medium,
            hint_used: false,
            stats: Stats::new(),
            rng: Lcg::new(seed),
            category_cursor: 0,
        };
        app.pick_word();
        app
    }

    // -- Word selection -------------------------------------------------

    /// Pick a random word from the current category, filtered by difficulty.
    fn pick_word(&mut self) {
        let words = self.category.words();
        let min_len = self.difficulty.min_length();
        let max_len = self.difficulty.max_length();

        // Collect eligible words.
        let eligible: Vec<&str> = words
            .iter()
            .filter(|w| w.len() >= min_len && w.len() <= max_len)
            .copied()
            .collect();

        // If no words match the difficulty filter, use all words.
        let pool = if eligible.is_empty() { words } else { &eligible };

        let idx = self.rng.next_bounded(pool.len());
        self.word = pool[idx].as_bytes().to_vec();

        // Apply free reveals for easy/medium difficulty.
        self.apply_free_reveals();
    }

    /// Reveal some letters for free at the start based on difficulty.
    fn apply_free_reveals(&mut self) {
        let count = self.difficulty.free_reveals();
        if count == 0 {
            return;
        }

        // Collect unique unrevealed letters in the word.
        let mut unrevealed: Vec<u8> = Vec::new();
        for &b in &self.word {
            let idx = letter_index(b);
            if let Some(i) = idx
                && !self.guessed[i] && !unrevealed.contains(&b) {
                    unrevealed.push(b);
                }
        }

        // Reveal up to `count` letters.
        let reveals = if count > unrevealed.len() {
            unrevealed.len()
        } else {
            count
        };
        for _ in 0..reveals {
            if unrevealed.is_empty() {
                break;
            }
            let pick = self.rng.next_bounded(unrevealed.len());
            let letter = unrevealed[pick];
            if let Some(i) = letter_index(letter) {
                self.guessed[i] = true;
            }
            unrevealed.swap_remove(pick);
        }
    }

    /// Start a new round, preserving stats and settings.
    fn new_round(&mut self) {
        self.guessed = [false; 26];
        self.wrong_count = 0;
        self.hint_used = false;
        self.phase = GamePhase::Playing;
        self.pick_word();
    }

    /// Start a new round after returning to category select.
    fn start_from_category(&mut self) {
        self.guessed = [false; 26];
        self.wrong_count = 0;
        self.hint_used = false;
        self.phase = GamePhase::Playing;
        self.pick_word();
    }

    // -- Guess logic ----------------------------------------------------

    /// Attempt to guess a letter. Returns true if the letter was new.
    fn guess_letter(&mut self, letter: u8) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }
        let idx = match letter_index(letter) {
            Some(i) => i,
            None => return false,
        };
        if self.guessed[idx] {
            return false;
        }
        self.guessed[idx] = true;

        let lower = letter.to_ascii_lowercase();
        let in_word = self.word.contains(&lower);
        if !in_word {
            self.wrong_count += 1;
        }

        // Check for win/loss.
        if self.wrong_count >= MAX_WRONG {
            self.phase = GamePhase::Lost;
            self.stats.record_loss();
        } else if self.is_word_revealed() {
            self.phase = GamePhase::Won;
            self.stats.record_win();
        }

        true
    }

    /// Check if every letter in the word has been guessed.
    fn is_word_revealed(&self) -> bool {
        self.word.iter().all(|&b| {
            if let Some(i) = letter_index(b) {
                self.guessed[i]
            } else {
                // Non-letter characters (hyphens, etc.) are always shown.
                true
            }
        })
    }

    /// Use the hint: reveal one unrevealed letter. Only allowed once.
    fn use_hint(&mut self) -> bool {
        if self.phase != GamePhase::Playing || self.hint_used {
            return false;
        }

        // Find unrevealed letters in the word.
        let mut unrevealed: Vec<u8> = Vec::new();
        for &b in &self.word {
            if let Some(i) = letter_index(b)
                && !self.guessed[i] && !unrevealed.contains(&b) {
                    unrevealed.push(b);
                }
        }

        if unrevealed.is_empty() {
            return false;
        }

        let pick = self.rng.next_bounded(unrevealed.len());
        let letter = unrevealed[pick];
        if let Some(i) = letter_index(letter) {
            self.guessed[i] = true;
        }
        self.hint_used = true;

        // Check for win after hint.
        if self.is_word_revealed() {
            self.phase = GamePhase::Won;
            self.stats.record_win();
        }

        true
    }

    /// Get the word as a displayable string with blanks for unguessed letters.
    fn display_word(&self) -> String {
        let mut result = String::new();
        for (i, &b) in self.word.iter().enumerate() {
            if i > 0 {
                result.push(' ');
            }
            if let Some(idx) = letter_index(b) {
                if self.guessed[idx] {
                    result.push(b as char);
                } else {
                    result.push('_');
                }
            } else {
                result.push(b as char);
            }
        }
        result
    }

    /// Get the full word as a string (for game over reveal).
    fn word_string(&self) -> String {
        String::from_utf8(self.word.clone()).unwrap_or_default()
    }

    /// Count correctly guessed letters.
    fn correct_count(&self) -> usize {
        let mut count = 0;
        for i in 0..26 {
            if self.guessed[i] {
                let letter = b'a' + i as u8;
                if self.word.contains(&letter) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Count incorrectly guessed letters.
    fn incorrect_letters(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for i in 0..26 {
            if self.guessed[i] {
                let letter = b'a' + i as u8;
                if !self.word.contains(&letter) {
                    result.push(letter);
                }
            }
        }
        result
    }

    /// Count correctly guessed letters (that appear in word).
    fn correct_letters(&self) -> Vec<u8> {
        let mut result = Vec::new();
        for i in 0..26 {
            if self.guessed[i] {
                let letter = b'a' + i as u8;
                if self.word.contains(&letter) {
                    result.push(letter);
                }
            }
        }
        result
    }

    /// Total unique letters guessed.
    fn total_guessed(&self) -> usize {
        self.guessed.iter().filter(|&&g| g).count()
    }

    /// Remaining wrong guesses before loss.
    fn remaining_guesses(&self) -> usize {
        MAX_WRONG.saturating_sub(self.wrong_count)
    }

    // -- Rendering ------------------------------------------------------

    /// Produce the full set of render commands.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        match self.phase {
            GamePhase::CategorySelect => self.render_category_select(&mut cmds),
            _ => {
                self.render_header(&mut cmds);
                self.render_gallows(&mut cmds);
                self.render_word_display(&mut cmds);
                self.render_keyboard(&mut cmds);
                self.render_stats_panel(&mut cmds);
                if self.phase == GamePhase::Won || self.phase == GamePhase::Lost {
                    self.render_result_overlay(&mut cmds);
                }
            }
        }

        cmds
    }

    fn render_category_select(&self, cmds: &mut Vec<RenderCommand>) {
        // Title.
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 80.0,
            y: 40.0,
            text: String::from("HANGMAN"),
            color: LAVENDER,
            font_size: TITLE_FONT + 8.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Subtitle.
        cmds.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 100.0,
            y: 80.0,
            text: String::from("Choose a Category"),
            color: SUBTEXT0,
            font_size: HEADER_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Category buttons.
        let btn_w = 200.0_f32;
        let btn_h = 44.0_f32;
        let btn_gap = 12.0_f32;
        let start_y = 120.0_f32;
        let start_x = (WINDOW_WIDTH - btn_w) / 2.0;

        for (i, cat) in Category::ALL.iter().enumerate() {
            let y = start_y + i as f32 * (btn_h + btn_gap);
            let is_highlighted = i == self.category_cursor;

            let bg_color = if is_highlighted { SURFACE0 } else { MANTLE };
            let border_color = if is_highlighted {
                cat.color()
            } else {
                OVERLAY0
            };

            cmds.push(RenderCommand::FillRect {
                x: start_x,
                y,
                width: btn_w,
                height: btn_h,
                color: bg_color,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::StrokeRect {
                x: start_x,
                y,
                width: btn_w,
                height: btn_h,
                color: border_color,
                line_width: if is_highlighted { 2.0 } else { 1.0 },
                corner_radii: CornerRadii::all(6.0),
            });

            let word_count = cat.words().len();
            let label = format!("{} ({})", cat.label(), word_count);
            cmds.push(RenderCommand::Text {
                x: start_x + 16.0,
                y: y + 13.0,
                text: label,
                color: if is_highlighted {
                    cat.color()
                } else {
                    TEXT_COLOR
                },
                font_size: CATEGORY_FONT + 2.0,
                font_weight: if is_highlighted {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(btn_w - 32.0),
            });
        }

        // Difficulty indicator.
        let diff_y = start_y + 5.0 * (btn_h + btn_gap) + 10.0;
        cmds.push(RenderCommand::Text {
            x: start_x,
            y: diff_y,
            text: format!("Difficulty: {} (1/2/3 to change)", self.difficulty.label()),
            color: OVERLAY0,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(btn_w),
        });

        // Controls.
        let ctrl_y = diff_y + 30.0;
        let controls = [
            "Up/Down: Select category",
            "Enter: Start game",
            "1/2/3: Easy/Medium/Hard",
        ];
        for (i, line) in controls.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: start_x,
                y: ctrl_y + i as f32 * 18.0,
                text: String::from(*line),
                color: OVERLAY0,
                font_size: HINT_FONT,
                font_weight: FontWeightHint::Light,
                max_width: Some(btn_w),
            });
        }
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_w = WINDOW_WIDTH - PADDING * 2.0;

        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: header_w,
            height: HEADER_HEIGHT - 4.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 10.0,
            text: String::from("Hangman"),
            color: LAVENDER,
            font_size: HEADER_FONT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Category badge.
        cmds.push(RenderCommand::Text {
            x: PADDING + 100.0,
            y: PADDING + 10.0,
            text: self.category.label().to_string(),
            color: self.category.color(),
            font_size: HEADER_FONT - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Difficulty.
        cmds.push(RenderCommand::Text {
            x: PADDING + 220.0,
            y: PADDING + 10.0,
            text: self.difficulty.label().to_string(),
            color: SUBTEXT0,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Remaining guesses.
        let remaining = self.remaining_guesses();
        let rem_color = if remaining <= 2 { RED } else { TEAL };
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 30.0,
            text: format!("Remaining: {remaining}"),
            color: rem_color,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Hint status.
        let hint_text = if self.hint_used {
            "Hint: used"
        } else {
            "Hint: H key"
        };
        let hint_color = if self.hint_used { OVERLAY0 } else { YELLOW };
        cmds.push(RenderCommand::Text {
            x: PADDING + 140.0,
            y: PADDING + 30.0,
            text: String::from(hint_text),
            color: hint_color,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Win rate in header.
        let rate = self.stats.win_rate_percent();
        cmds.push(RenderCommand::Text {
            x: header_w - 60.0,
            y: PADDING + 10.0,
            text: format!("{rate}% win"),
            color: OVERLAY0,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_gallows(&self, cmds: &mut Vec<RenderCommand>) {
        let gx = PADDING + 20.0;
        let gy = HEADER_HEIGHT + PADDING + 10.0;
        let line_color = SUBTEXT0;
        let line_w = 3.0;

        // Base.
        cmds.push(RenderCommand::Line {
            x1: gx,
            y1: gy + GALLOWS_SIZE - 10.0,
            x2: gx + 120.0,
            y2: gy + GALLOWS_SIZE - 10.0,
            color: line_color,
            width: line_w,
        });

        // Vertical pole.
        cmds.push(RenderCommand::Line {
            x1: gx + 30.0,
            y1: gy + 10.0,
            x2: gx + 30.0,
            y2: gy + GALLOWS_SIZE - 10.0,
            color: line_color,
            width: line_w,
        });

        // Top beam.
        cmds.push(RenderCommand::Line {
            x1: gx + 30.0,
            y1: gy + 10.0,
            x2: gx + 100.0,
            y2: gy + 10.0,
            color: line_color,
            width: line_w,
        });

        // Rope.
        cmds.push(RenderCommand::Line {
            x1: gx + 100.0,
            y1: gy + 10.0,
            x2: gx + 100.0,
            y2: gy + 40.0,
            color: line_color,
            width: 2.0,
        });

        // Draw the hangman figure based on wrong_count.
        let body_color = if self.phase == GamePhase::Lost {
            RED
        } else {
            TEXT_COLOR
        };
        let cx = gx + 100.0;
        let head_y = gy + 40.0;
        let body_w = 2.0;

        if self.wrong_count >= 1 {
            // Head (circle approximated with lines).
            let head_r = 15.0;
            let head_cx = cx;
            let head_cy = head_y + head_r;
            let segments = 12;
            for seg_i in 0..segments {
                let a1 =
                    (seg_i as f32) * std::f32::consts::TAU / (segments as f32);
                let a2 = ((seg_i + 1) as f32) * std::f32::consts::TAU
                    / (segments as f32);
                cmds.push(RenderCommand::Line {
                    x1: head_cx + head_r * a1.cos(),
                    y1: head_cy + head_r * a1.sin(),
                    x2: head_cx + head_r * a2.cos(),
                    y2: head_cy + head_r * a2.sin(),
                    color: body_color,
                    width: body_w,
                });
            }
        }

        if self.wrong_count >= 2 {
            // Body.
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: head_y + 30.0,
                x2: cx,
                y2: head_y + 80.0,
                color: body_color,
                width: body_w,
            });
        }

        if self.wrong_count >= 3 {
            // Left arm.
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: head_y + 45.0,
                x2: cx - 25.0,
                y2: head_y + 65.0,
                color: body_color,
                width: body_w,
            });
        }

        if self.wrong_count >= 4 {
            // Right arm.
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: head_y + 45.0,
                x2: cx + 25.0,
                y2: head_y + 65.0,
                color: body_color,
                width: body_w,
            });
        }

        if self.wrong_count >= 5 {
            // Left leg.
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: head_y + 80.0,
                x2: cx - 22.0,
                y2: head_y + 110.0,
                color: body_color,
                width: body_w,
            });
        }

        if self.wrong_count >= 6 {
            // Right leg.
            cmds.push(RenderCommand::Line {
                x1: cx,
                y1: head_y + 80.0,
                x2: cx + 22.0,
                y2: head_y + 110.0,
                color: body_color,
                width: body_w,
            });
        }
    }

    fn render_word_display(&self, cmds: &mut Vec<RenderCommand>) {
        let word_y = HEADER_HEIGHT + PADDING + GALLOWS_SIZE + 20.0;
        let start_x = PADDING + 20.0;

        // Word display with blanks.
        let letter_spacing = 26.0_f32;
        let max_letters = self.word.len();
        let total_w = max_letters as f32 * letter_spacing;
        let cx = start_x + (WINDOW_WIDTH - STATS_PANEL_WIDTH - start_x * 2.0 - total_w) / 2.0;

        for (i, &b) in self.word.iter().enumerate() {
            let x = cx + i as f32 * letter_spacing;

            let (ch, color) = if let Some(idx) = letter_index(b) {
                if self.guessed[idx] {
                    (b.to_ascii_uppercase() as char, GREEN)
                } else if self.phase == GamePhase::Lost {
                    // Reveal unguessed letters in red on loss.
                    (b.to_ascii_uppercase() as char, RED)
                } else {
                    ('_', OVERLAY0)
                }
            } else {
                (b as char, TEXT_COLOR)
            };

            cmds.push(RenderCommand::Text {
                x,
                y: word_y,
                text: ch.to_string(),
                color,
                font_size: WORD_FONT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Underline for each letter slot.
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: word_y + 30.0,
                x2: x + 18.0,
                y2: word_y + 30.0,
                color: SURFACE0,
                width: 2.0,
            });
        }
    }

    fn render_keyboard(&self, cmds: &mut Vec<RenderCommand>) {
        let kb_y = HEADER_HEIGHT + PADDING + GALLOWS_SIZE + WORD_AREA_HEIGHT + 40.0;
        let key_size = 28.0_f32;
        let key_gap = 4.0_f32;

        // Three rows of keys: QWERTYUIOP / ASDFGHJKL / ZXCVBNM
        let rows: [&[u8]; 3] = [
            b"QWERTYUIOP",
            b"ASDFGHJKL",
            b"ZXCVBNM",
        ];
        let row_offsets: [f32; 3] = [0.0, 16.0, 40.0];

        for (row_i, row) in rows.iter().enumerate() {
            let x_start = PADDING + 10.0 + row_offsets[row_i];
            let y = kb_y + row_i as f32 * (key_size + key_gap);

            for (col, &letter) in row.iter().enumerate() {
                let x = x_start + col as f32 * (key_size + key_gap);
                let lower = letter.to_ascii_lowercase();

                let (bg, fg) = if let Some(idx) = letter_index(lower) {
                    if self.guessed[idx] {
                        let in_word = self.word.contains(&lower);
                        if in_word {
                            (GREEN, BASE)
                        } else {
                            (RED, BASE)
                        }
                    } else {
                        (SURFACE0, TEXT_COLOR)
                    }
                } else {
                    (SURFACE0, TEXT_COLOR)
                };

                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: key_size,
                    height: key_size,
                    color: bg,
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 7.0,
                    y: y + 6.0,
                    text: (letter as char).to_string(),
                    color: fg,
                    font_size: KEY_FONT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
        }
    }

    fn render_stats_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let sx = WINDOW_WIDTH - STATS_PANEL_WIDTH - PADDING;
        let sy = HEADER_HEIGHT + PADDING + 10.0;
        let panel_h = GALLOWS_SIZE + WORD_AREA_HEIGHT + KEYBOARD_HEIGHT;

        // Panel background.
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: STATS_PANEL_WIDTH,
            height: panel_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        let mut y_off = sy + 12.0;
        let line_h = 20.0_f32;

        // Stats header.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: y_off,
            text: String::from("Statistics"),
            color: LAVENDER,
            font_size: STATS_FONT + 1.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(STATS_PANEL_WIDTH - 20.0),
        });
        y_off += line_h + 4.0;

        // Stats rows.
        let stat_lines = [
            (format!("Wins: {}", self.stats.wins), GREEN),
            (format!("Losses: {}", self.stats.losses), RED),
            (format!("Streak: {}", self.stats.current_streak), YELLOW),
            (format!("Best: {}", self.stats.best_streak), PEACH),
            (
                format!("Win Rate: {}%", self.stats.win_rate_percent()),
                TEAL,
            ),
            (format!("Games: {}", self.stats.total_games()), SUBTEXT0),
        ];

        for (text, color) in &stat_lines {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: text.clone(),
                color: *color,
                font_size: STATS_FONT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(STATS_PANEL_WIDTH - 20.0),
            });
            y_off += line_h;
        }

        // Separator.
        y_off += 6.0;
        cmds.push(RenderCommand::Line {
            x1: sx + 10.0,
            y1: y_off,
            x2: sx + STATS_PANEL_WIDTH - 10.0,
            y2: y_off,
            color: SURFACE0,
            width: 1.0,
        });
        y_off += 12.0;

        // Wrong letters.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: y_off,
            text: String::from("Wrong:"),
            color: OVERLAY0,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y_off += line_h;

        let wrong = self.incorrect_letters();
        if wrong.is_empty() {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: String::from("None yet"),
                color: OVERLAY0,
                font_size: STATS_FONT - 1.0,
                font_weight: FontWeightHint::Light,
                max_width: None,
            });
        } else {
            let wrong_str: String =
                wrong.iter().map(|&b| (b as char).to_ascii_uppercase()).collect::<Vec<_>>().iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" ");
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: wrong_str,
                color: RED,
                font_size: STATS_FONT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(STATS_PANEL_WIDTH - 20.0),
            });
        }
        y_off += line_h + 6.0;

        // Correct letters.
        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: y_off,
            text: String::from("Correct:"),
            color: OVERLAY0,
            font_size: STATS_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        y_off += line_h;

        let correct = self.correct_letters();
        if correct.is_empty() {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: String::from("None yet"),
                color: OVERLAY0,
                font_size: STATS_FONT - 1.0,
                font_weight: FontWeightHint::Light,
                max_width: None,
            });
        } else {
            let correct_str: String =
                correct.iter().map(|&b| (b as char).to_ascii_uppercase()).collect::<Vec<_>>().iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" ");
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: correct_str,
                color: GREEN,
                font_size: STATS_FONT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(STATS_PANEL_WIDTH - 20.0),
            });
        }
        y_off += line_h + 12.0;

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: sx + 10.0,
            y1: y_off,
            x2: sx + STATS_PANEL_WIDTH - 10.0,
            y2: y_off,
            color: SURFACE0,
            width: 1.0,
        });
        y_off += 12.0;

        // Controls.
        let controls = [
            "A-Z: Guess letter",
            "H: Use hint (1/game)",
            "Esc: Category select",
            "Enter: New round",
            "1/2/3: Difficulty",
        ];
        for line in &controls {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: y_off,
                text: String::from(*line),
                color: OVERLAY0,
                font_size: HINT_FONT,
                font_weight: FontWeightHint::Light,
                max_width: Some(STATS_PANEL_WIDTH - 20.0),
            });
            y_off += line_h - 2.0;
        }
    }

    fn render_result_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = PADDING + 20.0;
        let oy = HEADER_HEIGHT + PADDING + 10.0;
        let gw = WINDOW_WIDTH - STATS_PANEL_WIDTH - PADDING * 2.0 - 20.0;
        let gh = GALLOWS_SIZE;

        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: ox,
            y: oy,
            width: gw,
            height: gh,
            color: Color::rgba(17, 17, 27, 200),
            corner_radii: CornerRadii::ZERO,
        });

        let box_w = 280.0;
        let box_h = 160.0;
        let box_x = ox + (gw - box_w) / 2.0;
        let box_y = oy + (gh - box_h) / 2.0;

        let accent = if self.phase == GamePhase::Won {
            GREEN
        } else {
            RED
        };

        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: accent,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        let title = if self.phase == GamePhase::Won {
            "YOU WIN!"
        } else {
            "GAME OVER"
        };
        cmds.push(RenderCommand::Text {
            x: box_x + 70.0,
            y: box_y + 20.0,
            text: String::from(title),
            color: accent,
            font_size: TITLE_FONT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // The word.
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 55.0,
            text: format!("Word: {}", self.word_string().to_uppercase()),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(box_w - 40.0),
        });

        // Streak.
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 80.0,
            text: format!("Streak: {}", self.stats.current_streak),
            color: YELLOW,
            font_size: OVERLAY_FONT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Restart hint.
        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 110.0,
            text: String::from("Press Enter for new round"),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 30.0,
            y: box_y + 130.0,
            text: String::from("Esc: Category select"),
            color: OVERLAY0,
            font_size: OVERLAY_FONT - 2.0,
            font_weight: FontWeightHint::Light,
            max_width: None,
        });
    }

    // -- Event handling -------------------------------------------------

    /// Handle incoming events (called by the framework).
    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event
            && ke.pressed {
                self.handle_key(ke.key);
            }
    }

    fn handle_key(&mut self, key: Key) {
        match self.phase {
            GamePhase::CategorySelect => self.handle_category_key(key),
            GamePhase::Playing => self.handle_playing_key(key),
            GamePhase::Won | GamePhase::Lost => self.handle_result_key(key),
        }
    }

    fn handle_category_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                if self.category_cursor == 0 {
                    self.category_cursor = Category::ALL.len() - 1;
                } else {
                    self.category_cursor -= 1;
                }
            }
            Key::Down => {
                self.category_cursor = (self.category_cursor + 1) % Category::ALL.len();
            }
            Key::Enter => {
                if let Some(cat) = Category::from_index(self.category_cursor) {
                    self.category = cat;
                }
                self.start_from_category();
            }
            key => {
                if let Some(diff) = difficulty_from_key(key) {
                    self.difficulty = diff;
                }
            }
        }
    }

    fn handle_playing_key(&mut self, key: Key) {
        // Check for letter guess (A-Z keys).
        if let Some(letter) = key_to_letter(key) {
            // H is special: if not yet guessed, treat as hint key.
            if letter == b'h' && !self.guessed[letter_index(b'h').unwrap_or(0)] && !self.hint_used {
                self.use_hint();
            } else {
                self.guess_letter(letter);
            }
            return;
        }

        match key {
            Key::Escape => {
                self.phase = GamePhase::CategorySelect;
            }
            Key::Enter => {
                self.new_round();
            }
            _ => {
                if let Some(diff) = difficulty_from_key(key) {
                    self.difficulty = diff;
                }
            }
        }
    }

    fn handle_result_key(&mut self, key: Key) {
        match key {
            Key::Enter => {
                self.new_round();
            }
            Key::Escape => {
                self.phase = GamePhase::CategorySelect;
            }
            _ => {
                if let Some(diff) = difficulty_from_key(key) {
                    self.difficulty = diff;
                }
            }
        }
    }
}

// -- Utility functions --------------------------------------------------

/// Convert a byte to its 0-25 index (a=0, z=25). Returns None for non-letters.
fn letter_index(b: u8) -> Option<usize> {
    let lower = b.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        Some((lower - b'a') as usize)
    } else {
        None
    }
}

/// Convert a Key enum variant to a lowercase ASCII letter byte.
fn key_to_letter(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(b'a'),
        Key::B => Some(b'b'),
        Key::C => Some(b'c'),
        Key::D => Some(b'd'),
        Key::E => Some(b'e'),
        Key::F => Some(b'f'),
        Key::G => Some(b'g'),
        Key::H => Some(b'h'),
        Key::I => Some(b'i'),
        Key::J => Some(b'j'),
        Key::K => Some(b'k'),
        Key::L => Some(b'l'),
        Key::M => Some(b'm'),
        Key::N => Some(b'n'),
        Key::O => Some(b'o'),
        Key::P => Some(b'p'),
        Key::Q => Some(b'q'),
        Key::R => Some(b'r'),
        Key::S => Some(b's'),
        Key::T => Some(b't'),
        Key::U => Some(b'u'),
        Key::V => Some(b'v'),
        Key::W => Some(b'w'),
        Key::X => Some(b'x'),
        Key::Y => Some(b'y'),
        Key::Z => Some(b'z'),
        _ => None,
    }
}

/// Convert a Key to a difficulty level (1=Easy, 2=Medium, 3=Hard).
fn difficulty_from_key(key: Key) -> Option<Difficulty> {
    match key {
        Key::Num1 => Some(Difficulty::Easy),
        Key::Num2 => Some(Difficulty::Medium),
        Key::Num3 => Some(Difficulty::Hard),
        _ => None,
    }
}

fn main() {
    let _app = HangmanApp::new();
}

// =====================================================================
// Tests
// =====================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a game with a fixed seed for deterministic tests.
    fn test_app() -> HangmanApp {
        HangmanApp::with_seed(12345)
    }

    /// Helper to create a playing-state game with a known word.
    fn playing_app(word: &str) -> HangmanApp {
        let mut app = test_app();
        app.word = word.as_bytes().to_vec();
        app.guessed = [false; 26];
        app.wrong_count = 0;
        app.hint_used = false;
        app.phase = GamePhase::Playing;
        app
    }

    // -- Construction & initialization ----------------------------------

    #[test]
    fn test_initial_phase_is_category_select() {
        let app = test_app();
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    #[test]
    fn test_initial_category_is_animals() {
        let app = test_app();
        assert_eq!(app.category, Category::Animals);
    }

    #[test]
    fn test_initial_difficulty_is_medium() {
        let app = test_app();
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn test_initial_wrong_count_zero() {
        let app = test_app();
        assert_eq!(app.wrong_count, 0);
    }

    #[test]
    fn test_initial_hint_not_used() {
        let app = test_app();
        assert!(!app.hint_used);
    }

    #[test]
    fn test_initial_stats_clean() {
        let app = test_app();
        assert_eq!(app.stats.wins, 0);
        assert_eq!(app.stats.losses, 0);
        assert_eq!(app.stats.current_streak, 0);
        assert_eq!(app.stats.best_streak, 0);
    }

    #[test]
    fn test_initial_word_not_empty() {
        let app = test_app();
        assert!(!app.word.is_empty());
    }

    #[test]
    fn test_initial_category_cursor_zero() {
        let app = test_app();
        assert_eq!(app.category_cursor, 0);
    }

    #[test]
    fn test_new_creates_valid_state() {
        let app = HangmanApp::new();
        assert!(!app.word.is_empty());
        assert_eq!(app.wrong_count, 0);
    }

    // -- Word list validation -------------------------------------------

    #[test]
    fn test_total_word_count_over_100() {
        assert!(total_word_count() >= 100);
    }

    #[test]
    fn test_animals_count() {
        assert!(ANIMALS.len() >= 20);
    }

    #[test]
    fn test_fruits_count() {
        assert!(FRUITS.len() >= 20);
    }

    #[test]
    fn test_countries_count() {
        assert!(COUNTRIES.len() >= 20);
    }

    #[test]
    fn test_sports_count() {
        assert!(SPORTS.len() >= 20);
    }

    #[test]
    fn test_technology_count() {
        assert!(TECHNOLOGY.len() >= 20);
    }

    #[test]
    fn test_all_words_lowercase() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(
                    word.chars().all(|c| c.is_ascii_lowercase()),
                    "Word '{}' in {:?} is not all lowercase",
                    word,
                    cat
                );
            }
        }
    }

    #[test]
    fn test_all_words_non_empty() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(!word.is_empty(), "{:?} contains empty word", cat);
            }
        }
    }

    #[test]
    fn test_all_words_no_duplicates() {
        let mut all_words: Vec<&str> = Vec::new();
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(!all_words.contains(word), "Duplicate word: {}", word);
                all_words.push(word);
            }
        }
    }

    // -- Category -------------------------------------------------------

    #[test]
    fn test_category_all_has_five() {
        assert_eq!(Category::ALL.len(), 5);
    }

    #[test]
    fn test_category_labels() {
        assert_eq!(Category::Animals.label(), "Animals");
        assert_eq!(Category::Fruits.label(), "Fruits");
        assert_eq!(Category::Countries.label(), "Countries");
        assert_eq!(Category::Sports.label(), "Sports");
        assert_eq!(Category::Technology.label(), "Technology");
    }

    #[test]
    fn test_category_from_index() {
        assert_eq!(Category::from_index(0), Some(Category::Animals));
        assert_eq!(Category::from_index(4), Some(Category::Technology));
        assert_eq!(Category::from_index(5), None);
    }

    #[test]
    fn test_category_words_returns_correct_array() {
        assert_eq!(Category::Animals.words().len(), ANIMALS.len());
        assert_eq!(Category::Technology.words().len(), TECHNOLOGY.len());
    }

    // -- Difficulty -----------------------------------------------------

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Easy.label(), "Easy");
        assert_eq!(Difficulty::Medium.label(), "Medium");
        assert_eq!(Difficulty::Hard.label(), "Hard");
    }

    #[test]
    fn test_difficulty_length_ranges() {
        // Easy words are shorter.
        assert!(Difficulty::Easy.max_length() <= Difficulty::Medium.max_length());
        assert!(Difficulty::Medium.min_length() >= Difficulty::Easy.min_length());
    }

    #[test]
    fn test_easy_has_free_reveals() {
        assert!(Difficulty::Easy.free_reveals() > 0);
    }

    #[test]
    fn test_hard_has_no_free_reveals() {
        assert_eq!(Difficulty::Hard.free_reveals(), 0);
    }

    #[test]
    fn test_medium_has_one_free_reveal() {
        assert_eq!(Difficulty::Medium.free_reveals(), 1);
    }

    // -- LCG RNG --------------------------------------------------------

    #[test]
    fn test_lcg_deterministic() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        for _ in 0..10 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn test_lcg_different_seeds() {
        let mut a = Lcg::new(1);
        let mut b = Lcg::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(99);
        for _ in 0..100 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_lcg_bounded_one() {
        let mut rng = Lcg::new(77);
        for _ in 0..20 {
            assert_eq!(rng.next_bounded(1), 0);
        }
    }

    // -- letter_index ---------------------------------------------------

    #[test]
    fn test_letter_index_lowercase() {
        assert_eq!(letter_index(b'a'), Some(0));
        assert_eq!(letter_index(b'z'), Some(25));
        assert_eq!(letter_index(b'm'), Some(12));
    }

    #[test]
    fn test_letter_index_uppercase() {
        assert_eq!(letter_index(b'A'), Some(0));
        assert_eq!(letter_index(b'Z'), Some(25));
    }

    #[test]
    fn test_letter_index_non_letter() {
        assert_eq!(letter_index(b'1'), None);
        assert_eq!(letter_index(b' '), None);
        assert_eq!(letter_index(b'-'), None);
    }

    // -- key_to_letter --------------------------------------------------

    #[test]
    fn test_key_to_letter_a() {
        assert_eq!(key_to_letter(Key::A), Some(b'a'));
    }

    #[test]
    fn test_key_to_letter_z() {
        assert_eq!(key_to_letter(Key::Z), Some(b'z'));
    }

    #[test]
    fn test_key_to_letter_non_letter() {
        assert_eq!(key_to_letter(Key::Enter), None);
        assert_eq!(key_to_letter(Key::Space), None);
        assert_eq!(key_to_letter(Key::Escape), None);
    }

    // -- difficulty_from_key --------------------------------------------

    #[test]
    fn test_difficulty_from_key_digits() {
        assert_eq!(difficulty_from_key(Key::Num1), Some(Difficulty::Easy));
        assert_eq!(difficulty_from_key(Key::Num2), Some(Difficulty::Medium));
        assert_eq!(difficulty_from_key(Key::Num3), Some(Difficulty::Hard));
    }

    #[test]
    fn test_difficulty_from_key_other() {
        assert_eq!(difficulty_from_key(Key::A), None);
        assert_eq!(difficulty_from_key(Key::Enter), None);
    }

    // -- Guessing logic -------------------------------------------------

    #[test]
    fn test_guess_correct_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'c');
        assert!(result);
        assert_eq!(app.wrong_count, 0);
    }

    #[test]
    fn test_guess_wrong_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'x');
        assert!(result);
        assert_eq!(app.wrong_count, 1);
    }

    #[test]
    fn test_guess_duplicate_rejected() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        let result = app.guess_letter(b'c');
        assert!(!result);
    }

    #[test]
    fn test_guess_in_wrong_phase() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        let result = app.guess_letter(b'a');
        assert!(!result);
    }

    #[test]
    fn test_win_detection() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(app.phase, GamePhase::Won);
    }

    #[test]
    fn test_loss_detection() {
        let mut app = playing_app("cat");
        for &letter in b"xyzqwe" {
            app.guess_letter(letter);
        }
        assert_eq!(app.phase, GamePhase::Lost);
        assert_eq!(app.wrong_count, MAX_WRONG);
    }

    #[test]
    fn test_win_increments_stats() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(app.stats.wins, 1);
        assert_eq!(app.stats.current_streak, 1);
    }

    #[test]
    fn test_loss_increments_stats() {
        let mut app = playing_app("cat");
        for &letter in b"xyzqwe" {
            app.guess_letter(letter);
        }
        assert_eq!(app.stats.losses, 1);
        assert_eq!(app.stats.current_streak, 0);
    }

    // -- Word display ---------------------------------------------------

    #[test]
    fn test_display_word_all_blanks() {
        let app = playing_app("cat");
        assert_eq!(app.display_word(), "_ _ _");
    }

    #[test]
    fn test_display_word_partial() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        assert_eq!(app.display_word(), "c _ _");
    }

    #[test]
    fn test_display_word_full() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert_eq!(app.display_word(), "c a t");
    }

    #[test]
    fn test_word_string() {
        let app = playing_app("dolphin");
        assert_eq!(app.word_string(), "dolphin");
    }

    // -- Incorrect / correct letters ------------------------------------

    #[test]
    fn test_incorrect_letters_empty() {
        let app = playing_app("cat");
        assert!(app.incorrect_letters().is_empty());
    }

    #[test]
    fn test_incorrect_letters_tracked() {
        let mut app = playing_app("cat");
        app.guess_letter(b'x');
        app.guess_letter(b'z');
        let wrong = app.incorrect_letters();
        assert_eq!(wrong.len(), 2);
        assert!(wrong.contains(&b'x'));
        assert!(wrong.contains(&b'z'));
    }

    #[test]
    fn test_correct_letters_tracked() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        let correct = app.correct_letters();
        assert_eq!(correct.len(), 1);
        assert!(correct.contains(&b'c'));
    }

    // -- Hint system ----------------------------------------------------

    #[test]
    fn test_hint_reveals_letter() {
        let mut app = playing_app("cat");
        let before = app.total_guessed();
        app.use_hint();
        assert!(app.total_guessed() > before);
    }

    #[test]
    fn test_hint_can_only_be_used_once() {
        let mut app = playing_app("cat");
        assert!(app.use_hint());
        assert!(!app.use_hint());
    }

    #[test]
    fn test_hint_sets_flag() {
        let mut app = playing_app("cat");
        app.use_hint();
        assert!(app.hint_used);
    }

    #[test]
    fn test_hint_not_available_when_won() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        assert!(!app.use_hint());
    }

    #[test]
    fn test_hint_not_available_when_lost() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        assert!(!app.use_hint());
    }

    // -- Remaining guesses ----------------------------------------------

    #[test]
    fn test_remaining_guesses_initial() {
        let app = playing_app("cat");
        assert_eq!(app.remaining_guesses(), MAX_WRONG);
    }

    #[test]
    fn test_remaining_guesses_after_wrong() {
        let mut app = playing_app("cat");
        app.guess_letter(b'x');
        assert_eq!(app.remaining_guesses(), MAX_WRONG - 1);
    }

    #[test]
    fn test_remaining_guesses_correct_no_change() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        assert_eq!(app.remaining_guesses(), MAX_WRONG);
    }

    // -- Stats ----------------------------------------------------------

    #[test]
    fn test_stats_record_win() {
        let mut stats = Stats::new();
        stats.record_win();
        assert_eq!(stats.wins, 1);
        assert_eq!(stats.current_streak, 1);
        assert_eq!(stats.best_streak, 1);
    }

    #[test]
    fn test_stats_record_loss_resets_streak() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.current_streak, 0);
        assert_eq!(stats.best_streak, 2);
    }

    #[test]
    fn test_stats_best_streak_preserved() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_win();
        stats.record_win();
        stats.record_loss();
        stats.record_win();
        assert_eq!(stats.best_streak, 3);
        assert_eq!(stats.current_streak, 1);
    }

    #[test]
    fn test_stats_total_games() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.total_games(), 2);
    }

    #[test]
    fn test_stats_win_rate_zero() {
        let stats = Stats::new();
        assert_eq!(stats.win_rate_percent(), 0);
    }

    #[test]
    fn test_stats_win_rate_100() {
        let mut stats = Stats::new();
        stats.record_win();
        assert_eq!(stats.win_rate_percent(), 100);
    }

    #[test]
    fn test_stats_win_rate_50() {
        let mut stats = Stats::new();
        stats.record_win();
        stats.record_loss();
        assert_eq!(stats.win_rate_percent(), 50);
    }

    // -- New round / restart --------------------------------------------

    #[test]
    fn test_new_round_resets_guesses() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        app.new_round();
        assert_eq!(app.wrong_count, 0);
        assert_eq!(app.total_guessed(), app.difficulty.free_reveals().min(26));
    }

    #[test]
    fn test_new_round_preserves_stats() {
        let mut app = playing_app("cat");
        app.stats.record_win();
        app.new_round();
        assert_eq!(app.stats.wins, 1);
    }

    #[test]
    fn test_new_round_sets_playing() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        app.new_round();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_new_round_resets_hint() {
        let mut app = playing_app("cat");
        app.hint_used = true;
        app.new_round();
        assert!(!app.hint_used);
    }

    // -- Category selection keys ----------------------------------------

    #[test]
    fn test_category_down_key() {
        let mut app = test_app();
        app.handle_key(Key::Down);
        assert_eq!(app.category_cursor, 1);
    }

    #[test]
    fn test_category_up_key_wraps() {
        let mut app = test_app();
        app.handle_key(Key::Up);
        assert_eq!(app.category_cursor, 4);
    }

    #[test]
    fn test_category_down_wraps() {
        let mut app = test_app();
        app.category_cursor = 4;
        app.handle_key(Key::Down);
        assert_eq!(app.category_cursor, 0);
    }

    #[test]
    fn test_category_enter_starts_game() {
        let mut app = test_app();
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_category_enter_selects_category() {
        let mut app = test_app();
        app.category_cursor = 2;
        app.handle_key(Key::Enter);
        assert_eq!(app.category, Category::Countries);
    }

    #[test]
    fn test_category_difficulty_change() {
        let mut app = test_app();
        app.handle_key(Key::Num1);
        assert_eq!(app.difficulty, Difficulty::Easy);
        app.handle_key(Key::Num3);
        assert_eq!(app.difficulty, Difficulty::Hard);
    }

    // -- Playing keys ---------------------------------------------------

    #[test]
    fn test_playing_letter_key() {
        let mut app = playing_app("cat");
        app.handle_key(Key::C);
        assert!(app.guessed[letter_index(b'c').unwrap()]);
    }

    #[test]
    fn test_playing_escape_to_category() {
        let mut app = playing_app("cat");
        app.handle_key(Key::Escape);
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    #[test]
    fn test_playing_enter_new_round() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
        assert_eq!(app.wrong_count, 0);
    }

    // -- Result keys ----------------------------------------------------

    #[test]
    fn test_result_enter_new_round() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        app.handle_key(Key::Enter);
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_result_escape_to_category() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        app.handle_key(Key::Escape);
        assert_eq!(app.phase, GamePhase::CategorySelect);
    }

    // -- Event handling -------------------------------------------------

    #[test]
    fn test_handle_event_key_press() {
        let mut app = playing_app("cat");
        let event = Event::Key(KeyEvent {
            key: Key::C,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        });
        app.handle_event(&event);
        assert!(app.guessed[letter_index(b'c').unwrap()]);
    }

    #[test]
    fn test_handle_event_key_release_ignored() {
        let mut app = playing_app("cat");
        let event = Event::Key(KeyEvent {
            key: Key::C,
            pressed: false,
            modifiers: Modifiers::default(),
            text: None,
        });
        app.handle_event(&event);
        assert!(!app.guessed[letter_index(b'c').unwrap()]);
    }

    // -- is_word_revealed -----------------------------------------------

    #[test]
    fn test_word_not_revealed_initially() {
        let app = playing_app("cat");
        assert!(!app.is_word_revealed());
    }

    #[test]
    fn test_word_revealed_after_all_guessed() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        assert!(app.is_word_revealed());
    }

    #[test]
    fn test_word_with_duplicate_letters() {
        let mut app = playing_app("banana");
        app.guess_letter(b'b');
        app.guess_letter(b'a');
        app.guess_letter(b'n');
        assert!(app.is_word_revealed());
    }

    // -- Rendering smoke tests ------------------------------------------

    #[test]
    fn test_render_category_select() {
        let app = test_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Playing;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Won;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_lost() {
        let mut app = playing_app("cat");
        app.phase = GamePhase::Lost;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_guesses() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_gallows_wrong_count_progression() {
        // Each wrong count should produce more render commands for the body.
        let mut app = playing_app("zzz");
        let cmds_0 = app.render().len();
        app.wrong_count = 3;
        let cmds_3 = app.render().len();
        app.wrong_count = 6;
        let cmds_6 = app.render().len();
        assert!(cmds_3 > cmds_0);
        assert!(cmds_6 > cmds_3);
    }

    // -- Free reveals ---------------------------------------------------

    #[test]
    fn test_easy_gives_free_reveals() {
        let mut app = test_app();
        app.difficulty = Difficulty::Easy;
        app.guessed = [false; 26];
        app.word = b"cat".to_vec();
        app.apply_free_reveals();
        // Easy gives 2 free reveals.
        let revealed = app.total_guessed();
        assert_eq!(revealed, 2);
    }

    #[test]
    fn test_hard_no_free_reveals() {
        let mut app = test_app();
        app.difficulty = Difficulty::Hard;
        app.guessed = [false; 26];
        app.word = b"cat".to_vec();
        app.apply_free_reveals();
        assert_eq!(app.total_guessed(), 0);
    }

    // -- Correct/incorrect count ----------------------------------------

    #[test]
    fn test_correct_count() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        assert_eq!(app.correct_count(), 1);
    }

    #[test]
    fn test_total_guessed() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'x');
        assert_eq!(app.total_guessed(), 2);
    }

    // -- H key as hint --------------------------------------------------

    #[test]
    fn test_h_key_triggers_hint() {
        let mut app = playing_app("cat");
        app.handle_key(Key::H);
        assert!(app.hint_used);
    }

    #[test]
    fn test_h_key_after_hint_used_guesses_h() {
        let mut app = playing_app("hat");
        app.hint_used = true;
        app.handle_key(Key::H);
        assert!(app.guessed[letter_index(b'h').unwrap()]);
    }

    // -- Pick word respects difficulty ----------------------------------

    #[test]
    fn test_pick_word_easy_short() {
        let mut app = test_app();
        app.difficulty = Difficulty::Easy;
        app.category = Category::Animals;
        for _ in 0..20 {
            app.guessed = [false; 26];
            app.pick_word();
            assert!(
                app.word.len() <= Difficulty::Easy.max_length(),
                "Easy word '{}' too long (len {})",
                app.word_string(),
                app.word.len()
            );
        }
    }

    #[test]
    fn test_pick_word_hard_long() {
        let mut app = test_app();
        app.difficulty = Difficulty::Hard;
        app.category = Category::Technology;
        for _ in 0..20 {
            app.guessed = [false; 26];
            app.pick_word();
            assert!(
                app.word.len() >= Difficulty::Hard.min_length(),
                "Hard word '{}' too short (len {})",
                app.word_string(),
                app.word.len()
            );
        }
    }

    // -- Misc edge cases ------------------------------------------------

    #[test]
    fn test_guess_non_letter() {
        let mut app = playing_app("cat");
        let result = app.guess_letter(b'1');
        assert!(!result);
    }

    #[test]
    fn test_max_wrong_equals_six() {
        assert_eq!(MAX_WRONG, 6);
    }

    #[test]
    fn test_display_word_repeated_letters() {
        let mut app = playing_app("banana");
        app.guess_letter(b'a');
        // All 'a's should be revealed.
        assert_eq!(app.display_word(), "_ a _ a _ a");
    }

    #[test]
    fn test_start_from_category() {
        let mut app = test_app();
        app.phase = GamePhase::CategorySelect;
        app.category = Category::Sports;
        app.start_from_category();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_category_color_unique() {
        let colors: Vec<Color> = Category::ALL.iter().map(|c| c.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "Categories {} and {} share a color", i, j);
            }
        }
    }

    #[test]
    fn test_hint_on_fully_revealed_word() {
        let mut app = playing_app("cat");
        app.guess_letter(b'c');
        app.guess_letter(b'a');
        app.guess_letter(b't');
        // Word is fully revealed; hint should do nothing.
        assert!(!app.use_hint());
    }
}
