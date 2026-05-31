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

//! OurOS Match-3 — Bejeweled-style puzzle game.
//!
//! Features an 8x8 grid of colored gems with match-3 mechanics,
//! gravity/cascading, special gems (line clear, color bomb),
//! three game modes (Classic, Timed, Moves), high score tracking,
//! a hint system, and automatic shuffle when no moves exist.
//! Uses an LCG pseudo-random number generator (no external rand crate).

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
const GRID_SIZE: usize = 8;
const CELL_SIZE: f32 = 48.0;
const CELL_GAP: f32 = 2.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 60.0;
const FOOTER_HEIGHT: f32 = 40.0;
const HEADER_FONT_SIZE: f32 = 18.0;
const CELL_FONT_SIZE: f32 = 22.0;
const TITLE_FONT_SIZE: f32 = 28.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;
const CELL_CORNER_RADIUS: f32 = 6.0;

/// Number of gem types.
const GEM_TYPE_COUNT: u8 = 7;

/// Idle milliseconds before showing a hint.
const HINT_DELAY_MS: u64 = 5000;

/// Timed mode duration in seconds.
const TIMED_MODE_SECONDS: u64 = 60;

/// Moves mode move count.
const MOVES_MODE_COUNT: u32 = 30;

/// Base score for a 3-match.
const SCORE_3: u32 = 100;

/// Base score for a 4-match.
const SCORE_4: u32 = 200;

/// Base score for a 5-match.
const SCORE_5: u32 = 500;

/// Cascade multiplier increase per chain level (1.5x).
/// Stored as fixed-point: 150 means 1.50x.
const CASCADE_MULTIPLIER_FP: u32 = 150;

/// Fixed-point base (100 = 1.0x).
const FP_BASE: u32 = 100;

// ── Gem symbols for rendering ───────────────────────────────────────
const GEM_SYMBOLS: [&str; 7] = [
    "\u{25C6}", "\u{25CF}", "\u{25A0}", "\u{2605}", "\u{25B2}", "\u{2666}", "\u{2764}",
];

// ── Gem colors ──────────────────────────────────────────────────────
const GEM_COLORS: [Color; 7] = [RED, BLUE, GREEN, YELLOW, PEACH, MAUVE, TEAL];

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Rng {
    state: u64,
}

impl Rng {
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

// ── Gem types ───────────────────────────────────────────────────────
/// The type/color of a gem on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum GemType {
    Ruby = 0,
    Sapphire = 1,
    Emerald = 2,
    Topaz = 3,
    Amber = 4,
    Amethyst = 5,
    Aqua = 6,
}

impl GemType {
    fn from_index(i: u8) -> Self {
        match i {
            0 => GemType::Ruby,
            1 => GemType::Sapphire,
            2 => GemType::Emerald,
            3 => GemType::Topaz,
            4 => GemType::Amber,
            5 => GemType::Amethyst,
            _ => GemType::Aqua,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn color(self) -> Color {
        GEM_COLORS[self.index()]
    }

    fn symbol(self) -> &'static str {
        GEM_SYMBOLS[self.index()]
    }
}

/// Special gem abilities created by larger matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpecialKind {
    /// Normal gem with no special ability.
    None,
    /// Line clear: clears the entire row or column (created by 4-match).
    LineClearH,
    LineClearV,
    /// Color bomb: clears all gems of one color (created by 5-match).
    ColorBomb,
}

/// A single gem on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gem {
    gem_type: GemType,
    special: SpecialKind,
}

impl Gem {
    fn new(gem_type: GemType) -> Self {
        Self {
            gem_type,
            special: SpecialKind::None,
        }
    }

    fn with_special(gem_type: GemType, special: SpecialKind) -> Self {
        Self { gem_type, special }
    }
}

// ── Board position ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: usize,
    col: usize,
}

impl Pos {
    const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    fn in_bounds(self) -> bool {
        self.row < GRID_SIZE && self.col < GRID_SIZE
    }

    /// Returns true if `other` is horizontally or vertically adjacent.
    fn is_adjacent(self, other: Pos) -> bool {
        let dr = if self.row > other.row {
            self.row - other.row
        } else {
            other.row - self.row
        };
        let dc = if self.col > other.col {
            self.col - other.col
        } else {
            other.col - self.col
        };
        (dr == 1 && dc == 0) || (dr == 0 && dc == 1)
    }
}

// ── Match info ──────────────────────────────────────────────────────
/// Describes a match found on the board.
#[derive(Clone, Debug)]
struct MatchInfo {
    /// All positions involved in this match.
    positions: Vec<Pos>,
    /// Whether this match is horizontal.
    horizontal: bool,
    /// Length of the match.
    length: usize,
}

impl MatchInfo {
    fn score(&self) -> u32 {
        match self.length {
            3 => SCORE_3,
            4 => SCORE_4,
            _ => SCORE_5,
        }
    }
}

// ── Game mode ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameMode {
    /// Play until no valid moves remain.
    Classic,
    /// Play for a fixed time.
    Timed,
    /// Play with a fixed number of moves.
    Moves,
}

impl GameMode {
    fn label(self) -> &'static str {
        match self {
            GameMode::Classic => "Classic",
            GameMode::Timed => "Timed",
            GameMode::Moves => "Moves",
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    /// Waiting for player input.
    Idle,
    /// A gem is selected, waiting for swap target.
    Selected,
    /// Game is over.
    GameOver,
}

// ── High scores ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct HighScores {
    classic: u32,
    timed: u32,
    moves: u32,
}

impl HighScores {
    fn new() -> Self {
        Self {
            classic: 0,
            timed: 0,
            moves: 0,
        }
    }

    fn get(&self, mode: GameMode) -> u32 {
        match mode {
            GameMode::Classic => self.classic,
            GameMode::Timed => self.timed,
            GameMode::Moves => self.moves,
        }
    }

    fn update(&mut self, mode: GameMode, score: u32) {
        let current = match mode {
            GameMode::Classic => &mut self.classic,
            GameMode::Timed => &mut self.timed,
            GameMode::Moves => &mut self.moves,
        };
        if score > *current {
            *current = score;
        }
    }
}

// ── Main app struct ─────────────────────────────────────────────────
struct Match3 {
    /// The 8x8 board of gems. `board[row][col]`.
    board: [[Option<Gem>; GRID_SIZE]; GRID_SIZE],
    /// Current game state.
    state: GameState,
    /// Current game mode.
    mode: GameMode,
    /// Current score.
    score: u32,
    /// Current cascade chain level (0 = no cascade).
    chain_level: u32,
    /// Cursor position (for keyboard navigation).
    cursor: Pos,
    /// Currently selected gem position (for swap).
    selected: Option<Pos>,
    /// Hint position pair (source, target).
    hint: Option<(Pos, Pos)>,
    /// Milliseconds since last user action (for hint timer).
    idle_ms: u64,
    /// Whether the hint is currently visible.
    hint_visible: bool,
    /// Remaining time in milliseconds (Timed mode).
    time_remaining_ms: u64,
    /// Remaining moves (Moves mode).
    moves_remaining: u32,
    /// High scores per mode.
    high_scores: HighScores,
    /// RNG state.
    rng: Rng,
    /// Animation pulse counter.
    pulse_counter: u32,
    /// Total elapsed time in ms.
    total_elapsed_ms: u64,
}

