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

//! OurOS Rush Hour -- sliding car puzzle game.
//!
//! A 6x6 grid contains vehicles (cars occupy 2 cells, trucks occupy 3 cells).
//! Each vehicle can only slide along its orientation axis (horizontal or
//! vertical). The goal is to slide the red car (always horizontal on row 2) to
//! the right exit at column 5. Includes 8 built-in puzzles of varying
//! difficulty, move counter, undo, puzzle selection, and restart. Uses the
//! Catppuccin Mocha dark theme.

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
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Vehicle color palette (distinct from UI colors) ─────────────────
const VEHICLE_COLORS: [Color; 12] = [
    Color::from_hex(0xF38BA8), // 0: Red (player car) -- always index 0
    Color::from_hex(0x89B4FA), // 1: Blue
    Color::from_hex(0xA6E3A1), // 2: Green
    Color::from_hex(0xF9E2AF), // 3: Yellow
    Color::from_hex(0xFAB387), // 4: Peach
    Color::from_hex(0xCBA6F7), // 5: Mauve
    Color::from_hex(0x94E2D5), // 6: Teal
    Color::from_hex(0xB4BEFE), // 7: Lavender
    Color::from_hex(0xEBA0AC), // 8: Maroon
    Color::from_hex(0x74C7EC), // 9: Sapphire
    Color::from_hex(0xF2CDCD), // 10: Flamingo
    Color::from_hex(0xA6ADC8), // 11: Subtext0
];

// ── Layout constants ────────────────────────────────────────────────
const GRID_SIZE: usize = 6;
const CELL_SIZE: f32 = 72.0;
const CELL_GAP: f32 = 3.0;
const PADDING: f32 = 20.0;
const HEADER_HEIGHT: f32 = 60.0;
const FOOTER_HEIGHT: f32 = 48.0;
const EXIT_MARKER_WIDTH: f32 = 16.0;
const CELL_CORNER_RADIUS: f32 = 6.0;
const VEHICLE_CORNER_RADIUS: f32 = 8.0;

const HEADER_FONT_SIZE: f32 = 22.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const LABEL_FONT_SIZE: f32 = 13.0;
const VEHICLE_FONT_SIZE: f32 = 16.0;
const WIN_FONT_SIZE: f32 = 28.0;

const MAX_UNDO: usize = 500;

// ── Orientation ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
    Horizontal,
    Vertical,
}

// ── Vehicle ─────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
struct Vehicle {
    /// Row of top-left cell.
    row: usize,
    /// Column of top-left cell.
    col: usize,
    /// Number of cells (2 for car, 3 for truck).
    length: usize,
    orientation: Orientation,
    /// Index into VEHICLE_COLORS. 0 = red/player car.
    color_index: usize,
    /// Label character shown on the vehicle.
    label: char,
}

impl Vehicle {
    /// Returns all grid cells occupied by this vehicle as (row, col) pairs.
    fn cells(&self) -> Vec<(usize, usize)> {
        let mut result = Vec::with_capacity(self.length);
        for i in 0..self.length {
            match self.orientation {
                Orientation::Horizontal => result.push((self.row, self.col + i)),
                Orientation::Vertical => result.push((self.row + i, self.col)),
            }
        }
        result
    }

    /// Whether this vehicle is the player's red car (always index 0).
    fn is_player(&self) -> bool {
        self.color_index == 0
    }

    /// Whether a given cell (row, col) is occupied by this vehicle.
    fn occupies(&self, row: usize, col: usize) -> bool {
        match self.orientation {
            Orientation::Horizontal => {
                self.row == row && col >= self.col && col < self.col + self.length
            }
            Orientation::Vertical => {
                self.col == col && row >= self.row && row < self.row + self.length
            }
        }
    }

    /// The maximum position this vehicle can reach (the tail end).
    fn tail_row(&self) -> usize {
        match self.orientation {
            Orientation::Vertical => self.row + self.length - 1,
            Orientation::Horizontal => self.row,
        }
    }

    fn tail_col(&self) -> usize {
        match self.orientation {
            Orientation::Horizontal => self.col + self.length - 1,
            Orientation::Vertical => self.col,
        }
    }
}

// ── Undo action ─────────────────────────────────────────────────────
#[derive(Clone, Debug)]
struct UndoAction {
    vehicle_index: usize,
    old_row: usize,
    old_col: usize,
    new_row: usize,
    new_col: usize,
}

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Beginner,
    Intermediate,
    Advanced,
    Expert,
}

impl Difficulty {
    fn label(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Intermediate => "Intermediate",
            Self::Advanced => "Advanced",
            Self::Expert => "Expert",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Beginner => GREEN,
            Self::Intermediate => YELLOW,
            Self::Advanced => PEACH,
            Self::Expert => RED,
        }
    }
}

// ── Game status ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
}

// ── Puzzle definition ───────────────────────────────────────────────
/// A puzzle is defined by a list of vehicle descriptors:
/// (row, col, length, orientation, label_char).
/// The first vehicle is always the player's red car.
struct PuzzleDef {
    difficulty: Difficulty,
    /// (row, col, length, orientation, label)
    vehicles: &'static [(usize, usize, usize, u8, char)],
}

/// Orientation encoding in puzzle defs: b'H' = horizontal, b'V' = vertical.
fn orient_from_byte(b: u8) -> Orientation {
    if b == b'H' {
        Orientation::Horizontal
    } else {
        Orientation::Vertical
    }
}

// ── Built-in puzzles ────────────────────────────────────────────────
// 8 puzzles of increasing difficulty. Vehicle 0 is always the red player car.
// Format: (row, col, length, H/V, label)

const PUZZLE_1: &[(usize, usize, usize, u8, char)] = &[
    (2, 0, 2, b'H', 'X'), // player car, row 2
    (0, 0, 3, b'V', 'A'), // truck blocking col 0
    (0, 3, 2, b'H', 'B'),
    (1, 4, 2, b'V', 'C'),
    (3, 2, 2, b'H', 'D'),
    (4, 0, 2, b'H', 'E'),
    (4, 4, 2, b'V', 'F'),
];

const PUZZLE_2: &[(usize, usize, usize, u8, char)] = &[
    (2, 1, 2, b'H', 'X'), // player car
    (0, 0, 2, b'H', 'A'),
    (0, 3, 3, b'V', 'B'),
    (1, 0, 2, b'V', 'C'),
    (3, 1, 3, b'H', 'D'),
    (4, 4, 2, b'V', 'E'),
    (5, 0, 2, b'H', 'F'),
    (0, 5, 3, b'V', 'G'),
];

const PUZZLE_3: &[(usize, usize, usize, u8, char)] = &[
    (2, 0, 2, b'H', 'X'), // player car
    (0, 0, 2, b'V', 'A'),
    (0, 1, 2, b'H', 'B'),
    (0, 4, 2, b'V', 'C'),
    (1, 2, 2, b'V', 'D'),
    (3, 0, 3, b'V', 'E'),
    (3, 3, 2, b'V', 'F'),
    (3, 4, 2, b'H', 'G'),
    (5, 1, 3, b'H', 'H'),
    (4, 5, 2, b'V', 'I'),
];

const PUZZLE_4: &[(usize, usize, usize, u8, char)] = &[
    (2, 0, 2, b'H', 'X'),
    (0, 0, 2, b'H', 'A'),
    (0, 2, 3, b'H', 'B'),
    (1, 0, 2, b'V', 'C'),
    (1, 4, 2, b'V', 'D'),
    (0, 5, 3, b'V', 'E'),
    (3, 1, 2, b'V', 'F'),
    (3, 2, 2, b'H', 'G'),
    (3, 4, 3, b'V', 'H'),
];

const PUZZLE_5: &[(usize, usize, usize, u8, char)] = &[
    (2, 2, 2, b'H', 'X'),
    (0, 0, 3, b'V', 'A'),
    (0, 1, 2, b'H', 'B'),
    (0, 5, 2, b'V', 'C'),
    (1, 3, 2, b'V', 'D'),
    (2, 4, 2, b'V', 'E'),
    (3, 0, 2, b'H', 'F'),
    (3, 2, 3, b'V', 'G'),
    (4, 4, 2, b'H', 'H'),
    (5, 0, 3, b'H', 'I'),
    (5, 3, 2, b'H', 'J'),
];

const PUZZLE_6: &[(usize, usize, usize, u8, char)] = &[
    (2, 0, 2, b'H', 'X'),
    (0, 0, 2, b'H', 'A'),
    (0, 2, 2, b'V', 'B'),
    (0, 3, 2, b'H', 'C'),
    (1, 4, 3, b'V', 'D'),
    (0, 5, 2, b'V', 'E'),
    (2, 2, 2, b'V', 'F'),
    (3, 0, 2, b'H', 'G'),
    (4, 0, 3, b'H', 'H'),
    (4, 3, 2, b'V', 'I'),
    (5, 4, 2, b'H', 'J'),
];

