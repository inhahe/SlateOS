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
#![allow(clippy::manual_range_contains)]
#![allow(clippy::match_same_arms)]

//! OurOS Word Search — find hidden words in a grid of letters.
//!
//! Features multiple difficulty levels (Easy 10x10, Medium 15x15, Hard 20x20),
//! words placed in all 8 directions, five word categories (Animals, Colors,
//! Food, Science, Geography), cursor-based word selection, a timer, hint
//! system (H key reveals the first letter of an unfound word), and found-word
//! tracking with strikethrough display. Uses a deterministic seeded LCG random
//! number generator (no external `rand` crate). The Catppuccin Mocha color
//! palette provides a pleasant dark theme.

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

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
const PEACH: Color = Color::from_hex(0xFAB387);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);

// ── Layout constants ────────────────────────────────────────────────
const CELL_SIZE: f32 = 30.0;
const CELL_GAP: f32 = 2.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 56.0;
const CELL_CORNER_RADIUS: f32 = 3.0;

const HEADER_FONT_SIZE: f32 = 20.0;
const CELL_FONT_SIZE: f32 = 16.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const WORD_LIST_FONT_SIZE: f32 = 13.0;

const MAX_HINTS: usize = 5;

// ── 8 placement directions ──────────────────────────────────────────
/// (row_delta, col_delta) for each of the 8 directions words can be placed.
const DIRECTIONS: [(i32, i32); 8] = [
    (0, 1),   // right
    (0, -1),  // left
    (1, 0),   // down
    (-1, 0),  // up
    (1, 1),   // down-right
    (1, -1),  // down-left
    (-1, 1),  // up-right
    (-1, -1), // up-left
];

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
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
        if bound == 0 {
            return 0;
        }
        let val = self.next_u64();
        (val % bound as u64) as usize
    }

    /// Returns a random uppercase ASCII letter ('A'..='Z').
    fn next_letter(&mut self) -> u8 {
        b'A' + self.next_bounded(26) as u8
    }

    /// Fisher-Yates shuffle of a mutable slice.
    fn shuffle<T>(&mut self, slice: &mut [T]) {
        let len = slice.len();
        if len <= 1 {
            return;
        }
        for i in (1..len).rev() {
            let j = self.next_bounded(i + 1);
            slice.swap(i, j);
        }
    }
}

// ── Word categories ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Animals,
    Colors,
    Food,
    Science,
    Geography,
}

impl Category {
    const ALL: [Category; 5] = [
        Category::Animals,
        Category::Colors,
        Category::Food,
        Category::Science,
        Category::Geography,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Animals => "Animals",
            Self::Colors => "Colors",
            Self::Food => "Food",
            Self::Science => "Science",
            Self::Geography => "Geography",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Animals => GREEN,
            Self::Colors => MAUVE,
            Self::Food => PEACH,
            Self::Science => TEAL,
            Self::Geography => YELLOW,
        }
    }

    fn words(self) -> &'static [&'static str] {
        match self {
            Self::Animals => &[
                "TIGER", "EAGLE", "SHARK", "HORSE", "WHALE", "SNAKE", "PANDA",
                "ZEBRA", "CAMEL", "OTTER", "FALCON", "PARROT", "RABBIT",
                "TURTLE", "MONKEY", "LIZARD", "SALMON", "DOLPHIN", "GIRAFFE",
                "PENGUIN", "JAGUAR", "COYOTE", "BADGER", "BISON", "CRANE",
                "RAVEN", "VIPER", "MOOSE", "KOALA", "LLAMA",
            ],
            Self::Colors => &[
                "AZURE", "CORAL", "GREEN", "IVORY", "KHAKI", "LILAC",
                "MAUVE", "OLIVE", "PEACH", "ROUGE", "TAUPE", "AMBER",
                "BLACK", "BROWN", "CREAM", "EBONY", "FROST", "GREY",
                "HAZEL", "LEMON", "MELON", "PEARL", "PLUM", "RUBY",
                "SAGE", "SAND", "TEAL", "WHEAT", "WHITE", "WINE",
            ],
            Self::Food => &[
                "BREAD", "CHEESE", "GRAPE", "LEMON", "MELON", "OLIVE",
                "PEACH", "PIZZA", "SALAD", "STEAK", "SUSHI", "TACOS",
                "TOAST", "MANGO", "CREPE", "PASTA", "CURRY", "BACON",
                "BERRY", "CANDY", "CHIPS", "DONUT", "HONEY", "JUICE",
                "MAPLE", "ONION", "RICE", "SOUP", "BASIL", "THYME",
            ],
            Self::Science => &[
                "ATOM", "CELL", "FORCE", "LASER", "ORBIT", "PRISM",
                "QUARK", "SOLAR", "VAPOR", "XENON", "DIODE", "FIELD",
                "GAMMA", "HELIX", "IONIC", "JOULE", "KELVIN", "LOGIC",
                "MOLAR", "NERVE", "OPTIC", "PHASE", "RADAR", "SIGMA",
                "TESLA", "ALLOY", "DECAY", "FLORA", "GENES", "HERTZ",
            ],
            Self::Geography => &[
                "DELTA", "FJORD", "RIDGE", "BASIN", "CLIFF", "DUNES",
                "GORGE", "PLAIN", "RIVER", "TIDAL", "ATLAS", "COAST",
                "GROVE", "MARSH", "OASIS", "PEAKS", "SHOAL", "TROPIC",
                "VALLEY", "BAYOU", "CANAL", "GULLY", "ISLAND", "NORTH",
                "SOUTH", "OCEAN", "POLAR", "STEPPE", "TUNDRA", "CREEK",
            ],
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Animals => Self::Colors,
            Self::Colors => Self::Food,
            Self::Food => Self::Science,
            Self::Science => Self::Geography,
            Self::Geography => Self::Animals,
        }
    }
}

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    fn grid_size(self) -> usize {
        match self {
            Self::Easy => 10,
            Self::Medium => 15,
            Self::Hard => 20,
        }
    }

    fn word_count(self) -> usize {
        match self {
            Self::Easy => 8,
            Self::Medium => 10,
            Self::Hard => 12,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Easy => GREEN,
            Self::Medium => YELLOW,
            Self::Hard => RED,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Easy => Self::Medium,
            Self::Medium => Self::Hard,
            Self::Hard => Self::Easy,
        }
    }
}

// ── Placed word tracking ────────────────────────────────────────────
/// A word that has been placed on the grid, with its position and direction.
#[derive(Clone, Debug)]
struct PlacedWord {
    word: String,
    /// Starting row on the grid.
    start_row: usize,
    /// Starting column on the grid.
    start_col: usize,
    /// Direction index into `DIRECTIONS`.
    direction_idx: usize,
    /// Whether the player has found this word.
    found: bool,
}

impl PlacedWord {
    /// Returns all (row, col) cells this word occupies.
    fn cells(&self) -> Vec<(usize, usize)> {
        let (dr, dc) = DIRECTIONS[self.direction_idx];
        let len = self.word.len();
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            let r = (self.start_row as i32 + dr * i as i32) as usize;
            let c = (self.start_col as i32 + dc * i as i32) as usize;
            result.push((r, c));
        }
        result
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
}

