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

//! OurOS Sudoku — classic 9x9 number puzzle game.
//!
//! Features three difficulty levels (Easy, Medium, Hard), a backtracking
//! solver for puzzle generation and hint delivery, pencil marks / notes mode,
//! undo / redo history, a game timer, conflict highlighting, and statistics
//! tracking (games completed, best times). Uses a deterministic seeded LCG
//! random number generator (no external `rand` crate). The Catppuccin Mocha
//! color palette provides a pleasant dark theme.

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
const SURFACE2: Color = Color::from_hex(0x585B70);
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

// ── Layout constants ────────────────────────────────────────────────
const GRID_SIZE: usize = 9;
const BOX_SIZE: usize = 3;
const TOTAL_CELLS: usize = GRID_SIZE * GRID_SIZE;

const CELL_SIZE: f32 = 52.0;
const CELL_GAP: f32 = 2.0;
const BOX_GAP: f32 = 4.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 60.0;
const FOOTER_HEIGHT: f32 = 44.0;
const CELL_CORNER_RADIUS: f32 = 3.0;

const HEADER_FONT_SIZE: f32 = 20.0;
const CELL_FONT_SIZE: f32 = 24.0;
const NOTE_FONT_SIZE: f32 = 10.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const LABEL_FONT_SIZE: f32 = 13.0;

const MAX_HINTS: usize = 5;
const MAX_UNDO: usize = 500;