const PUZZLE_7: &[(usize, usize, usize, u8, char)] = &[
    (2, 1, 2, b'H', 'X'),
    (0, 0, 2, b'V', 'A'),
    (0, 1, 2, b'H', 'B'),
    (0, 4, 3, b'V', 'C'),
    (1, 1, 2, b'V', 'D'),
    (1, 2, 2, b'H', 'E'),
    (2, 3, 3, b'V', 'F'),
    (3, 0, 3, b'H', 'G'),
    (4, 0, 2, b'V', 'H'),
    (4, 2, 2, b'H', 'I'),
    (5, 4, 2, b'H', 'J'),
    (4, 5, 2, b'V', 'K'),
];

const PUZZLE_8: &[(usize, usize, usize, u8, char)] = &[
    (2, 0, 2, b'H', 'X'),
    (0, 0, 2, b'H', 'A'),
    (0, 2, 3, b'V', 'B'),
    (0, 3, 2, b'H', 'C'),
    (0, 5, 3, b'V', 'D'),
    (1, 3, 2, b'V', 'E'),
    (2, 4, 2, b'V', 'F'),
    (3, 0, 3, b'H', 'G'),
    (3, 3, 3, b'V', 'H'),
    (4, 0, 2, b'V', 'I'),
    (4, 1, 2, b'H', 'J'),
    (5, 2, 2, b'H', 'K'),
    (4, 4, 2, b'H', 'L'),
];

static PUZZLES: &[PuzzleDef] = &[
    PuzzleDef {
        difficulty: Difficulty::Beginner,
        vehicles: PUZZLE_1,
    },
    PuzzleDef {
        difficulty: Difficulty::Beginner,
        vehicles: PUZZLE_2,
    },
    PuzzleDef {
        difficulty: Difficulty::Intermediate,
        vehicles: PUZZLE_3,
    },
    PuzzleDef {
        difficulty: Difficulty::Intermediate,
        vehicles: PUZZLE_4,
    },
    PuzzleDef {
        difficulty: Difficulty::Advanced,
        vehicles: PUZZLE_5,
    },
    PuzzleDef {
        difficulty: Difficulty::Advanced,
        vehicles: PUZZLE_6,
    },
    PuzzleDef {
        difficulty: Difficulty::Expert,
        vehicles: PUZZLE_7,
    },
    PuzzleDef {
        difficulty: Difficulty::Expert,
        vehicles: PUZZLE_8,
    },
];

// ── Helper: build occupancy grid ────────────────────────────────────
/// Returns a 6x6 grid where each cell contains `Some(vehicle_index)` or `None`.
fn build_occupancy(vehicles: &[Vehicle]) -> [[Option<usize>; GRID_SIZE]; GRID_SIZE] {
    let mut grid = [[None; GRID_SIZE]; GRID_SIZE];
    for (vi, v) in vehicles.iter().enumerate() {
        for (r, c) in v.cells() {
            if r < GRID_SIZE && c < GRID_SIZE {
                grid[r][c] = Some(vi);
            }
        }
    }
    grid
}

/// Check if a vehicle placement is valid (no overlap, within bounds).
fn is_valid_placement(vehicles: &[Vehicle], index: usize) -> bool {
    let v = &vehicles[index];
    // Bounds check
    for (r, c) in v.cells() {
        if r >= GRID_SIZE || c >= GRID_SIZE {
            return false;
        }
    }
    // Overlap check (skip self)
    for (vi, other) in vehicles.iter().enumerate() {
        if vi == index {
            continue;
        }
        for (r, c) in v.cells() {
            if other.occupies(r, c) {
                return false;
            }
        }
    }
    true
}

/// Check whether the player car (vehicle 0) has its rightmost cell at column 4
/// (meaning the car spans columns 4-5, reaching the exit at the right edge).
fn check_win(vehicles: &[Vehicle]) -> bool {
    if vehicles.is_empty() {
        return false;
    }
    let player = &vehicles[0];
    // Player car is horizontal, length 2. Win when tail_col == 5 (rightmost
    // column, since the exit is at the right side of row 2).
    player.tail_col() >= GRID_SIZE - 1
}

/// Can vehicle at `index` move by `delta` steps along its axis?
/// `delta` is negative for left/up, positive for right/down.
fn can_move(vehicles: &[Vehicle], index: usize, delta: i32) -> bool {
    if delta == 0 {
        return true;
    }
    let v = &vehicles[index];
    let occupancy = build_occupancy(vehicles);

    match v.orientation {
        Orientation::Horizontal => {
            if delta < 0 {
                // Moving left
                let steps = (-delta) as usize;
                if v.col < steps {
                    return false;
                }
                for s in 1..=steps {
                    let check_col = v.col - s;
                    if let Some(occ) = occupancy[v.row][check_col] {
                        if occ != index {
                            return false;
                        }
                    }
                }
            } else {
                // Moving right
                let steps = delta as usize;
                if v.col + v.length - 1 + steps >= GRID_SIZE {
                    return false;
                }
                for s in 1..=steps {
                    let check_col = v.col + v.length - 1 + s;
                    if let Some(occ) = occupancy[v.row][check_col] {
                        if occ != index {
                            return false;
                        }
                    }
                }
            }
            true
        }
        Orientation::Vertical => {
            if delta < 0 {
                // Moving up
                let steps = (-delta) as usize;
                if v.row < steps {
                    return false;
                }
                for s in 1..=steps {
                    let check_row = v.row - s;
                    if let Some(occ) = occupancy[check_row][v.col] {
                        if occ != index {
                            return false;
                        }
                    }
                }
            } else {
                // Moving down
                let steps = delta as usize;
                if v.row + v.length - 1 + steps >= GRID_SIZE {
                    return false;
                }
                for s in 1..=steps {
                    let check_row = v.row + v.length - 1 + s;
                    if let Some(occ) = occupancy[check_row][v.col] {
                        if occ != index {
                            return false;
                        }
                    }
                }
            }
            true
        }
    }
}

/// Move vehicle at `index` by `delta` steps. Returns true if successful.
fn try_move(vehicles: &mut [Vehicle], index: usize, delta: i32) -> bool {
    if !can_move(vehicles, index, delta) {
        return false;
    }
    let v = &mut vehicles[index];
    match v.orientation {
        Orientation::Horizontal => {
            v.col = (v.col as i32 + delta) as usize;
        }
        Orientation::Vertical => {
            v.row = (v.row as i32 + delta) as usize;
        }
    }
    true
}

/// Compute the maximum number of steps a vehicle can move in a direction.
/// `direction`: -1 for left/up, +1 for right/down.
fn max_slide(vehicles: &[Vehicle], index: usize, direction: i32) -> usize {
    let mut steps: usize = 0;
    loop {
        let next_delta = if direction > 0 {
            (steps + 1) as i32
        } else {
            -((steps + 1) as i32)
        };
        if can_move(vehicles, index, next_delta) {
            steps += 1;
        } else {
            break;
        }
    }
    steps
}

// ── Grid pixel math ─────────────────────────────────────────────────
/// Total pixel size of the grid area.
fn grid_pixel_size() -> f32 {
    GRID_SIZE as f32 * CELL_SIZE + (GRID_SIZE as f32 - 1.0) * CELL_GAP
}

/// Top-left pixel position of cell (row, col) relative to grid origin.
fn cell_pixel_pos(row: usize, col: usize) -> (f32, f32) {
    let x = col as f32 * (CELL_SIZE + CELL_GAP);
    let y = row as f32 * (CELL_SIZE + CELL_GAP);
    (x, y)
}

/// Grid origin (top-left pixel of cell 0,0) in absolute coordinates.
fn grid_origin() -> (f32, f32) {
    (PADDING, PADDING + HEADER_HEIGHT)
}

/// Convert a pixel coordinate (relative to grid origin) to a grid cell.
/// Returns None if outside the grid.
fn pixel_to_cell(px: f32, py: f32) -> Option<(usize, usize)> {
    if px < 0.0 || py < 0.0 {
        return None;
    }
    let total = grid_pixel_size();
    if px >= total || py >= total {
        return None;
    }
    let col = (px / (CELL_SIZE + CELL_GAP)) as usize;
    let row = (py / (CELL_SIZE + CELL_GAP)) as usize;
    if row < GRID_SIZE && col < GRID_SIZE {
        Some((row, col))
    } else {
        None
    }
}

// ── Load puzzle ─────────────────────────────────────────────────────
fn load_puzzle(puzzle_index: usize) -> Vec<Vehicle> {
    let def = &PUZZLES[puzzle_index % PUZZLES.len()];
    def.vehicles
        .iter()
        .enumerate()
        .map(|(i, &(row, col, length, orient, label))| Vehicle {
            row,
            col,
            length,
            orientation: orient_from_byte(orient),
            color_index: i % VEHICLE_COLORS.len(),
            label,
        })
        .collect()
}