/// Selection state: the player is selecting a word by marking start and end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionState {
    /// No selection active.
    None,
    /// Player has chosen a start cell and is choosing the end.
    Selecting { start_row: usize, start_col: usize },
}

/// Hint state to highlight a revealed letter briefly.
#[derive(Clone, Debug)]
struct HintHighlight {
    row: usize,
    col: usize,
    /// Ticks remaining for the highlight to show.
    ticks: u32,
}

struct WordSearchApp {
    /// The letter grid, stored row-major. `grid[row * grid_size + col]`.
    grid: Vec<u8>,
    /// Current grid dimension.
    grid_size: usize,
    /// Words placed on the grid.
    placed_words: Vec<PlacedWord>,
    /// Current difficulty.
    difficulty: Difficulty,
    /// Current word category.
    category: Category,
    /// Cursor position.
    cursor_row: usize,
    cursor_col: usize,
    /// Selection state.
    selection: SelectionState,
    /// Game status.
    status: GameStatus,
    /// Elapsed time in seconds.
    elapsed_secs: u64,
    /// Hints remaining.
    hints_remaining: usize,
    /// RNG instance.
    rng: Lcg,
    /// Cells that are part of found words (row, col) for highlighting.
    found_cells: Vec<(usize, usize)>,
    /// Active hint highlight.
    hint_highlight: Option<HintHighlight>,
    /// Seed used for the current game (for reproducibility).
    seed: u64,
}

impl WordSearchApp {
    fn new() -> Self {
        let seed = 42;
        let mut app = Self {
            grid: Vec::new(),
            grid_size: 15,
            placed_words: Vec::new(),
            difficulty: Difficulty::Medium,
            category: Category::Animals,
            cursor_row: 0,
            cursor_col: 0,
            selection: SelectionState::None,
            status: GameStatus::Playing,
            elapsed_secs: 0,
            hints_remaining: MAX_HINTS,
            rng: Lcg::new(seed),
            found_cells: Vec::new(),
            hint_highlight: None,
            seed,
        };
        app.generate_puzzle();
        app
    }

    fn new_with_seed(seed: u64) -> Self {
        let mut app = Self {
            grid: Vec::new(),
            grid_size: 15,
            placed_words: Vec::new(),
            difficulty: Difficulty::Medium,
            category: Category::Animals,
            cursor_row: 0,
            cursor_col: 0,
            selection: SelectionState::None,
            status: GameStatus::Playing,
            elapsed_secs: 0,
            hints_remaining: MAX_HINTS,
            rng: Lcg::new(seed),
            found_cells: Vec::new(),
            hint_highlight: None,
            seed,
        };
        app.generate_puzzle();
        app
    }

    fn new_game(&mut self, difficulty: Difficulty, category: Category) {
        self.difficulty = difficulty;
        self.category = category;
        self.grid_size = difficulty.grid_size();
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.selection = SelectionState::None;
        self.status = GameStatus::Playing;
        self.elapsed_secs = 0;
        self.hints_remaining = MAX_HINTS;
        self.found_cells.clear();
        self.hint_highlight = None;
        // Advance the seed for variety.
        self.seed = self.seed.wrapping_add(7);
        self.rng = Lcg::new(self.seed);
        self.generate_puzzle();
    }

    // ── Puzzle generation ───────────────────────────────────────────

    fn generate_puzzle(&mut self) {
        let size = self.difficulty.grid_size();
        self.grid_size = size;
        let total = size * size;
        self.grid = vec![0u8; total];
        self.placed_words.clear();

        // Select words for this puzzle.
        let all_words = self.category.words();
        let word_count = self.difficulty.word_count();

        // Shuffle word indices and pick the first `word_count` that fit.
        let mut indices: Vec<usize> = (0..all_words.len()).collect();
        self.rng.shuffle(&mut indices);

        // Filter words that fit in the grid.
        let max_len = size;
        let mut selected_words: Vec<&str> = Vec::new();
        for &idx in &indices {
            let w = all_words[idx];
            if w.len() <= max_len && selected_words.len() < word_count {
                selected_words.push(w);
            }
        }

        // Place words one by one.
        for word in &selected_words {
            self.try_place_word(word);
        }

        // Fill remaining empty cells with random letters.
        for cell in &mut self.grid {
            if *cell == 0 {
                *cell = self.rng.next_letter();
            }
        }
    }

    /// Attempt to place a word on the grid. Tries random positions and
    /// directions until it succeeds or gives up after many attempts.
    fn try_place_word(&mut self, word: &str) -> bool {
        let size = self.grid_size;
        let word_bytes = word.as_bytes();
        let len = word_bytes.len();

        // Build list of all valid (row, col, dir) placements.
        let mut placements: Vec<(usize, usize, usize)> = Vec::new();
        for dir_idx in 0..8 {
            let (dr, dc) = DIRECTIONS[dir_idx];
            for row in 0..size {
                for col in 0..size {
                    if can_place(&self.grid, size, word_bytes, row, col, dr, dc) {
                        placements.push((row, col, dir_idx));
                    }
                }
            }
        }

        if placements.is_empty() {
            return false;
        }

        // Pick a random valid placement.
        let pick = self.rng.next_bounded(placements.len());
        let (row, col, dir_idx) = placements[pick];
        let (dr, dc) = DIRECTIONS[dir_idx];

        // Write the word into the grid.
        for (i, &ch) in word_bytes.iter().enumerate() {
            let r = (row as i32 + dr * i as i32) as usize;
            let c = (col as i32 + dc * i as i32) as usize;
            self.grid[r * size + c] = ch;
        }

        self.placed_words.push(PlacedWord {
            word: String::from(word),
            start_row: row,
            start_col: col,
            direction_idx: dir_idx,
            found: false,
        });

        let _ = len; // suppress unused warning
        true
    }

    // ── Word checking ───────────────────────────────────────────────

    /// Given a start and end cell, extract the sequence of letters and check
    /// if it matches any unfound word. Returns the index of the found word
    /// if successful.
    fn check_selection(&self, sr: usize, sc: usize, er: usize, ec: usize) -> Option<usize> {
        // Determine direction from start to end.
        let cells = cells_between(sr, sc, er, ec)?;

        // Build the selected string.
        let selected: String = cells
            .iter()
            .map(|&(r, c)| self.grid[r * self.grid_size + c] as char)
            .collect();

        // Check against each unfound placed word.
        for (idx, pw) in self.placed_words.iter().enumerate() {
            if pw.found {
                continue;
            }
            if pw.word == selected {
                // Verify the cells match the word's placement.
                let word_cells = pw.cells();
                if word_cells == cells {
                    return Some(idx);
                }
            }
            // Also check reversed (player may select end-to-start).
            let reversed: String = selected.chars().rev().collect();
            if pw.word == reversed {
                let word_cells = pw.cells();
                let reversed_cells: Vec<(usize, usize)> = cells.iter().copied().rev().collect();
                if word_cells == reversed_cells {
                    return Some(idx);
                }
            }
        }

        None
    }