impl Match3 {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            board: [[None; GRID_SIZE]; GRID_SIZE],
            state: GameState::Idle,
            mode: GameMode::Classic,
            score: 0,
            chain_level: 0,
            cursor: Pos::new(0, 0),
            selected: None,
            hint: None,
            idle_ms: 0,
            hint_visible: false,
            time_remaining_ms: TIMED_MODE_SECONDS * 1000,
            moves_remaining: MOVES_MODE_COUNT,
            high_scores: HighScores::new(),
            rng: Rng::new(seed),
            pulse_counter: 0,
            total_elapsed_ms: 0,
        };
        app.fill_board_no_matches();
        app
    }

    // ── Board initialization ────────────────────────────────────────

    /// Fill the board with random gems, ensuring no initial matches.
    fn fill_board_no_matches(&mut self) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                self.board[row][col] = Some(self.random_gem_no_match(row, col));
            }
        }
    }

    /// Generate a random gem for (row, col) that does not create a match.
    fn random_gem_no_match(&mut self, row: usize, col: usize) -> Gem {
        loop {
            let gem_type =
                GemType::from_index(self.rng.next_bounded(GEM_TYPE_COUNT as usize) as u8);
            let gem = Gem::new(gem_type);
            // Check horizontal: if two to the left are the same type, skip.
            if col >= 2 {
                if let (Some(a), Some(b)) = (self.board[row][col - 1], self.board[row][col - 2]) {
                    if a.gem_type == gem_type && b.gem_type == gem_type {
                        continue;
                    }
                }
            }
            // Check vertical: if two above are the same type, skip.
            if row >= 2 {
                if let (Some(a), Some(b)) = (self.board[row - 1][col], self.board[row - 2][col]) {
                    if a.gem_type == gem_type && b.gem_type == gem_type {
                        continue;
                    }
                }
            }
            return gem;
        }
    }

    /// Generate a random gem type.
    fn random_gem(&mut self) -> Gem {
        let gem_type = GemType::from_index(self.rng.next_bounded(GEM_TYPE_COUNT as usize) as u8);
        Gem::new(gem_type)
    }

    // ── Match detection ─────────────────────────────────────────────

    /// Find all matches on the board. Returns a list of match groups.
    fn find_matches(&self) -> Vec<MatchInfo> {
        let mut matches = Vec::new();

        // Horizontal matches.
        for row in 0..GRID_SIZE {
            let mut col = 0;
            while col < GRID_SIZE {
                if let Some(gem) = self.board[row][col] {
                    let gem_type = gem.gem_type;
                    let start = col;
                    while col < GRID_SIZE {
                        if let Some(g) = self.board[row][col] {
                            if g.gem_type == gem_type {
                                col += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let length = col - start;
                    if length >= 3 {
                        let positions: Vec<Pos> = (start..col).map(|c| Pos::new(row, c)).collect();
                        matches.push(MatchInfo {
                            positions,
                            horizontal: true,
                            length,
                        });
                    }
                } else {
                    col += 1;
                }
            }
        }

        // Vertical matches.
        for col in 0..GRID_SIZE {
            let mut row = 0;
            while row < GRID_SIZE {
                if let Some(gem) = self.board[row][col] {
                    let gem_type = gem.gem_type;
                    let start = row;
                    while row < GRID_SIZE {
                        if let Some(g) = self.board[row][col] {
                            if g.gem_type == gem_type {
                                row += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let length = row - start;
                    if length >= 3 {
                        let positions: Vec<Pos> = (start..row).map(|r| Pos::new(r, col)).collect();
                        matches.push(MatchInfo {
                            positions,
                            horizontal: false,
                            length,
                        });
                    }
                } else {
                    row += 1;
                }
            }
        }

        matches
    }

    /// Check if a specific swap would create any match.
    fn swap_creates_match(&mut self, a: Pos, b: Pos) -> bool {
        // Perform swap.
        let tmp = self.board[a.row][a.col];
        self.board[a.row][a.col] = self.board[b.row][b.col];
        self.board[b.row][b.col] = tmp;

        let has_match = !self.find_matches().is_empty();

        // Undo swap.
        let tmp = self.board[a.row][a.col];
        self.board[a.row][a.col] = self.board[b.row][b.col];
        self.board[b.row][b.col] = tmp;

        has_match
    }

    // ── Match processing ────────────────────────────────────────────

    /// Remove matched gems and award score. Returns true if any matches were removed.
    fn process_matches(&mut self) -> bool {
        let matches = self.find_matches();
        if matches.is_empty() {
            return false;
        }

        // Calculate score with cascade multiplier.
        let multiplier = self.cascade_multiplier();
        for m in &matches {
            let base = m.score();
            let scored = (base * multiplier) / FP_BASE;
            self.score = self.score.saturating_add(scored);
        }

        // Create special gems and mark positions for removal.
        let mut to_remove = Vec::new();
        for m in &matches {
            // Determine if a special gem should be created.
            let special_pos = self.determine_special_gem(m);
            for &pos in &m.positions {
                // Handle existing special gems being matched.
                if let Some(gem) = self.board[pos.row][pos.col] {
                    match gem.special {
                        SpecialKind::LineClearH => {
                            // Clear entire row.
                            for c in 0..GRID_SIZE {
                                let p = Pos::new(pos.row, c);
                                if !to_remove.contains(&p) {
                                    to_remove.push(p);
                                }
                            }
                        }
                        SpecialKind::LineClearV => {
                            // Clear entire column.
                            for r in 0..GRID_SIZE {
                                let p = Pos::new(r, pos.col);
                                if !to_remove.contains(&p) {
                                    to_remove.push(p);
                                }
                            }
                        }
                        SpecialKind::ColorBomb => {
                            // Clear all gems of the matched type.
                            let target_type = gem.gem_type;
                            for r in 0..GRID_SIZE {
                                for c in 0..GRID_SIZE {
                                    if let Some(g) = self.board[r][c] {
                                        if g.gem_type == target_type {
                                            let p = Pos::new(r, c);
                                            if !to_remove.contains(&p) {
                                                to_remove.push(p);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        SpecialKind::None => {}
                    }
                }
                if !to_remove.contains(&pos) {
                    to_remove.push(pos);
                }
            }

            // Place special gem if one was determined.
            if let Some((pos, special)) = special_pos {
                if let Some(gem) = self.board[pos.row][pos.col] {
                    self.board[pos.row][pos.col] = Some(Gem::with_special(gem.gem_type, special));
                    // Remove this position from the removal list so the special gem survives.
                    to_remove.retain(|&p| p != pos);
                }
            }
        }

        // Remove matched gems.
        for pos in &to_remove {
            self.board[pos.row][pos.col] = None;
        }

        true
    }

    /// Determine if a match should create a special gem and where.
    fn determine_special_gem(&self, m: &MatchInfo) -> Option<(Pos, SpecialKind)> {
        if m.length == 4 {
            // 4-match: line clear gem at the center of the match.
            let center = m.length / 2;
            let pos = m.positions[center];
            let special = if m.horizontal {
                SpecialKind::LineClearH
            } else {
                SpecialKind::LineClearV
            };
            Some((pos, special))
        } else if m.length >= 5 {
            // 5+ match: color bomb at the center.
            let center = m.length / 2;
            let pos = m.positions[center];
            Some((pos, SpecialKind::ColorBomb))
        } else {
            None
        }
    }

    /// Calculate cascade multiplier as fixed-point (100 = 1.0x).
    fn cascade_multiplier(&self) -> u32 {
        let mut mult = FP_BASE;
        for _ in 0..self.chain_level {
            mult = (mult * CASCADE_MULTIPLIER_FP) / FP_BASE;
        }
        mult
    }

    /// Apply gravity: gems fall down to fill empty spaces.
    /// Returns true if any gems moved.
    fn apply_gravity(&mut self) -> bool {
        let mut moved = false;
        for col in 0..GRID_SIZE {
            // Compact column: move gems down to fill gaps.
            let mut write = GRID_SIZE;
            for read in (0..GRID_SIZE).rev() {
                if self.board[read][col].is_some() {
                    write -= 1;
                    if write != read {
                        self.board[write][col] = self.board[read][col];
                        self.board[read][col] = None;
                        moved = true;
                    }
                }
            }
        }
        moved
    }

    /// Fill empty spaces at the top with new random gems.
    fn fill_empty_spaces(&mut self) {
        for col in 0..GRID_SIZE {
            for row in 0..GRID_SIZE {
                if self.board[row][col].is_none() {
                    self.board[row][col] = Some(self.random_gem());
                }
            }
        }
    }

    /// Run the full cascade loop: match -> remove -> gravity -> fill -> repeat.
    /// Returns total score delta from cascades.
    fn run_cascade(&mut self) {
        self.chain_level = 0;
        loop {
            if !self.process_matches() {
                break;
            }
            self.chain_level += 1;
            self.apply_gravity();
            self.fill_empty_spaces();
        }
        self.chain_level = 0;
    }

    // ── Move validation ─────────────────────────────────────────────

    /// Find all valid moves on the board.
    fn find_valid_moves(&mut self) -> Vec<(Pos, Pos)> {
        let mut moves = Vec::new();
        // Check horizontal swaps.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE - 1 {
                let a = Pos::new(row, col);
                let b = Pos::new(row, col + 1);
                if self.swap_creates_match(a, b) {
                    moves.push((a, b));
                }
            }
        }
        // Check vertical swaps.
        for row in 0..GRID_SIZE - 1 {
            for col in 0..GRID_SIZE {
                let a = Pos::new(row, col);
                let b = Pos::new(row + 1, col);
                if self.swap_creates_match(a, b) {
                    moves.push((a, b));
                }
            }
        }
        moves
    }

    /// Check if any valid moves exist.
    fn has_valid_moves(&mut self) -> bool {
        // Check horizontal swaps.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE - 1 {
                let a = Pos::new(row, col);
                let b = Pos::new(row, col + 1);
                if self.swap_creates_match(a, b) {
                    return true;
                }
            }
        }
        // Check vertical swaps.
        for row in 0..GRID_SIZE - 1 {
            for col in 0..GRID_SIZE {
                let a = Pos::new(row, col);
                let b = Pos::new(row + 1, col);
                if self.swap_creates_match(a, b) {
                    return true;
                }
            }
        }
        false
    }

    /// Shuffle the board until valid moves exist.
    fn shuffle_board(&mut self) {
        let mut attempts = 0;
        loop {
            // Fisher-Yates shuffle of all gems.
            let mut gems: Vec<Option<Gem>> = Vec::new();
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    gems.push(self.board[row][col]);
                }
            }
            let len = gems.len();
            for i in (1..len).rev() {
                let j = self.rng.next_bounded(i + 1);
                gems.swap(i, j);
            }
            // Place back on board.
            for row in 0..GRID_SIZE {
                for col in 0..GRID_SIZE {
                    self.board[row][col] = gems[row * GRID_SIZE + col];
                }
            }
            // Remove any existing matches first.
            while self.process_matches() {
                self.apply_gravity();
                self.fill_empty_spaces();
            }
            if self.has_valid_moves() {
                break;
            }
            attempts += 1;
            if attempts > 100 {
                // Fallback: regenerate board from scratch.
                self.fill_board_no_matches();
                break;
            }
        }
    }

    // ── Hint system ─────────────────────────────────────────────────

    /// Find one valid move to use as a hint.
    fn find_hint(&mut self) -> Option<(Pos, Pos)> {
        let moves = self.find_valid_moves();
        if moves.is_empty() {
            None
        } else {
            let idx = self.rng.next_bounded(moves.len());
            Some(moves[idx])
        }
    }

    /// Update the hint timer and compute hint if needed.
    fn update_hint(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Idle {
            self.hint_visible = false;
            return;
        }
        self.idle_ms = self.idle_ms.saturating_add(elapsed_ms);
        if self.idle_ms >= HINT_DELAY_MS && !self.hint_visible {
            self.hint = self.find_hint();
            self.hint_visible = true;
        }
    }

    /// Reset the idle timer (called on user action).
    fn reset_idle(&mut self) {
        self.idle_ms = 0;
        self.hint_visible = false;
        self.hint = None;
    }

    // ── Swap logic ──────────────────────────────────────────────────

    /// Attempt to swap two adjacent gems. Returns true if the swap was valid.
    fn try_swap(&mut self, a: Pos, b: Pos) -> bool {
        if !a.in_bounds() || !b.in_bounds() || !a.is_adjacent(b) {
            return false;
        }
        if self.board[a.row][a.col].is_none() || self.board[b.row][b.col].is_none() {
            return false;
        }

        // Check if either gem is a color bomb being swapped with a regular gem.
        let gem_a = self.board[a.row][a.col];
        let gem_b = self.board[b.row][b.col];
        if let (Some(ga), Some(gb)) = (gem_a, gem_b) {
            if ga.special == SpecialKind::ColorBomb && gb.special != SpecialKind::ColorBomb {
                // Color bomb: remove all gems of the target color.
                let target = gb.gem_type;
                self.board[a.row][a.col] = None;
                for r in 0..GRID_SIZE {
                    for c in 0..GRID_SIZE {
                        if let Some(g) = self.board[r][c] {
                            if g.gem_type == target {
                                self.board[r][c] = None;
                            }
                        }
                    }
                }
                let scored = SCORE_5 * 2;
                self.score = self.score.saturating_add(scored);
                self.apply_gravity();
                self.fill_empty_spaces();
                self.run_cascade();
                self.consume_move();
                return true;
            }
            if gb.special == SpecialKind::ColorBomb && ga.special != SpecialKind::ColorBomb {
                let target = ga.gem_type;
                self.board[b.row][b.col] = None;
                for r in 0..GRID_SIZE {
                    for c in 0..GRID_SIZE {
                        if let Some(g) = self.board[r][c] {
                            if g.gem_type == target {
                                self.board[r][c] = None;
                            }
                        }
                    }
                }
                let scored = SCORE_5 * 2;
                self.score = self.score.saturating_add(scored);
                self.apply_gravity();
                self.fill_empty_spaces();
                self.run_cascade();
                self.consume_move();
                return true;
            }
        }

        // Check if swap creates a match.
        if !self.swap_creates_match(a, b) {
            return false;
        }

        // Perform the swap.
        let tmp = self.board[a.row][a.col];
        self.board[a.row][a.col] = self.board[b.row][b.col];
        self.board[b.row][b.col] = tmp;

        // Run cascade.
        self.run_cascade();

        // Consume a move in Moves mode.
        self.consume_move();

        // Check if no valid moves remain.
        if !self.has_valid_moves() {
            if self.mode == GameMode::Classic {
                self.end_game();
            } else {
                self.shuffle_board();
            }
        }

        true
    }

    /// Consume one move (for Moves mode tracking).
    fn consume_move(&mut self) {
        if self.mode == GameMode::Moves {
            self.moves_remaining = self.moves_remaining.saturating_sub(1);
            if self.moves_remaining == 0 {
                self.end_game();
            }
        }
    }

    /// End the current game.
    fn end_game(&mut self) {
        self.state = GameState::GameOver;
        self.high_scores.update(self.mode, self.score);
    }

    /// Start a new game with the current mode.
    fn new_game(&mut self) {
        let mode = self.mode;
        let high_scores = self.high_scores.clone();
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.mode = mode;
        self.high_scores = high_scores;
        self.time_remaining_ms = TIMED_MODE_SECONDS * 1000;
        self.moves_remaining = MOVES_MODE_COUNT;
    }

    /// Switch to a different game mode and start a new game.
    fn switch_mode(&mut self, mode: GameMode) {
        self.mode = mode;
        self.new_game();
    }

    // ── Grid pixel math ─────────────────────────────────────────────

    fn grid_origin_x() -> f32 {
        PADDING
    }

    fn grid_origin_y() -> f32 {
        PADDING + HEADER_HEIGHT
    }

    fn grid_width() -> f32 {
        GRID_SIZE as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    fn grid_height() -> f32 {
        GRID_SIZE as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    fn window_width() -> f32 {
        PADDING * 2.0 + Self::grid_width()
    }

    fn window_height() -> f32 {
        PADDING * 2.0 + HEADER_HEIGHT + Self::grid_height() + FOOTER_HEIGHT
    }

    /// Convert pixel coordinates to grid position.
    fn pixel_to_grid(px: f32, py: f32) -> Option<Pos> {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let gx = px - ox;
        let gy = py - oy;
        if gx < 0.0 || gy < 0.0 {
            return None;
        }
        let col = (gx / (CELL_SIZE + CELL_GAP)) as usize;
        let row = (gy / (CELL_SIZE + CELL_GAP)) as usize;
        if row < GRID_SIZE && col < GRID_SIZE {
            // Check we are actually within the cell, not in the gap.
            let cell_x = gx - col as f32 * (CELL_SIZE + CELL_GAP);
            let cell_y = gy - row as f32 * (CELL_SIZE + CELL_GAP);
            if cell_x <= CELL_SIZE && cell_y <= CELL_SIZE {
                Some(Pos::new(row, col))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get the pixel center of a grid cell.
    fn cell_center(pos: Pos) -> (f32, f32) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let x = ox + pos.col as f32 * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2.0;
        let y = oy + pos.row as f32 * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2.0;
        (x, y)
    }

    /// Get the top-left pixel of a grid cell.
    fn cell_origin(pos: Pos) -> (f32, f32) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let x = ox + pos.col as f32 * (CELL_SIZE + CELL_GAP);
        let y = oy + pos.row as f32 * (CELL_SIZE + CELL_GAP);
        (x, y)
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Produce the full render command list for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let win_w = Self::window_width();
        let win_h = Self::window_height();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);
        self.render_grid(&mut cmds);
        self.render_gems(&mut cmds);
        self.render_cursor(&mut cmds);
        self.render_selection(&mut cmds);
        self.render_hint_highlight(&mut cmds);
        self.render_footer(&mut cmds);

        if self.state == GameState::GameOver {
            self.render_game_over_overlay(&mut cmds);
        }

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_w = Self::window_width() - PADDING * 2.0;

        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING / 2.0,
            width: header_w,
            height: HEADER_HEIGHT - PADDING / 2.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Mode label.
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: PADDING / 2.0 + 8.0,
            text: format!("Mode: {}", self.mode.label()),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score.
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: PADDING / 2.0 + 30.0,
            text: format!("Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // High score.
        let high = self.high_scores.get(self.mode);
        cmds.push(RenderCommand::Text {
            x: PADDING + header_w / 2.0 - 20.0,
            y: PADDING / 2.0 + 8.0,
            text: format!("Best: {high}"),
            color: YELLOW,
            font_size: HEADER_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Mode-specific info.
        match self.mode {
            GameMode::Timed => {
                let secs = self.time_remaining_ms / 1000;
                let color = if secs <= 10 { RED } else { GREEN };
                cmds.push(RenderCommand::Text {
                    x: PADDING + header_w - 120.0,
                    y: PADDING / 2.0 + 8.0,
                    text: format!("Time: {secs}s"),
                    color,
                    font_size: HEADER_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameMode::Moves => {
                let color = if self.moves_remaining <= 5 { RED } else { BLUE };
                cmds.push(RenderCommand::Text {
                    x: PADDING + header_w - 120.0,
                    y: PADDING / 2.0 + 8.0,
                    text: format!("Moves: {}", self.moves_remaining),
                    color,
                    font_size: HEADER_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameMode::Classic => {
                cmds.push(RenderCommand::Text {
                    x: PADDING + header_w - 120.0,
                    y: PADDING / 2.0 + 8.0,
                    text: String::from("No limit"),
                    color: SUBTEXT0,
                    font_size: HEADER_FONT_SIZE - 2.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();

        // Grid background.
        cmds.push(RenderCommand::FillRect {
            x: ox - 4.0,
            y: oy - 4.0,
            width: Self::grid_width() + 8.0,
            height: Self::grid_height() + 8.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Cell backgrounds.
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let (cx, cy) = Self::cell_origin(Pos::new(row, col));
                let bg = if (row + col) % 2 == 0 {
                    SURFACE1
                } else {
                    SURFACE0
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });
            }
        }
    }

    fn render_gems(&self, cmds: &mut Vec<RenderCommand>) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if let Some(gem) = self.board[row][col] {
                    let pos = Pos::new(row, col);
                    self.render_gem(cmds, pos, gem);
                }
            }
        }
    }

    fn render_gem(&self, cmds: &mut Vec<RenderCommand>, pos: Pos, gem: Gem) {
        let (cx, cy) = Self::cell_origin(pos);
        let inset = 4.0;
        let gem_color = gem.gem_type.color();

        // Gem body.
        cmds.push(RenderCommand::FillRect {
            x: cx + inset,
            y: cy + inset,
            width: CELL_SIZE - inset * 2.0,
            height: CELL_SIZE - inset * 2.0,
            color: gem_color,
            corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
        });

        // Gem symbol.
        cmds.push(RenderCommand::Text {
            x: cx + CELL_SIZE / 2.0 - 6.0,
            y: cy + CELL_SIZE / 2.0 - 8.0,
            text: String::from(gem.gem_type.symbol()),
            color: BASE,
            font_size: CELL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Special gem indicator.
        match gem.special {
            SpecialKind::LineClearH => {
                // Horizontal line indicator.
                cmds.push(RenderCommand::Line {
                    x1: cx + inset + 2.0,
                    y1: cy + CELL_SIZE / 2.0,
                    x2: cx + CELL_SIZE - inset - 2.0,
                    y2: cy + CELL_SIZE / 2.0,
                    color: TEXT_COLOR,
                    width: 2.0,
                });
            }
            SpecialKind::LineClearV => {
                // Vertical line indicator.
                cmds.push(RenderCommand::Line {
                    x1: cx + CELL_SIZE / 2.0,
                    y1: cy + inset + 2.0,
                    x2: cx + CELL_SIZE / 2.0,
                    y2: cy + CELL_SIZE - inset - 2.0,
                    color: TEXT_COLOR,
                    width: 2.0,
                });
            }
            SpecialKind::ColorBomb => {
                // Star burst indicator: small circles at corners.
                let r = 3.0;
                for &(dx, dy) in &[
                    (inset + r, inset + r),
                    (CELL_SIZE - inset - r, inset + r),
                    (inset + r, CELL_SIZE - inset - r),
                    (CELL_SIZE - inset - r, CELL_SIZE - inset - r),
                ] {
                    cmds.push(RenderCommand::FillRect {
                        x: cx + dx - r,
                        y: cy + dy - r,
                        width: r * 2.0,
                        height: r * 2.0,
                        color: YELLOW,
                        corner_radii: CornerRadii::all(r),
                    });
                }
            }
            SpecialKind::None => {}
        }
    }

    fn render_cursor(&self, cmds: &mut Vec<RenderCommand>) {
        if self.state == GameState::GameOver {
            return;
        }
        let (cx, cy) = Self::cell_origin(self.cursor);

        // Cursor highlight (subtle border).
        cmds.push(RenderCommand::FillRect {
            x: cx - 2.0,
            y: cy - 2.0,
            width: CELL_SIZE + 4.0,
            height: CELL_SIZE + 4.0,
            color: Color::rgba(205, 214, 244, 80),
            corner_radii: CornerRadii::all(CELL_CORNER_RADIUS + 2.0),
        });
    }

    fn render_selection(&self, cmds: &mut Vec<RenderCommand>) {
        if let Some(sel) = self.selected {
            let (cx, cy) = Self::cell_origin(sel);

            // Selection highlight (bright border).
            cmds.push(RenderCommand::FillRect {
                x: cx - 3.0,
                y: cy - 3.0,
                width: CELL_SIZE + 6.0,
                height: CELL_SIZE + 6.0,
                color: Color::rgba(137, 180, 250, 120),
                corner_radii: CornerRadii::all(CELL_CORNER_RADIUS + 3.0),
            });
        }
    }

    fn render_hint_highlight(&self, cmds: &mut Vec<RenderCommand>) {
        if !self.hint_visible {
            return;
        }
        if let Some((a, b)) = self.hint {
            // Pulsing glow effect on hint gems.
            let pulse =
                ((self.pulse_counter % 60) as f32 / 60.0 * std::f32::consts::PI * 2.0).sin();
            let alpha = (40.0 + pulse * 40.0) as u8;

            for pos in &[a, b] {
                let (cx, cy) = Self::cell_origin(*pos);
                cmds.push(RenderCommand::FillRect {
                    x: cx - 3.0,
                    y: cy - 3.0,
                    width: CELL_SIZE + 6.0,
                    height: CELL_SIZE + 6.0,
                    color: Color::rgba(166, 227, 161, alpha),
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS + 3.0),
                });
            }
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let y = Self::window_height() - FOOTER_HEIGHT + 8.0;

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y,
            text: String::from("Arrows/Click:Move  Enter/Click:Swap  H:Hint  N:New  1/2/3:Mode"),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let ox = Self::grid_origin_x();
        let oy = Self::grid_origin_y();
        let gw = Self::grid_width();
        let gh = Self::grid_height();

        // Dim overlay.
        cmds.push(RenderCommand::FillRect {
            x: ox - 4.0,
            y: oy - 4.0,
            width: gw + 8.0,
            height: gh + 8.0,
            color: Color::rgba(30, 30, 46, 200),
            corner_radii: CornerRadii::all(8.0),
        });

        // Game Over text.
        let center_x = ox + gw / 2.0;
        let center_y = oy + gh / 2.0;

        cmds.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y - 50.0,
            text: String::from("GAME OVER"),
            color: RED,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 60.0,
            y: center_y - 10.0,
            text: format!("Final Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let high = self.high_scores.get(self.mode);
        cmds.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: center_y + 15.0,
            text: format!("Best: {high}"),
            color: YELLOW,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y + 50.0,
            text: String::from("Press N for new game"),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE - 2.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => {
                if ke.pressed {
                    self.handle_key(ke.key);
                }
            }
            Event::Mouse(me) => self.handle_mouse(me),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key) {
        self.reset_idle();

        match key {
            Key::N => {
                self.new_game();
                return;
            }
            Key::Num1 => {
                self.switch_mode(GameMode::Classic);
                return;
            }
            Key::Num2 => {
                self.switch_mode(GameMode::Timed);
                return;
            }
            Key::Num3 => {
                self.switch_mode(GameMode::Moves);
                return;
            }
            Key::H => {
                if self.state != GameState::GameOver {
                    self.hint = self.find_hint();
                    self.hint_visible = true;
                }
                return;
            }
            _ => {}
        }

        if self.state == GameState::GameOver {
            return;
        }

        match key {
            Key::Left => {
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
            }
            Key::Right => {
                if self.cursor.col < GRID_SIZE - 1 {
                    self.cursor.col += 1;
                }
            }
            Key::Up => {
                if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                }
            }
            Key::Down => {
                if self.cursor.row < GRID_SIZE - 1 {
                    self.cursor.row += 1;
                }
            }
            Key::Enter | Key::Space => {
                self.select_or_swap(self.cursor);
            }
            Key::Escape => {
                self.selected = None;
                self.state = GameState::Idle;
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, me: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = me.kind {
            self.reset_idle();
            if self.state == GameState::GameOver {
                return;
            }
            if let Some(pos) = Self::pixel_to_grid(me.x, me.y) {
                self.select_or_swap(pos);
            }
        }
    }

    fn select_or_swap(&mut self, pos: Pos) {
        match self.state {
            GameState::Idle => {
                self.selected = Some(pos);
                self.state = GameState::Selected;
            }
            GameState::Selected => {
                if let Some(sel) = self.selected {
                    if sel == pos {
                        // Deselect.
                        self.selected = None;
                        self.state = GameState::Idle;
                    } else if sel.is_adjacent(pos) {
                        // Attempt swap.
                        self.try_swap(sel, pos);
                        self.selected = None;
                        self.state = GameState::Idle;
                    } else {
                        // Select new gem.
                        self.selected = Some(pos);
                    }
                }
            }
            GameState::GameOver => {}
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        self.total_elapsed_ms = self.total_elapsed_ms.saturating_add(elapsed_ms);
        self.pulse_counter = self.pulse_counter.wrapping_add(1);
        self.update_hint(elapsed_ms);

        if self.state != GameState::GameOver && self.mode == GameMode::Timed {
            if self.time_remaining_ms > elapsed_ms {
                self.time_remaining_ms -= elapsed_ms;
            } else {
                self.time_remaining_ms = 0;
                self.end_game();
            }
        }
    }

    // ── Board queries (for testing) ─────────────────────────────────

    /// Get the gem at a position.
    fn get_gem(&self, row: usize, col: usize) -> Option<Gem> {
        if row < GRID_SIZE && col < GRID_SIZE {
            self.board[row][col]
        } else {
            None
        }
    }

    /// Set a gem at a position (for testing).
    fn set_gem(&mut self, row: usize, col: usize, gem: Option<Gem>) {
        if row < GRID_SIZE && col < GRID_SIZE {
            self.board[row][col] = gem;
        }
    }

    /// Clear the board (for testing setups).
    fn clear_board(&mut self) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                self.board[row][col] = None;
            }
        }
    }

    /// Fill entire board with a single gem type (for testing).
    fn fill_board_with(&mut self, gem: Gem) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                self.board[row][col] = Some(gem);
            }
        }
    }

    /// Count non-empty cells.
    fn gem_count(&self) -> usize {
        let mut count = 0;
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if self.board[row][col].is_some() {
                    count += 1;
                }
            }
        }
        count
    }
}

fn main() {
    let _app = Match3::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper functions ────────────────────────────────────────────

    fn make_gem(t: u8) -> Gem {
        Gem::new(GemType::from_index(t))
    }

    fn make_special_gem(t: u8, special: SpecialKind) -> Gem {
        Gem::with_special(GemType::from_index(t), special)
    }

    /// Create a game with a fully empty board for test setups.
    fn empty_game() -> Match3 {
        let mut game = Match3::with_seed(1);
        game.clear_board();
        game
    }

    /// Fill the board with alternating gems so no matches exist.
    fn fill_no_matches(game: &mut Match3) {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                // Use a pattern that never creates 3-in-a-row.
                let t = ((row * 2 + col) % GEM_TYPE_COUNT as usize) as u8;
                game.board[row][col] = Some(make_gem(t));
            }
        }
    }

    // ── Rng tests ───────────────────────────────────────────────────

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(r1.next_u64(), r2.next_u64());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        // Should diverge immediately.
        assert_ne!(r1.next_u64(), r2.next_u64());
    }

    #[test]
    fn test_rng_bounded() {
        let mut r = Rng::new(42);
        for _ in 0..1000 {
            let v = r.next_bounded(7);
            assert!(v < 7);
        }
    }

    #[test]
    fn test_rng_bounded_one() {
        let mut r = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(r.next_bounded(1), 0);
        }
    }

    // ── GemType tests ───────────────────────────────────────────────

    #[test]
    fn test_gem_type_roundtrip() {
        for i in 0..GEM_TYPE_COUNT {
            let gt = GemType::from_index(i);
            assert_eq!(gt.index(), i as usize);
        }
    }

    #[test]
    fn test_gem_type_color_distinct() {
        let colors: Vec<Color> = (0..GEM_TYPE_COUNT)
            .map(|i| GemType::from_index(i).color())
            .collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_gem_type_symbol_distinct() {
        let syms: Vec<&str> = (0..GEM_TYPE_COUNT)
            .map(|i| GemType::from_index(i).symbol())
            .collect();
        for i in 0..syms.len() {
            for j in (i + 1)..syms.len() {
                assert_ne!(syms[i], syms[j]);
            }
        }
    }

    #[test]
    fn test_gem_type_out_of_range() {
        // Values >= 6 should map to Aqua.
        assert_eq!(GemType::from_index(7), GemType::Aqua);
        assert_eq!(GemType::from_index(100), GemType::Aqua);
    }

    // ── Pos tests ───────────────────────────────────────────────────

    #[test]
    fn test_pos_in_bounds() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(7, 7).in_bounds());
        assert!(!Pos::new(8, 0).in_bounds());
        assert!(!Pos::new(0, 8).in_bounds());
    }

    #[test]
    fn test_pos_adjacent_horizontal() {
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(3, 5)));
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(3, 3)));
    }

    #[test]
    fn test_pos_adjacent_vertical() {
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(4, 4)));
        assert!(Pos::new(3, 4).is_adjacent(Pos::new(2, 4)));
    }

    #[test]
    fn test_pos_not_adjacent_diagonal() {
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(4, 5)));
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(2, 3)));
    }

    #[test]
    fn test_pos_not_adjacent_same() {
        assert!(!Pos::new(3, 4).is_adjacent(Pos::new(3, 4)));
    }

    #[test]
    fn test_pos_not_adjacent_far() {
        assert!(!Pos::new(0, 0).is_adjacent(Pos::new(2, 0)));
        assert!(!Pos::new(0, 0).is_adjacent(Pos::new(0, 2)));
    }

    // ── Board initialization tests ──────────────────────────────────

    #[test]
    fn test_new_game_fills_board() {
        let game = Match3::new();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_new_game_no_initial_matches() {
        let game = Match3::new();
        let matches = game.find_matches();
        assert!(matches.is_empty(), "New board should have no matches");
    }

    #[test]
    fn test_deterministic_seed() {
        let g1 = Match3::with_seed(123);
        let g2 = Match3::with_seed(123);
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                assert_eq!(g1.board[row][col], g2.board[row][col]);
            }
        }
    }

    #[test]
    fn test_different_seeds_different_boards() {
        let g1 = Match3::with_seed(1);
        let g2 = Match3::with_seed(2);
        let mut same = 0;
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                if g1.board[row][col] == g2.board[row][col] {
                    same += 1;
                }
            }
        }
        // Should not be identical (statistically nearly impossible).
        assert!(same < GRID_SIZE * GRID_SIZE);
    }

    // ── Match detection tests ───────────────────────────────────────

    #[test]
    fn test_find_horizontal_match_3() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Place 3 rubies in a row.
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));
        let matches = game.find_matches();
        assert!(!matches.is_empty());
        let m = &matches[0];
        assert!(m.horizontal);
        assert_eq!(m.length, 3);
    }

    #[test]
    fn test_find_vertical_match_3() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[1][0] = Some(make_gem(0));
        game.board[2][0] = Some(make_gem(0));
        let matches = game.find_matches();
        assert!(!matches.is_empty());
        let has_vertical = matches.iter().any(|m| !m.horizontal && m.length == 3);
        assert!(has_vertical);
    }

    #[test]
    fn test_find_match_4() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[3][2] = Some(make_gem(1));
        game.board[3][3] = Some(make_gem(1));
        game.board[3][4] = Some(make_gem(1));
        game.board[3][5] = Some(make_gem(1));
        let matches = game.find_matches();
        let has_4 = matches.iter().any(|m| m.length == 4);
        assert!(has_4);
    }

    #[test]
    fn test_find_match_5() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[5][1] = Some(make_gem(2));
        game.board[5][2] = Some(make_gem(2));
        game.board[5][3] = Some(make_gem(2));
        game.board[5][4] = Some(make_gem(2));
        game.board[5][5] = Some(make_gem(2));
        let matches = game.find_matches();
        let has_5 = matches.iter().any(|m| m.length >= 5);
        assert!(has_5);
    }

    #[test]
    fn test_no_match_with_2() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        // Only 2 in a row: no match.
        let matches = game.find_matches();
        let has_match_at_row0 = matches
            .iter()
            .any(|m| m.positions.iter().any(|p| p.row == 0 && p.col <= 1));
        assert!(!has_match_at_row0);
    }

    #[test]
    fn test_empty_board_no_matches() {
        let game = empty_game();
        assert!(game.find_matches().is_empty());
    }

    // ── Match scoring tests ─────────────────────────────────────────

    #[test]
    fn test_score_3_match() {
        let m = MatchInfo {
            positions: vec![Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 2)],
            horizontal: true,
            length: 3,
        };
        assert_eq!(m.score(), SCORE_3);
    }

    #[test]
    fn test_score_4_match() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
            ],
            horizontal: true,
            length: 4,
        };
        assert_eq!(m.score(), SCORE_4);
    }

    #[test]
    fn test_score_5_match() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
                Pos::new(0, 4),
            ],
            horizontal: true,
            length: 5,
        };
        assert_eq!(m.score(), SCORE_5);
    }

    // ── Cascade multiplier tests ────────────────────────────────────

    #[test]
    fn test_cascade_multiplier_base() {
        let game = Match3::new();
        assert_eq!(game.cascade_multiplier(), FP_BASE);
    }

    #[test]
    fn test_cascade_multiplier_level_1() {
        let mut game = Match3::new();
        game.chain_level = 1;
        assert_eq!(game.cascade_multiplier(), CASCADE_MULTIPLIER_FP);
    }

    #[test]
    fn test_cascade_multiplier_level_2() {
        let mut game = Match3::new();
        game.chain_level = 2;
        // 1.5 * 1.5 = 2.25 -> 225 in FP.
        assert_eq!(game.cascade_multiplier(), 225);
    }

    // ── Gravity tests ───────────────────────────────────────────────

    #[test]
    fn test_gravity_single_gap() {
        let mut game = empty_game();
        // Place a gem at row 0, gap at row 1, gem at row 2.
        game.board[0][0] = Some(make_gem(0));
        game.board[1][0] = None;
        game.board[2][0] = Some(make_gem(1));

        let moved = game.apply_gravity();
        assert!(moved);
        // Gravity compacts gems to the bottom of the GRID_SIZE-tall column, so the
        // two gems settle in the last two rows and everything above is empty.
        for row in 0..GRID_SIZE - 2 {
            assert!(game.board[row][0].is_none());
        }
        assert!(game.board[GRID_SIZE - 2][0].is_some());
        assert!(game.board[GRID_SIZE - 1][0].is_some());
    }

    #[test]
    fn test_gravity_multiple_gaps() {
        let mut game = empty_game();
        game.board[0][3] = Some(make_gem(0));
        // rows 1-6 empty
        game.board[7][3] = Some(make_gem(1));

        game.apply_gravity();
        // gem from row 0 should fall to row 6.
        assert!(game.board[6][3].is_some());
        assert!(game.board[7][3].is_some());
        // rows 0-5 should be empty.
        for r in 0..6 {
            assert!(game.board[r][3].is_none());
        }
    }

    #[test]
    fn test_gravity_no_gaps() {
        let mut game = empty_game();
        for r in 0..GRID_SIZE {
            game.board[r][0] = Some(make_gem((r % 7) as u8));
        }
        let moved = game.apply_gravity();
        assert!(!moved);
    }

    #[test]
    fn test_gravity_preserves_order() {
        let mut game = empty_game();
        game.board[0][0] = Some(make_gem(0));
        game.board[2][0] = Some(make_gem(1));
        game.board[4][0] = Some(make_gem(2));

        game.apply_gravity();
        // Should be stacked at bottom in original order.
        assert_eq!(game.board[5][0].unwrap().gem_type, GemType::Ruby);
        assert_eq!(game.board[6][0].unwrap().gem_type, GemType::Sapphire);
        assert_eq!(game.board[7][0].unwrap().gem_type, GemType::Emerald);
    }

    // ── Fill empty spaces tests ─────────────────────────────────────

    #[test]
    fn test_fill_empty_spaces() {
        let mut game = empty_game();
        game.fill_empty_spaces();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_fill_preserves_existing() {
        let mut game = empty_game();
        let gem = make_gem(3);
        game.board[4][4] = Some(gem);
        game.fill_empty_spaces();
        assert_eq!(game.board[4][4], Some(gem));
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    // ── Swap tests ──────────────────────────────────────────────────

    #[test]
    fn test_swap_creates_match_detection() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Set up so swapping (0,2) and (0,3) creates a 3-match.
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(1));
        game.board[0][3] = Some(make_gem(0));

        let a = Pos::new(0, 2);
        let b = Pos::new(0, 3);
        assert!(game.swap_creates_match(a, b));
    }

    #[test]
    fn test_swap_no_match() {
        let mut game = empty_game();
        fill_no_matches(&mut game);

        // Two different gems, swapping won't create a match.
        let a = Pos::new(3, 3);
        let b = Pos::new(3, 4);
        // May or may not create a match depending on fill_no_matches pattern.
        // Just verify it doesn't panic.
        let _ = game.swap_creates_match(a, b);
    }

    #[test]
    fn test_try_swap_non_adjacent() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(0, 0), Pos::new(0, 2));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_out_of_bounds() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(0, 0), Pos::new(8, 0));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_same_position() {
        let mut game = Match3::new();
        let result = game.try_swap(Pos::new(3, 3), Pos::new(3, 3));
        assert!(!result);
    }

    #[test]
    fn test_try_swap_valid_match() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Set up guaranteed match.
        game.board[4][0] = Some(make_gem(5));
        game.board[4][1] = Some(make_gem(5));
        game.board[4][2] = Some(make_gem(3));
        game.board[4][3] = Some(make_gem(5));

        let old_score = game.score;
        let result = game.try_swap(Pos::new(4, 2), Pos::new(4, 3));
        assert!(result);
        assert!(game.score > old_score);
    }

    // ── Game mode tests ─────────────────────────────────────────────

    #[test]
    fn test_classic_mode_label() {
        assert_eq!(GameMode::Classic.label(), "Classic");
    }

    #[test]
    fn test_timed_mode_label() {
        assert_eq!(GameMode::Timed.label(), "Timed");
    }

    #[test]
    fn test_moves_mode_label() {
        assert_eq!(GameMode::Moves.label(), "Moves");
    }

    #[test]
    fn test_initial_mode_classic() {
        let game = Match3::new();
        assert_eq!(game.mode, GameMode::Classic);
    }

    #[test]
    fn test_switch_mode() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        assert_eq!(game.mode, GameMode::Timed);
        assert_eq!(game.time_remaining_ms, TIMED_MODE_SECONDS * 1000);
    }

    #[test]
    fn test_switch_mode_moves() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        assert_eq!(game.mode, GameMode::Moves);
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT);
    }

    // ── Game state tests ────────────────────────────────────────────

    #[test]
    fn test_initial_state_idle() {
        let game = Match3::new();
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_initial_score_zero() {
        let game = Match3::new();
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_end_game_updates_state() {
        let mut game = Match3::new();
        game.score = 500;
        game.end_game();
        assert_eq!(game.state, GameState::GameOver);
    }

    #[test]
    fn test_end_game_updates_high_score() {
        let mut game = Match3::new();
        game.score = 500;
        game.end_game();
        assert_eq!(game.high_scores.get(GameMode::Classic), 500);
    }

    #[test]
    fn test_high_score_preserved_across_games() {
        let mut game = Match3::new();
        game.score = 1000;
        game.end_game();
        game.new_game();
        assert_eq!(game.high_scores.get(GameMode::Classic), 1000);
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_high_score_per_mode() {
        let mut game = Match3::new();
        game.mode = GameMode::Classic;
        game.score = 100;
        game.end_game();

        game.new_game();
        game.mode = GameMode::Timed;
        game.score = 200;
        game.end_game();

        assert_eq!(game.high_scores.get(GameMode::Classic), 100);
        assert_eq!(game.high_scores.get(GameMode::Timed), 200);
        assert_eq!(game.high_scores.get(GameMode::Moves), 0);
    }

    // ── New game tests ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_score() {
        let mut game = Match3::new();
        game.score = 500;
        game.new_game();
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_new_game_resets_state() {
        let mut game = Match3::new();
        game.state = GameState::GameOver;
        game.new_game();
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_new_game_refills_board() {
        let mut game = Match3::new();
        game.clear_board();
        game.new_game();
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
    }

    #[test]
    fn test_new_game_preserves_mode() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.new_game();
        assert_eq!(game.mode, GameMode::Moves);
    }

    // ── Selection and cursor tests ──────────────────────────────────

    #[test]
    fn test_select_gem() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(3, 3));
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(3, 3)));
    }

    #[test]
    fn test_deselect_same_gem() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(3, 3));
        game.select_or_swap(Pos::new(3, 3));
        assert_eq!(game.state, GameState::Idle);
        assert_eq!(game.selected, None);
    }

    #[test]
    fn test_select_different_non_adjacent() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(0, 0));
        game.select_or_swap(Pos::new(5, 5));
        // Should reselect the new gem.
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(5, 5)));
    }

    #[test]
    fn test_cursor_move_right() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Right);
        assert_eq!(game.cursor.col, 1);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Down);
        assert_eq!(game.cursor.row, 1);
    }

    #[test]
    fn test_cursor_bounded_left() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Left);
        assert_eq!(game.cursor.col, 0);
    }

    #[test]
    fn test_cursor_bounded_up() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, 0);
        game.handle_key(Key::Up);
        assert_eq!(game.cursor.row, 0);
    }

    #[test]
    fn test_cursor_bounded_right() {
        let mut game = Match3::new();
        game.cursor = Pos::new(0, GRID_SIZE - 1);
        game.handle_key(Key::Right);
        assert_eq!(game.cursor.col, GRID_SIZE - 1);
    }

    #[test]
    fn test_cursor_bounded_down() {
        let mut game = Match3::new();
        game.cursor = Pos::new(GRID_SIZE - 1, 0);
        game.handle_key(Key::Down);
        assert_eq!(game.cursor.row, GRID_SIZE - 1);
    }

    // ── Keyboard event tests ────────────────────────────────────────

    #[test]
    fn test_key_n_new_game() {
        let mut game = Match3::new();
        game.score = 999;
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('n'),
        }));
        assert_eq!(game.score, 0);
    }

    #[test]
    fn test_key_1_classic_mode() {
        let mut game = Match3::new();
        game.mode = GameMode::Timed;
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('1'),
        }));
        assert_eq!(game.mode, GameMode::Classic);
    }

    #[test]
    fn test_key_2_timed_mode() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('2'),
        }));
        assert_eq!(game.mode, GameMode::Timed);
    }

    #[test]
    fn test_key_3_moves_mode() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('3'),
        }));
        assert_eq!(game.mode, GameMode::Moves);
    }

    #[test]
    fn test_key_escape_deselects() {
        let mut game = Match3::new();
        game.select_or_swap(Pos::new(2, 2));
        assert_eq!(game.state, GameState::Selected);
        game.handle_key(Key::Escape);
        assert_eq!(game.state, GameState::Idle);
        assert_eq!(game.selected, None);
    }

    #[test]
    fn test_key_released_ignored() {
        let mut game = Match3::new();
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::N,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        // Should not create a new game (key release ignored).
        // Can't easily verify no-op, but at least no panic.
    }

    // ── Mouse event tests ───────────────────────────────────────────

    #[test]
    fn test_mouse_click_selects() {
        let mut game = Match3::new();
        let (cx, cy) = Match3::cell_center(Pos::new(2, 3));
        game.handle_event(&Event::Mouse(MouseEvent {
            x: cx,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        assert_eq!(game.state, GameState::Selected);
        assert_eq!(game.selected, Some(Pos::new(2, 3)));
    }

    #[test]
    fn test_mouse_click_outside_grid() {
        let mut game = Match3::new();
        game.handle_event(&Event::Mouse(MouseEvent {
            x: 0.0,
            y: 0.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        }));
        // Clicking outside the grid should not select anything.
        assert_eq!(game.state, GameState::Idle);
    }

    #[test]
    fn test_mouse_right_click_ignored() {
        let mut game = Match3::new();
        let (cx, cy) = Match3::cell_center(Pos::new(2, 3));
        game.handle_event(&Event::Mouse(MouseEvent {
            x: cx,
            y: cy,
            kind: MouseEventKind::Press(MouseButton::Right),
        }));
        assert_eq!(game.state, GameState::Idle);
    }

    // ── Hint system tests ───────────────────────────────────────────

    #[test]
    fn test_hint_not_visible_initially() {
        let game = Match3::new();
        assert!(!game.hint_visible);
    }

    #[test]
    fn test_hint_appears_after_delay() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS);
        assert!(game.hint_visible);
    }

    #[test]
    fn test_hint_not_visible_before_delay() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS - 1);
        assert!(!game.hint_visible);
    }

    #[test]
    fn test_hint_reset_on_action() {
        let mut game = Match3::new();
        game.update_hint(HINT_DELAY_MS);
        assert!(game.hint_visible);
        game.reset_idle();
        assert!(!game.hint_visible);
        assert_eq!(game.idle_ms, 0);
    }

    #[test]
    fn test_key_h_shows_hint() {
        let mut game = Match3::new();
        game.handle_key(Key::H);
        assert!(game.hint_visible);
    }

    // ── Timed mode tests ────────────────────────────────────────────

    #[test]
    fn test_timed_mode_countdown() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        let initial = game.time_remaining_ms;
        game.handle_tick(1000);
        assert_eq!(game.time_remaining_ms, initial - 1000);
    }

    #[test]
    fn test_timed_mode_game_over() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        game.handle_tick(TIMED_MODE_SECONDS * 1000 + 1);
        assert_eq!(game.state, GameState::GameOver);
        assert_eq!(game.time_remaining_ms, 0);
    }

    #[test]
    fn test_timed_mode_not_game_over_early() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Timed);
        game.handle_tick(1000);
        assert_eq!(game.state, GameState::Idle);
    }

    // ── Moves mode tests ────────────────────────────────────────────

    #[test]
    fn test_moves_mode_initial_count() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT);
    }

    #[test]
    fn test_moves_consume_move() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.consume_move();
        assert_eq!(game.moves_remaining, MOVES_MODE_COUNT - 1);
    }

    #[test]
    fn test_moves_game_over_at_zero() {
        let mut game = Match3::new();
        game.switch_mode(GameMode::Moves);
        game.moves_remaining = 1;
        game.consume_move();
        assert_eq!(game.state, GameState::GameOver);
    }

    #[test]
    fn test_classic_mode_no_move_consume() {
        let mut game = Match3::new();
        game.mode = GameMode::Classic;
        game.moves_remaining = 30;
        game.consume_move();
        // Classic mode does not decrement moves.
        assert_eq!(game.moves_remaining, 30);
    }

    // ── Special gem tests ───────────────────────────────────────────

    #[test]
    fn test_special_gem_4_match_horizontal() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
            ],
            horizontal: true,
            length: 4,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::LineClearH);
    }

    #[test]
    fn test_special_gem_4_match_vertical() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(1, 0),
                Pos::new(2, 0),
                Pos::new(3, 0),
            ],
            horizontal: false,
            length: 4,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::LineClearV);
    }

    #[test]
    fn test_special_gem_5_match_color_bomb() {
        let m = MatchInfo {
            positions: vec![
                Pos::new(0, 0),
                Pos::new(0, 1),
                Pos::new(0, 2),
                Pos::new(0, 3),
                Pos::new(0, 4),
            ],
            horizontal: true,
            length: 5,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_some());
        let (_, special) = result.unwrap();
        assert_eq!(special, SpecialKind::ColorBomb);
    }

    #[test]
    fn test_no_special_gem_3_match() {
        let m = MatchInfo {
            positions: vec![Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 2)],
            horizontal: true,
            length: 3,
        };
        let game = Match3::new();
        let result = game.determine_special_gem(&m);
        assert!(result.is_none());
    }

    // ── Process matches tests ───────────────────────────────────────

    #[test]
    fn test_process_matches_removes_gems() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));

        let had_match = game.process_matches();
        assert!(had_match);
        // The matched gems should be removed.
        assert!(
            game.board[0][0].is_none() || game.board[0][1].is_none() || game.board[0][2].is_none()
        );
    }

    #[test]
    fn test_process_matches_awards_score() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[0][0] = Some(make_gem(0));
        game.board[0][1] = Some(make_gem(0));
        game.board[0][2] = Some(make_gem(0));

        game.process_matches();
        assert!(game.score > 0);
    }

    #[test]
    fn test_process_matches_none() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        let had_match = game.process_matches();
        assert!(!had_match);
    }

    // ── Pixel to grid tests ─────────────────────────────────────────

    #[test]
    fn test_pixel_to_grid_origin() {
        let ox = Match3::grid_origin_x();
        let oy = Match3::grid_origin_y();
        let pos = Match3::pixel_to_grid(ox + 1.0, oy + 1.0);
        assert_eq!(pos, Some(Pos::new(0, 0)));
    }

    #[test]
    fn test_pixel_to_grid_last_cell() {
        let ox = Match3::grid_origin_x();
        let oy = Match3::grid_origin_y();
        let x = ox + (GRID_SIZE - 1) as f32 * (CELL_SIZE + CELL_GAP) + 1.0;
        let y = oy + (GRID_SIZE - 1) as f32 * (CELL_SIZE + CELL_GAP) + 1.0;
        let pos = Match3::pixel_to_grid(x, y);
        assert_eq!(pos, Some(Pos::new(GRID_SIZE - 1, GRID_SIZE - 1)));
    }

    #[test]
    fn test_pixel_to_grid_outside() {
        assert_eq!(Match3::pixel_to_grid(0.0, 0.0), None);
    }

    #[test]
    fn test_pixel_to_grid_negative() {
        assert_eq!(Match3::pixel_to_grid(-10.0, -10.0), None);
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let game = Match3::new();
        let cmds = game.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut game = Match3::new();
        game.end_game();
        let cmds = game.render();
        // Should have more commands than an idle game (overlay).
        let idle_game = Match3::new();
        let idle_cmds = idle_game.render();
        assert!(cmds.len() > idle_cmds.len());
    }

    // ── HighScores tests ────────────────────────────────────────────

    #[test]
    fn test_high_scores_initial() {
        let hs = HighScores::new();
        assert_eq!(hs.get(GameMode::Classic), 0);
        assert_eq!(hs.get(GameMode::Timed), 0);
        assert_eq!(hs.get(GameMode::Moves), 0);
    }

    #[test]
    fn test_high_scores_update() {
        let mut hs = HighScores::new();
        hs.update(GameMode::Classic, 100);
        assert_eq!(hs.get(GameMode::Classic), 100);
    }

    #[test]
    fn test_high_scores_only_higher() {
        let mut hs = HighScores::new();
        hs.update(GameMode::Classic, 100);
        hs.update(GameMode::Classic, 50);
        assert_eq!(hs.get(GameMode::Classic), 100);
    }

    // ── Layout math tests ───────────────────────────────────────────

    #[test]
    fn test_grid_dimensions() {
        let w = Match3::grid_width();
        let h = Match3::grid_height();
        assert!(w > 0.0);
        assert_eq!(w, h); // Square grid.
    }

    #[test]
    fn test_window_dimensions() {
        let w = Match3::window_width();
        let h = Match3::window_height();
        assert!(w > 0.0);
        assert!(h > w); // Window is taller than wide (header + footer).
    }

    #[test]
    fn test_cell_center_in_bounds() {
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let (cx, cy) = Match3::cell_center(Pos::new(row, col));
                assert!(cx >= 0.0);
                assert!(cy >= 0.0);
                assert!(cx < Match3::window_width());
                assert!(cy < Match3::window_height());
            }
        }
    }

    #[test]
    fn test_cell_origin_sequential() {
        // Each cell origin should be to the right/below the previous.
        for col in 0..GRID_SIZE - 1 {
            let (x1, _) = Match3::cell_origin(Pos::new(0, col));
            let (x2, _) = Match3::cell_origin(Pos::new(0, col + 1));
            assert!(x2 > x1);
        }
        for row in 0..GRID_SIZE - 1 {
            let (_, y1) = Match3::cell_origin(Pos::new(row, 0));
            let (_, y2) = Match3::cell_origin(Pos::new(row + 1, 0));
            assert!(y2 > y1);
        }
    }

    // ── Tick tests ──────────────────────────────────────────────────

    #[test]
    fn test_tick_increments_total_elapsed() {
        let mut game = Match3::new();
        game.handle_tick(100);
        assert_eq!(game.total_elapsed_ms, 100);
        game.handle_tick(200);
        assert_eq!(game.total_elapsed_ms, 300);
    }

    #[test]
    fn test_tick_increments_pulse() {
        let mut game = Match3::new();
        let initial = game.pulse_counter;
        game.handle_tick(16);
        assert_eq!(game.pulse_counter, initial + 1);
    }

    // ── Shuffle tests ───────────────────────────────────────────────

    #[test]
    fn test_shuffle_produces_valid_moves() {
        let mut game = Match3::new();
        game.shuffle_board();
        assert!(game.has_valid_moves());
    }

    #[test]
    fn test_shuffle_preserves_gem_count() {
        let mut game = Match3::new();
        let count_before = game.gem_count();
        game.shuffle_board();
        assert_eq!(game.gem_count(), count_before);
    }

    // ── Gem equality tests ──────────────────────────────────────────

    #[test]
    fn test_gem_equality() {
        let a = make_gem(0);
        let b = make_gem(0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_gem_inequality() {
        let a = make_gem(0);
        let b = make_gem(1);
        assert_ne!(a, b);
    }

    #[test]
    fn test_gem_special_inequality() {
        let a = make_gem(0);
        let b = make_special_gem(0, SpecialKind::LineClearH);
        assert_ne!(a, b);
    }

    // ── Run cascade tests ───────────────────────────────────────────

    #[test]
    fn test_run_cascade_clears_matches() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        game.board[7][0] = Some(make_gem(0));
        game.board[7][1] = Some(make_gem(0));
        game.board[7][2] = Some(make_gem(0));

        game.run_cascade();
        // After cascade, board should be full (filled empty spaces).
        assert_eq!(game.gem_count(), GRID_SIZE * GRID_SIZE);
        assert!(game.score > 0);
    }

    #[test]
    fn test_run_cascade_resets_chain_level() {
        let mut game = Match3::new();
        game.chain_level = 5;
        game.run_cascade();
        assert_eq!(game.chain_level, 0);
    }

    // ── Color bomb swap tests ───────────────────────────────────────

    #[test]
    fn test_color_bomb_swap_clears_color() {
        let mut game = empty_game();
        fill_no_matches(&mut game);
        // Place a color bomb and a regular gem adjacent to it.
        game.board[3][3] = Some(make_special_gem(0, SpecialKind::ColorBomb));
        game.board[3][4] = Some(make_gem(2));

        // Place more gems of type 2 around the board.
        game.board[0][0] = Some(make_gem(2));
        game.board[5][5] = Some(make_gem(2));

        let old_score = game.score;
        let result = game.try_swap(Pos::new(3, 3), Pos::new(3, 4));
        assert!(result);
        assert!(game.score > old_score);
        // The color bomb should be gone.
        // Other gems of type 2 should be cleared.
        assert!(game.board[0][0].map_or(true, |g| g.gem_type != GemType::Emerald));
    }

    // ── Comprehensive integration test ──────────────────────────────

    #[test]
    fn test_full_game_loop() {
        let mut game = Match3::with_seed(42);
        // Play through a few moves with keyboard.
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        game.handle_event(&Event::Key(KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        // Tick forward.
        game.handle_event(&Event::Tick { elapsed_ms: 100 });
        // Render should not panic.
        let cmds = game.render();
        assert!(!cmds.is_empty());
    }
}