// ── Main application state ──────────────────────────────────────────
struct RushHour {
    vehicles: Vec<Vehicle>,
    /// Currently selected vehicle index.
    selected: usize,
    /// Game status.
    status: GameStatus,
    /// Current puzzle index.
    puzzle_index: usize,
    /// Move counter.
    moves: usize,
    /// Undo history.
    undo_stack: Vec<UndoAction>,
    /// Whether the puzzle-select overlay is shown.
    selecting_puzzle: bool,
    /// Cursor position in the puzzle-select overlay.
    puzzle_select_cursor: usize,
}

impl RushHour {
    fn new() -> Self {
        let vehicles = load_puzzle(0);
        Self {
            vehicles,
            selected: 0,
            status: GameStatus::Playing,
            puzzle_index: 0,
            moves: 0,
            undo_stack: Vec::new(),
            selecting_puzzle: false,
            puzzle_select_cursor: 0,
        }
    }

    /// Load a specific puzzle by index.
    fn load_puzzle_at(&mut self, index: usize) {
        self.puzzle_index = index % PUZZLES.len();
        self.vehicles = load_puzzle(self.puzzle_index);
        self.selected = 0;
        self.status = GameStatus::Playing;
        self.moves = 0;
        self.undo_stack.clear();
        self.selecting_puzzle = false;
    }

    /// Restart current puzzle.
    fn restart(&mut self) {
        self.load_puzzle_at(self.puzzle_index);
    }

    /// Number of vehicles.
    fn vehicle_count(&self) -> usize {
        self.vehicles.len()
    }

    /// Move the currently selected vehicle by `delta` steps.
    /// Returns true if the move was performed.
    fn move_selected(&mut self, delta: i32) -> bool {
        if self.status == GameStatus::Won {
            return false;
        }
        if self.selected >= self.vehicles.len() {
            return false;
        }
        let old_row = self.vehicles[self.selected].row;
        let old_col = self.vehicles[self.selected].col;
        if try_move(&mut self.vehicles, self.selected, delta) {
            let new_row = self.vehicles[self.selected].row;
            let new_col = self.vehicles[self.selected].col;
            self.undo_stack.push(UndoAction {
                vehicle_index: self.selected,
                old_row,
                old_col,
                new_row,
                new_col,
            });
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            self.moves += 1;
            // Check win condition
            if check_win(&self.vehicles) {
                self.status = GameStatus::Won;
            }
            true
        } else {
            false
        }
    }

    /// Undo the last move.
    fn undo(&mut self) {
        if self.status == GameStatus::Won {
            return;
        }
        if let Some(action) = self.undo_stack.pop() {
            self.vehicles[action.vehicle_index].row = action.old_row;
            self.vehicles[action.vehicle_index].col = action.old_col;
            if self.moves > 0 {
                self.moves -= 1;
            }
        }
    }

    /// Select the next vehicle (Tab / cycle forward).
    fn select_next(&mut self) {
        if self.vehicles.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.vehicles.len();
    }