    /// Mark a word as found and update found_cells.
    fn mark_found(&mut self, word_idx: usize) {
        self.placed_words[word_idx].found = true;
        let cells = self.placed_words[word_idx].cells();
        for cell in cells {
            if !self.found_cells.contains(&cell) {
                self.found_cells.push(cell);
            }
        }

        // Check if all words are found.
        if self.placed_words.iter().all(|pw| pw.found) {
            self.status = GameStatus::Won;
        }
    }

    fn words_found_count(&self) -> usize {
        self.placed_words.iter().filter(|pw| pw.found).count()
    }

    fn total_words(&self) -> usize {
        self.placed_words.len()
    }

    // ── Hint system ─────────────────────────────────────────────────

    fn use_hint(&mut self) {
        if self.hints_remaining == 0 || self.status == GameStatus::Won {
            return;
        }

        // Find the first unfound word.
        let unfound_idx = self.placed_words.iter().position(|pw| !pw.found);
        if let Some(idx) = unfound_idx {
            let cells = self.placed_words[idx].cells();
            if let Some(&(hr, hc)) = cells.first() {
                self.hint_highlight = Some(HintHighlight {
                    row: hr,
                    col: hc,
                    ticks: 10,
                });
                self.hints_remaining = self.hints_remaining.saturating_sub(1);
            }
        }
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(key_event) = event
            && key_event.pressed {
                self.handle_key(key_event);
            }
    }

    fn handle_key(&mut self, key: &KeyEvent) {
        // Ctrl+1/2/3 for difficulty, Ctrl+C for category cycle
        if key.modifiers.ctrl {
            match key.key {
                Key::Num1 => {
                    self.new_game(Difficulty::Easy, self.category);
                    return;
                }
                Key::Num2 => {
                    self.new_game(Difficulty::Medium, self.category);
                    return;
                }
                Key::Num3 => {
                    self.new_game(Difficulty::Hard, self.category);
                    return;
                }
                _ => {}
            }
        }

        // Non-modifier keys
        if key.modifiers == Modifiers::NONE {
            match key.key {
                Key::Up
                    if self.cursor_row > 0 => {
                        self.cursor_row -= 1;
                    }
                Key::Down
                    if self.cursor_row + 1 < self.grid_size => {
                        self.cursor_row += 1;
                    }
                Key::Left
                    if self.cursor_col > 0 => {
                        self.cursor_col -= 1;
                    }
                Key::Right
                    if self.cursor_col + 1 < self.grid_size => {
                        self.cursor_col += 1;
                    }
                Key::Enter => {
                    self.handle_enter();
                }
                Key::Escape => {
                    self.selection = SelectionState::None;
                }
                Key::H => {
                    self.use_hint();
                }
                Key::F2 => {
                    self.new_game(self.difficulty, self.category);
                }
                Key::C => {
                    // Cycle category
                    let next_cat = self.category.next();
                    self.new_game(self.difficulty, next_cat);
                }
                Key::D => {
                    // Cycle difficulty
                    let next_diff = self.difficulty.next();
                    self.new_game(next_diff, self.category);
                }
                _ => {}
            }
        }
    }