// ── LCG random number generator ────────────────────────────────────
/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        // Ensure the seed is nonzero so the generator is not stuck at 0.
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

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    /// Number of given (pre-filled) cells for this difficulty.
    /// Easy: 35-40, Medium: 28-34, Hard: 22-27.
    fn givens_range(self) -> (usize, usize) {
        match self {
            Self::Easy => (35, 40),
            Self::Medium => (28, 34),
            Self::Hard => (22, 27),
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
}

// ── Cell representation ─────────────────────────────────────────────
#[derive(Clone, Debug)]
struct Cell {
    /// The current value (0 = empty, 1-9 = digit).
    value: u8,
    /// Whether this cell was given at puzzle start (immutable).
    given: bool,
    /// Pencil marks / candidate notes (indices 0-8 represent digits 1-9).
    notes: [bool; 9],
}

impl Cell {
    fn empty() -> Self {
        Self {
            value: 0,
            given: false,
            notes: [false; 9],
        }
    }

    fn with_value(value: u8) -> Self {
        Self {
            value,
            given: false,
            notes: [false; 9],
        }
    }

    fn as_given(value: u8) -> Self {
        Self {
            value,
            given: true,
            notes: [false; 9],
        }
    }

    fn is_empty(&self) -> bool {
        self.value == 0
    }

    fn clear_notes(&mut self) {
        self.notes = [false; 9];
    }

    fn has_any_note(&self) -> bool {
        self.notes.iter().any(|&n| n)
    }

    fn toggle_note(&mut self, digit: u8) {
        if digit >= 1 && digit <= 9 {
            let idx = (digit - 1) as usize;
            self.notes[idx] = !self.notes[idx];
        }
    }

    fn has_note(&self, digit: u8) -> bool {
        if digit >= 1 && digit <= 9 {
            self.notes[(digit - 1) as usize]
        } else {
            false
        }
    }
}

// ── Undo / redo ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
enum Action {
    SetValue {
        row: usize,
        col: usize,
        old_value: u8,
        new_value: u8,
        old_notes: [bool; 9],
    },
    ToggleNote {
        row: usize,
        col: usize,
        digit: u8,
    },
    Hint {
        row: usize,
        col: usize,
        old_value: u8,
        new_value: u8,
        old_notes: [bool; 9],
    },
}

// ── Statistics ──────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
struct Stats {
    games_completed_easy: u32,
    games_completed_medium: u32,
    games_completed_hard: u32,
    best_time_easy: Option<u64>,
    best_time_medium: Option<u64>,
    best_time_hard: Option<u64>,
}

impl Stats {
    fn games_completed(&self, difficulty: Difficulty) -> u32 {
        match difficulty {
            Difficulty::Easy => self.games_completed_easy,
            Difficulty::Medium => self.games_completed_medium,
            Difficulty::Hard => self.games_completed_hard,
        }
    }

    fn best_time(&self, difficulty: Difficulty) -> Option<u64> {
        match difficulty {
            Difficulty::Easy => self.best_time_easy,
            Difficulty::Medium => self.best_time_medium,
            Difficulty::Hard => self.best_time_hard,
        }
    }

    fn record_completion(&mut self, difficulty: Difficulty, elapsed_secs: u64) {
        match difficulty {
            Difficulty::Easy => {
                self.games_completed_easy += 1;
                self.best_time_easy = Some(match self.best_time_easy {
                    Some(prev) if prev <= elapsed_secs => prev,
                    _ => elapsed_secs,
                });
            }
            Difficulty::Medium => {
                self.games_completed_medium += 1;
                self.best_time_medium = Some(match self.best_time_medium {
                    Some(prev) if prev <= elapsed_secs => prev,
                    _ => elapsed_secs,
                });
            }
            Difficulty::Hard => {
                self.games_completed_hard += 1;
                self.best_time_hard = Some(match self.best_time_hard {
                    Some(prev) if prev <= elapsed_secs => prev,
                    _ => elapsed_secs,
                });
            }
        }
    }

    fn total_completed(&self) -> u32 {
        self.games_completed_easy + self.games_completed_medium + self.games_completed_hard
    }
}

// ── Grid utilities ──────────────────────────────────────────────────

/// Convert (row, col) to a flat index.
fn idx(row: usize, col: usize) -> usize {
    row * GRID_SIZE + col
}

/// Convert flat index to (row, col).
fn row_col(index: usize) -> (usize, usize) {
    (index / GRID_SIZE, index % GRID_SIZE)
}

/// Return the top-left corner (row, col) of the 3x3 box containing (row, col).
fn box_origin(row: usize, col: usize) -> (usize, usize) {
    ((row / BOX_SIZE) * BOX_SIZE, (col / BOX_SIZE) * BOX_SIZE)
}

/// Check if placing `digit` at `(row, col)` conflicts with any existing value
/// in the same row, column, or 3x3 box.
fn has_conflict(grid: &[u8; TOTAL_CELLS], row: usize, col: usize, digit: u8) -> bool {
    if digit == 0 {
        return false;
    }

    // Row check
    for c in 0..GRID_SIZE {
        if c != col && grid[idx(row, c)] == digit {
            return true;
        }
    }

    // Column check
    for r in 0..GRID_SIZE {
        if r != row && grid[idx(r, col)] == digit {
            return true;
        }
    }

    // Box check
    let (br, bc) = box_origin(row, col);
    for r in br..br + BOX_SIZE {
        for c in bc..bc + BOX_SIZE {
            if (r, c) != (row, col) && grid[idx(r, c)] == digit {
                return true;
            }
        }
    }

    false
}

/// Return the set of conflicting cell positions for a given cell.
fn find_conflicts(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> Vec<(usize, usize)> {
    let digit = grid[idx(row, col)];
    if digit == 0 {
        return Vec::new();
    }

    let mut conflicts = Vec::new();

    // Row
    for c in 0..GRID_SIZE {
        if c != col && grid[idx(row, c)] == digit {
            conflicts.push((row, c));
        }
    }

    // Column
    for r in 0..GRID_SIZE {
        if r != row && grid[idx(r, col)] == digit {
            conflicts.push((r, col));
        }
    }

    // Box
    let (br, bc) = box_origin(row, col);
    for r in br..br + BOX_SIZE {
        for c in bc..bc + BOX_SIZE {
            if (r, c) != (row, col) && grid[idx(r, c)] == digit {
                // Avoid duplicates if cell is also in the same row/col
                if !conflicts.contains(&(r, c)) {
                    conflicts.push((r, c));
                }
            }
        }
    }

    conflicts
}

/// Check whether the grid is completely filled and valid.
fn is_grid_complete(grid: &[u8; TOTAL_CELLS]) -> bool {
    for i in 0..TOTAL_CELLS {
        if grid[i] == 0 {
            return false;
        }
    }
    // Verify no conflicts
    for r in 0..GRID_SIZE {
        for c in 0..GRID_SIZE {
            if has_conflict(grid, r, c, grid[idx(r, c)]) {
                return false;
            }
        }
    }
    true
}

/// Check whether every filled cell has no conflicts (partial validity).
fn is_grid_valid(grid: &[u8; TOTAL_CELLS]) -> bool {
    for r in 0..GRID_SIZE {
        for c in 0..GRID_SIZE {
            let v = grid[idx(r, c)];
            if v != 0 && has_conflict(grid, r, c, v) {
                return false;
            }
        }
    }
    true
}

/// Extract a flat u8 array of cell values from the Cell array.
fn values_array(cells: &[Cell; TOTAL_CELLS]) -> [u8; TOTAL_CELLS] {
    let mut arr = [0u8; TOTAL_CELLS];
    for (i, cell) in cells.iter().enumerate() {
        arr[i] = cell.value;
    }
    arr
}

// ── Solver (backtracking) ───────────────────────────────────────────

/// Find the first empty cell (value == 0) in the grid, scanning left-to-right,
/// top-to-bottom.
fn find_empty(grid: &[u8; TOTAL_CELLS]) -> Option<(usize, usize)> {
    for r in 0..GRID_SIZE {
        for c in 0..GRID_SIZE {
            if grid[idx(r, c)] == 0 {
                return Some((r, c));
            }
        }
    }
    None
}

/// Compute which digits are candidates for a cell using bitmask elimination.
fn candidates(grid: &[u8; TOTAL_CELLS], row: usize, col: usize) -> u16 {
    let mut mask: u16 = 0x1FF; // bits 0-8 represent digits 1-9

    // Eliminate row
    for c in 0..GRID_SIZE {
        let v = grid[idx(row, c)];
        if v >= 1 && v <= 9 {
            mask &= !(1 << (v - 1));
        }
    }

    // Eliminate column
    for r in 0..GRID_SIZE {
        let v = grid[idx(r, col)];
        if v >= 1 && v <= 9 {
            mask &= !(1 << (v - 1));
        }
    }

    // Eliminate box
    let (br, bc) = box_origin(row, col);
    for r in br..br + BOX_SIZE {
        for c in bc..bc + BOX_SIZE {
            let v = grid[idx(r, c)];
            if v >= 1 && v <= 9 {
                mask &= !(1 << (v - 1));
            }
        }
    }

    mask
}

/// Solve the grid using backtracking. Returns true if a solution is found.
/// The grid is modified in place with the solution.
fn solve(grid: &mut [u8; TOTAL_CELLS]) -> bool {
    let cell = find_empty(grid);
    let (row, col) = match cell {
        Some(rc) => rc,
        None => return true, // All cells filled — solved
    };

    let cands = candidates(grid, row, col);
    for digit in 1..=9u8 {
        if cands & (1 << (digit - 1)) != 0 {
            grid[idx(row, col)] = digit;
            if solve(grid) {
                return true;
            }
            grid[idx(row, col)] = 0;
        }
    }

    false
}

/// Count the number of solutions (up to `limit`). Returns the count and stops
/// early once `limit` is reached.
fn count_solutions(grid: &mut [u8; TOTAL_CELLS], limit: usize) -> usize {
    count_solutions_inner(grid, limit, 0)
}

fn count_solutions_inner(
    grid: &mut [u8; TOTAL_CELLS],
    limit: usize,
    found: usize,
) -> usize {
    if found >= limit {
        return found;
    }

    let cell = find_empty(grid);
    let (row, col) = match cell {
        Some(rc) => rc,
        None => return found + 1, // Complete valid grid counts as one solution
    };

    let cands = candidates(grid, row, col);
    let mut total = found;
    for digit in 1..=9u8 {
        if total >= limit {
            break;
        }
        if cands & (1 << (digit - 1)) != 0 {
            grid[idx(row, col)] = digit;
            total = count_solutions_inner(grid, limit, total);
            grid[idx(row, col)] = 0;
        }
    }
    total
}

/// Solve with a shuffled digit order (for generation). Returns true if solved.
fn solve_shuffled(grid: &mut [u8; TOTAL_CELLS], rng: &mut Lcg) -> bool {
    let cell = find_empty(grid);
    let (row, col) = match cell {
        Some(rc) => rc,
        None => return true,
    };

    let cands = candidates(grid, row, col);
    let mut digits: Vec<u8> = (1..=9)
        .filter(|&d| cands & (1 << (d - 1)) != 0)
        .collect();
    rng.shuffle(&mut digits);

    for digit in digits {
        grid[idx(row, col)] = digit;
        if solve_shuffled(grid, rng) {
            return true;
        }
        grid[idx(row, col)] = 0;
    }

    false
}

// ── Puzzle generation ───────────────────────────────────────────────

/// Generate a complete valid Sudoku grid.
fn generate_full_grid(rng: &mut Lcg) -> [u8; TOTAL_CELLS] {
    let mut grid = [0u8; TOTAL_CELLS];
    let solved = solve_shuffled(&mut grid, rng);
    // The solver should always succeed on an empty grid.
    debug_assert!(solved, "Failed to generate a complete Sudoku grid");
    if !solved {
        // Fallback: fill with a known valid grid
        let fallback = [
            5, 3, 4, 6, 7, 8, 9, 1, 2, 6, 7, 2, 1, 9, 5, 3, 4, 8, 1, 9, 8, 3, 4, 2, 5, 6, 7,
            8, 5, 9, 7, 6, 1, 4, 2, 3, 4, 2, 6, 8, 5, 3, 7, 9, 1, 7, 1, 3, 9, 2, 4, 8, 5, 6,
            9, 6, 1, 5, 3, 7, 2, 8, 4, 2, 8, 7, 4, 1, 9, 6, 3, 5, 3, 4, 5, 2, 8, 6, 1, 7, 9,
        ];
        grid = fallback;
    }
    grid
}

/// Generate a puzzle by removing cells from a complete grid while ensuring a
/// unique solution. Returns (puzzle, solution).
fn generate_puzzle(
    rng: &mut Lcg,
    difficulty: Difficulty,
) -> ([Cell; TOTAL_CELLS], [u8; TOTAL_CELLS]) {
    let solution = generate_full_grid(rng);

    let (min_givens, max_givens) = difficulty.givens_range();
    let target_givens = min_givens + rng.next_bounded(max_givens - min_givens + 1);
    let target_removals = TOTAL_CELLS - target_givens;

    // Build a shuffled list of cell indices to try removing.
    let mut order: Vec<usize> = (0..TOTAL_CELLS).collect();
    rng.shuffle(&mut order);

    let mut puzzle_values = solution;
    let mut removed = 0usize;

    for &cell_idx in &order {
        if removed >= target_removals {
            break;
        }

        let saved = puzzle_values[cell_idx];
        puzzle_values[cell_idx] = 0;

        // Check unique solution
        let mut test = puzzle_values;
        let count = count_solutions(&mut test, 2);
        if count == 1 {
            removed += 1;
        } else {
            // Restore — removing this cell would create multiple solutions
            puzzle_values[cell_idx] = saved;
        }
    }

    // Build Cell array
    let mut cells: [Cell; TOTAL_CELLS] = core::array::from_fn(|_| Cell::empty());
    for i in 0..TOTAL_CELLS {
        if puzzle_values[i] != 0 {
            cells[i] = Cell::as_given(puzzle_values[i]);
        }
    }

    (cells, solution)
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
    Paused,
}

struct SudokuApp {
    cells: [Cell; TOTAL_CELLS],
    solution: [u8; TOTAL_CELLS],
    difficulty: Difficulty,
    status: GameStatus,

    // Selection
    selected_row: usize,
    selected_col: usize,

    // Mode
    note_mode: bool,

    // History
    undo_stack: Vec<Action>,
    redo_stack: Vec<Action>,

    // Hints
    hints_remaining: usize,

    // Timer
    elapsed_secs: u64,

    // Stats
    stats: Stats,

    // RNG seed counter for new games
    seed_counter: u64,
}

impl SudokuApp {
    fn new() -> Self {
        Self::with_seed_and_difficulty(42, Difficulty::Easy)
    }

    fn with_seed_and_difficulty(seed: u64, difficulty: Difficulty) -> Self {
        let mut rng = Lcg::new(seed);
        let (cells, solution) = generate_puzzle(&mut rng, difficulty);

        Self {
            cells,
            solution,
            difficulty,
            status: GameStatus::Playing,
            selected_row: 4,
            selected_col: 4,
            note_mode: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            hints_remaining: MAX_HINTS,
            elapsed_secs: 0,
            stats: Stats::default(),
            seed_counter: seed,
        }
    }

    /// Start a new game, preserving stats.
    fn new_game(&mut self, difficulty: Difficulty) {
        self.seed_counter = self.seed_counter.wrapping_add(1);
        let mut rng = Lcg::new(self.seed_counter);
        let (cells, solution) = generate_puzzle(&mut rng, difficulty);

        self.cells = cells;
        self.solution = solution;
        self.difficulty = difficulty;
        self.status = GameStatus::Playing;
        self.selected_row = 4;
        self.selected_col = 4;
        self.note_mode = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.hints_remaining = MAX_HINTS;
        self.elapsed_secs = 0;
    }

    // ── Input handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(key_event) if key_event.pressed => {
                self.handle_key(key_event);
            }
            Event::Mouse(mouse_event) => {
                self.handle_mouse(mouse_event);
            }
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key_event: &KeyEvent) {
        if self.status == GameStatus::Won {
            // F2 starts a new game even after winning
            if key_event.key == Key::F2 {
                self.new_game(self.difficulty);
            }
            return;
        }

        // Pause toggle
        if key_event.key == Key::P && key_event.modifiers == Modifiers::NONE {
            self.toggle_pause();
            return;
        }

        if self.status == GameStatus::Paused {
            return;
        }

        match key_event.key {
            // Navigation
            Key::Up => {
                if self.selected_row > 0 {
                    self.selected_row -= 1;
                }
            }
            Key::Down => {
                if self.selected_row < GRID_SIZE - 1 {
                    self.selected_row += 1;
                }
            }
            Key::Left => {
                if self.selected_col > 0 {
                    self.selected_col -= 1;
                }
            }
            Key::Right => {
                if self.selected_col < GRID_SIZE - 1 {
                    self.selected_col += 1;
                }
            }

            // Undo / Redo (before number input so Ctrl combos match first)
            Key::Z if key_event.modifiers.ctrl => self.undo(),
            Key::Y if key_event.modifiers.ctrl => self.redo(),

            // New game shortcuts (Ctrl+1/2/3 must precede bare 1/2/3)
            Key::Num1 if key_event.modifiers.ctrl => self.new_game(Difficulty::Easy),
            Key::Num2 if key_event.modifiers.ctrl => self.new_game(Difficulty::Medium),
            Key::Num3 if key_event.modifiers.ctrl => self.new_game(Difficulty::Hard),
            Key::F2 => self.new_game(self.difficulty),

            // Number input
            Key::Num1 => self.input_digit(1),
            Key::Num2 => self.input_digit(2),
            Key::Num3 => self.input_digit(3),
            Key::Num4 => self.input_digit(4),
            Key::Num5 => self.input_digit(5),
            Key::Num6 => self.input_digit(6),
            Key::Num7 => self.input_digit(7),
            Key::Num8 => self.input_digit(8),
            Key::Num9 => self.input_digit(9),

            // Clear cell
            Key::Delete | Key::Backspace => self.clear_cell(),

            // Note mode toggle
            Key::N if key_event.modifiers == Modifiers::NONE => {
                self.note_mode = !self.note_mode;
            }

            // Hint
            Key::H if key_event.modifiers == Modifiers::NONE => self.use_hint(),

            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse_event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = mouse_event.kind {
            if self.status != GameStatus::Playing {
                return;
            }

            // Calculate which cell was clicked
            let gx = mouse_event.x - PADDING;
            let gy = mouse_event.y - PADDING - HEADER_HEIGHT;

            if gx < 0.0 || gy < 0.0 {
                return;
            }

            // Account for box gaps in the calculation
            let col = pixel_to_grid_coord(gx);
            let row = pixel_to_grid_coord(gy);

            if let (Some(r), Some(c)) = (row, col) {
                self.selected_row = r;
                self.selected_col = c;
            }
        }
    }

    fn handle_tick(&mut self, _elapsed_ms: u64) {
        if self.status == GameStatus::Playing {
            self.elapsed_secs += 1;
        }
    }

    fn toggle_pause(&mut self) {
        self.status = match self.status {
            GameStatus::Playing => GameStatus::Paused,
            GameStatus::Paused => GameStatus::Playing,
            GameStatus::Won => GameStatus::Won,
        };
    }

    // ── Game actions ────────────────────────────────────────────────

    fn input_digit(&mut self, digit: u8) {
        if self.status != GameStatus::Playing {
            return;
        }

        let r = self.selected_row;
        let c = self.selected_col;
        let cell = &self.cells[idx(r, c)];

        if cell.given {
            return;
        }

        if self.note_mode {
            // Toggle pencil mark
            let action = Action::ToggleNote {
                row: r,
                col: c,
                digit,
            };
            self.cells[idx(r, c)].toggle_note(digit);
            self.push_action(action);
        } else {
            // Place value
            let old_value = cell.value;
            let old_notes = cell.notes;

            if old_value == digit {
                return; // Already has this value, no-op
            }

            let action = Action::SetValue {
                row: r,
                col: c,
                old_value,
                new_value: digit,
                old_notes,
            };

            self.cells[idx(r, c)].value = digit;
            self.cells[idx(r, c)].clear_notes();
            self.push_action(action);

            self.check_completion();
        }
    }

    fn clear_cell(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }

        let r = self.selected_row;
        let c = self.selected_col;
        let cell = &self.cells[idx(r, c)];

        if cell.given {
            return;
        }

        let old_value = cell.value;
        let old_notes = cell.notes;

        if old_value == 0 && !cell.has_any_note() {
            return; // Already empty, no-op
        }

        let action = Action::SetValue {
            row: r,
            col: c,
            old_value,
            new_value: 0,
            old_notes,
        };

        self.cells[idx(r, c)].value = 0;
        self.cells[idx(r, c)].clear_notes();
        self.push_action(action);
    }

    fn use_hint(&mut self) {
        if self.status != GameStatus::Playing || self.hints_remaining == 0 {
            return;
        }

        let r = self.selected_row;
        let c = self.selected_col;
        let cell = &self.cells[idx(r, c)];

        if cell.given {
            return;
        }

        let correct_value = self.solution[idx(r, c)];
        if cell.value == correct_value {
            return; // Already correct
        }

        let old_value = cell.value;
        let old_notes = cell.notes;

        let action = Action::Hint {
            row: r,
            col: c,
            old_value,
            new_value: correct_value,
            old_notes,
        };

        self.cells[idx(r, c)].value = correct_value;
        self.cells[idx(r, c)].clear_notes();
        self.cells[idx(r, c)].given = true; // Mark as given so it can't be changed
        self.hints_remaining -= 1;
        self.push_action(action);

        self.check_completion();
    }

    fn push_action(&mut self, action: Action) {
        self.undo_stack.push(action);
        self.redo_stack.clear();
        // Cap undo history
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }

        let action = match self.undo_stack.pop() {
            Some(a) => a,
            None => return,
        };

        match &action {
            Action::SetValue {
                row,
                col,
                old_value,
                old_notes,
                ..
            } => {
                self.cells[idx(*row, *col)].value = *old_value;
                self.cells[idx(*row, *col)].notes = *old_notes;
            }
            Action::ToggleNote { row, col, digit } => {
                self.cells[idx(*row, *col)].toggle_note(*digit);
            }
            Action::Hint {
                row,
                col,
                old_value,
                old_notes,
                ..
            } => {
                self.cells[idx(*row, *col)].value = *old_value;
                self.cells[idx(*row, *col)].notes = *old_notes;
                self.cells[idx(*row, *col)].given = false;
                self.hints_remaining += 1;
            }
        }

        self.redo_stack.push(action);
    }

    fn redo(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }

        let action = match self.redo_stack.pop() {
            Some(a) => a,
            None => return,
        };

        match &action {
            Action::SetValue {
                row,
                col,
                new_value,
                ..
            } => {
                self.cells[idx(*row, *col)].value = *new_value;
                self.cells[idx(*row, *col)].clear_notes();
            }
            Action::ToggleNote { row, col, digit } => {
                self.cells[idx(*row, *col)].toggle_note(*digit);
            }
            Action::Hint {
                row,
                col,
                new_value,
                ..
            } => {
                self.cells[idx(*row, *col)].value = *new_value;
                self.cells[idx(*row, *col)].clear_notes();
                self.cells[idx(*row, *col)].given = true;
                self.hints_remaining -= 1;
            }
        }

        self.undo_stack.push(action);
    }

    fn check_completion(&mut self) {
        let vals = values_array(&self.cells);
        if is_grid_complete(&vals) {
            self.status = GameStatus::Won;
            self.stats
                .record_completion(self.difficulty, self.elapsed_secs);
        }
    }

    // ── Conflict detection ──────────────────────────────────────────

    /// Return a set of all cells that participate in a conflict.
    fn all_conflict_cells(&self) -> Vec<(usize, usize)> {
        let vals = values_array(&self.cells);
        let mut result = Vec::new();

        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if vals[idx(r, c)] != 0 && has_conflict(&vals, r, c, vals[idx(r, c)]) {
                    if !result.contains(&(r, c)) {
                        result.push((r, c));
                    }
                }
            }
        }

        result
    }

    /// Count of filled (non-empty) cells.
    fn filled_count(&self) -> usize {
        self.cells.iter().filter(|c| c.value != 0).count()
    }

    /// Count of given (pre-filled) cells.
    fn given_count(&self) -> usize {
        self.cells.iter().filter(|c| c.given).count()
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let grid_pixel_size = grid_total_size();
        let total_width = grid_pixel_size + PADDING * 2.0;
        let total_height = HEADER_HEIGHT + grid_pixel_size + FOOTER_HEIGHT + PADDING * 2.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: total_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        self.render_header(&mut cmds, total_width);

        // Grid
        self.render_grid(&mut cmds);

        // Footer
        self.render_footer(&mut cmds, total_width, total_height);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, total_width: f32) {
        // Header background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 10.0,
            text: "Sudoku".to_string(),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Difficulty label
        cmds.push(RenderCommand::Text {
            x: PADDING + 80.0,
            y: 13.0,
            text: self.difficulty.label().to_string(),
            color: self.difficulty.color(),
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Timer
        let mins = self.elapsed_secs / 60;
        let secs = self.elapsed_secs % 60;
        let timer_text = format!("{mins:02}:{secs:02}");
        cmds.push(RenderCommand::Text {
            x: total_width - PADDING - 60.0,
            y: 10.0,
            text: timer_text,
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status indicators
        let status_y = 35.0;

        // Note mode indicator
        let note_label = if self.note_mode {
            "Notes: ON"
        } else {
            "Notes: OFF"
        };
        let note_color = if self.note_mode { TEAL } else { OVERLAY0 };
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: status_y,
            text: note_label.to_string(),
            color: note_color,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Hints remaining
        let hints_text = format!("Hints: {}", self.hints_remaining);
        cmds.push(RenderCommand::Text {
            x: PADDING + 100.0,
            y: status_y,
            text: hints_text,
            color: if self.hints_remaining > 0 {
                PEACH
            } else {
                OVERLAY0
            },
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress
        let filled = self.filled_count();
        let progress_text = format!("{filled}/{TOTAL_CELLS}");
        cmds.push(RenderCommand::Text {
            x: total_width - PADDING - 60.0,
            y: status_y,
            text: progress_text,
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Game status text
        match self.status {
            GameStatus::Won => {
                cmds.push(RenderCommand::Text {
                    x: total_width / 2.0 - 60.0,
                    y: status_y,
                    text: "Completed!".to_string(),
                    color: GREEN,
                    font_size: LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameStatus::Paused => {
                cmds.push(RenderCommand::Text {
                    x: total_width / 2.0 - 40.0,
                    y: status_y,
                    text: "Paused".to_string(),
                    color: YELLOW,
                    font_size: LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameStatus::Playing => {}
        }
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let origin_x = PADDING;
        let origin_y = PADDING + HEADER_HEIGHT;
        let conflict_cells = self.all_conflict_cells();

        // Grid background
        let grid_size = grid_total_size();
        cmds.push(RenderCommand::FillRect {
            x: origin_x - 2.0,
            y: origin_y - 2.0,
            width: grid_size + 4.0,
            height: grid_size + 4.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });

        // Cells
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let (cx, cy) = cell_pixel_pos(r, c);
                let px = origin_x + cx;
                let py = origin_y + cy;

                let cell = &self.cells[idx(r, c)];
                let is_selected = r == self.selected_row && c == self.selected_col;
                let is_conflict = conflict_cells.contains(&(r, c));
                let is_same_value = cell.value != 0
                    && self.cells[idx(self.selected_row, self.selected_col)].value == cell.value;
                let is_in_scope = r == self.selected_row
                    || c == self.selected_col
                    || (box_origin(r, c) == box_origin(self.selected_row, self.selected_col));

                // Cell background
                let bg = if is_selected {
                    SURFACE2
                } else if is_conflict {
                    Color::rgba(243, 139, 168, 40) // Red with low alpha
                } else if is_same_value {
                    Color::rgba(137, 180, 250, 30) // Blue with low alpha
                } else if is_in_scope {
                    SURFACE0
                } else {
                    Color::from_hex(0x272838) // Slightly lighter than BASE
                };

                cmds.push(RenderCommand::FillRect {
                    x: px,
                    y: py,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });

                // Selection border
                if is_selected {
                    cmds.push(RenderCommand::StrokeRect {
                        x: px,
                        y: py,
                        width: CELL_SIZE,
                        height: CELL_SIZE,
                        color: BLUE,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                    });
                }

                // Cell content
                if cell.value != 0 {
                    let digit_color = if is_conflict {
                        RED
                    } else if cell.given {
                        TEXT_COLOR
                    } else {
                        BLUE
                    };

                    let weight = if cell.given {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    };

                    cmds.push(RenderCommand::Text {
                        x: px + CELL_SIZE / 2.0 - 7.0,
                        y: py + CELL_SIZE / 2.0 - 10.0,
                        text: cell.value.to_string(),
                        color: digit_color,
                        font_size: CELL_FONT_SIZE,
                        font_weight: weight,
                        max_width: None,
                    });
                } else if cell.has_any_note() {
                    // Render pencil marks in a 3x3 grid within the cell
                    self.render_notes(cmds, px, py, cell);
                }
            }
        }

        // 3x3 box borders (thicker lines)
        for box_r in 0..BOX_SIZE {
            for box_c in 0..BOX_SIZE {
                let r = box_r * BOX_SIZE;
                let c = box_c * BOX_SIZE;
                let (bx, by) = cell_pixel_pos(r, c);
                // Width spans 3 cells + 2 inner gaps
                let box_w = CELL_SIZE * 3.0 + CELL_GAP * 2.0;
                let box_h = CELL_SIZE * 3.0 + CELL_GAP * 2.0;

                cmds.push(RenderCommand::StrokeRect {
                    x: origin_x + bx - 1.0,
                    y: origin_y + by - 1.0,
                    width: box_w + 2.0,
                    height: box_h + 2.0,
                    color: OVERLAY0,
                    line_width: 1.5,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }
    }

    fn render_notes(&self, cmds: &mut Vec<RenderCommand>, cell_x: f32, cell_y: f32, cell: &Cell) {
        let note_cell_w = CELL_SIZE / 3.0;
        let note_cell_h = CELL_SIZE / 3.0;

        for digit in 1..=9u8 {
            if cell.has_note(digit) {
                let note_row = ((digit - 1) / 3) as f32;
                let note_col = ((digit - 1) % 3) as f32;
                let nx = cell_x + note_col * note_cell_w + note_cell_w / 2.0 - 3.0;
                let ny = cell_y + note_row * note_cell_h + note_cell_h / 2.0 - 5.0;

                cmds.push(RenderCommand::Text {
                    x: nx,
                    y: ny,
                    text: digit.to_string(),
                    color: SUBTEXT0,
                    font_size: NOTE_FONT_SIZE,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                });
            }
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>, total_width: f32, total_height: f32) {
        let footer_y = total_height - FOOTER_HEIGHT;

        // Footer background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: footer_y,
            width: total_width,
            height: FOOTER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Key hints
        let hints = "Arrow:Move  1-9:Fill  N:Notes  H:Hint  Del:Clear  Ctrl+Z:Undo  F2:New";
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y + 8.0,
            text: hints.to_string(),
            color: OVERLAY0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(total_width - PADDING * 2.0),
        });

        // Stats line
        let total = self.stats.total_completed();
        let best = self
            .stats
            .best_time(self.difficulty)
            .map_or_else(|| "--:--".to_string(), |t| format!("{:02}:{:02}", t / 60, t % 60));
        let stats_text = format!("Completed: {total}  Best ({}):{best}", self.difficulty.label());
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y + 24.0,
            text: stats_text,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(total_width - PADDING * 2.0),
        });
    }
}

// ── Pixel geometry helpers ──────────────────────────────────────────

/// Total pixel size of the 9x9 grid including gaps between cells and boxes.
fn grid_total_size() -> f32 {
    // 9 cells + 6 inner cell gaps (within boxes) + 2 box gaps
    CELL_SIZE * 9.0 + CELL_GAP * 6.0 + BOX_GAP * 2.0
}

/// Pixel position of the top-left corner of cell (row, col) relative to grid origin.
fn cell_pixel_pos(row: usize, col: usize) -> (f32, f32) {
    let x = col as f32 * CELL_SIZE
        + (col / BOX_SIZE) as f32 * BOX_GAP
        + inner_gaps_before(col) * CELL_GAP;
    let y = row as f32 * CELL_SIZE
        + (row / BOX_SIZE) as f32 * BOX_GAP
        + inner_gaps_before(row) * CELL_GAP;
    (x, y)
}

/// Number of inner (within-box) cell gaps before position `pos` in one axis.
fn inner_gaps_before(pos: usize) -> f32 {
    // Within each box of 3, there are gaps between cells 0-1 and 1-2.
    // Box 0: positions 0,1,2 => 0 gaps before 0, 1 gap before 1, 2 before 2
    // Box 1: positions 3,4,5 => 0 extra inner gaps at box boundary (that's a box gap)
    // So inner gaps = pos - (pos / 3) = the index within all boxes minus box transitions
    if pos == 0 {
        return 0.0;
    }
    // Total gaps before this position = (pos - 1) gaps between consecutive cells.
    // Of those, box boundaries (at 3 and 6) use BOX_GAP not CELL_GAP.
    // Inner gaps = total gaps - box boundary gaps
    let total_gaps = pos;
    let box_boundary_gaps = pos / BOX_SIZE;
    (total_gaps - box_boundary_gaps) as f32
}

/// Simplified cell_pixel_pos using the inner_gaps_before helper properly.
fn cell_pixel_pos_clean(row: usize, col: usize) -> (f32, f32) {
    let x = col as f32 * CELL_SIZE
        + inner_gaps_before(col) * CELL_GAP
        + (col / BOX_SIZE) as f32 * BOX_GAP;
    let y = row as f32 * CELL_SIZE
        + inner_gaps_before(row) * CELL_GAP
        + (row / BOX_SIZE) as f32 * BOX_GAP;
    (x, y)
}

/// Convert a pixel coordinate (relative to grid origin) to a grid coordinate.
/// Returns None if outside the grid.
fn pixel_to_grid_coord(pixel: f32) -> Option<usize> {
    if pixel < 0.0 {
        return None;
    }
    // Try each position and check if the pixel falls within it
    for i in 0..GRID_SIZE {
        let pos = i as f32 * CELL_SIZE
            + inner_gaps_before(i) * CELL_GAP
            + (i / BOX_SIZE) as f32 * BOX_GAP;
        if pixel >= pos && pixel < pos + CELL_SIZE {
            return Some(i);
        }
    }
    None
}

// Override cell_pixel_pos to use the clean version
fn cell_pos(row: usize, col: usize) -> (f32, f32) {
    cell_pixel_pos_clean(row, col)
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() {
    let _app = SudokuApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper: create a known valid complete grid ──────────────────

    fn known_solution() -> [u8; TOTAL_CELLS] {
        [
            5, 3, 4, 6, 7, 8, 9, 1, 2,
            6, 7, 2, 1, 9, 5, 3, 4, 8,
            1, 9, 8, 3, 4, 2, 5, 6, 7,
            8, 5, 9, 7, 6, 1, 4, 2, 3,
            4, 2, 6, 8, 5, 3, 7, 9, 1,
            7, 1, 3, 9, 2, 4, 8, 5, 6,
            9, 6, 1, 5, 3, 7, 2, 8, 4,
            2, 8, 7, 4, 1, 9, 6, 3, 5,
            3, 4, 5, 2, 8, 6, 1, 7, 9,
        ]
    }

    fn make_cells_from_values(values: &[u8; TOTAL_CELLS], givens: &[bool; TOTAL_CELLS]) -> [Cell; TOTAL_CELLS] {
        core::array::from_fn(|i| {
            if givens[i] {
                Cell::as_given(values[i])
            } else if values[i] != 0 {
                Cell::with_value(values[i])
            } else {
                Cell::empty()
            }
        })
    }

    // ── idx / row_col ──────────────────────────────────────────────

    #[test]
    fn test_idx_round_trip() {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let i = idx(r, c);
                assert_eq!(row_col(i), (r, c));
            }
        }
    }

    #[test]
    fn test_idx_values() {
        assert_eq!(idx(0, 0), 0);
        assert_eq!(idx(0, 8), 8);
        assert_eq!(idx(1, 0), 9);
        assert_eq!(idx(8, 8), 80);
    }

    // ── box_origin ─────────────────────────────────────────────────

    #[test]
    fn test_box_origin() {
        assert_eq!(box_origin(0, 0), (0, 0));
        assert_eq!(box_origin(1, 2), (0, 0));
        assert_eq!(box_origin(2, 2), (0, 0));
        assert_eq!(box_origin(3, 0), (3, 0));
        assert_eq!(box_origin(4, 5), (3, 3));
        assert_eq!(box_origin(8, 8), (6, 6));
        assert_eq!(box_origin(7, 3), (6, 3));
    }

    // ── has_conflict ───────────────────────────────────────────────

    #[test]
    fn test_no_conflict_on_valid_grid() {
        let grid = known_solution();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                assert!(
                    !has_conflict(&grid, r, c, grid[idx(r, c)]),
                    "Unexpected conflict at ({r}, {c})"
                );
            }
        }
    }

    #[test]
    fn test_conflict_in_row() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 0)] = 5;
        grid[idx(0, 5)] = 5;
        assert!(has_conflict(&grid, 0, 0, 5));
        assert!(has_conflict(&grid, 0, 5, 5));
    }

    #[test]
    fn test_conflict_in_col() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 3)] = 7;
        grid[idx(6, 3)] = 7;
        assert!(has_conflict(&grid, 0, 3, 7));
        assert!(has_conflict(&grid, 6, 3, 7));
    }

    #[test]
    fn test_conflict_in_box() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 0)] = 3;
        grid[idx(2, 2)] = 3;
        assert!(has_conflict(&grid, 0, 0, 3));
        assert!(has_conflict(&grid, 2, 2, 3));
    }

    #[test]
    fn test_no_conflict_for_empty() {
        let grid = [0u8; TOTAL_CELLS];
        assert!(!has_conflict(&grid, 4, 4, 0));
    }

    #[test]
    fn test_no_conflict_different_digits() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 0)] = 1;
        grid[idx(0, 1)] = 2;
        assert!(!has_conflict(&grid, 0, 0, 1));
        assert!(!has_conflict(&grid, 0, 1, 2));
    }

    // ── find_conflicts ─────────────────────────────────────────────

    #[test]
    fn test_find_conflicts_empty_grid() {
        let grid = [0u8; TOTAL_CELLS];
        assert!(find_conflicts(&grid, 0, 0).is_empty());
    }

    #[test]
    fn test_find_conflicts_row_and_col() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 0)] = 5;
        grid[idx(0, 7)] = 5; // same row
        grid[idx(4, 0)] = 5; // same col
        let conflicts = find_conflicts(&grid, 0, 0);
        assert!(conflicts.contains(&(0, 7)));
        assert!(conflicts.contains(&(4, 0)));
        assert_eq!(conflicts.len(), 2);
    }

    // ── is_grid_complete / is_grid_valid ────────────────────────────

    #[test]
    fn test_complete_valid_grid() {
        let grid = known_solution();
        assert!(is_grid_complete(&grid));
        assert!(is_grid_valid(&grid));
    }

    #[test]
    fn test_incomplete_grid() {
        let mut grid = known_solution();
        grid[idx(4, 4)] = 0;
        assert!(!is_grid_complete(&grid));
        assert!(is_grid_valid(&grid)); // Still valid, just incomplete
    }

    #[test]
    fn test_invalid_grid() {
        let mut grid = known_solution();
        grid[idx(0, 0)] = grid[idx(0, 1)]; // Duplicate in row
        assert!(!is_grid_valid(&grid));
    }

    // ── Solver ─────────────────────────────────────────────────────

    #[test]
    fn test_solve_empty_grid() {
        let mut grid = [0u8; TOTAL_CELLS];
        assert!(solve(&mut grid));
        assert!(is_grid_complete(&grid));
    }

    #[test]
    fn test_solve_partial_grid() {
        let solution = known_solution();
        let mut grid = [0u8; TOTAL_CELLS];
        // Give some cells
        for i in 0..20 {
            grid[i] = solution[i];
        }
        assert!(solve(&mut grid));
        assert!(is_grid_complete(&grid));
    }

    #[test]
    fn test_solve_already_complete() {
        let mut grid = known_solution();
        assert!(solve(&mut grid));
        assert_eq!(grid, known_solution());
    }

    #[test]
    fn test_solve_valid_partial_box() {
        // Test that the solver can extend a valid partial grid (complete top-left box).
        let mut grid2 = [0u8; TOTAL_CELLS];
        grid2[idx(0, 0)] = 1;
        grid2[idx(0, 1)] = 2;
        grid2[idx(0, 2)] = 3;
        grid2[idx(1, 0)] = 4;
        grid2[idx(1, 1)] = 5;
        grid2[idx(1, 2)] = 6;
        grid2[idx(2, 0)] = 7;
        grid2[idx(2, 1)] = 8;
        grid2[idx(2, 2)] = 9;
        // This top-left box is complete and valid. Solver should extend it.
        assert!(solve(&mut grid2));
        assert!(is_grid_complete(&grid2));
    }

    #[test]
    fn test_count_solutions_unique() {
        let solution = known_solution();
        let mut grid = [0u8; TOTAL_CELLS];
        // Remove only a few cells from a known grid
        for i in 0..TOTAL_CELLS {
            grid[i] = solution[i];
        }
        // Remove 5 cells
        grid[idx(0, 0)] = 0;
        grid[idx(1, 1)] = 0;
        grid[idx(2, 2)] = 0;
        grid[idx(3, 3)] = 0;
        grid[idx(4, 4)] = 0;

        let count = count_solutions(&mut grid, 2);
        assert_eq!(count, 1, "Should have exactly one solution");
    }

    #[test]
    fn test_count_solutions_empty_grid_has_many() {
        let mut grid = [0u8; TOTAL_CELLS];
        let count = count_solutions(&mut grid, 3);
        assert!(count >= 2, "Empty grid should have multiple solutions");
    }

    #[test]
    fn test_solve_shuffled_produces_valid() {
        let mut rng = Lcg::new(12345);
        let mut grid = [0u8; TOTAL_CELLS];
        assert!(solve_shuffled(&mut grid, &mut rng));
        assert!(is_grid_complete(&grid));
    }

    // ── Candidates bitmask ─────────────────────────────────────────

    #[test]
    fn test_candidates_empty_grid() {
        let grid = [0u8; TOTAL_CELLS];
        // Every digit is a candidate in an empty grid
        assert_eq!(candidates(&grid, 0, 0), 0x1FF);
    }

    #[test]
    fn test_candidates_eliminates_row() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(0, 1)] = 3; // Eliminates 3 from row 0
        let cands = candidates(&grid, 0, 0);
        assert_eq!(cands & (1 << 2), 0); // Bit for digit 3 is clear
        assert_ne!(cands & (1 << 0), 0); // Digit 1 still available
    }

    #[test]
    fn test_candidates_eliminates_col() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(5, 0)] = 7;
        let cands = candidates(&grid, 0, 0);
        assert_eq!(cands & (1 << 6), 0); // Bit for digit 7 is clear
    }

    #[test]
    fn test_candidates_eliminates_box() {
        let mut grid = [0u8; TOTAL_CELLS];
        grid[idx(1, 1)] = 9;
        let cands = candidates(&grid, 0, 0);
        assert_eq!(cands & (1 << 8), 0); // Bit for digit 9 is clear
    }

    #[test]
    fn test_candidates_complete_grid_no_candidates() {
        let grid = known_solution();
        // For an occupied cell, candidates reflects what's "available" if it
        // were empty, which would be nothing (all 9 digits taken by row/col/box).
        // But the cell itself has a value that also counts in the elimination.
        // Actually, candidates doesn't skip the cell itself, so the digit at
        // this position also gets eliminated. For a complete valid grid,
        // every digit 1-9 appears in each row/col/box, so candidates = 0.
        assert_eq!(candidates(&grid, 0, 0), 0);
    }

    // ── Puzzle generation ──────────────────────────────────────────

    #[test]
    fn test_generate_full_grid_valid() {
        let mut rng = Lcg::new(999);
        let grid = generate_full_grid(&mut rng);
        assert!(is_grid_complete(&grid));
    }

    #[test]
    fn test_generate_full_grid_deterministic() {
        let grid1 = generate_full_grid(&mut Lcg::new(42));
        let grid2 = generate_full_grid(&mut Lcg::new(42));
        assert_eq!(grid1, grid2, "Same seed should produce same grid");
    }

    #[test]
    fn test_generate_full_grid_different_seeds() {
        let grid1 = generate_full_grid(&mut Lcg::new(1));
        let grid2 = generate_full_grid(&mut Lcg::new(2));
        assert_ne!(grid1, grid2, "Different seeds should produce different grids");
    }

    #[test]
    fn test_generate_puzzle_easy_givens() {
        let mut rng = Lcg::new(100);
        let (cells, _solution) = generate_puzzle(&mut rng, Difficulty::Easy);
        let given_count: usize = cells.iter().filter(|c| c.given).count();
        assert!(
            given_count >= 35 && given_count <= 40,
            "Easy should have 35-40 givens, got {given_count}"
        );
    }

    #[test]
    fn test_generate_puzzle_medium_givens() {
        let mut rng = Lcg::new(200);
        let (cells, _solution) = generate_puzzle(&mut rng, Difficulty::Medium);
        let given_count: usize = cells.iter().filter(|c| c.given).count();
        assert!(
            given_count >= 28 && given_count <= 34,
            "Medium should have 28-34 givens, got {given_count}"
        );
    }

    #[test]
    fn test_generate_puzzle_hard_givens() {
        let mut rng = Lcg::new(300);
        let (cells, _solution) = generate_puzzle(&mut rng, Difficulty::Hard);
        let given_count: usize = cells.iter().filter(|c| c.given).count();
        // Hard may have more givens than target if uniqueness constraint prevents removal
        assert!(
            given_count >= 22,
            "Hard should have at least 22 givens, got {given_count}"
        );
    }

    #[test]
    fn test_generate_puzzle_unique_solution() {
        let mut rng = Lcg::new(500);
        let (cells, _solution) = generate_puzzle(&mut rng, Difficulty::Medium);
        let mut puzzle_vals = values_array(&cells);
        let count = count_solutions(&mut puzzle_vals, 2);
        assert_eq!(count, 1, "Generated puzzle must have exactly one solution");
    }

    #[test]
    fn test_generate_puzzle_solution_matches() {
        let mut rng = Lcg::new(600);
        let (cells, solution) = generate_puzzle(&mut rng, Difficulty::Easy);
        // Every given cell must match the solution
        for i in 0..TOTAL_CELLS {
            if cells[i].given {
                assert_eq!(
                    cells[i].value, solution[i],
                    "Given cell at {i} doesn't match solution"
                );
            }
        }
        // The solution itself must be valid
        assert!(is_grid_complete(&solution));
    }

    // ── Cell operations ────────────────────────────────────────────

    #[test]
    fn test_cell_empty() {
        let cell = Cell::empty();
        assert!(cell.is_empty());
        assert!(!cell.given);
        assert!(!cell.has_any_note());
    }

    #[test]
    fn test_cell_with_value() {
        let cell = Cell::with_value(5);
        assert!(!cell.is_empty());
        assert_eq!(cell.value, 5);
        assert!(!cell.given);
    }

    #[test]
    fn test_cell_as_given() {
        let cell = Cell::as_given(3);
        assert_eq!(cell.value, 3);
        assert!(cell.given);
    }

    #[test]
    fn test_cell_notes_toggle() {
        let mut cell = Cell::empty();
        assert!(!cell.has_note(5));
        cell.toggle_note(5);
        assert!(cell.has_note(5));
        assert!(cell.has_any_note());
        cell.toggle_note(5);
        assert!(!cell.has_note(5));
        assert!(!cell.has_any_note());
    }

    #[test]
    fn test_cell_notes_multiple() {
        let mut cell = Cell::empty();
        cell.toggle_note(1);
        cell.toggle_note(5);
        cell.toggle_note(9);
        assert!(cell.has_note(1));
        assert!(!cell.has_note(2));
        assert!(cell.has_note(5));
        assert!(cell.has_note(9));
        assert!(cell.has_any_note());
    }

    #[test]
    fn test_cell_clear_notes() {
        let mut cell = Cell::empty();
        cell.toggle_note(3);
        cell.toggle_note(7);
        cell.clear_notes();
        assert!(!cell.has_any_note());
        assert!(!cell.has_note(3));
        assert!(!cell.has_note(7));
    }

    #[test]
    fn test_cell_note_out_of_range() {
        let mut cell = Cell::empty();
        cell.toggle_note(0); // Out of range
        cell.toggle_note(10); // Out of range
        assert!(!cell.has_any_note());
        assert!(!cell.has_note(0));
        assert!(!cell.has_note(10));
    }

    // ── Difficulty ──────────────────────────────────────────────────

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Easy.label(), "Easy");
        assert_eq!(Difficulty::Medium.label(), "Medium");
        assert_eq!(Difficulty::Hard.label(), "Hard");
    }

    #[test]
    fn test_difficulty_givens_ranges() {
        assert_eq!(Difficulty::Easy.givens_range(), (35, 40));
        assert_eq!(Difficulty::Medium.givens_range(), (28, 34));
        assert_eq!(Difficulty::Hard.givens_range(), (22, 27));
    }

    #[test]
    fn test_difficulty_colors_differ() {
        assert_ne!(Difficulty::Easy.color(), Difficulty::Medium.color());
        assert_ne!(Difficulty::Medium.color(), Difficulty::Hard.color());
    }

    // ── LCG RNG ────────────────────────────────────────────────────

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
        let v1 = rng1.next_u64();
        let v2 = rng2.next_u64();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(42);
        for _ in 0..1000 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    #[test]
    fn test_lcg_shuffle_preserves_elements() {
        let mut rng = Lcg::new(42);
        let mut arr = [0, 1, 2, 3, 4, 5, 6, 7, 8];
        rng.shuffle(&mut arr);
        arr.sort();
        assert_eq!(arr, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_lcg_shuffle_changes_order() {
        let mut rng = Lcg::new(42);
        let original = [0, 1, 2, 3, 4, 5, 6, 7, 8];
        let mut arr = original;
        rng.shuffle(&mut arr);
        assert_ne!(arr, original, "Shuffle should change the order");
    }

    #[test]
    fn test_lcg_zero_seed_handled() {
        let mut rng = Lcg::new(0);
        let val = rng.next_u64();
        assert_ne!(val, 0, "Zero seed should still produce non-zero output");
    }

    #[test]
    fn test_lcg_bounded_zero() {
        let mut rng = Lcg::new(42);
        assert_eq!(rng.next_bounded(0), 0);
    }

    // ── SudokuApp construction ─────────────────────────────────────

    #[test]
    fn test_app_new() {
        let app = SudokuApp::new();
        assert_eq!(app.difficulty, Difficulty::Easy);
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.selected_row, 4);
        assert_eq!(app.selected_col, 4);
        assert!(!app.note_mode);
        assert_eq!(app.hints_remaining, MAX_HINTS);
        assert_eq!(app.elapsed_secs, 0);
    }

    #[test]
    fn test_app_with_difficulty() {
        let app = SudokuApp::with_seed_and_difficulty(99, Difficulty::Hard);
        assert_eq!(app.difficulty, Difficulty::Hard);
        assert_eq!(app.status, GameStatus::Playing);
    }

    // ── Navigation ─────────────────────────────────────────────────

    #[test]
    fn test_arrow_key_navigation() {
        let mut app = SudokuApp::new();
        app.selected_row = 4;
        app.selected_col = 4;

        let up = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&up);
        assert_eq!(app.selected_row, 3);

        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&down);
        assert_eq!(app.selected_row, 4);

        let left = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&left);
        assert_eq!(app.selected_col, 3);

        let right = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&right);
        assert_eq!(app.selected_col, 4);
    }

    #[test]
    fn test_navigation_bounds_top_left() {
        let mut app = SudokuApp::new();
        app.selected_row = 0;
        app.selected_col = 0;

        let up = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&up);
        assert_eq!(app.selected_row, 0);

        let left = Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&left);
        assert_eq!(app.selected_col, 0);
    }

    #[test]
    fn test_navigation_bounds_bottom_right() {
        let mut app = SudokuApp::new();
        app.selected_row = 8;
        app.selected_col = 8;

        let down = Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&down);
        assert_eq!(app.selected_row, 8);

        let right = Event::Key(KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&right);
        assert_eq!(app.selected_col, 8);
    }

    // ── Digit input ────────────────────────────────────────────────

    #[test]
    fn test_input_digit_on_empty_cell() {
        let mut app = SudokuApp::new();
        // Find an empty cell
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let event = Event::Key(KeyEvent {
            key: Key::Num5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.cells[idx(r, c)].value, 5);
    }

    #[test]
    fn test_cannot_modify_given_cell() {
        let mut app = SudokuApp::new();
        // Find a given cell
        let (r, c) = find_given_cell_in_app(&app);
        let original_value = app.cells[idx(r, c)].value;

        app.selected_row = r;
        app.selected_col = c;

        let event = Event::Key(KeyEvent {
            key: Key::Num1,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(
            app.cells[idx(r, c)].value, original_value,
            "Given cell should not change"
        );
    }

    #[test]
    fn test_clear_cell() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        // Place a digit
        app.input_digit(7);
        assert_eq!(app.cells[idx(r, c)].value, 7);

        // Clear it
        let del = Event::Key(KeyEvent {
            key: Key::Delete,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&del);
        assert_eq!(app.cells[idx(r, c)].value, 0);
    }

    #[test]
    fn test_cannot_clear_given_cell() {
        let mut app = SudokuApp::new();
        let (r, c) = find_given_cell_in_app(&app);
        let original = app.cells[idx(r, c)].value;

        app.selected_row = r;
        app.selected_col = c;

        let del = Event::Key(KeyEvent {
            key: Key::Delete,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&del);
        assert_eq!(app.cells[idx(r, c)].value, original);
    }

    // ── Note / pencil mark mode ────────────────────────────────────

    #[test]
    fn test_toggle_note_mode() {
        let mut app = SudokuApp::new();
        assert!(!app.note_mode);

        let toggle = Event::Key(KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&toggle);
        assert!(app.note_mode);

        app.handle_event(&toggle);
        assert!(!app.note_mode);
    }

    #[test]
    fn test_pencil_marks_in_note_mode() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.note_mode = true;

        let event = Event::Key(KeyEvent {
            key: Key::Num3,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert!(app.cells[idx(r, c)].has_note(3));
        assert_eq!(app.cells[idx(r, c)].value, 0, "Note mode should not set value");
    }

    #[test]
    fn test_pencil_marks_toggle_off() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.note_mode = true;

        app.input_digit(4);
        assert!(app.cells[idx(r, c)].has_note(4));

        app.input_digit(4);
        assert!(!app.cells[idx(r, c)].has_note(4));
    }

    #[test]
    fn test_placing_value_clears_notes() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        // Add some notes
        app.note_mode = true;
        app.input_digit(1);
        app.input_digit(5);
        assert!(app.cells[idx(r, c)].has_any_note());

        // Switch to value mode and place a digit
        app.note_mode = false;
        app.input_digit(3);
        assert!(!app.cells[idx(r, c)].has_any_note(), "Notes should be cleared when placing a value");
        assert_eq!(app.cells[idx(r, c)].value, 3);
    }

    // ── Undo / redo ────────────────────────────────────────────────

    #[test]
    fn test_undo_value_placement() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        app.input_digit(5);
        assert_eq!(app.cells[idx(r, c)].value, 5);

        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 0);
    }

    #[test]
    fn test_redo_value_placement() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        app.input_digit(5);
        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 0);

        app.redo();
        assert_eq!(app.cells[idx(r, c)].value, 5);
    }

    #[test]
    fn test_undo_note_toggle() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.note_mode = true;

        app.input_digit(7);
        assert!(app.cells[idx(r, c)].has_note(7));

        app.undo();
        assert!(!app.cells[idx(r, c)].has_note(7));
    }

    #[test]
    fn test_undo_clears_redo_on_new_action() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        app.input_digit(1);
        app.undo();
        assert!(!app.redo_stack.is_empty());

        // New action should clear redo
        app.input_digit(2);
        assert!(app.redo_stack.is_empty());
    }

    #[test]
    fn test_undo_nothing_is_noop() {
        let mut app = SudokuApp::new();
        assert!(app.undo_stack.is_empty());
        app.undo(); // Should not panic
        assert!(app.undo_stack.is_empty());
    }

    #[test]
    fn test_redo_nothing_is_noop() {
        let mut app = SudokuApp::new();
        assert!(app.redo_stack.is_empty());
        app.redo(); // Should not panic
        assert!(app.redo_stack.is_empty());
    }

    #[test]
    fn test_multiple_undo_redo() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        app.input_digit(1);
        app.input_digit(2);
        app.input_digit(3);

        assert_eq!(app.cells[idx(r, c)].value, 3);
        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 2);
        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 1);
        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 0);

        app.redo();
        assert_eq!(app.cells[idx(r, c)].value, 1);
        app.redo();
        assert_eq!(app.cells[idx(r, c)].value, 2);
    }

    // ── Hint system ────────────────────────────────────────────────

    #[test]
    fn test_hint_reveals_correct_value() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let expected = app.solution[idx(r, c)];
        app.use_hint();
        assert_eq!(app.cells[idx(r, c)].value, expected);
        assert!(app.cells[idx(r, c)].given, "Hinted cell should become given");
    }

    #[test]
    fn test_hint_decrements_remaining() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let before = app.hints_remaining;
        app.use_hint();
        assert_eq!(app.hints_remaining, before - 1);
    }

    #[test]
    fn test_hint_limit() {
        let mut app = SudokuApp::new();

        for _ in 0..MAX_HINTS {
            let pos = find_empty_cell_in_app(&app);
            app.selected_row = pos.0;
            app.selected_col = pos.1;
            app.use_hint();
        }

        assert_eq!(app.hints_remaining, 0);

        // Find another empty cell and try to hint
        if let Some((r, c)) = try_find_empty_cell_in_app(&app) {
            app.selected_row = r;
            app.selected_col = c;
            let val_before = app.cells[idx(r, c)].value;
            app.use_hint();
            assert_eq!(
                app.cells[idx(r, c)].value, val_before,
                "No hint should be given when limit reached"
            );
        }
    }

    #[test]
    fn test_hint_on_given_is_noop() {
        let mut app = SudokuApp::new();
        let (r, c) = find_given_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let before = app.hints_remaining;
        app.use_hint();
        assert_eq!(app.hints_remaining, before, "Hint on given should not consume a hint");
    }

    #[test]
    fn test_undo_hint() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let hints_before = app.hints_remaining;
        app.use_hint();
        assert_eq!(app.hints_remaining, hints_before - 1);

        app.undo();
        assert_eq!(app.cells[idx(r, c)].value, 0);
        assert!(!app.cells[idx(r, c)].given);
        assert_eq!(app.hints_remaining, hints_before, "Undo should restore hint count");
    }

    // ── Game completion ────────────────────────────────────────────

    #[test]
    fn test_completion_detection() {
        let mut app = SudokuApp::new();
        // Fill in all empty cells with the solution values
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    app.selected_row = r;
                    app.selected_col = c;
                    app.input_digit(app.solution[idx(r, c)]);
                }
            }
        }
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn test_no_input_after_won() {
        let mut app = SudokuApp::new();
        // Complete the puzzle
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    app.selected_row = r;
                    app.selected_col = c;
                    app.input_digit(app.solution[idx(r, c)]);
                }
            }
        }
        assert_eq!(app.status, GameStatus::Won);

        // Try to navigate (should be no effect since game is won)
        let old_row = app.selected_row;
        let up = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&up);
        assert_eq!(app.selected_row, old_row, "Navigation should be blocked after winning");
    }

    // ── New game ───────────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_state() {
        let mut app = SudokuApp::new();
        app.elapsed_secs = 120;
        app.hints_remaining = 2;
        app.note_mode = true;
        app.input_digit(5);

        app.new_game(Difficulty::Medium);

        assert_eq!(app.difficulty, Difficulty::Medium);
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.elapsed_secs, 0);
        assert_eq!(app.hints_remaining, MAX_HINTS);
        assert!(!app.note_mode);
        assert!(app.undo_stack.is_empty());
        assert!(app.redo_stack.is_empty());
    }

    #[test]
    fn test_f2_starts_new_game() {
        let mut app = SudokuApp::new();
        let _old_cells = app.cells.clone();
        let old_seed = app.seed_counter;

        let f2 = Event::Key(KeyEvent {
            key: Key::F2,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&f2);

        assert_ne!(app.seed_counter, old_seed, "F2 should generate new puzzle");
    }

    // ── Timer ──────────────────────────────────────────────────────

    #[test]
    fn test_timer_increments() {
        let mut app = SudokuApp::new();
        assert_eq!(app.elapsed_secs, 0);

        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(app.elapsed_secs, 1);
    }

    #[test]
    fn test_timer_stops_when_won() {
        let mut app = SudokuApp::new();
        // Complete the puzzle
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    app.selected_row = r;
                    app.selected_col = c;
                    app.input_digit(app.solution[idx(r, c)]);
                }
            }
        }
        assert_eq!(app.status, GameStatus::Won);
        let time = app.elapsed_secs;

        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(app.elapsed_secs, time, "Timer should stop after winning");
    }

    #[test]
    fn test_timer_stops_when_paused() {
        let mut app = SudokuApp::new();
        app.elapsed_secs = 10;
        app.toggle_pause();
        assert_eq!(app.status, GameStatus::Paused);

        app.handle_event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(app.elapsed_secs, 10, "Timer should not advance when paused");
    }

    // ── Pause ──────────────────────────────────────────────────────

    #[test]
    fn test_pause_toggle() {
        let mut app = SudokuApp::new();
        assert_eq!(app.status, GameStatus::Playing);
        app.toggle_pause();
        assert_eq!(app.status, GameStatus::Paused);
        app.toggle_pause();
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_no_input_while_paused() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.toggle_pause();

        let event = Event::Key(KeyEvent {
            key: Key::Num5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.cells[idx(r, c)].value, 0, "Input should be blocked when paused");
    }

    // ── Statistics ─────────────────────────────────────────────────

    #[test]
    fn test_stats_record_completion() {
        let mut stats = Stats::default();
        assert_eq!(stats.games_completed(Difficulty::Easy), 0);
        assert_eq!(stats.best_time(Difficulty::Easy), None);

        stats.record_completion(Difficulty::Easy, 120);
        assert_eq!(stats.games_completed(Difficulty::Easy), 1);
        assert_eq!(stats.best_time(Difficulty::Easy), Some(120));

        stats.record_completion(Difficulty::Easy, 90);
        assert_eq!(stats.games_completed(Difficulty::Easy), 2);
        assert_eq!(stats.best_time(Difficulty::Easy), Some(90));

        stats.record_completion(Difficulty::Easy, 150);
        assert_eq!(stats.games_completed(Difficulty::Easy), 3);
        assert_eq!(stats.best_time(Difficulty::Easy), Some(90), "Best time should not increase");
    }

    #[test]
    fn test_stats_per_difficulty() {
        let mut stats = Stats::default();
        stats.record_completion(Difficulty::Easy, 100);
        stats.record_completion(Difficulty::Medium, 200);
        stats.record_completion(Difficulty::Hard, 300);

        assert_eq!(stats.games_completed(Difficulty::Easy), 1);
        assert_eq!(stats.games_completed(Difficulty::Medium), 1);
        assert_eq!(stats.games_completed(Difficulty::Hard), 1);
        assert_eq!(stats.total_completed(), 3);

        assert_eq!(stats.best_time(Difficulty::Easy), Some(100));
        assert_eq!(stats.best_time(Difficulty::Medium), Some(200));
        assert_eq!(stats.best_time(Difficulty::Hard), Some(300));
    }

    #[test]
    fn test_completion_records_stats() {
        let mut app = SudokuApp::new();
        app.elapsed_secs = 42;

        // Complete the puzzle
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    app.selected_row = r;
                    app.selected_col = c;
                    app.input_digit(app.solution[idx(r, c)]);
                }
            }
        }
        assert_eq!(app.stats.games_completed(Difficulty::Easy), 1);
        assert_eq!(app.stats.best_time(Difficulty::Easy), Some(42));
    }

    // ── Conflict detection ─────────────────────────────────────────

    #[test]
    fn test_all_conflict_cells_clean_start() {
        let app = SudokuApp::new();
        // A freshly generated puzzle should have no conflicts
        let conflicts = app.all_conflict_cells();
        assert!(conflicts.is_empty(), "Fresh puzzle should have no conflicts");
    }

    #[test]
    fn test_all_conflict_cells_after_bad_move() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        // Place a digit that creates a conflict (same as a given in the row)
        let vals = values_array(&app.cells);
        let mut conflict_digit = 0u8;
        for cc in 0..GRID_SIZE {
            if cc != c && vals[idx(r, cc)] != 0 {
                conflict_digit = vals[idx(r, cc)];
                break;
            }
        }

        if conflict_digit != 0 {
            app.input_digit(conflict_digit);
            let conflicts = app.all_conflict_cells();
            assert!(!conflicts.is_empty(), "Should detect conflict after placing duplicate");
            assert!(conflicts.contains(&(r, c)));
        }
    }

    // ── Rendering ──────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = SudokuApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty(), "Render should produce commands");
    }

    #[test]
    fn test_render_contains_background() {
        let app = SudokuApp::new();
        let cmds = app.render();
        // First command should be the background fill
        match &cmds[0] {
            RenderCommand::FillRect { color, .. } => {
                assert_eq!(*color, BASE, "Background should be BASE color");
            }
            _ => panic!("First render command should be FillRect"),
        }
    }

    #[test]
    fn test_render_contains_title() {
        let app = SudokuApp::new();
        let cmds = app.render();
        let has_title = cmds.iter().any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text == "Sudoku"));
        assert!(has_title, "Render should contain title text");
    }

    #[test]
    fn test_render_contains_cell_digits() {
        let app = SudokuApp::new();
        let cmds = app.render();
        // At least some given digits should be rendered
        let digit_count = cmds.iter().filter(|cmd| {
            matches!(cmd, RenderCommand::Text { text, font_size, .. }
                if text.len() == 1
                    && text.chars().next().map_or(false, |ch| ch.is_ascii_digit() && ch != '0')
                    && (*font_size - CELL_FONT_SIZE).abs() < 0.1)
        }).count();
        assert!(digit_count > 0, "Render should contain cell digits");
    }

    #[test]
    fn test_render_won_shows_completed() {
        let mut app = SudokuApp::new();
        // Complete the puzzle
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    app.selected_row = r;
                    app.selected_col = c;
                    app.input_digit(app.solution[idx(r, c)]);
                }
            }
        }
        let cmds = app.render();
        let has_completed = cmds.iter().any(|cmd| matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Completed")));
        assert!(has_completed, "Won state should show 'Completed' text");
    }

    // ── Mouse input ────────────────────────────────────────────────

    #[test]
    fn test_mouse_click_selects_cell() {
        let mut app = SudokuApp::new();
        // Click on cell (0,0) position
        let (cx, cy) = cell_pixel_pos_clean(0, 0);
        let click = Event::Mouse(MouseEvent {
            x: PADDING + cx + CELL_SIZE / 2.0,
            y: PADDING + HEADER_HEIGHT + cy + CELL_SIZE / 2.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        app.handle_event(&click);
        assert_eq!(app.selected_row, 0);
        assert_eq!(app.selected_col, 0);
    }

    // ── Pixel geometry ─────────────────────────────────────────────

    #[test]
    fn test_grid_total_size() {
        let size = grid_total_size();
        // 9 * CELL_SIZE + 6 * CELL_GAP + 2 * BOX_GAP
        let expected = 9.0 * CELL_SIZE + 6.0 * CELL_GAP + 2.0 * BOX_GAP;
        assert!((size - expected).abs() < 0.01);
    }

    #[test]
    fn test_pixel_to_grid_coord_first_cell() {
        let coord = pixel_to_grid_coord(5.0);
        assert_eq!(coord, Some(0));
    }

    #[test]
    fn test_pixel_to_grid_coord_negative() {
        assert_eq!(pixel_to_grid_coord(-1.0), None);
    }

    #[test]
    fn test_pixel_to_grid_round_trip() {
        for i in 0..GRID_SIZE {
            let (px, _) = cell_pixel_pos_clean(0, i);
            let mid = px + CELL_SIZE / 2.0;
            let result = pixel_to_grid_coord(mid);
            assert_eq!(result, Some(i), "Round trip failed for column {i}");
        }
    }

    #[test]
    fn test_inner_gaps_before() {
        assert!((inner_gaps_before(0) - 0.0).abs() < 0.01);
        assert!((inner_gaps_before(1) - 1.0).abs() < 0.01);
        assert!((inner_gaps_before(2) - 2.0).abs() < 0.01);
        // Position 3 is start of box 1: 3 total gaps - 1 box boundary = 2 inner gaps
        assert!((inner_gaps_before(3) - 2.0).abs() < 0.01);
        assert!((inner_gaps_before(4) - 3.0).abs() < 0.01);
    }

    // ── Key release is ignored ─────────────────────────────────────

    #[test]
    fn test_key_release_ignored() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;

        let release = Event::Key(KeyEvent {
            key: Key::Num5,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&release);
        assert_eq!(app.cells[idx(r, c)].value, 0, "Key release should not place a digit");
    }

    // ── values_array ───────────────────────────────────────────────

    #[test]
    fn test_values_array() {
        let app = SudokuApp::new();
        let vals = values_array(&app.cells);
        for i in 0..TOTAL_CELLS {
            assert_eq!(vals[i], app.cells[i].value);
        }
    }

    // ── Ctrl+Z / Ctrl+Y via events ─────────────────────────────────

    #[test]
    fn test_ctrl_z_undo_event() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.input_digit(8);

        let ctrl_z = Event::Key(KeyEvent {
            key: Key::Z,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ctrl_z);
        assert_eq!(app.cells[idx(r, c)].value, 0);
    }

    #[test]
    fn test_ctrl_y_redo_event() {
        let mut app = SudokuApp::new();
        let (r, c) = find_empty_cell_in_app(&app);
        app.selected_row = r;
        app.selected_col = c;
        app.input_digit(6);
        app.undo();

        let ctrl_y = Event::Key(KeyEvent {
            key: Key::Y,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        app.handle_event(&ctrl_y);
        assert_eq!(app.cells[idx(r, c)].value, 6);
    }

    // ── Helper functions for tests ──────────────────────────────────

    fn find_empty_cell_in_app(app: &SudokuApp) -> (usize, usize) {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    return (r, c);
                }
            }
        }
        panic!("No empty cell found in app");
    }

    fn try_find_empty_cell_in_app(app: &SudokuApp) -> Option<(usize, usize)> {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].is_empty() {
                    return Some((r, c));
                }
            }
        }
        None
    }

    fn find_given_cell_in_app(app: &SudokuApp) -> (usize, usize) {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.cells[idx(r, c)].given {
                    return (r, c);
                }
            }
        }
        panic!("No given cell found in app");
    }
}