    /// Select the previous vehicle (Shift+Tab / cycle backward).
    fn select_prev(&mut self) {
        if self.vehicles.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.vehicles.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Select a vehicle by clicking on a cell.
    fn select_at_cell(&mut self, row: usize, col: usize) {
        let occupancy = build_occupancy(&self.vehicles);
        if let Some(vi) = occupancy[row][col] {
            self.selected = vi;
        }
    }

    /// Current puzzle difficulty.
    fn difficulty(&self) -> Difficulty {
        PUZZLES[self.puzzle_index % PUZZLES.len()].difficulty
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(key_event) if key_event.pressed => {
                self.handle_key(key_event);
            }
            Event::Mouse(mouse_event) => {
                self.handle_mouse(mouse_event);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key_event: &KeyEvent) {
        // Puzzle-select overlay intercepts all keys
        if self.selecting_puzzle {
            self.handle_puzzle_select_key(key_event);
            return;
        }

        // Global keys (work even when won)
        match key_event.key {
            Key::P if key_event.modifiers == Modifiers::NONE => {
                self.selecting_puzzle = true;
                self.puzzle_select_cursor = self.puzzle_index;
                return;
            }
            Key::R if key_event.modifiers == Modifiers::NONE => {
                self.restart();
                return;
            }
            Key::N if key_event.modifiers == Modifiers::NONE => {
                let next = (self.puzzle_index + 1) % PUZZLES.len();
                self.load_puzzle_at(next);
                return;
            }
            _ => {}
        }

        if self.status == GameStatus::Won {
            return;
        }

        match key_event.key {
            // Vehicle selection
            Key::Tab if key_event.modifiers == Modifiers::NONE => self.select_next(),
            Key::Tab if key_event.modifiers.shift => self.select_prev(),

            // Movement
            Key::Left if key_event.modifiers == Modifiers::NONE => {
                if self.selected < self.vehicles.len() {
                    let v = &self.vehicles[self.selected];
                    match v.orientation {
                        Orientation::Horizontal => {
                            self.move_selected(-1);
                        }
                        Orientation::Vertical => {} // can't move horizontally
                    }
                }
            }
            Key::Right if key_event.modifiers == Modifiers::NONE => {
                if self.selected < self.vehicles.len() {
                    let v = &self.vehicles[self.selected];
                    match v.orientation {
                        Orientation::Horizontal => {
                            self.move_selected(1);
                        }
                        Orientation::Vertical => {}
                    }
                }
            }
            Key::Up if key_event.modifiers == Modifiers::NONE => {
                if self.selected < self.vehicles.len() {
                    let v = &self.vehicles[self.selected];
                    match v.orientation {
                        Orientation::Vertical => {
                            self.move_selected(-1);
                        }
                        Orientation::Horizontal => {}
                    }
                }
            }
            Key::Down if key_event.modifiers == Modifiers::NONE => {
                if self.selected < self.vehicles.len() {
                    let v = &self.vehicles[self.selected];
                    match v.orientation {
                        Orientation::Vertical => {
                            self.move_selected(1);
                        }
                        Orientation::Horizontal => {}
                    }
                }
            }

            // Undo
            Key::Z if key_event.modifiers == Modifiers::NONE => self.undo(),

            _ => {}
        }
    }

    fn handle_puzzle_select_key(&mut self, key_event: &KeyEvent) {
        match key_event.key {
            Key::Escape => {
                self.selecting_puzzle = false;
            }
            Key::Up => {
                if self.puzzle_select_cursor > 0 {
                    self.puzzle_select_cursor -= 1;
                }
            }
            Key::Down => {
                if self.puzzle_select_cursor < PUZZLES.len() - 1 {
                    self.puzzle_select_cursor += 1;
                }
            }
            Key::Enter => {
                self.load_puzzle_at(self.puzzle_select_cursor);
            }
            Key::Num1 => self.load_puzzle_at(0),
            Key::Num2 => self.load_puzzle_at(1),
            Key::Num3 => self.load_puzzle_at(2),
            Key::Num4 => self.load_puzzle_at(3),
            Key::Num5 => self.load_puzzle_at(4),
            Key::Num6 => self.load_puzzle_at(5),
            Key::Num7 => self.load_puzzle_at(6),
            Key::Num8 => self.load_puzzle_at(7),
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse_event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = mouse_event.kind {
            if self.selecting_puzzle || self.status == GameStatus::Won {
                return;
            }
            let (gx_origin, gy_origin) = grid_origin();
            let gx = mouse_event.x - gx_origin;
            let gy = mouse_event.y - gy_origin;

            if let Some((row, col)) = pixel_to_cell(gx, gy) {
                self.select_at_cell(row, col);
            }
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let grid_px = grid_pixel_size();
        let total_width = grid_px + PADDING * 2.0 + EXIT_MARKER_WIDTH;
        let total_height = HEADER_HEIGHT + grid_px + FOOTER_HEIGHT + PADDING * 2.0;

        let _ = (width, height); // use layout params if needed

        // Full background
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

        // Grid background
        let (gx, gy) = grid_origin();
        cmds.push(RenderCommand::FillRect {
            x: gx - 2.0,
            y: gy - 2.0,
            width: grid_px + 4.0,
            height: grid_px + 4.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });

        // Grid cells (empty squares)
        self.render_grid_cells(&mut cmds);

        // Exit marker on row 2, right side
        self.render_exit_marker(&mut cmds);

        // Vehicles
        self.render_vehicles(&mut cmds);

        // Footer
        self.render_footer(&mut cmds, total_width, total_height);

        // Win overlay
        if self.status == GameStatus::Won {
            self.render_win_overlay(&mut cmds, total_width, total_height);
        }

        // Puzzle select overlay
        if self.selecting_puzzle {
            self.render_puzzle_select(&mut cmds, total_width, total_height);
        }

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, total_width: f32) {
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
            text: "Rush Hour".to_string(),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Puzzle info
        let diff = self.difficulty();
        let info = format!("Puzzle {} - {}", self.puzzle_index + 1, diff.label());
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 35.0,
            text: info,
            color: diff.color(),
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Move counter (right side)
        let moves_text = format!("Moves: {}", self.moves);
        cmds.push(RenderCommand::Text {
            x: total_width - 120.0,
            y: 10.0,
            text: moves_text,
            color: TEXT_COLOR,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status
        let status_text = match self.status {
            GameStatus::Playing => "Playing",
            GameStatus::Won => "Solved!",
        };
        let status_color = match self.status {
            GameStatus::Playing => SUBTEXT0,
            GameStatus::Won => GREEN,
        };
        cmds.push(RenderCommand::Text {
            x: total_width - 120.0,
            y: 30.0,
            text: status_text.to_string(),
            color: status_color,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_grid_cells(&self, cmds: &mut Vec<RenderCommand>) {
        let (gx, gy) = grid_origin();
        for row in 0..GRID_SIZE {
            for col in 0..GRID_SIZE {
                let (cx, cy) = cell_pixel_pos(row, col);
                cmds.push(RenderCommand::FillRect {
                    x: gx + cx,
                    y: gy + cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });
            }
        }
    }

    fn render_exit_marker(&self, cmds: &mut Vec<RenderCommand>) {
        let (gx, gy) = grid_origin();
        // Exit is at row 2, right side of the grid
        let (_, exit_y) = cell_pixel_pos(2, 0);
        let exit_x = gx + grid_pixel_size() + 2.0;

        // Arrow / exit indicator
        cmds.push(RenderCommand::FillRect {
            x: exit_x,
            y: gy + exit_y + 10.0,
            width: EXIT_MARKER_WIDTH - 4.0,
            height: CELL_SIZE - 20.0,
            color: RED,
            corner_radii: CornerRadii::all(4.0),
        });

        // Arrow text
        cmds.push(RenderCommand::Text {
            x: exit_x + 1.0,
            y: gy + exit_y + CELL_SIZE / 2.0 - 8.0,
            text: ">".to_string(),
            color: CRUST,
            font_size: VEHICLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_vehicles(&self, cmds: &mut Vec<RenderCommand>) {
        let (gx, gy) = grid_origin();

        for (vi, v) in self.vehicles.iter().enumerate() {
            let (cy, cx) = cell_pixel_pos(v.row, v.col);
            let is_selected = vi == self.selected && self.status == GameStatus::Playing;

            // Vehicle dimensions
            let (vw, vh) = match v.orientation {
                Orientation::Horizontal => {
                    let w = v.length as f32 * CELL_SIZE + (v.length as f32 - 1.0) * CELL_GAP;
                    (w, CELL_SIZE)
                }
                Orientation::Vertical => {
                    let h = v.length as f32 * CELL_SIZE + (v.length as f32 - 1.0) * CELL_GAP;
                    (CELL_SIZE, h)
                }
            };

            let base_color = VEHICLE_COLORS[v.color_index % VEHICLE_COLORS.len()];

            // Selection highlight (slightly larger background)
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: gx + cx - 3.0,
                    y: gy + cy - 3.0,
                    width: vw + 6.0,
                    height: vh + 6.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(VEHICLE_CORNER_RADIUS + 2.0),
                });
            }

            // Vehicle body
            cmds.push(RenderCommand::FillRect {
                x: gx + cx,
                y: gy + cy,
                width: vw,
                height: vh,
                color: base_color,
                corner_radii: CornerRadii::all(VEHICLE_CORNER_RADIUS),
            });

            // Vehicle label (centered)
            let label_x = gx + cx + vw / 2.0 - 5.0;
            let label_y = gy + cy + vh / 2.0 - 9.0;
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: label_y,
                text: v.label.to_string(),
                color: CRUST,
                font_size: VEHICLE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>, total_width: f32, total_height: f32) {
        let footer_y = total_height - FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: footer_y,
            width: total_width,
            height: FOOTER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Key hints
        let hints = "Tab:select  Arrows:move  Z:undo  R:restart  P:puzzles  N:next";
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y + 16.0,
            text: hints.to_string(),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_win_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        total_width: f32,
        total_height: f32,
    ) {
        // Semi-transparent overlay
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: total_height,
            color: Color::from_hex(0x11111B), // dark overlay
            corner_radii: CornerRadii::ZERO,
        });

        // Win banner background
        let banner_w = 300.0;
        let banner_h = 120.0;
        let banner_x = (total_width - banner_w) / 2.0;
        let banner_y = (total_height - banner_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: banner_x,
            y: banner_y,
            width: banner_w,
            height: banner_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Win text
        cmds.push(RenderCommand::Text {
            x: banner_x + 50.0,
            y: banner_y + 20.0,
            text: "Puzzle Solved!".to_string(),
            color: GREEN,
            font_size: WIN_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Moves text
        let moves_msg = format!("Completed in {} moves", self.moves);
        cmds.push(RenderCommand::Text {
            x: banner_x + 60.0,
            y: banner_y + 58.0,
            text: moves_msg,
            color: TEXT_COLOR,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Hint
        cmds.push(RenderCommand::Text {
            x: banner_x + 40.0,
            y: banner_y + 85.0,
            text: "N: next puzzle  R: restart  P: select".to_string(),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_puzzle_select(
        &self,
        cmds: &mut Vec<RenderCommand>,
        total_width: f32,
        total_height: f32,
    ) {
        // Overlay background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: total_height,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 20.0,
            text: "Select Puzzle".to_string(),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Puzzle list
        for (i, puzzle) in PUZZLES.iter().enumerate() {
            let y = 60.0 + i as f32 * 36.0;
            let is_cursor = i == self.puzzle_select_cursor;
            let is_current = i == self.puzzle_index;

            // Highlight current selection
            if is_cursor {
                cmds.push(RenderCommand::FillRect {
                    x: PADDING - 4.0,
                    y: y - 4.0,
                    width: total_width - PADDING * 2.0 + 8.0,
                    height: 28.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let marker = if is_current { "> " } else { "  " };
            let label = format!(
                "{}{}. Puzzle {} - {}",
                marker,
                i + 1,
                i + 1,
                puzzle.difficulty.label()
            );
            let color = if is_cursor {
                TEXT_COLOR
            } else {
                puzzle.difficulty.color()
            };

            cmds.push(RenderCommand::Text {
                x: PADDING + 4.0,
                y,
                text: label,
                color,
                font_size: STATUS_FONT_SIZE,
                font_weight: if is_cursor {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }

        // Footer hint
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: total_height - 30.0,
            text: "Up/Down: browse  Enter: select  Esc: cancel  1-8: jump".to_string(),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() {
    let _app = RushHour::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper: create key event ────────────────────────────────────

    fn key_event(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    fn key_event_shift(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::shift(),
            text: None,
        })
    }

    fn mouse_click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    // ── Vehicle construction ────────────────────────────────────────

    #[test]
    fn test_vehicle_cells_horizontal() {
        let v = Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        };
        assert_eq!(v.cells(), vec![(2, 0), (2, 1)]);
    }

    #[test]
    fn test_vehicle_cells_vertical() {
        let v = Vehicle {
            row: 0,
            col: 3,
            length: 3,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        };
        assert_eq!(v.cells(), vec![(0, 3), (1, 3), (2, 3)]);
    }

    #[test]
    fn test_vehicle_cells_single_cell() {
        let v = Vehicle {
            row: 4,
            col: 5,
            length: 1,
            orientation: Orientation::Horizontal,
            color_index: 2,
            label: 'B',
        };
        assert_eq!(v.cells(), vec![(4, 5)]);
    }

    #[test]
    fn test_vehicle_occupies_horizontal() {
        let v = Vehicle {
            row: 2,
            col: 1,
            length: 3,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        };
        assert!(v.occupies(2, 1));
        assert!(v.occupies(2, 2));
        assert!(v.occupies(2, 3));
        assert!(!v.occupies(2, 0));
        assert!(!v.occupies(2, 4));
        assert!(!v.occupies(1, 1));
        assert!(!v.occupies(3, 2));
    }

    #[test]
    fn test_vehicle_occupies_vertical() {
        let v = Vehicle {
            row: 1,
            col: 4,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 3,
            label: 'C',
        };
        assert!(v.occupies(1, 4));
        assert!(v.occupies(2, 4));
        assert!(!v.occupies(0, 4));
        assert!(!v.occupies(3, 4));
        assert!(!v.occupies(1, 3));
    }

    #[test]
    fn test_vehicle_is_player() {
        let player = Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        };
        let other = Vehicle {
            row: 0,
            col: 0,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        };
        assert!(player.is_player());
        assert!(!other.is_player());
    }

    #[test]
    fn test_vehicle_tail_col_horizontal() {
        let v = Vehicle {
            row: 2,
            col: 1,
            length: 3,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        };
        assert_eq!(v.tail_col(), 3);
    }

    #[test]
    fn test_vehicle_tail_row_vertical() {
        let v = Vehicle {
            row: 0,
            col: 2,
            length: 3,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        };
        assert_eq!(v.tail_row(), 2);
    }

    #[test]
    fn test_vehicle_tail_col_vertical_same_col() {
        let v = Vehicle {
            row: 1,
            col: 3,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 2,
            label: 'B',
        };
        assert_eq!(v.tail_col(), 3);
    }

    #[test]
    fn test_vehicle_tail_row_horizontal_same_row() {
        let v = Vehicle {
            row: 4,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 3,
            label: 'C',
        };
        assert_eq!(v.tail_row(), 4);
    }

    // ── Orientation helper ──────────────────────────────────────────

    #[test]
    fn test_orient_from_byte_horizontal() {
        assert_eq!(orient_from_byte(b'H'), Orientation::Horizontal);
    }

    #[test]
    fn test_orient_from_byte_vertical() {
        assert_eq!(orient_from_byte(b'V'), Orientation::Vertical);
    }

    #[test]
    fn test_orient_from_byte_default_vertical() {
        // Anything not 'H' defaults to Vertical
        assert_eq!(orient_from_byte(b'X'), Orientation::Vertical);
    }

    // ── Occupancy grid ──────────────────────────────────────────────

    #[test]
    fn test_build_occupancy_empty() {
        let vehicles: Vec<Vehicle> = vec![];
        let occ = build_occupancy(&vehicles);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                assert!(occ[r][c].is_none());
            }
        }
    }

    #[test]
    fn test_build_occupancy_single_vehicle() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        let occ = build_occupancy(&vehicles);
        assert_eq!(occ[2][0], Some(0));
        assert_eq!(occ[2][1], Some(0));
        assert_eq!(occ[2][2], None);
        assert_eq!(occ[1][0], None);
    }

    #[test]
    fn test_build_occupancy_multiple_vehicles() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 0,
                col: 3,
                length: 3,
                orientation: Orientation::Vertical,
                color_index: 1,
                label: 'A',
            },
        ];
        let occ = build_occupancy(&vehicles);
        assert_eq!(occ[2][0], Some(0));
        assert_eq!(occ[2][1], Some(0));
        assert_eq!(occ[0][3], Some(1));
        assert_eq!(occ[1][3], Some(1));
        assert_eq!(occ[2][3], Some(1));
    }

    // ── Validity checks ─────────────────────────────────────────────

    #[test]
    fn test_valid_placement_in_bounds() {
        let vehicles = vec![Vehicle {
            row: 0,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(is_valid_placement(&vehicles, 0));
    }

    #[test]
    fn test_invalid_placement_out_of_bounds_horizontal() {
        let vehicles = vec![Vehicle {
            row: 0,
            col: 5,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(!is_valid_placement(&vehicles, 0));
    }

    #[test]
    fn test_invalid_placement_out_of_bounds_vertical() {
        let vehicles = vec![Vehicle {
            row: 5,
            col: 0,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 0,
            label: 'X',
        }];
        assert!(!is_valid_placement(&vehicles, 0));
    }

    #[test]
    fn test_invalid_placement_overlap() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 2,
                col: 1,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 1,
                label: 'A',
            },
        ];
        // Vehicle 1 overlaps vehicle 0 at (2,1)
        assert!(!is_valid_placement(&vehicles, 1));
    }

    // ── Win detection ───────────────────────────────────────────────

    #[test]
    fn test_check_win_not_yet() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(!check_win(&vehicles));
    }

    #[test]
    fn test_check_win_at_exit() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 4,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(check_win(&vehicles));
    }

    #[test]
    fn test_check_win_empty_vehicles() {
        let vehicles: Vec<Vehicle> = vec![];
        assert!(!check_win(&vehicles));
    }

    #[test]
    fn test_check_win_at_col3_not_won() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 3,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        // tail_col == 4, need >= 5
        assert!(!check_win(&vehicles));
    }

    // ── Movement ────────────────────────────────────────────────────

    #[test]
    fn test_can_move_right_free() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_can_move_left_at_edge() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(!can_move(&vehicles, 0, -1));
    }

    #[test]
    fn test_can_move_right_blocked_by_wall() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 4,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(!can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_can_move_right_blocked_by_vehicle() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 2,
                col: 2,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 1,
                label: 'A',
            },
        ];
        assert!(!can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_can_move_down_free() {
        let vehicles = vec![Vehicle {
            row: 0,
            col: 3,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_can_move_up_free() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 3,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(can_move(&vehicles, 0, -1));
    }

    #[test]
    fn test_can_move_up_at_top() {
        let vehicles = vec![Vehicle {
            row: 0,
            col: 3,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(!can_move(&vehicles, 0, -1));
    }

    #[test]
    fn test_can_move_down_at_bottom() {
        let vehicles = vec![Vehicle {
            row: 4,
            col: 3,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(!can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_can_move_zero_delta() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(can_move(&vehicles, 0, 0));
    }

    #[test]
    fn test_try_move_success() {
        let mut vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(try_move(&mut vehicles, 0, 1));
        assert_eq!(vehicles[0].col, 1);
    }

    #[test]
    fn test_try_move_failure() {
        let mut vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(!try_move(&mut vehicles, 0, -1));
        assert_eq!(vehicles[0].col, 0);
    }

    #[test]
    fn test_try_move_vertical_down() {
        let mut vehicles = vec![Vehicle {
            row: 0,
            col: 2,
            length: 3,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(try_move(&mut vehicles, 0, 2));
        assert_eq!(vehicles[0].row, 2);
    }

    #[test]
    fn test_try_move_vertical_up() {
        let mut vehicles = vec![Vehicle {
            row: 3,
            col: 2,
            length: 3,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(try_move(&mut vehicles, 0, -2));
        assert_eq!(vehicles[0].row, 1);
    }

    #[test]
    fn test_can_move_multiple_steps_horizontal() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert!(can_move(&vehicles, 0, 3)); // col 0 -> 3, tail at 4
        assert!(can_move(&vehicles, 0, 4)); // col 0 -> 4, tail at 5
        assert!(!can_move(&vehicles, 0, 5)); // col 0 -> 5, tail at 6 (OOB)
    }

    #[test]
    fn test_can_move_multiple_steps_blocked() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 2,
                col: 4,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 1,
                label: 'A',
            },
        ];
        assert!(can_move(&vehicles, 0, 1)); // 0->1 ok
        assert!(can_move(&vehicles, 0, 2)); // 0->2, tail at 3, ok
        assert!(!can_move(&vehicles, 0, 3)); // 0->3, tail at 4, blocked by A
    }

    // ── Max slide ───────────────────────────────────────────────────

    #[test]
    fn test_max_slide_right_free() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert_eq!(max_slide(&vehicles, 0, 1), 4); // 0->4, tail at 5
    }

    #[test]
    fn test_max_slide_left_at_edge() {
        let vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        assert_eq!(max_slide(&vehicles, 0, -1), 0);
    }

    #[test]
    fn test_max_slide_right_blocked() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 2,
                col: 3,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 1,
                label: 'A',
            },
        ];
        assert_eq!(max_slide(&vehicles, 0, 1), 1); // can move to col 1, then blocked at col 3
    }

    #[test]
    fn test_max_slide_vertical_down() {
        let vehicles = vec![Vehicle {
            row: 0,
            col: 2,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert_eq!(max_slide(&vehicles, 0, 1), 4); // 0->4, tail at 5
    }

    #[test]
    fn test_max_slide_vertical_up() {
        let vehicles = vec![Vehicle {
            row: 3,
            col: 2,
            length: 2,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert_eq!(max_slide(&vehicles, 0, -1), 3); // can go from row 3 to row 0
    }

    // ── Grid pixel math ─────────────────────────────────────────────

    #[test]
    fn test_grid_pixel_size() {
        let expected = GRID_SIZE as f32 * CELL_SIZE + (GRID_SIZE as f32 - 1.0) * CELL_GAP;
        assert!((grid_pixel_size() - expected).abs() < 0.001);
    }

    #[test]
    fn test_cell_pixel_pos_origin() {
        let (x, y) = cell_pixel_pos(0, 0);
        assert!((x - 0.0).abs() < 0.001);
        assert!((y - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cell_pixel_pos_second_cell() {
        let (x, _y) = cell_pixel_pos(0, 1);
        let expected = CELL_SIZE + CELL_GAP;
        assert!((x - expected).abs() < 0.001);
    }

    #[test]
    fn test_cell_pixel_pos_row() {
        let (_x, y) = cell_pixel_pos(1, 0);
        let expected = CELL_SIZE + CELL_GAP;
        assert!((y - expected).abs() < 0.001);
    }

    #[test]
    fn test_pixel_to_cell_origin() {
        let result = pixel_to_cell(5.0, 5.0);
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn test_pixel_to_cell_second() {
        let x = CELL_SIZE + CELL_GAP + 5.0;
        let result = pixel_to_cell(x, 5.0);
        assert_eq!(result, Some((0, 1)));
    }

    #[test]
    fn test_pixel_to_cell_last() {
        let pos = 5.0 * (CELL_SIZE + CELL_GAP) + 5.0;
        let result = pixel_to_cell(pos, pos);
        assert_eq!(result, Some((5, 5)));
    }

    #[test]
    fn test_pixel_to_cell_negative() {
        assert_eq!(pixel_to_cell(-1.0, 5.0), None);
        assert_eq!(pixel_to_cell(5.0, -1.0), None);
    }

    #[test]
    fn test_pixel_to_cell_out_of_bounds() {
        let too_far = grid_pixel_size() + 10.0;
        assert_eq!(pixel_to_cell(too_far, 5.0), None);
    }

    #[test]
    fn test_grid_origin() {
        let (gx, gy) = grid_origin();
        assert!((gx - PADDING).abs() < 0.001);
        assert!((gy - (PADDING + HEADER_HEIGHT)).abs() < 0.001);
    }

    // ── Load puzzle ─────────────────────────────────────────────────

    #[test]
    fn test_load_puzzle_0() {
        let vehicles = load_puzzle(0);
        assert!(!vehicles.is_empty());
        // First vehicle is always the player car
        assert_eq!(vehicles[0].color_index, 0);
        assert_eq!(vehicles[0].label, 'X');
        assert_eq!(vehicles[0].orientation, Orientation::Horizontal);
        assert_eq!(vehicles[0].row, 2);
    }

    #[test]
    fn test_load_puzzle_all_valid() {
        for i in 0..PUZZLES.len() {
            let vehicles = load_puzzle(i);
            assert!(!vehicles.is_empty(), "Puzzle {i} is empty");
            // Player is always first
            assert_eq!(vehicles[0].color_index, 0, "Puzzle {i} player not index 0");
            assert_eq!(
                vehicles[0].orientation,
                Orientation::Horizontal,
                "Puzzle {i} player not horizontal"
            );
            assert_eq!(vehicles[0].row, 2, "Puzzle {i} player not on row 2");
        }
    }

    #[test]
    fn test_load_puzzle_wraps_index() {
        let v1 = load_puzzle(0);
        let v2 = load_puzzle(PUZZLES.len());
        assert_eq!(v1.len(), v2.len());
    }

    #[test]
    fn test_puzzle_count() {
        assert!(PUZZLES.len() >= 8);
    }

    // ── RushHour app construction ───────────────────────────────────

    #[test]
    fn test_new_app() {
        let app = RushHour::new();
        assert_eq!(app.puzzle_index, 0);
        assert_eq!(app.selected, 0);
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
        assert!(app.undo_stack.is_empty());
        assert!(!app.selecting_puzzle);
    }

    #[test]
    fn test_load_puzzle_at() {
        let mut app = RushHour::new();
        app.moves = 10;
        app.status = GameStatus::Won;
        app.load_puzzle_at(3);
        assert_eq!(app.puzzle_index, 3);
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
        assert!(app.undo_stack.is_empty());
    }

    #[test]
    fn test_restart() {
        let mut app = RushHour::new();
        app.move_selected(1);
        app.move_selected(1);
        assert!(app.moves > 0);
        app.restart();
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_vehicle_count() {
        let app = RushHour::new();
        assert_eq!(app.vehicle_count(), app.vehicles.len());
    }

    // ── Selection ───────────────────────────────────────────────────

    #[test]
    fn test_select_next() {
        let mut app = RushHour::new();
        assert_eq!(app.selected, 0);
        app.select_next();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_select_next_wraps() {
        let mut app = RushHour::new();
        let count = app.vehicles.len();
        for _ in 0..count {
            app.select_next();
        }
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_select_prev() {
        let mut app = RushHour::new();
        app.selected = 2;
        app.select_prev();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_select_prev_wraps() {
        let mut app = RushHour::new();
        assert_eq!(app.selected, 0);
        app.select_prev();
        assert_eq!(app.selected, app.vehicles.len() - 1);
    }

    #[test]
    fn test_select_at_cell_occupied() {
        let app_orig = RushHour::new();
        let mut app = RushHour::new();
        // Player car occupies row 2, col 0 and 1 in puzzle 0
        let player_row = app_orig.vehicles[0].row;
        let player_col = app_orig.vehicles[0].col;
        app.selected = 3; // something else
        app.select_at_cell(player_row, player_col);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_select_at_cell_empty() {
        let mut app = RushHour::new();
        let prev = app.selected;
        // Find an unoccupied cell
        let occ = build_occupancy(&app.vehicles);
        let mut found = false;
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if occ[r][c].is_none() {
                    app.select_at_cell(r, c);
                    assert_eq!(app.selected, prev);
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
    }

    // ── Moving ──────────────────────────────────────────────────────

    #[test]
    fn test_move_selected_increments_moves() {
        let mut app = RushHour::new();
        // Player car at (2, 0), try to move right if possible
        if can_move(&app.vehicles, 0, 1) {
            assert!(app.move_selected(1));
            assert_eq!(app.moves, 1);
        }
    }

    #[test]
    fn test_move_selected_adds_undo() {
        let mut app = RushHour::new();
        if can_move(&app.vehicles, 0, 1) {
            app.move_selected(1);
            assert_eq!(app.undo_stack.len(), 1);
        }
    }

    #[test]
    fn test_move_selected_blocked_returns_false() {
        let mut app = RushHour::new();
        // Move left at col 0 should fail
        app.selected = 0;
        assert!(!app.move_selected(-1));
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_move_selected_after_win_returns_false() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        assert!(!app.move_selected(1));
    }

    #[test]
    fn test_move_selected_invalid_index() {
        let mut app = RushHour::new();
        app.selected = 999;
        assert!(!app.move_selected(1));
    }

    // ── Undo ────────────────────────────────────────────────────────

    #[test]
    fn test_undo_reverses_move() {
        let mut app = RushHour::new();
        let orig_col = app.vehicles[0].col;
        if can_move(&app.vehicles, 0, 1) {
            app.move_selected(1);
            assert_eq!(app.vehicles[0].col, orig_col + 1);
            app.undo();
            assert_eq!(app.vehicles[0].col, orig_col);
            assert_eq!(app.moves, 0);
        }
    }

    #[test]
    fn test_undo_empty_stack() {
        let mut app = RushHour::new();
        app.undo(); // Should not crash
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_undo_multiple() {
        let mut app = RushHour::new();
        let orig_col = app.vehicles[0].col;
        if can_move(&app.vehicles, 0, 1) && can_move(&app.vehicles, 0, 2) {
            app.move_selected(1);
            app.move_selected(1);
            assert_eq!(app.moves, 2);
            app.undo();
            app.undo();
            assert_eq!(app.vehicles[0].col, orig_col);
            assert_eq!(app.moves, 0);
        }
    }

    #[test]
    fn test_undo_does_nothing_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        app.undo_stack.push(UndoAction {
            vehicle_index: 0,
            old_row: 2,
            old_col: 0,
            new_row: 2,
            new_col: 1,
        });
        app.undo(); // Should not undo when won
        assert_eq!(app.undo_stack.len(), 1);
    }

    #[test]
    fn test_undo_stack_max_size() {
        let mut app = RushHour::new();
        // Fill undo stack beyond MAX_UNDO
        // We need to actually move, so create a simple scenario
        // Just use a single car that can slide all the way
        app.vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        // Slide back and forth many times
        for _ in 0..MAX_UNDO + 10 {
            if app.vehicles[0].col < 4 {
                app.move_selected(1);
            } else {
                app.move_selected(-1);
            }
        }
        assert!(app.undo_stack.len() <= MAX_UNDO);
    }

    // ── Key events ──────────────────────────────────────────────────

    #[test]
    fn test_key_tab_selects_next() {
        let mut app = RushHour::new();
        assert_eq!(app.selected, 0);
        app.handle_event(&key_event(Key::Tab));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_key_shift_tab_selects_prev() {
        let mut app = RushHour::new();
        app.selected = 2;
        app.handle_event(&key_event_shift(Key::Tab));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_key_z_undo() {
        let mut app = RushHour::new();
        let orig = app.vehicles[0].col;
        if can_move(&app.vehicles, 0, 1) {
            app.move_selected(1);
            app.handle_event(&key_event(Key::Z));
            assert_eq!(app.vehicles[0].col, orig);
        }
    }

    #[test]
    fn test_key_r_restarts() {
        let mut app = RushHour::new();
        app.moves = 5;
        app.handle_event(&key_event(Key::R));
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_key_n_next_puzzle() {
        let mut app = RushHour::new();
        assert_eq!(app.puzzle_index, 0);
        app.handle_event(&key_event(Key::N));
        assert_eq!(app.puzzle_index, 1);
    }

    #[test]
    fn test_key_n_wraps_around() {
        let mut app = RushHour::new();
        app.puzzle_index = PUZZLES.len() - 1;
        app.load_puzzle_at(app.puzzle_index);
        app.handle_event(&key_event(Key::N));
        assert_eq!(app.puzzle_index, 0);
    }

    #[test]
    fn test_key_p_opens_puzzle_select() {
        let mut app = RushHour::new();
        app.handle_event(&key_event(Key::P));
        assert!(app.selecting_puzzle);
    }

    #[test]
    fn test_key_escape_closes_puzzle_select() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Escape));
        assert!(!app.selecting_puzzle);
    }

    #[test]
    fn test_puzzle_select_up_down() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.puzzle_select_cursor = 0;
        app.handle_event(&key_event(Key::Down));
        assert_eq!(app.puzzle_select_cursor, 1);
        app.handle_event(&key_event(Key::Up));
        assert_eq!(app.puzzle_select_cursor, 0);
    }

    #[test]
    fn test_puzzle_select_up_at_top() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.puzzle_select_cursor = 0;
        app.handle_event(&key_event(Key::Up));
        assert_eq!(app.puzzle_select_cursor, 0);
    }

    #[test]
    fn test_puzzle_select_down_at_bottom() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.puzzle_select_cursor = PUZZLES.len() - 1;
        app.handle_event(&key_event(Key::Down));
        assert_eq!(app.puzzle_select_cursor, PUZZLES.len() - 1);
    }

    #[test]
    fn test_puzzle_select_enter_loads() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.puzzle_select_cursor = 3;
        app.handle_event(&key_event(Key::Enter));
        assert_eq!(app.puzzle_index, 3);
        assert!(!app.selecting_puzzle);
    }

    #[test]
    fn test_puzzle_select_number_keys() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num5));
        assert_eq!(app.puzzle_index, 4);
        assert!(!app.selecting_puzzle);
    }

    #[test]
    fn test_puzzle_select_intercepts_all_keys() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        let orig_selected = app.selected;
        // Tab should not select next vehicle when puzzle select is open
        app.handle_event(&key_event(Key::Tab));
        assert_eq!(app.selected, orig_selected);
    }

    #[test]
    fn test_key_left_horizontal_vehicle() {
        let mut app = RushHour::new();
        app.selected = 0;
        // Move player car right first, then left
        if can_move(&app.vehicles, 0, 1) {
            app.move_selected(1);
            let col_after_right = app.vehicles[0].col;
            app.handle_event(&key_event(Key::Left));
            assert_eq!(app.vehicles[0].col, col_after_right - 1);
        }
    }

    #[test]
    fn test_key_right_horizontal_vehicle() {
        let mut app = RushHour::new();
        app.selected = 0;
        let orig_col = app.vehicles[0].col;
        if can_move(&app.vehicles, 0, 1) {
            app.handle_event(&key_event(Key::Right));
            assert_eq!(app.vehicles[0].col, orig_col + 1);
        }
    }

    #[test]
    fn test_key_up_vertical_vehicle() {
        let mut app = RushHour::new();
        // Find a vertical vehicle
        let vi = app
            .vehicles
            .iter()
            .position(|v| v.orientation == Orientation::Vertical);
        if let Some(vi) = vi {
            app.selected = vi;
            let orig_row = app.vehicles[vi].row;
            if can_move(&app.vehicles, vi, -1) {
                app.handle_event(&key_event(Key::Up));
                assert_eq!(app.vehicles[vi].row, orig_row - 1);
            }
        }
    }

    #[test]
    fn test_key_down_vertical_vehicle() {
        let mut app = RushHour::new();
        let vi = app
            .vehicles
            .iter()
            .position(|v| v.orientation == Orientation::Vertical);
        if let Some(vi) = vi {
            app.selected = vi;
            let orig_row = app.vehicles[vi].row;
            if can_move(&app.vehicles, vi, 1) {
                app.handle_event(&key_event(Key::Down));
                assert_eq!(app.vehicles[vi].row, orig_row + 1);
            }
        }
    }

    #[test]
    fn test_key_left_on_vertical_does_nothing() {
        let mut app = RushHour::new();
        let vi = app
            .vehicles
            .iter()
            .position(|v| v.orientation == Orientation::Vertical);
        if let Some(vi) = vi {
            app.selected = vi;
            let orig = app.vehicles[vi].col;
            app.handle_event(&key_event(Key::Left));
            assert_eq!(app.vehicles[vi].col, orig);
        }
    }

    #[test]
    fn test_key_up_on_horizontal_does_nothing() {
        let mut app = RushHour::new();
        app.selected = 0; // player car, horizontal
        let orig = app.vehicles[0].row;
        app.handle_event(&key_event(Key::Up));
        assert_eq!(app.vehicles[0].row, orig);
    }

    #[test]
    fn test_keys_ignored_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        let orig_selected = app.selected;
        app.handle_event(&key_event(Key::Tab));
        assert_eq!(app.selected, orig_selected);
    }

    #[test]
    fn test_r_works_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        app.moves = 10;
        app.handle_event(&key_event(Key::R));
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_n_works_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        app.handle_event(&key_event(Key::N));
        assert_eq!(app.puzzle_index, 1);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_p_works_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        app.handle_event(&key_event(Key::P));
        assert!(app.selecting_puzzle);
    }

    // ── Mouse events ────────────────────────────────────────────────

    #[test]
    fn test_mouse_click_selects_vehicle() {
        let mut app = RushHour::new();
        app.selected = 999; // Invalid to prove click works
        let (gx, gy) = grid_origin();
        let player = &app.vehicles[0];
        let (cy, cx) = cell_pixel_pos(player.row, player.col);
        let click = mouse_click(gx + cx + CELL_SIZE / 2.0, gy + cy + CELL_SIZE / 2.0);
        app.handle_event(&click);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_mouse_click_on_empty_cell() {
        let mut app = RushHour::new();
        app.selected = 0;
        let occ = build_occupancy(&app.vehicles);
        let (gx, gy) = grid_origin();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if occ[r][c].is_none() {
                    let (cy, cx) = cell_pixel_pos(r, c);
                    let click =
                        mouse_click(gx + cx + CELL_SIZE / 2.0, gy + cy + CELL_SIZE / 2.0);
                    app.handle_event(&click);
                    assert_eq!(app.selected, 0); // unchanged
                    return;
                }
            }
        }
    }

    #[test]
    fn test_mouse_click_outside_grid() {
        let mut app = RushHour::new();
        app.selected = 0;
        let click = mouse_click(0.0, 0.0); // header area
        app.handle_event(&click);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_mouse_ignored_when_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        app.selected = 0;
        let (gx, gy) = grid_origin();
        // Click on second vehicle
        if app.vehicles.len() > 1 {
            let v = &app.vehicles[1];
            let (cy, cx) = cell_pixel_pos(v.row, v.col);
            let click = mouse_click(gx + cx + CELL_SIZE / 2.0, gy + cy + CELL_SIZE / 2.0);
            app.handle_event(&click);
            assert_eq!(app.selected, 0); // unchanged
        }
    }

    #[test]
    fn test_mouse_ignored_when_selecting_puzzle() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.selected = 0;
        let (gx, gy) = grid_origin();
        if app.vehicles.len() > 1 {
            let v = &app.vehicles[1];
            let (cy, cx) = cell_pixel_pos(v.row, v.col);
            let click = mouse_click(gx + cx + CELL_SIZE / 2.0, gy + cy + CELL_SIZE / 2.0);
            app.handle_event(&click);
            assert_eq!(app.selected, 0);
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    #[test]
    fn test_render_returns_commands() {
        let app = RushHour::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_win_overlay() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        let cmds = app.render(800.0, 600.0);
        // Should have more commands due to win overlay
        let normal_app = RushHour::new();
        let normal_cmds = normal_app.render(800.0, 600.0);
        assert!(cmds.len() > normal_cmds.len());
    }

    #[test]
    fn test_render_puzzle_select() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    // ── Win flow ────────────────────────────────────────────────────

    #[test]
    fn test_win_detection_on_move() {
        let mut app = RushHour::new();
        // Set up a simple winning scenario
        app.vehicles = vec![Vehicle {
            row: 2,
            col: 3,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        app.selected = 0;
        // Move right by 1 → col 4, tail at 5 → win
        app.move_selected(1);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn test_no_false_win() {
        let mut app = RushHour::new();
        // Move right by 1 if possible; should not win from col 0
        if can_move(&app.vehicles, 0, 1) {
            app.move_selected(1);
            // Player car was at col 0, now at col 1, tail at 2 → not a win
            assert_eq!(app.status, GameStatus::Playing);
        }
    }

    // ── Difficulty ──────────────────────────────────────────────────

    #[test]
    fn test_difficulty_label() {
        assert_eq!(Difficulty::Beginner.label(), "Beginner");
        assert_eq!(Difficulty::Intermediate.label(), "Intermediate");
        assert_eq!(Difficulty::Advanced.label(), "Advanced");
        assert_eq!(Difficulty::Expert.label(), "Expert");
    }

    #[test]
    fn test_difficulty_color_distinct() {
        // Colors for different difficulties should differ
        let b = Difficulty::Beginner.color();
        let i = Difficulty::Intermediate.color();
        let a = Difficulty::Advanced.color();
        let e = Difficulty::Expert.color();
        // Just check they are not all the same
        assert!(b != i || i != a || a != e);
    }

    #[test]
    fn test_app_difficulty() {
        let app = RushHour::new();
        let d = app.difficulty();
        // Puzzle 0 is Beginner
        assert_eq!(d, Difficulty::Beginner);
    }

    // ── Game status ─────────────────────────────────────────────────

    #[test]
    fn test_game_status_playing() {
        let app = RushHour::new();
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_game_status_won() {
        let mut app = RushHour::new();
        app.status = GameStatus::Won;
        assert_eq!(app.status, GameStatus::Won);
    }

    // ── Key release ignored ─────────────────────────────────────────

    #[test]
    fn test_key_release_ignored() {
        let mut app = RushHour::new();
        let orig = app.selected;
        let release = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&release);
        assert_eq!(app.selected, orig);
    }

    // ── Full play sequence ──────────────────────────────────────────

    #[test]
    fn test_full_play_simple() {
        // Create a simple puzzle: just the player car at col 0, nothing blocking
        let mut app = RushHour::new();
        app.vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        app.selected = 0;

        // Move right 4 times to reach col 4, tail at 5
        for _ in 0..4 {
            assert_eq!(app.status, GameStatus::Playing);
            app.handle_event(&key_event(Key::Right));
        }
        assert_eq!(app.status, GameStatus::Won);
        assert_eq!(app.moves, 4);
    }

    #[test]
    fn test_full_play_with_undo() {
        let mut app = RushHour::new();
        app.vehicles = vec![Vehicle {
            row: 2,
            col: 0,
            length: 2,
            orientation: Orientation::Horizontal,
            color_index: 0,
            label: 'X',
        }];
        app.selected = 0;

        // Move right, undo, move right again
        app.handle_event(&key_event(Key::Right));
        assert_eq!(app.vehicles[0].col, 1);
        app.handle_event(&key_event(Key::Z));
        assert_eq!(app.vehicles[0].col, 0);
        app.handle_event(&key_event(Key::Right));
        assert_eq!(app.vehicles[0].col, 1);
    }

    // ── Puzzle select number keys ───────────────────────────────────

    #[test]
    fn test_puzzle_select_num1() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.puzzle_index = 5;
        app.handle_event(&key_event(Key::Num1));
        assert_eq!(app.puzzle_index, 0);
        assert!(!app.selecting_puzzle);
    }

    #[test]
    fn test_puzzle_select_num2() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num2));
        assert_eq!(app.puzzle_index, 1);
    }

    #[test]
    fn test_puzzle_select_num3() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num3));
        assert_eq!(app.puzzle_index, 2);
    }

    #[test]
    fn test_puzzle_select_num4() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num4));
        assert_eq!(app.puzzle_index, 3);
    }

    #[test]
    fn test_puzzle_select_num6() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num6));
        assert_eq!(app.puzzle_index, 5);
    }

    #[test]
    fn test_puzzle_select_num7() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num7));
        assert_eq!(app.puzzle_index, 6);
    }

    #[test]
    fn test_puzzle_select_num8() {
        let mut app = RushHour::new();
        app.selecting_puzzle = true;
        app.handle_event(&key_event(Key::Num8));
        assert_eq!(app.puzzle_index, 7);
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_move_truck_vertical() {
        let mut vehicles = vec![Vehicle {
            row: 0,
            col: 0,
            length: 3,
            orientation: Orientation::Vertical,
            color_index: 1,
            label: 'A',
        }];
        assert!(try_move(&mut vehicles, 0, 1)); // row 0->1
        assert_eq!(vehicles[0].row, 1);
        assert!(try_move(&mut vehicles, 0, 1)); // row 1->2
        assert_eq!(vehicles[0].row, 2);
        assert!(try_move(&mut vehicles, 0, 1)); // row 2->3, tail at 5
        assert_eq!(vehicles[0].row, 3);
        assert!(!try_move(&mut vehicles, 0, 1)); // row 3->4, tail at 6 OOB
    }

    #[test]
    fn test_move_truck_horizontal() {
        let mut vehicles = vec![Vehicle {
            row: 0,
            col: 0,
            length: 3,
            orientation: Orientation::Horizontal,
            color_index: 1,
            label: 'A',
        }];
        assert!(try_move(&mut vehicles, 0, 1)); // col 0->1
        assert!(try_move(&mut vehicles, 0, 1)); // col 1->2
        assert!(try_move(&mut vehicles, 0, 1)); // col 2->3, tail at 5
        assert!(!try_move(&mut vehicles, 0, 1)); // col 3->4, tail at 6 OOB
    }

    #[test]
    fn test_two_cars_adjacent_no_move_through() {
        let vehicles = vec![
            Vehicle {
                row: 2,
                col: 0,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 0,
                label: 'X',
            },
            Vehicle {
                row: 2,
                col: 2,
                length: 2,
                orientation: Orientation::Horizontal,
                color_index: 1,
                label: 'A',
            },
        ];
        // X at 0-1, A at 2-3. X cannot move right.
        assert!(!can_move(&vehicles, 0, 1));
        // A cannot move left.
        assert!(!can_move(&vehicles, 1, -1));
    }

    #[test]
    fn test_vertical_blocked_by_horizontal() {
        let vehicles = vec![
            Vehicle {
                row: 0,
                col: 2,
                length: 2,
                orientation: Orientation::Vertical,
                color_index: 1,
                label: 'A',
            },
            Vehicle {
                row: 2,
                col: 1,
                length: 3,
                orientation: Orientation::Horizontal,
                color_index: 2,
                label: 'B',
            },
        ];
        // A occupies (0,2) and (1,2). B occupies (2,1),(2,2),(2,3).
        // A cannot move down because (2,2) is occupied by B.
        assert!(!can_move(&vehicles, 0, 1));
    }

    #[test]
    fn test_select_next_on_empty_vehicles() {
        let mut app = RushHour::new();
        app.vehicles.clear();
        app.select_next(); // Should not panic
    }

    #[test]
    fn test_select_prev_on_empty_vehicles() {
        let mut app = RushHour::new();
        app.vehicles.clear();
        app.select_prev(); // Should not panic
    }

    #[test]
    fn test_multiple_puzzle_restarts() {
        let mut app = RushHour::new();
        for _ in 0..10 {
            app.restart();
            assert_eq!(app.moves, 0);
            assert_eq!(app.status, GameStatus::Playing);
        }
    }

    #[test]
    fn test_load_all_puzzles_no_overlap() {
        for i in 0..PUZZLES.len() {
            let vehicles = load_puzzle(i);
            // Check no two vehicles overlap
            let occ = build_occupancy(&vehicles);
            let mut cell_count = 0;
            for r in 0..GRID_SIZE {
                for c in 0..GRID_SIZE {
                    if occ[r][c].is_some() {
                        cell_count += 1;
                    }
                }
            }
            let expected: usize = vehicles.iter().map(|v| v.length).sum();
            assert_eq!(
                cell_count, expected,
                "Puzzle {i} has overlapping vehicles"
            );
        }
    }

    #[test]
    fn test_puzzle_vehicles_in_bounds() {
        for i in 0..PUZZLES.len() {
            let vehicles = load_puzzle(i);
            for (vi, v) in vehicles.iter().enumerate() {
                for (r, c) in v.cells() {
                    assert!(
                        r < GRID_SIZE && c < GRID_SIZE,
                        "Puzzle {i}, vehicle {vi} out of bounds at ({r}, {c})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_render_different_puzzles() {
        for i in 0..PUZZLES.len() {
            let mut app = RushHour::new();
            app.load_puzzle_at(i);
            let cmds = app.render(800.0, 600.0);
            assert!(!cmds.is_empty(), "Puzzle {i} rendered no commands");
        }
    }

    #[test]
    fn test_cell_pixel_pos_last_cell() {
        let (x, y) = cell_pixel_pos(5, 5);
        let expected = 5.0 * (CELL_SIZE + CELL_GAP);
        assert!((x - expected).abs() < 0.001);
        assert!((y - expected).abs() < 0.001);
    }

    #[test]
    fn test_pixel_to_cell_each_cell() {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let (px, py) = cell_pixel_pos(r, c);
                let result = pixel_to_cell(px + CELL_SIZE / 2.0, py + CELL_SIZE / 2.0);
                assert_eq!(result, Some((r, c)), "Failed for cell ({r}, {c})");
            }
        }
    }
}