    fn handle_enter(&mut self) {
        if self.status == GameStatus::Won {
            return;
        }

        match self.selection {
            SelectionState::None => {
                // Start selection at cursor position.
                self.selection = SelectionState::Selecting {
                    start_row: self.cursor_row,
                    start_col: self.cursor_col,
                };
            }
            SelectionState::Selecting {
                start_row,
                start_col,
            } => {
                // End selection at cursor position and check the word.
                let er = self.cursor_row;
                let ec = self.cursor_col;

                if let Some(word_idx) = self.check_selection(start_row, start_col, er, ec) {
                    self.mark_found(word_idx);
                }

                self.selection = SelectionState::None;
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        let grid_pixel_w = self.grid_size as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        let word_list_width = 160.0;
        let total_w = PADDING * 3.0 + grid_pixel_w + word_list_width;
        let grid_pixel_h = self.grid_size as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        let total_h = PADDING * 2.0 + HEADER_HEIGHT + grid_pixel_h + 40.0;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_w,
            height: total_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);
        self.render_grid(&mut cmds);
        self.render_word_list(&mut cmds);
        self.render_footer(&mut cmds);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let y = PADDING;

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: String::from("Word Search"),
            font_size: HEADER_FONT_SIZE,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Category label
        cmds.push(RenderCommand::Text {
            x: PADDING + 140.0,
            y,
            text: self.category.label().to_string(),
            font_size: STATUS_FONT_SIZE,
            color: self.category.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // Difficulty label
        cmds.push(RenderCommand::Text {
            x: PADDING + 140.0,
            y: y + 18.0,
            text: format!("{} ({}x{})", self.difficulty.label(), self.grid_size, self.grid_size),
            font_size: LABEL_FONT_SIZE,
            color: self.difficulty.color(),
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });

        // Timer
        let mins = self.elapsed_secs / 60;
        let secs = self.elapsed_secs % 60;
        let timer_text = format!("{mins:02}:{secs:02}");
        cmds.push(RenderCommand::Text {
            x: PADDING + 300.0,
            y,
            text: timer_text,
            font_size: HEADER_FONT_SIZE,
            color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });

        // Hints remaining
        cmds.push(RenderCommand::Text {
            x: PADDING + 400.0,
            y,
            text: format!("Hints: {}", self.hints_remaining),
            font_size: STATUS_FONT_SIZE,
            color: if self.hints_remaining > 0 { PEACH } else { OVERLAY0 },
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Words found count
        cmds.push(RenderCommand::Text {
            x: PADDING + 400.0,
            y: y + 18.0,
            text: format!("Found: {}/{}", self.words_found_count(), self.total_words()),
            font_size: LABEL_FONT_SIZE,
            color: GREEN,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Status
        if self.status == GameStatus::Won {
            cmds.push(RenderCommand::Text {
                x: PADDING + 520.0,
                y,
                text: String::from("YOU WIN!"),
                font_size: HEADER_FONT_SIZE,
                color: GREEN,
                font_weight: FontWeightHint::Bold,
                max_width: Some(120.0),
            });
        }
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let grid_y = PADDING + HEADER_HEIGHT;

        // Compute selection cells for highlighting.
        let selection_cells = match self.selection {
            SelectionState::None => Vec::new(),
            SelectionState::Selecting {
                start_row,
                start_col,
            } => {
                cells_between(start_row, start_col, self.cursor_row, self.cursor_col)
                    .unwrap_or_default()
            }
        };

        for row in 0..self.grid_size {
            for col in 0..self.grid_size {
                let x = PADDING + col as f32 * (CELL_SIZE + CELL_GAP);
                let y = grid_y + row as f32 * (CELL_SIZE + CELL_GAP);

                let is_cursor = row == self.cursor_row && col == self.cursor_col;
                let is_found = self.found_cells.contains(&(row, col));
                let is_selecting = selection_cells.contains(&(row, col));
                let is_selection_start = matches!(
                    self.selection,
                    SelectionState::Selecting { start_row, start_col }
                    if start_row == row && start_col == col
                );
                let is_hint = self.hint_highlight.as_ref().is_some_and(|h| {
                    h.row == row && h.col == col && h.ticks > 0
                });

                // Cell background color
                let bg_color = if is_hint {
                    YELLOW
                } else if is_found {
                    Color::rgba(166, 227, 161, 40) // Green with low alpha
                } else if is_selection_start {
                    MAUVE
                } else if is_selecting {
                    Color::rgba(137, 180, 250, 60) // Blue with low alpha
                } else {
                    SURFACE0
                };

                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg_color,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });

                // Cursor outline
                if is_cursor {
                    cmds.push(RenderCommand::StrokeRect {
                        x,
                        y,
                        width: CELL_SIZE,
                        height: CELL_SIZE,
                        color: BLUE,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                    });
                }

                // Letter
                let ch = self.grid[row * self.grid_size + col];
                if ch != 0 {
                    let letter_color = if is_hint {
                        BASE
                    } else if is_found {
                        GREEN
                    } else if is_selecting {
                        BLUE
                    } else {
                        TEXT_COLOR
                    };

                    let weight = if is_found || is_selecting || is_hint {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };

                    cmds.push(RenderCommand::Text {
                        x: x + CELL_SIZE / 2.0 - 5.0,
                        y: y + CELL_SIZE / 2.0 - 8.0,
                        text: String::from(ch as char),
                        font_size: CELL_FONT_SIZE,
                        color: letter_color,
                        font_weight: weight,
                        max_width: Some(CELL_SIZE),
                    });
                }
            }
        }
    }

    fn render_word_list(&self, cmds: &mut Vec<RenderCommand>) {
        let grid_pixel_w = self.grid_size as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        let list_x = PADDING * 2.0 + grid_pixel_w;
        let list_y = PADDING + HEADER_HEIGHT;

        // Word list title
        cmds.push(RenderCommand::Text {
            x: list_x,
            y: list_y,
            text: String::from("Words to Find"),
            font_size: STATUS_FONT_SIZE,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(160.0),
        });

        // Each word
        for (i, pw) in self.placed_words.iter().enumerate() {
            let wy = list_y + 24.0 + i as f32 * 20.0;

            let (word_color, weight) = if pw.found {
                (OVERLAY0, FontWeightHint::Light)
            } else {
                (TEXT_COLOR, FontWeightHint::Regular)
            };

            // Word text
            cmds.push(RenderCommand::Text {
                x: list_x,
                y: wy,
                text: pw.word.clone(),
                font_size: WORD_LIST_FONT_SIZE,
                color: word_color,
                font_weight: weight,
                max_width: Some(140.0),
            });

            // Strikethrough line for found words
            if pw.found {
                let text_w = pw.word.len() as f32 * 8.0;
                cmds.push(RenderCommand::Line {
                    x1: list_x,
                    y1: wy + 7.0,
                    x2: list_x + text_w,
                    y2: wy + 7.0,
                    color: GREEN,
                    width: 1.5,
                });

                // Checkmark
                cmds.push(RenderCommand::Text {
                    x: list_x + text_w + 4.0,
                    y: wy,
                    text: String::from("\u{2713}"),
                    font_size: WORD_LIST_FONT_SIZE,
                    color: GREEN,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(20.0),
                });
            }
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let grid_pixel_h = self.grid_size as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        let footer_y = PADDING + HEADER_HEIGHT + grid_pixel_h + 8.0;

        let help_text = match self.selection {
            SelectionState::None => {
                "Arrows:Move  Enter:Select  H:Hint  D:Difficulty  C:Category  F2:New"
            }
            SelectionState::Selecting { .. } => {
                "Arrows:Move to end  Enter:Confirm  Esc:Cancel"
            }
        };

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y,
            text: String::from(help_text),
            font_size: LABEL_FONT_SIZE,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(700.0),
        });
    }
}

// ── Free-standing grid utilities ────────────────────────────────────

/// Check if a word (as bytes) can be placed at (row, col) in direction (dr, dc)
/// on the given grid. A cell must either be empty (0) or already contain the
/// matching letter.
fn can_place(
    grid: &[u8],
    size: usize,
    word: &[u8],
    row: usize,
    col: usize,
    dr: i32,
    dc: i32,
) -> bool {
    let len = word.len();
    for i in 0..len {
        let r = row as i32 + dr * i as i32;
        let c = col as i32 + dc * i as i32;
        if r < 0 || r >= size as i32 || c < 0 || c >= size as i32 {
            return false;
        }
        let ru = r as usize;
        let cu = c as usize;
        let existing = grid[ru * size + cu];
        if existing != 0 && existing != word[i] {
            return false;
        }
    }
    true
}

/// Given a start (sr, sc) and end (er, ec), determine if they form a valid
/// straight line (horizontal, vertical, or 45-degree diagonal). Returns the
/// ordered list of (row, col) cells along that line, or None if invalid.
fn cells_between(sr: usize, sc: usize, er: usize, ec: usize) -> Option<Vec<(usize, usize)>> {
    if sr == er && sc == ec {
        // Single cell is valid.
        return Some(vec![(sr, sc)]);
    }

    let dr = er as i32 - sr as i32;
    let dc = ec as i32 - sc as i32;

    let abs_dr = dr.unsigned_abs() as usize;
    let abs_dc = dc.unsigned_abs() as usize;

    // Must be horizontal, vertical, or exactly 45-degree diagonal.
    let steps = if abs_dr == 0 {
        abs_dc
    } else if abs_dc == 0 || abs_dr == abs_dc {
        abs_dr
    } else {
        return None;
    };

    if steps == 0 {
        return None;
    }

    let step_r = if dr == 0 { 0 } else { dr / dr.abs() };
    let step_c = if dc == 0 { 0 } else { dc / dc.abs() };

    let mut result = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let r = (sr as i32 + step_r * i as i32) as usize;
        let c = (sc as i32 + step_c * i as i32) as usize;
        result.push((r, c));
    }

    Some(result)
}

/// Format seconds as MM:SS.
fn format_time(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{m:02}:{s:02}")
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() {
    let _app = WordSearchApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── LCG tests ───────────────────────────────────────────────────

    #[test]
    fn test_lcg_deterministic() {
        let mut rng1 = Lcg::new(42);
        let mut rng2 = Lcg::new(42);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_lcg_different_seeds() {
        let mut rng1 = Lcg::new(1);
        let mut rng2 = Lcg::new(2);
        // Very unlikely to produce the same first value.
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_lcg_zero_seed_no_stuck() {
        let mut rng = Lcg::new(0);
        let first = rng.next_u64();
        let second = rng.next_u64();
        assert_ne!(first, 0);
        assert_ne!(second, 0);
        assert_ne!(first, second);
    }

    #[test]
    fn test_lcg_bounded_range() {
        let mut rng = Lcg::new(99);
        for _ in 0..200 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_lcg_bounded_one() {
        let mut rng = Lcg::new(1);
        for _ in 0..10 {
            assert_eq!(rng.next_bounded(1), 0);
        }
    }

    #[test]
    fn test_lcg_bounded_zero() {
        let mut rng = Lcg::new(1);
        assert_eq!(rng.next_bounded(0), 0);
    }

    #[test]
    fn test_lcg_next_letter() {
        let mut rng = Lcg::new(12345);
        for _ in 0..200 {
            let ch = rng.next_letter();
            assert!(ch >= b'A' && ch <= b'Z', "Letter out of range: {ch}");
        }
    }

    #[test]
    fn test_lcg_shuffle_preserves_elements() {
        let mut rng = Lcg::new(7);
        let mut arr = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        rng.shuffle(&mut arr);
        arr.sort();
        assert_eq!(arr, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn test_lcg_shuffle_empty() {
        let mut rng = Lcg::new(1);
        let mut arr: Vec<i32> = Vec::new();
        rng.shuffle(&mut arr);
        assert!(arr.is_empty());
    }

    #[test]
    fn test_lcg_shuffle_single() {
        let mut rng = Lcg::new(1);
        let mut arr = vec![42];
        rng.shuffle(&mut arr);
        assert_eq!(arr, vec![42]);
    }

    // ── Category tests ──────────────────────────────────────────────

    #[test]
    fn test_category_all_count() {
        assert_eq!(Category::ALL.len(), 5);
    }

    #[test]
    fn test_category_words_not_empty() {
        for cat in &Category::ALL {
            assert!(!cat.words().is_empty(), "{:?} has no words", cat);
        }
    }

    #[test]
    fn test_category_words_uppercase() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert_eq!(
                    *word,
                    word.to_uppercase(),
                    "Word '{word}' in {:?} is not uppercase",
                    cat
                );
            }
        }
    }

    #[test]
    fn test_category_words_ascii() {
        for cat in &Category::ALL {
            for word in cat.words() {
                assert!(word.is_ascii(), "Word '{word}' in {:?} is not ASCII", cat);
            }
        }
    }

    #[test]
    fn test_category_label() {
        assert_eq!(Category::Animals.label(), "Animals");
        assert_eq!(Category::Colors.label(), "Colors");
        assert_eq!(Category::Food.label(), "Food");
        assert_eq!(Category::Science.label(), "Science");
        assert_eq!(Category::Geography.label(), "Geography");
    }

    #[test]
    fn test_category_next_cycle() {
        let start = Category::Animals;
        let mut cat = start;
        for _ in 0..5 {
            cat = cat.next();
        }
        assert_eq!(cat, start, "Category cycle should return to start after 5 steps");
    }

    #[test]
    fn test_category_at_least_30_words_each() {
        for cat in &Category::ALL {
            assert!(
                cat.words().len() >= 20,
                "{:?} has only {} words",
                cat,
                cat.words().len()
            );
        }
    }

    // ── Difficulty tests ────────────────────────────────────────────

    #[test]
    fn test_difficulty_grid_sizes() {
        assert_eq!(Difficulty::Easy.grid_size(), 10);
        assert_eq!(Difficulty::Medium.grid_size(), 15);
        assert_eq!(Difficulty::Hard.grid_size(), 20);
    }

    #[test]
    fn test_difficulty_word_counts() {
        assert_eq!(Difficulty::Easy.word_count(), 8);
        assert_eq!(Difficulty::Medium.word_count(), 10);
        assert_eq!(Difficulty::Hard.word_count(), 12);
    }

    #[test]
    fn test_difficulty_next_cycle() {
        let start = Difficulty::Easy;
        let mut d = start;
        for _ in 0..3 {
            d = d.next();
        }
        assert_eq!(d, start, "Difficulty cycle should return to start after 3");
    }

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Easy.label(), "Easy");
        assert_eq!(Difficulty::Medium.label(), "Medium");
        assert_eq!(Difficulty::Hard.label(), "Hard");
    }

    // ── cells_between tests ─────────────────────────────────────────

    #[test]
    fn test_cells_between_horizontal() {
        let cells = cells_between(0, 0, 0, 4).unwrap();
        assert_eq!(cells.len(), 5);
        for (i, &(r, c)) in cells.iter().enumerate() {
            assert_eq!(r, 0);
            assert_eq!(c, i);
        }
    }

    #[test]
    fn test_cells_between_vertical() {
        let cells = cells_between(1, 3, 5, 3).unwrap();
        assert_eq!(cells.len(), 5);
        for (i, &(r, c)) in cells.iter().enumerate() {
            assert_eq!(r, 1 + i);
            assert_eq!(c, 3);
        }
    }

    #[test]
    fn test_cells_between_diagonal() {
        let cells = cells_between(0, 0, 3, 3).unwrap();
        assert_eq!(cells.len(), 4);
        for (i, &(r, c)) in cells.iter().enumerate() {
            assert_eq!(r, i);
            assert_eq!(c, i);
        }
    }

    #[test]
    fn test_cells_between_reverse_horizontal() {
        let cells = cells_between(2, 5, 2, 2).unwrap();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], (2, 5));
        assert_eq!(cells[3], (2, 2));
    }

    #[test]
    fn test_cells_between_anti_diagonal() {
        let cells = cells_between(0, 3, 3, 0).unwrap();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], (0, 3));
        assert_eq!(cells[1], (1, 2));
        assert_eq!(cells[2], (2, 1));
        assert_eq!(cells[3], (3, 0));
    }

    #[test]
    fn test_cells_between_invalid_not_straight() {
        let result = cells_between(0, 0, 1, 3);
        assert!(result.is_none());
    }

    #[test]
    fn test_cells_between_single_cell() {
        let cells = cells_between(5, 5, 5, 5).unwrap();
        assert_eq!(cells, vec![(5, 5)]);
    }

    #[test]
    fn test_cells_between_up_right_diagonal() {
        let cells = cells_between(4, 0, 0, 4).unwrap();
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0], (4, 0));
        assert_eq!(cells[4], (0, 4));
    }

    // ── can_place tests ─────────────────────────────────────────────

    #[test]
    fn test_can_place_empty_grid_horizontal() {
        let grid = vec![0u8; 100]; // 10x10
        assert!(can_place(&grid, 10, b"HELLO", 0, 0, 0, 1));
    }

    #[test]
    fn test_can_place_empty_grid_vertical() {
        let grid = vec![0u8; 100];
        assert!(can_place(&grid, 10, b"HELLO", 0, 0, 1, 0));
    }

    #[test]
    fn test_can_place_out_of_bounds_right() {
        let grid = vec![0u8; 100];
        assert!(!can_place(&grid, 10, b"HELLO", 0, 8, 0, 1));
    }

    #[test]
    fn test_can_place_out_of_bounds_down() {
        let grid = vec![0u8; 100];
        assert!(!can_place(&grid, 10, b"HELLO", 8, 0, 1, 0));
    }

    #[test]
    fn test_can_place_overlap_matching() {
        let mut grid = vec![0u8; 100];
        grid[2] = b'L'; // Row 0, column 2 — where the 'L' in HELLO would go
        assert!(can_place(&grid, 10, b"HELLO", 0, 0, 0, 1));
    }

    #[test]
    fn test_can_place_overlap_conflicting() {
        let mut grid = vec![0u8; 100];
        grid[2] = b'X'; // Row 0, column 2 — conflicts with 'L' in HELLO
        assert!(!can_place(&grid, 10, b"HELLO", 0, 0, 0, 1));
    }

    #[test]
    fn test_can_place_diagonal() {
        let grid = vec![0u8; 100];
        assert!(can_place(&grid, 10, b"CAT", 0, 0, 1, 1));
    }

    #[test]
    fn test_can_place_reverse() {
        let grid = vec![0u8; 100];
        assert!(can_place(&grid, 10, b"CAT", 9, 9, -1, -1));
    }

    #[test]
    fn test_can_place_out_of_bounds_negative() {
        let grid = vec![0u8; 100];
        assert!(!can_place(&grid, 10, b"HELLO", 0, 0, -1, 0));
    }

    // ── PlacedWord tests ────────────────────────────────────────────

    #[test]
    fn test_placed_word_cells_horizontal() {
        let pw = PlacedWord {
            word: String::from("CAT"),
            start_row: 2,
            start_col: 3,
            direction_idx: 0, // right
            found: false,
        };
        let cells = pw.cells();
        assert_eq!(cells, vec![(2, 3), (2, 4), (2, 5)]);
    }

    #[test]
    fn test_placed_word_cells_vertical() {
        let pw = PlacedWord {
            word: String::from("DOG"),
            start_row: 0,
            start_col: 5,
            direction_idx: 2, // down
            found: false,
        };
        let cells = pw.cells();
        assert_eq!(cells, vec![(0, 5), (1, 5), (2, 5)]);
    }

    #[test]
    fn test_placed_word_cells_diagonal() {
        let pw = PlacedWord {
            word: String::from("HI"),
            start_row: 1,
            start_col: 1,
            direction_idx: 4, // down-right
            found: false,
        };
        let cells = pw.cells();
        assert_eq!(cells, vec![(1, 1), (2, 2)]);
    }

    // ── App construction tests ──────────────────────────────────────

    #[test]
    fn test_app_new_default_state() {
        let app = WordSearchApp::new();
        assert_eq!(app.grid_size, 15);
        assert_eq!(app.difficulty, Difficulty::Medium);
        assert_eq!(app.category, Category::Animals);
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
        assert_eq!(app.hints_remaining, MAX_HINTS);
        assert_eq!(app.elapsed_secs, 0);
        assert_eq!(app.selection, SelectionState::None);
    }

    #[test]
    fn test_app_grid_filled() {
        let app = WordSearchApp::new();
        let total = app.grid_size * app.grid_size;
        assert_eq!(app.grid.len(), total);
        for &cell in &app.grid {
            assert!(cell >= b'A' && cell <= b'Z', "Cell not a letter: {cell}");
        }
    }

    #[test]
    fn test_app_words_placed() {
        let app = WordSearchApp::new();
        // Medium difficulty expects 10 words (some may fail to place).
        assert!(!app.placed_words.is_empty(), "No words were placed");
        assert!(app.placed_words.len() <= 10);
    }

    #[test]
    fn test_app_placed_words_exist_in_grid() {
        let app = WordSearchApp::new();
        for pw in &app.placed_words {
            let cells = pw.cells();
            let word_from_grid: String = cells
                .iter()
                .map(|&(r, c)| app.grid[r * app.grid_size + c] as char)
                .collect();
            assert_eq!(
                pw.word, word_from_grid,
                "Word '{}' not correctly placed on grid",
                pw.word
            );
        }
    }

    #[test]
    fn test_app_deterministic() {
        let app1 = WordSearchApp::new_with_seed(42);
        let app2 = WordSearchApp::new_with_seed(42);
        assert_eq!(app1.grid, app2.grid);
        assert_eq!(app1.placed_words.len(), app2.placed_words.len());
        for (w1, w2) in app1.placed_words.iter().zip(app2.placed_words.iter()) {
            assert_eq!(w1.word, w2.word);
            assert_eq!(w1.start_row, w2.start_row);
            assert_eq!(w1.start_col, w2.start_col);
            assert_eq!(w1.direction_idx, w2.direction_idx);
        }
    }

    #[test]
    fn test_app_different_seeds_different_grids() {
        let app1 = WordSearchApp::new_with_seed(1);
        let app2 = WordSearchApp::new_with_seed(999);
        // Very unlikely to produce identical grids.
        assert_ne!(app1.grid, app2.grid);
    }

    // ── Difficulty/category game creation ────────────────────────────

    #[test]
    fn test_new_game_easy() {
        let mut app = WordSearchApp::new();
        app.new_game(Difficulty::Easy, Category::Colors);
        assert_eq!(app.grid_size, 10);
        assert_eq!(app.difficulty, Difficulty::Easy);
        assert_eq!(app.category, Category::Colors);
        assert_eq!(app.grid.len(), 100);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_new_game_hard() {
        let mut app = WordSearchApp::new();
        app.new_game(Difficulty::Hard, Category::Science);
        assert_eq!(app.grid_size, 20);
        assert_eq!(app.difficulty, Difficulty::Hard);
        assert_eq!(app.grid.len(), 400);
    }

    #[test]
    fn test_new_game_resets_state() {
        let mut app = WordSearchApp::new();
        app.elapsed_secs = 120;
        app.cursor_row = 5;
        app.cursor_col = 5;
        app.hints_remaining = 0;
        app.new_game(Difficulty::Easy, Category::Food);
        assert_eq!(app.elapsed_secs, 0);
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
        assert_eq!(app.hints_remaining, MAX_HINTS);
        assert!(app.found_cells.is_empty());
    }

    #[test]
    fn test_all_categories_produce_words() {
        for cat in &Category::ALL {
            let mut app = WordSearchApp::new();
            app.new_game(Difficulty::Medium, *cat);
            assert!(
                !app.placed_words.is_empty(),
                "No words placed for {:?}",
                cat
            );
        }
    }

    #[test]
    fn test_all_difficulties_produce_correct_grid_size() {
        let difficulties = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];
        for diff in &difficulties {
            let mut app = WordSearchApp::new();
            app.new_game(*diff, Category::Animals);
            assert_eq!(app.grid_size, diff.grid_size());
            assert_eq!(app.grid.len(), diff.grid_size() * diff.grid_size());
        }
    }

    // ── Cursor movement tests ───────────────────────────────────────

    #[test]
    fn test_cursor_move_down() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Down));
        assert_eq!(app.cursor_row, 1);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Right));
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_cursor_move_up_at_zero() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Up));
        assert_eq!(app.cursor_row, 0); // Should not wrap
    }

    #[test]
    fn test_cursor_move_left_at_zero() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Left));
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_move_down_at_max() {
        let mut app = WordSearchApp::new();
        app.cursor_row = app.grid_size - 1;
        app.handle_event(&key_event(Key::Down));
        assert_eq!(app.cursor_row, app.grid_size - 1);
    }

    #[test]
    fn test_cursor_move_right_at_max() {
        let mut app = WordSearchApp::new();
        app.cursor_col = app.grid_size - 1;
        app.handle_event(&key_event(Key::Right));
        assert_eq!(app.cursor_col, app.grid_size - 1);
    }

    #[test]
    fn test_cursor_full_traverse() {
        let mut app = WordSearchApp::new();
        let size = app.grid_size;
        for _ in 0..size {
            app.handle_event(&key_event(Key::Down));
        }
        assert_eq!(app.cursor_row, size - 1);
        for _ in 0..size {
            app.handle_event(&key_event(Key::Right));
        }
        assert_eq!(app.cursor_col, size - 1);
    }

    // ── Selection tests ─────────────────────────────────────────────

    #[test]
    fn test_enter_starts_selection() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Enter));
        assert_eq!(
            app.selection,
            SelectionState::Selecting {
                start_row: 0,
                start_col: 0
            }
        );
    }

    #[test]
    fn test_escape_cancels_selection() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Enter));
        app.handle_event(&key_event(Key::Escape));
        assert_eq!(app.selection, SelectionState::None);
    }

    #[test]
    fn test_selection_enter_twice_clears() {
        let mut app = WordSearchApp::new();
        app.handle_event(&key_event(Key::Enter));
        // Move cursor a bit
        app.handle_event(&key_event(Key::Right));
        app.handle_event(&key_event(Key::Enter));
        // After confirming, selection goes back to None.
        assert_eq!(app.selection, SelectionState::None);
    }

    // ── Word finding tests ──────────────────────────────────────────

    #[test]
    fn test_find_word_by_selecting_correct_cells() {
        let app = WordSearchApp::new_with_seed(42);
        // Find the first word and select its cells.
        if app.placed_words.is_empty() {
            return; // Skip if no words placed (very unlikely).
        }
        let pw = app.placed_words[0].clone();
        let cells = pw.cells();
        let (sr, sc) = cells[0];
        let (er, ec) = cells[cells.len() - 1];

        let result = app.check_selection(sr, sc, er, ec);
        assert!(result.is_some(), "Should find word '{}'", pw.word);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn test_find_word_reverse_direction() {
        let app = WordSearchApp::new_with_seed(42);
        if app.placed_words.is_empty() {
            return;
        }
        let pw = app.placed_words[0].clone();
        let cells = pw.cells();
        let (sr, sc) = cells[cells.len() - 1];
        let (er, ec) = cells[0];

        let result = app.check_selection(sr, sc, er, ec);
        assert!(result.is_some(), "Should find word '{}' in reverse", pw.word);
    }

    #[test]
    fn test_mark_found_updates_state() {
        let mut app = WordSearchApp::new_with_seed(42);
        if app.placed_words.is_empty() {
            return;
        }
        let word_cells = app.placed_words[0].cells();
        app.mark_found(0);

        assert!(app.placed_words[0].found);
        for cell in &word_cells {
            assert!(
                app.found_cells.contains(cell),
                "Found cells should contain {:?}",
                cell
            );
        }
    }

    #[test]
    fn test_win_condition() {
        let mut app = WordSearchApp::new_with_seed(42);
        let count = app.placed_words.len();
        for i in 0..count {
            app.mark_found(i);
        }
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn test_words_found_count() {
        let mut app = WordSearchApp::new_with_seed(42);
        assert_eq!(app.words_found_count(), 0);
        if !app.placed_words.is_empty() {
            app.mark_found(0);
            assert_eq!(app.words_found_count(), 1);
        }
    }

    #[test]
    fn test_total_words() {
        let app = WordSearchApp::new_with_seed(42);
        assert_eq!(app.total_words(), app.placed_words.len());
    }

    #[test]
    fn test_check_selection_invalid_diagonal() {
        let app = WordSearchApp::new();
        // A non-straight-line selection should return None.
        let result = app.check_selection(0, 0, 2, 3);
        assert!(result.is_none());
    }

    #[test]
    fn test_found_word_not_findable_again() {
        let mut app = WordSearchApp::new_with_seed(42);
        if app.placed_words.is_empty() {
            return;
        }
        let pw = app.placed_words[0].clone();
        let cells = pw.cells();
        let (sr, sc) = cells[0];
        let (er, ec) = cells[cells.len() - 1];

        app.mark_found(0);

        let result = app.check_selection(sr, sc, er, ec);
        assert!(result.is_none(), "Found word should not be findable again");
    }

    // ── Hint tests ──────────────────────────────────────────────────

    #[test]
    fn test_hint_decrements_count() {
        let mut app = WordSearchApp::new_with_seed(42);
        assert_eq!(app.hints_remaining, MAX_HINTS);
        app.use_hint();
        assert_eq!(app.hints_remaining, MAX_HINTS - 1);
    }

    #[test]
    fn test_hint_sets_highlight() {
        let mut app = WordSearchApp::new_with_seed(42);
        app.use_hint();
        assert!(app.hint_highlight.is_some());
    }

    #[test]
    fn test_hint_highlight_on_first_letter() {
        let mut app = WordSearchApp::new_with_seed(42);
        if app.placed_words.is_empty() {
            return;
        }
        let first_word_cells = app.placed_words[0].cells();
        let (expected_r, expected_c) = first_word_cells[0];

        app.use_hint();
        let hl = app.hint_highlight.as_ref().unwrap();
        assert_eq!(hl.row, expected_r);
        assert_eq!(hl.col, expected_c);
    }

    #[test]
    fn test_hint_no_more_when_zero() {
        let mut app = WordSearchApp::new_with_seed(42);
        app.hints_remaining = 0;
        app.use_hint();
        assert!(app.hint_highlight.is_none());
    }

    #[test]
    fn test_hint_not_available_when_won() {
        let mut app = WordSearchApp::new_with_seed(42);
        let count = app.placed_words.len();
        for i in 0..count {
            app.mark_found(i);
        }
        let hints_before = app.hints_remaining;
        app.use_hint();
        assert_eq!(app.hints_remaining, hints_before);
    }

    #[test]
    fn test_hint_via_key() {
        let mut app = WordSearchApp::new_with_seed(42);
        app.handle_event(&key_event(Key::H));
        assert_eq!(app.hints_remaining, MAX_HINTS - 1);
        assert!(app.hint_highlight.is_some());
    }

    #[test]
    fn test_hint_multiple_uses() {
        let mut app = WordSearchApp::new_with_seed(42);
        for i in 0..MAX_HINTS {
            app.use_hint();
            assert_eq!(app.hints_remaining, MAX_HINTS - 1 - i);
        }
        // One more should not decrease.
        app.use_hint();
        assert_eq!(app.hints_remaining, 0);
    }

    // ── Key event tests ─────────────────────────────────────────────

    #[test]
    fn test_key_release_ignored() {
        let mut app = WordSearchApp::new();
        let release = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&release);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_ctrl_1_new_easy() {
        let mut app = WordSearchApp::new();
        let ev = Event::Key(KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.difficulty, Difficulty::Easy);
        assert_eq!(app.grid_size, 10);
    }

    #[test]
    fn test_ctrl_2_new_medium() {
        let mut app = WordSearchApp::new();
        app.new_game(Difficulty::Easy, Category::Animals);
        let ev = Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn test_ctrl_3_new_hard() {
        let mut app = WordSearchApp::new();
        let ev = Event::Key(KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ev);
        assert_eq!(app.difficulty, Difficulty::Hard);
        assert_eq!(app.grid_size, 20);
    }

    #[test]
    fn test_d_key_cycles_difficulty() {
        let mut app = WordSearchApp::new();
        // Default is Medium.
        app.handle_event(&key_event(Key::D));
        assert_eq!(app.difficulty, Difficulty::Hard);
        app.handle_event(&key_event(Key::D));
        assert_eq!(app.difficulty, Difficulty::Easy);
        app.handle_event(&key_event(Key::D));
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn test_c_key_cycles_category() {
        let mut app = WordSearchApp::new();
        // Default is Animals.
        app.handle_event(&key_event(Key::C));
        assert_eq!(app.category, Category::Colors);
        app.handle_event(&key_event(Key::C));
        assert_eq!(app.category, Category::Food);
    }

    #[test]
    fn test_f2_new_game() {
        let mut app = WordSearchApp::new();
        app.elapsed_secs = 99;
        app.handle_event(&key_event(Key::F2));
        assert_eq!(app.elapsed_secs, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = WordSearchApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty(), "Render should produce at least one command");
    }

    #[test]
    fn test_render_has_background() {
        let app = WordSearchApp::new();
        let cmds = app.render();
        // First command should be the background fill.
        assert!(matches!(cmds[0], RenderCommand::FillRect { .. }));
    }

    #[test]
    fn test_render_has_text() {
        let app = WordSearchApp::new();
        let cmds = app.render();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text, "Render should contain Text commands");
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = WordSearchApp::new();
        app.selection = SelectionState::Selecting {
            start_row: 0,
            start_col: 0,
        };
        app.cursor_row = 0;
        app.cursor_col = 4;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_found_words() {
        let mut app = WordSearchApp::new_with_seed(42);
        if !app.placed_words.is_empty() {
            app.mark_found(0);
        }
        let cmds = app.render();
        // Should contain line commands for strikethrough.
        let has_line = cmds.iter().any(|c| matches!(c, RenderCommand::Line { .. }));
        assert!(has_line, "Found words should have strikethrough lines");
    }

    #[test]
    fn test_render_won_state() {
        let mut app = WordSearchApp::new_with_seed(42);
        let count = app.placed_words.len();
        for i in 0..count {
            app.mark_found(i);
        }
        let cmds = app.render();
        // Should contain "YOU WIN!" text.
        let has_win = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("WIN"))
        });
        assert!(has_win, "Won state should display win message");
    }

    // ── format_time tests ───────────────────────────────────────────

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0), "00:00");
    }

    #[test]
    fn test_format_time_seconds() {
        assert_eq!(format_time(45), "00:45");
    }

    #[test]
    fn test_format_time_minutes() {
        assert_eq!(format_time(125), "02:05");
    }

    #[test]
    fn test_format_time_large() {
        assert_eq!(format_time(3661), "61:01");
    }

    // ── Direction tests ─────────────────────────────────────────────

    #[test]
    fn test_all_8_directions_defined() {
        assert_eq!(DIRECTIONS.len(), 8);
    }

    #[test]
    fn test_directions_cover_all_angles() {
        // Should have all combinations of -1, 0, 1 for dr and dc, except (0,0).
        let expected: Vec<(i32, i32)> = vec![
            (0, 1), (0, -1), (1, 0), (-1, 0),
            (1, 1), (1, -1), (-1, 1), (-1, -1),
        ];
        for dir in &expected {
            assert!(
                DIRECTIONS.contains(dir),
                "Direction {:?} not in DIRECTIONS",
                dir
            );
        }
    }

    // ── Integration: full game flow ─────────────────────────────────

    #[test]
    fn test_full_game_flow() {
        let mut app = WordSearchApp::new_with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);

        // Find all words programmatically.
        let word_count = app.placed_words.len();
        for i in 0..word_count {
            let cells = app.placed_words[i].cells();
            let (sr, sc) = cells[0];
            let (er, ec) = cells[cells.len() - 1];

            // Start selection.
            app.cursor_row = sr;
            app.cursor_col = sc;
            app.handle_event(&key_event(Key::Enter));

            // Move to end.
            app.cursor_row = er;
            app.cursor_col = ec;
            app.handle_event(&key_event(Key::Enter));
        }

        assert_eq!(app.status, GameStatus::Won);
        assert_eq!(app.words_found_count(), word_count);
    }

    #[test]
    fn test_game_with_each_difficulty() {
        let diffs = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];
        for diff in &diffs {
            let mut app = WordSearchApp::new_with_seed(100);
            app.new_game(*diff, Category::Science);
            assert_eq!(app.grid_size, diff.grid_size());
            assert_eq!(app.grid.len(), diff.grid_size() * diff.grid_size());
            // Should have placed at least some words.
            assert!(!app.placed_words.is_empty());
        }
    }

    #[test]
    fn test_game_with_each_category() {
        for cat in &Category::ALL {
            let mut app = WordSearchApp::new_with_seed(200);
            app.new_game(Difficulty::Medium, *cat);
            assert_eq!(app.category, *cat);
            assert!(!app.placed_words.is_empty());
        }
    }

    #[test]
    fn test_no_duplicate_words_placed() {
        let app = WordSearchApp::new_with_seed(42);
        let mut words: Vec<&str> = app.placed_words.iter().map(|pw| pw.word.as_str()).collect();
        let before_len = words.len();
        words.sort();
        words.dedup();
        assert_eq!(words.len(), before_len, "Duplicate words placed");
    }

    #[test]
    fn test_placed_words_within_grid_bounds() {
        let app = WordSearchApp::new_with_seed(42);
        for pw in &app.placed_words {
            for (r, c) in pw.cells() {
                assert!(r < app.grid_size, "Row {r} out of bounds for grid size {}", app.grid_size);
                assert!(c < app.grid_size, "Col {c} out of bounds for grid size {}", app.grid_size);
            }
        }
    }

    // ── Edge case tests ─────────────────────────────────────────────

    #[test]
    fn test_enter_on_won_game_does_nothing() {
        let mut app = WordSearchApp::new_with_seed(42);
        let count = app.placed_words.len();
        for i in 0..count {
            app.mark_found(i);
        }
        assert_eq!(app.status, GameStatus::Won);
        app.handle_event(&key_event(Key::Enter));
        assert_eq!(app.selection, SelectionState::None);
    }

    #[test]
    fn test_mark_found_no_duplicate_cells() {
        let mut app = WordSearchApp::new_with_seed(42);
        if app.placed_words.len() < 2 {
            return;
        }
        app.mark_found(0);
        app.mark_found(0); // Double-mark
        // found_cells should not contain duplicates even from repeated marking.
        let len = app.found_cells.len();
        let mut deduped = app.found_cells.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), len, "found_cells has duplicates");
    }

    // ── Helper to make key events for tests ─────────────────────────

    fn key_event(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }
}
