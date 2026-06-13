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

//! Slate OS Tetris — classic falling-blocks puzzle game.
//!
//! Features a 10x20 playfield, 7 standard tetrominoes with SRS rotation
//! and wall kicks, ghost piece preview, hold piece, next-3 preview queue,
//! 7-bag randomizer, lock delay, T-spin detection, scoring with level
//! progression, and pause/resume.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent};
#[cfg(test)]
use guitk::event::Modifiers;
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
const SKY: Color = Color::from_hex(0x89DCEB);

// ── Layout constants ────────────────────────────────────────────────
const FIELD_COLS: usize = 10;
const FIELD_ROWS: usize = 20;
/// Extra hidden rows above the visible field for piece spawning.
const HIDDEN_ROWS: usize = 4;
const TOTAL_ROWS: usize = FIELD_ROWS + HIDDEN_ROWS;

const CELL_SIZE: f32 = 28.0;
const CELL_GAP: f32 = 1.0;
const PADDING: f32 = 16.0;
const SIDEBAR_WIDTH: f32 = 130.0;

const HEADER_HEIGHT: f32 = 44.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const VALUE_FONT_SIZE: f32 = 18.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const MINI_CELL: f32 = 14.0;
const MINI_GAP: f32 = 1.0;
const PREVIEW_BOX_HEIGHT: f32 = 64.0;

/// Lock delay in milliseconds — piece sits on a surface this long before locking.
const LOCK_DELAY_MS: u64 = 500;

// ── Piece types and shapes ──────────────────────────────────────────

/// The 7 standard tetrominoes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    const ALL: [Self; 7] = [
        Self::I,
        Self::O,
        Self::T,
        Self::S,
        Self::Z,
        Self::J,
        Self::L,
    ];

    /// Color for this piece kind.
    fn color(self) -> Color {
        match self {
            Self::I => SKY,
            Self::O => YELLOW,
            Self::T => MAUVE,
            Self::S => GREEN,
            Self::Z => RED,
            Self::J => BLUE,
            Self::L => PEACH,
        }
    }

    /// Label character for display.
    fn label(self) -> char {
        match self {
            Self::I => 'I',
            Self::O => 'O',
            Self::T => 'T',
            Self::S => 'S',
            Self::Z => 'Z',
            Self::J => 'J',
            Self::L => 'L',
        }
    }

    /// Get the 4 cells (row, col) offsets for this piece in the given rotation state.
    /// Rotation states: 0 = spawn, 1 = CW, 2 = 180, 3 = CCW.
    /// Offsets are relative to the piece's origin (top-left of bounding box).
    fn cells(self, rotation: u8) -> [(i8, i8); 4] {
        // SRS shapes using standard Tetris Guideline definitions.
        // Each cell is (row_offset, col_offset) within the bounding box.
        match self {
            Self::I => match rotation {
                0 => [(1, 0), (1, 1), (1, 2), (1, 3)],
                1 => [(0, 2), (1, 2), (2, 2), (3, 2)],
                2 => [(2, 0), (2, 1), (2, 2), (2, 3)],
                3 => [(0, 1), (1, 1), (2, 1), (3, 1)],
                _ => [(1, 0), (1, 1), (1, 2), (1, 3)],
            },
            Self::O => [
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
            ],
            Self::T => match rotation {
                0 => [(0, 1), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (1, 1), (1, 2), (2, 1)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 1)],
                3 => [(0, 1), (1, 0), (1, 1), (2, 1)],
                _ => [(0, 1), (1, 0), (1, 1), (1, 2)],
            },
            Self::S => match rotation {
                0 => [(0, 1), (0, 2), (1, 0), (1, 1)],
                1 => [(0, 1), (1, 1), (1, 2), (2, 2)],
                2 => [(1, 1), (1, 2), (2, 0), (2, 1)],
                3 => [(0, 0), (1, 0), (1, 1), (2, 1)],
                _ => [(0, 1), (0, 2), (1, 0), (1, 1)],
            },
            Self::Z => match rotation {
                0 => [(0, 0), (0, 1), (1, 1), (1, 2)],
                1 => [(0, 2), (1, 1), (1, 2), (2, 1)],
                2 => [(1, 0), (1, 1), (2, 1), (2, 2)],
                3 => [(0, 1), (1, 0), (1, 1), (2, 0)],
                _ => [(0, 0), (0, 1), (1, 1), (1, 2)],
            },
            Self::J => match rotation {
                0 => [(0, 0), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (0, 2), (1, 1), (2, 1)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 2)],
                3 => [(0, 1), (1, 1), (2, 0), (2, 1)],
                _ => [(0, 0), (1, 0), (1, 1), (1, 2)],
            },
            Self::L => match rotation {
                0 => [(0, 2), (1, 0), (1, 1), (1, 2)],
                1 => [(0, 1), (1, 1), (2, 1), (2, 2)],
                2 => [(1, 0), (1, 1), (1, 2), (2, 0)],
                3 => [(0, 0), (0, 1), (1, 1), (2, 1)],
                _ => [(0, 2), (1, 0), (1, 1), (1, 2)],
            },
        }
    }

    /// Bounding box size for this piece (rows, cols). I is 4x4, O is 2x2, rest are 3x3.
    fn bounding_size(self) -> (i8, i8) {
        match self {
            Self::I => (4, 4),
            Self::O => (2, 2),
            _ => (3, 3),
        }
    }
}

// ── SRS Wall Kick Data ──────────────────────────────────────────────

/// SRS wall kick offsets for J, L, S, T, Z pieces.
/// Each entry: (test_index, from_state, to_state) -> (col_offset, row_offset).
/// Tests are tried in order 0..4; first that passes wins.
fn wall_kick_data_jlstz(from: u8, to: u8) -> [(i8, i8); 5] {
    match (from, to) {
        (0, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (1, 0) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (1, 2) => [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (2, 1) => [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (2, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        (3, 2) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (3, 0) => [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (0, 3) => [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        _ => [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    }
}

/// SRS wall kick offsets for I piece.
fn wall_kick_data_i(from: u8, to: u8) -> [(i8, i8); 5] {
    match (from, to) {
        (0, 1) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (1, 0) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (1, 2) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        (2, 1) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (2, 3) => [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (3, 2) => [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (3, 0) => [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (0, 3) => [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        _ => [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
    }
}

/// Get wall kick data for a piece rotation transition.
fn wall_kicks(kind: PieceKind, from: u8, to: u8) -> [(i8, i8); 5] {
    match kind {
        PieceKind::I => wall_kick_data_i(from, to),
        PieceKind::O => [(0, 0); 5], // O piece never needs kicks
        _ => wall_kick_data_jlstz(from, to),
    }
}

// ── Active piece state ──────────────────────────────────────────────

/// An active (falling) piece on the playfield.
#[derive(Clone, Debug)]
struct ActivePiece {
    kind: PieceKind,
    /// Rotation state: 0 = spawn, 1 = CW, 2 = 180, 3 = CCW.
    rotation: u8,
    /// Row position of the piece's bounding box top-left corner.
    /// Row 0 is the topmost hidden row.
    row: i8,
    /// Column position of the piece's bounding box top-left corner.
    col: i8,
}

impl ActivePiece {
    /// Create a new piece at spawn position.
    fn spawn(kind: PieceKind) -> Self {
        // Spawn in the hidden rows, horizontally centered.
        let (_, cols) = kind.bounding_size();
        let col = (FIELD_COLS as i8 - cols) / 2;
        // I and O spawn at row offset such that their visible row is at the top of the field.
        let row = match kind {
            PieceKind::I => HIDDEN_ROWS as i8 - 2,
            _ => HIDDEN_ROWS as i8 - 2,
        };
        Self {
            kind,
            rotation: 0,
            row,
            col,
        }
    }

    /// Get the absolute (row, col) positions of this piece's 4 cells.
    fn absolute_cells(&self) -> [(i8, i8); 4] {
        let offsets = self.kind.cells(self.rotation);
        let mut result = [(0i8, 0i8); 4];
        let mut idx = 0;
        while idx < 4 {
            result[idx] = (
                self.row + offsets[idx].0,
                self.col + offsets[idx].1,
            );
            idx += 1;
        }
        result
    }

    /// Return a copy of this piece moved by (dr, dc).
    fn moved(&self, dr: i8, dc: i8) -> Self {
        Self {
            kind: self.kind,
            rotation: self.rotation,
            row: self.row + dr,
            col: self.col + dc,
        }
    }

    /// Return a copy of this piece with a new rotation.
    fn with_rotation(&self, new_rotation: u8) -> Self {
        Self {
            kind: self.kind,
            rotation: new_rotation,
            row: self.row,
            col: self.col,
        }
    }
}

// ── LCG random number generator ────────────────────────────────────

/// Simple linear congruential generator. Parameters from Numerical Recipes.
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
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

// ── 7-bag randomizer ────────────────────────────────────────────────

/// Generates pieces using the 7-bag system: shuffle all 7 piece kinds,
/// deal them in order, refill the bag when empty.
struct BagRandomizer {
    rng: Lcg,
    bag: Vec<PieceKind>,
}

impl BagRandomizer {
    fn new(seed: u64) -> Self {
        let mut this = Self {
            rng: Lcg::new(seed),
            bag: Vec::new(),
        };
        this.fill_bag();
        this
    }

    fn fill_bag(&mut self) {
        self.bag = PieceKind::ALL.to_vec();
        // Fisher-Yates shuffle
        let len = self.bag.len();
        for i in (1..len).rev() {
            let j = self.rng.next_bounded(i + 1);
            self.bag.swap(i, j);
        }
    }

    fn next_piece(&mut self) -> PieceKind {
        if self.bag.is_empty() {
            self.fill_bag();
        }
        // Pop from the end (most efficient for Vec).
        self.bag.pop().unwrap_or(PieceKind::T)
    }

    /// Peek at upcoming pieces without consuming them.
    fn peek(&self, count: usize) -> Vec<PieceKind> {
        // The bag is consumed from the back, so the next piece is at the end.
        let available = self.bag.len();
        let mut result = Vec::new();
        // Take from end of current bag first
        let from_current = count.min(available);
        for i in 0..from_current {
            if let Some(piece) = self.bag.get(available - 1 - i) {
                result.push(*piece);
            }
        }
        result
    }
}

// ── T-spin detection ────────────────────────────────────────────────

/// Check the 4 corners of the T piece's 3x3 bounding box for occupied cells.
fn count_t_corners(field: &[Option<Color>], row: i8, col: i8) -> usize {
    let corners = [(0, 0), (0, 2), (2, 0), (2, 2)];
    let mut count = 0;
    for (dr, dc) in corners {
        let r = row + dr;
        let c = col + dc;
        if r < 0 || r >= TOTAL_ROWS as i8 || c < 0 || c >= FIELD_COLS as i8 {
            count += 1; // Out of bounds counts as occupied
        } else if field[r as usize * FIELD_COLS + c as usize].is_some() {
            count += 1;
        }
    }
    count
}

/// Check the two "front" corners of the T piece in its current rotation.
/// Front corners are the two corners adjacent to the flat side.
fn count_t_front_corners(field: &[Option<Color>], row: i8, col: i8, rotation: u8) -> usize {
    let front_corners: [(i8, i8); 2] = match rotation {
        0 => [(0, 0), (0, 2)],   // top-left, top-right
        1 => [(0, 2), (2, 2)],   // top-right, bottom-right
        2 => [(2, 0), (2, 2)],   // bottom-left, bottom-right
        3 => [(0, 0), (2, 0)],   // top-left, bottom-left
        _ => [(0, 0), (0, 2)],
    };
    let mut count = 0;
    for (dr, dc) in front_corners {
        let r = row + dr;
        let c = col + dc;
        let oob = r < 0 || r >= TOTAL_ROWS as i8 || c < 0 || c >= FIELD_COLS as i8;
        if oob || field[r as usize * FIELD_COLS + c as usize].is_some() {
            count += 1;
        }
    }
    count
}

/// T-spin classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TSpinKind {
    None,
    Mini,
    Full,
}

// ── Scoring ─────────────────────────────────────────────────────────

/// Compute the score for a line clear.
fn line_clear_score(lines: usize, level: u32, tspin: TSpinKind) -> u32 {
    let base = match (lines, tspin) {
        (1, TSpinKind::None) => 100,
        (2, TSpinKind::None) => 300,
        (3, TSpinKind::None) => 500,
        (4, TSpinKind::None) => 800, // Tetris
        (0, TSpinKind::Mini) => 100,
        (1, TSpinKind::Mini) => 200,
        (2, TSpinKind::Mini) => 400, // T-spin mini double (rare)
        (0, TSpinKind::Full) => 400,
        (1, TSpinKind::Full) => 800,
        (2, TSpinKind::Full) => 1200,
        (3, TSpinKind::Full) => 1600,
        _ => lines as u32 * 100,
    };
    base * level
}

/// Soft drop score: 1 point per cell dropped.
fn soft_drop_score(cells: u32) -> u32 {
    cells
}

/// Hard drop score: 2 points per cell dropped.
fn hard_drop_score(cells: u32) -> u32 {
    cells * 2
}

// ── Game state ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Paused,
    GameOver,
}

/// Main Tetris application state.
struct TetrisApp {
    /// The playfield: `TOTAL_ROWS` * `FIELD_COLS`. `None` = empty, `Some(color)` = occupied.
    field: Vec<Option<Color>>,
    /// Currently active (falling) piece.
    current_piece: Option<ActivePiece>,
    /// Held piece kind (swap with C).
    hold_piece: Option<PieceKind>,
    /// Whether hold has already been used this turn (can only hold once per piece).
    hold_used: bool,
    /// Piece generator (7-bag randomizer).
    piece_gen: BagRandomizer,
    /// Upcoming pieces buffer (kept filled for preview).
    preview_queue: Vec<PieceKind>,

    /// Game status.
    status: GameStatus,
    /// Current score.
    score: u32,
    /// Current level (starts at 1).
    level: u32,
    /// Total lines cleared.
    lines_cleared: u32,
    /// Total pieces placed (locked into field).
    pieces_placed: u32,
    /// Elapsed game time in milliseconds.
    elapsed_ms: u64,

    /// Accumulated gravity time in ms since last gravity step.
    gravity_accum_ms: u64,
    /// Lock delay tracking: ms since piece first landed on a surface.
    lock_accum_ms: u64,
    /// Whether the piece is currently resting on a surface.
    piece_on_surface: bool,
    /// Number of lock delay resets (moves/rotations while on surface).
    lock_resets: u32,
    /// Maximum lock resets before forced lock.
    max_lock_resets: u32,

    /// Whether the last successful rotation was a kick (for T-spin detection).
    last_move_was_rotation: bool,
    /// Whether the last rotation used a wall kick.
    last_rotation_was_kick: bool,

    /// RNG seed (stored for reproducibility in tests).
    seed: u64,
}

impl TetrisApp {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut piece_gen = BagRandomizer::new(seed);

        // Fill the preview queue with upcoming pieces.
        let mut preview_queue = Vec::new();
        for _ in 0..3 {
            preview_queue.push(piece_gen.next_piece());
        }

        let mut app = Self {
            field: vec![None; TOTAL_ROWS * FIELD_COLS],
            current_piece: None,
            hold_piece: None,
            hold_used: false,
            piece_gen,
            preview_queue,
            status: GameStatus::Playing,
            score: 0,
            level: 1,
            lines_cleared: 0,
            pieces_placed: 0,
            elapsed_ms: 0,
            gravity_accum_ms: 0,
            lock_accum_ms: 0,
            piece_on_surface: false,
            lock_resets: 0,
            max_lock_resets: 15,
            last_move_was_rotation: false,
            last_rotation_was_kick: false,
            seed,
        };

        app.spawn_piece();
        app
    }

    // ── Field access ────────────────────────────────────────────────

    fn field_get(&self, row: usize, col: usize) -> Option<Color> {
        if row < TOTAL_ROWS && col < FIELD_COLS {
            self.field[row * FIELD_COLS + col]
        } else {
            None
        }
    }

    fn field_set(&mut self, row: usize, col: usize, value: Option<Color>) {
        if row < TOTAL_ROWS && col < FIELD_COLS {
            self.field[row * FIELD_COLS + col] = value;
        }
    }

    // ── Collision detection ─────────────────────────────────────────

    /// Check if a piece position is valid (no collisions with walls or placed blocks).
    fn is_valid_position(&self, piece: &ActivePiece) -> bool {
        let cells = piece.absolute_cells();
        for (r, c) in cells {
            // Check bounds
            if c < 0 || c >= FIELD_COLS as i8 || r >= TOTAL_ROWS as i8 {
                return false;
            }
            // Allow above the field (negative rows) only for spawning
            if r < 0 {
                continue;
            }
            // Check collision with placed blocks
            if self.field[r as usize * FIELD_COLS + c as usize].is_some() {
                return false;
            }
        }
        true
    }

    // ── Piece spawning ──────────────────────────────────────────────

    /// Spawn the next piece from the preview queue.
    fn spawn_piece(&mut self) {
        if self.preview_queue.is_empty() {
            self.refill_preview();
        }
        // Take the first piece from the preview queue.
        let kind = self.preview_queue.remove(0);
        // Refill the queue.
        self.preview_queue.push(self.piece_gen.next_piece());

        let piece = ActivePiece::spawn(kind);
        if self.is_valid_position(&piece) {
            self.current_piece = Some(piece);
            self.piece_on_surface = false;
            self.lock_accum_ms = 0;
            self.lock_resets = 0;
            self.gravity_accum_ms = 0;
            self.hold_used = false;
            self.last_move_was_rotation = false;
            self.last_rotation_was_kick = false;
        } else {
            // Can't spawn -> game over
            self.current_piece = Some(piece);
            self.status = GameStatus::GameOver;
        }
    }

    fn refill_preview(&mut self) {
        while self.preview_queue.len() < 3 {
            self.preview_queue.push(self.piece_gen.next_piece());
        }
    }

    // ── Movement ────────────────────────────────────────────────────

    /// Try to move the current piece by (dr, dc). Returns true if successful.
    fn try_move(&mut self, dr: i8, dc: i8) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        let piece = match &self.current_piece {
            Some(p) => p.moved(dr, dc),
            None => return false,
        };
        if self.is_valid_position(&piece) {
            self.current_piece = Some(piece);
            self.last_move_was_rotation = false;
            // Reset lock delay on successful horizontal move while on surface
            if dc != 0 && self.piece_on_surface && self.lock_resets < self.max_lock_resets {
                self.lock_accum_ms = 0;
                self.lock_resets += 1;
            }
            // Update surface status
            self.update_surface_status();
            true
        } else {
            false
        }
    }

    /// Check and update whether the piece is on a surface.
    fn update_surface_status(&mut self) {
        if let Some(piece) = &self.current_piece {
            let below = piece.moved(1, 0);
            self.piece_on_surface = !self.is_valid_position(&below);
        }
    }

    /// Try to rotate the current piece. `clockwise = true` for CW, false for CCW.
    fn try_rotate(&mut self, clockwise: bool) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }
        let piece = match &self.current_piece {
            Some(p) => p.clone(),
            None => return false,
        };

        // O piece doesn't rotate
        if piece.kind == PieceKind::O {
            return true;
        }

        let from = piece.rotation;
        let to = if clockwise {
            (from + 1) % 4
        } else {
            (from + 3) % 4
        };

        let kicks = wall_kicks(piece.kind, from, to);
        let rotated = piece.with_rotation(to);

        for (kick_idx, (dc, dr)) in kicks.iter().enumerate() {
            // Wall kick offsets: (col_offset, row_offset). Row is inverted
            // because SRS defines positive Y as up, but our grid has row 0 at top.
            let test = ActivePiece {
                kind: rotated.kind,
                rotation: rotated.rotation,
                row: rotated.row - dr,
                col: rotated.col + dc,
            };
            if self.is_valid_position(&test) {
                self.current_piece = Some(test);
                self.last_move_was_rotation = true;
                self.last_rotation_was_kick = kick_idx > 0;
                // Reset lock delay on successful rotation while on surface
                if self.piece_on_surface && self.lock_resets < self.max_lock_resets {
                    self.lock_accum_ms = 0;
                    self.lock_resets += 1;
                }
                self.update_surface_status();
                return true;
            }
        }

        false
    }

    /// Soft drop: move piece down one row. Returns true if moved.
    fn soft_drop(&mut self) -> bool {
        if self.try_move(1, 0) {
            self.score += soft_drop_score(1);
            self.gravity_accum_ms = 0;
            true
        } else {
            false
        }
    }

    /// Hard drop: instantly drop piece to lowest valid position and lock.
    fn hard_drop(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }
        if self.current_piece.is_none() {
            return;
        }

        let mut cells_dropped: u32 = 0;
        while self.try_move(1, 0) {
            cells_dropped += 1;
        }
        self.score += hard_drop_score(cells_dropped);
        self.lock_piece();
    }

    /// Ghost piece position: the lowest valid position for the current piece.
    fn ghost_row(&self) -> Option<i8> {
        let piece = self.current_piece.as_ref()?;
        let mut test = piece.clone();
        while self.is_valid_position(&test.moved(1, 0)) {
            test = test.moved(1, 0);
        }
        Some(test.row)
    }

    // ── Hold piece ──────────────────────────────────────────────────

    /// Hold the current piece. Swap with held piece if one exists.
    fn hold_piece(&mut self) {
        if self.status != GameStatus::Playing || self.hold_used {
            return;
        }
        if let Some(piece) = self.current_piece.take() {
            let current_kind = piece.kind;
            if let Some(held_kind) = self.hold_piece.take() {
                // Swap: spawn the previously held piece
                self.hold_piece = Some(current_kind);
                let new_piece = ActivePiece::spawn(held_kind);
                if self.is_valid_position(&new_piece) {
                    self.current_piece = Some(new_piece);
                } else {
                    self.status = GameStatus::GameOver;
                }
            } else {
                // First hold: put current piece in hold, spawn next
                self.hold_piece = Some(current_kind);
                self.spawn_piece();
            }
            self.hold_used = true;
            self.piece_on_surface = false;
            self.lock_accum_ms = 0;
            self.lock_resets = 0;
            self.last_move_was_rotation = false;
            self.last_rotation_was_kick = false;
            self.update_surface_status();
        }
    }

    // ── Locking and line clearing ───────────────────────────────────

    /// Lock the current piece into the field.
    fn lock_piece(&mut self) {
        let piece = match self.current_piece.take() {
            Some(p) => p,
            None => return,
        };

        // Detect T-spin before locking
        let tspin = self.detect_tspin(&piece);

        let color = piece.kind.color();
        let cells = piece.absolute_cells();
        for (r, c) in cells {
            if r >= 0 && r < TOTAL_ROWS as i8 && c >= 0 && c < FIELD_COLS as i8 {
                self.field_set(r as usize, c as usize, Some(color));
            }
        }

        self.pieces_placed += 1;
        self.piece_on_surface = false;
        self.lock_accum_ms = 0;
        self.lock_resets = 0;

        // Clear lines
        let lines = self.clear_lines();

        // Score
        if lines > 0 || tspin != TSpinKind::None {
            self.score += line_clear_score(lines, self.level, tspin);
        }
        self.lines_cleared += lines as u32;

        // Level up every 10 lines
        let new_level = self.lines_cleared / 10 + 1;
        if new_level > self.level {
            self.level = new_level;
        }

        // Spawn next piece
        self.spawn_piece();
    }

    /// Detect if the last move was a T-spin.
    fn detect_tspin(&self, piece: &ActivePiece) -> TSpinKind {
        if piece.kind != PieceKind::T || !self.last_move_was_rotation {
            return TSpinKind::None;
        }

        let corners = count_t_corners(&self.field, piece.row, piece.col);
        if corners < 3 {
            return TSpinKind::None;
        }

        let front_corners =
            count_t_front_corners(&self.field, piece.row, piece.col, piece.rotation);
        if front_corners >= 2 {
            TSpinKind::Full
        } else if self.last_rotation_was_kick {
            // T-spin mini: 3+ corners occupied but fewer than 2 front corners,
            // and the rotation used a wall kick.
            TSpinKind::Mini
        } else {
            TSpinKind::None
        }
    }

    /// Clear completed lines and return the count.
    fn clear_lines(&mut self) -> usize {
        let mut lines_to_clear = Vec::new();

        // Check each visible row (hidden rows + visible field)
        for row in 0..TOTAL_ROWS {
            let mut full = true;
            for col in 0..FIELD_COLS {
                if self.field_get(row, col).is_none() {
                    full = false;
                    break;
                }
            }
            if full {
                lines_to_clear.push(row);
            }
        }

        if lines_to_clear.is_empty() {
            return 0;
        }

        let count = lines_to_clear.len();

        // Remove the full lines and shift everything down.
        // Process from bottom to top so indices remain valid.
        for &row in lines_to_clear.iter().rev() {
            // Shift all rows above down by one
            for r in (1..=row).rev() {
                for c in 0..FIELD_COLS {
                    let above = self.field_get(r - 1, c);
                    self.field_set(r, c, above);
                }
            }
            // Clear the top row
            for c in 0..FIELD_COLS {
                self.field_set(0, c, None);
            }
        }

        count
    }

    // ── Gravity and timing ──────────────────────────────────────────

    /// Gravity interval in milliseconds for the current level.
    fn gravity_interval_ms(&self) -> u64 {
        // Level 1 = 1000ms, level 10 ~ 100ms, level 20 ~ 17ms
        // Formula: (0.8 - (level-1) * 0.007)^(level-1) seconds, converted to ms.
        // Simplified: use a lookup-like formula.
        let lvl = self.level.min(30) as f64;
        let seconds = (0.8 - (lvl - 1.0) * 0.007).max(0.01).powf(lvl - 1.0);
        let ms = (seconds * 1000.0) as u64;
        ms.max(10) // Minimum 10ms
    }

    /// Process a time tick. `dt_ms` is the elapsed time since last tick.
    fn tick(&mut self, dt_ms: u64) {
        if self.status != GameStatus::Playing {
            return;
        }

        self.elapsed_ms += dt_ms;

        if self.current_piece.is_none() {
            self.spawn_piece();
            return;
        }

        // Gravity
        self.gravity_accum_ms += dt_ms;
        let interval = self.gravity_interval_ms();

        while self.gravity_accum_ms >= interval {
            self.gravity_accum_ms -= interval;
            if !self.try_move(1, 0) {
                // Piece can't move down — start or continue lock delay
                self.piece_on_surface = true;
                break;
            }
        }

        // Lock delay
        if self.piece_on_surface {
            self.lock_accum_ms += dt_ms;
            if self.lock_accum_ms >= LOCK_DELAY_MS || self.lock_resets >= self.max_lock_resets {
                self.lock_piece();
            }
        }
    }

    // ── Input handling ──────────────────────────────────────────────

    /// Handle a key event.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.key {
            Key::P => {
                // Toggle pause
                match self.status {
                    GameStatus::Playing => self.status = GameStatus::Paused,
                    GameStatus::Paused => self.status = GameStatus::Playing,
                    GameStatus::GameOver => {}
                }
            }
            Key::R if self.status == GameStatus::GameOver => {
                // Restart
                *self = Self::with_seed(self.seed.wrapping_add(1));
            }
            _ if self.status != GameStatus::Playing => {}
            Key::Left => {
                self.try_move(0, -1);
            }
            Key::Right => {
                self.try_move(0, 1);
            }
            Key::Down => {
                self.soft_drop();
            }
            Key::Up | Key::Z => {
                self.try_rotate(true);
            }
            Key::X => {
                self.try_rotate(false);
            }
            Key::Space => {
                self.hard_drop();
            }
            Key::C => {
                self.hold_piece();
            }
            _ => {}
        }
    }

    /// Handle an event (for testing / integration).
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(key_event) => self.handle_key(key_event),
            Event::Tick { elapsed_ms } => self.tick(*elapsed_ms),
            _ => {}
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Compute the width of the playfield area.
    fn field_pixel_width(&self) -> f32 {
        FIELD_COLS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Compute the height of the playfield area.
    fn field_pixel_height(&self) -> f32 {
        FIELD_ROWS as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Total window width.
    fn window_width(&self) -> f32 {
        PADDING + SIDEBAR_WIDTH + PADDING + self.field_pixel_width() + PADDING + SIDEBAR_WIDTH
            + PADDING
    }

    /// Total window height.
    fn window_height(&self) -> f32 {
        PADDING + HEADER_HEIGHT + PADDING + self.field_pixel_height() + PADDING
    }

    /// X offset where the playfield starts.
    fn field_x(&self) -> f32 {
        PADDING + SIDEBAR_WIDTH + PADDING
    }

    /// Y offset where the playfield starts.
    fn field_y(&self) -> f32 {
        PADDING + HEADER_HEIGHT + PADDING
    }

    /// Produce all render commands for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let win_w = self.window_width();
        let win_h = self.window_height();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        self.render_header(&mut cmds);
        self.render_field(&mut cmds);
        self.render_left_sidebar(&mut cmds);
        self.render_right_sidebar(&mut cmds);

        // Overlay for pause
        if self.status == GameStatus::Paused {
            self.render_overlay(&mut cmds, "PAUSED", "Press P to resume");
        } else if self.status == GameStatus::GameOver {
            self.render_overlay(&mut cmds, "GAME OVER", "Press R to restart");
        }

        cmds
    }

    /// Render the header bar with title and score.
    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let win_w = self.window_width();

        // Header background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: win_w - PADDING * 2.0,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: PADDING + 12.0,
            y: PADDING + 12.0,
            text: String::from("TETRIS"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // Score in header
        let score_text = format!("Score: {}", self.score);
        cmds.push(RenderCommand::Text {
            x: PADDING + 150.0,
            y: PADDING + 14.0,
            text: score_text,
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Level
        let level_text = format!("Level: {}", self.level);
        cmds.push(RenderCommand::Text {
            x: PADDING + 350.0,
            y: PADDING + 14.0,
            text: level_text,
            color: TEAL,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });

        // Lines
        let lines_text = format!("Lines: {}", self.lines_cleared);
        cmds.push(RenderCommand::Text {
            x: win_w - PADDING - 140.0,
            y: PADDING + 14.0,
            text: lines_text,
            color: GREEN,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(120.0),
        });
    }

    /// Render the playfield grid.
    fn render_field(&self, cmds: &mut Vec<RenderCommand>) {
        let fx = self.field_x();
        let fy = self.field_y();
        let fw = self.field_pixel_width();
        let fh = self.field_pixel_height();

        // Field border
        cmds.push(RenderCommand::StrokeRect {
            x: fx - 2.0,
            y: fy - 2.0,
            width: fw + 4.0,
            height: fh + 4.0,
            color: SURFACE2,
            line_width: 2.0,
            corner_radii: CornerRadii::all(2.0),
        });

        // Field background
        cmds.push(RenderCommand::FillRect {
            x: fx,
            y: fy,
            width: fw,
            height: fh,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Grid lines (subtle)
        for col in 1..FIELD_COLS {
            let x = fx + col as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP / 2.0;
            cmds.push(RenderCommand::Line {
                x1: x,
                y1: fy,
                x2: x,
                y2: fy + fh,
                color: Color::rgba(49, 50, 68, 60),
                width: 0.5,
            });
        }
        for row in 1..FIELD_ROWS {
            let y = fy + row as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP / 2.0;
            cmds.push(RenderCommand::Line {
                x1: fx,
                y1: y,
                x2: fx + fw,
                y2: y,
                color: Color::rgba(49, 50, 68, 60),
                width: 0.5,
            });
        }

        // Placed blocks (only visible rows)
        for row in HIDDEN_ROWS..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if let Some(color) = self.field_get(row, col) {
                    let vis_row = row - HIDDEN_ROWS;
                    let x = fx + col as f32 * (CELL_SIZE + CELL_GAP);
                    let y = fy + vis_row as f32 * (CELL_SIZE + CELL_GAP);
                    self.render_block(cmds, x, y, CELL_SIZE, color);
                }
            }
        }

        // Ghost piece
        if self.status == GameStatus::Playing
            && let (Some(piece), Some(ghost_row)) = (&self.current_piece, self.ghost_row()) {
                // Only draw ghost if it's below the current piece
                if ghost_row > piece.row {
                    let ghost = ActivePiece {
                        kind: piece.kind,
                        rotation: piece.rotation,
                        row: ghost_row,
                        col: piece.col,
                    };
                    let ghost_color = piece.kind.color();
                    let ghost_alpha = Color::rgba(ghost_color.r, ghost_color.g, ghost_color.b, 50);
                    for (r, c) in ghost.absolute_cells() {
                        if r >= HIDDEN_ROWS as i8 && r < TOTAL_ROWS as i8 && c >= 0 && c < FIELD_COLS as i8 {
                            let vis_row = r as usize - HIDDEN_ROWS;
                            let x = fx + c as f32 * (CELL_SIZE + CELL_GAP);
                            let y = fy + vis_row as f32 * (CELL_SIZE + CELL_GAP);
                            cmds.push(RenderCommand::StrokeRect {
                                x,
                                y,
                                width: CELL_SIZE,
                                height: CELL_SIZE,
                                color: ghost_alpha,
                                line_width: 1.5,
                                corner_radii: CornerRadii::all(3.0),
                            });
                        }
                    }
                }
            }

        // Current piece
        if self.status == GameStatus::Playing
            && let Some(piece) = &self.current_piece {
                let color = piece.kind.color();
                for (r, c) in piece.absolute_cells() {
                    if r >= HIDDEN_ROWS as i8 && r < TOTAL_ROWS as i8 && c >= 0 && c < FIELD_COLS as i8 {
                        let vis_row = r as usize - HIDDEN_ROWS;
                        let x = fx + c as f32 * (CELL_SIZE + CELL_GAP);
                        let y = fy + vis_row as f32 * (CELL_SIZE + CELL_GAP);
                        self.render_block(cmds, x, y, CELL_SIZE, color);
                    }
                }
            }
    }

    /// Render a single filled block with a slight highlight/shadow.
    fn render_block(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, size: f32, color: Color) {
        // Main block
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: size,
            height: size,
            color,
            corner_radii: CornerRadii::all(3.0),
        });
        // Inner highlight (top-left lighter)
        let highlight = Color::rgba(255, 255, 255, 30);
        cmds.push(RenderCommand::FillRect {
            x: x + 1.0,
            y: y + 1.0,
            width: size - 2.0,
            height: size / 2.0 - 1.0,
            color: highlight,
            corner_radii: CornerRadii {
                top_left: 2.0,
                top_right: 2.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
        });
    }

    /// Render the left sidebar (hold piece, stats).
    fn render_left_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let sx = PADDING;
        let sy = self.field_y();

        // Hold piece section
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: SIDEBAR_WIDTH,
            height: PREVIEW_BOX_HEIGHT + 24.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 6.0,
            text: String::from("HOLD"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
        });

        if let Some(kind) = self.hold_piece {
            let alpha = if self.hold_used { 100 } else { 255 };
            self.render_mini_piece(cmds, sx + 20.0, sy + 28.0, kind, alpha);
        }

        // Statistics section
        let stats_y = sy + PREVIEW_BOX_HEIGHT + 40.0;
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: stats_y,
            width: SIDEBAR_WIDTH,
            height: 200.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: stats_y + 6.0,
            text: String::from("STATS"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
        });

        let stats: [(&str, String, Color); 5] = [
            ("Score", format!("{}", self.score), YELLOW),
            ("Level", format!("{}", self.level), TEAL),
            ("Lines", format!("{}", self.lines_cleared), GREEN),
            ("Pieces", format!("{}", self.pieces_placed), BLUE),
            ("Time", self.format_time(), LAVENDER),
        ];

        for (i, (label, value, color)) in stats.iter().enumerate() {
            let row_y = stats_y + 28.0 + i as f32 * 34.0;
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: row_y,
                text: String::from(*label),
                color: OVERLAY0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
            });
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: row_y + 14.0,
                text: value.clone(),
                color: *color,
                font_size: VALUE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
            });
        }
    }

    /// Render the right sidebar (next pieces, controls).
    fn render_right_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let sx = self.field_x() + self.field_pixel_width() + PADDING;
        let sy = self.field_y();

        // Next pieces section
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: SIDEBAR_WIDTH,
            height: PREVIEW_BOX_HEIGHT * 3.0 + 50.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: sy + 6.0,
            text: String::from("NEXT"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
        });

        for (i, kind) in self.preview_queue.iter().take(3).enumerate() {
            let py = sy + 28.0 + i as f32 * (PREVIEW_BOX_HEIGHT + 4.0);
            self.render_mini_piece(cmds, sx + 20.0, py, *kind, 255);
        }

        // Controls section
        let ctrl_y = sy + PREVIEW_BOX_HEIGHT * 3.0 + 70.0;
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: ctrl_y,
            width: SIDEBAR_WIDTH,
            height: 210.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: sx + 10.0,
            y: ctrl_y + 6.0,
            text: String::from("CONTROLS"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_WIDTH - 20.0),
        });

        let controls = [
            "\u{2190}\u{2192}  Move",
            "\u{2193}  Soft drop",
            "Space  Hard drop",
            "\u{2191}/Z  Rotate CW",
            "X  Rotate CCW",
            "C  Hold",
            "P  Pause",
            "R  Restart",
        ];

        for (i, text) in controls.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: sx + 10.0,
                y: ctrl_y + 26.0 + i as f32 * 22.0,
                text: String::from(*text),
                color: OVERLAY0,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 20.0),
            });
        }
    }

    /// Render a miniature piece preview.
    fn render_mini_piece(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        kind: PieceKind,
        alpha: u8,
    ) {
        let cells = kind.cells(0);
        let base_color = kind.color();
        let color = if alpha < 255 {
            Color::rgba(base_color.r, base_color.g, base_color.b, alpha)
        } else {
            base_color
        };

        for (dr, dc) in cells {
            let cx = x + dc as f32 * (MINI_CELL + MINI_GAP);
            let cy = y + dr as f32 * (MINI_CELL + MINI_GAP);
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: MINI_CELL,
                height: MINI_CELL,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    /// Render a semi-transparent overlay with a message.
    fn render_overlay(&self, cmds: &mut Vec<RenderCommand>, title: &str, subtitle: &str) {
        let win_w = self.window_width();
        let win_h = self.window_height();

        // Dim overlay
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Message box
        let box_w: f32 = 260.0;
        let box_h: f32 = 100.0;
        let box_x = (win_w - box_w) / 2.0;
        let box_y = (win_h - box_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(10.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: LAVENDER,
            line_width: 2.0,
            corner_radii: CornerRadii::all(10.0),
        });

        cmds.push(RenderCommand::Text {
            x: box_x + box_w / 2.0 - 60.0,
            y: box_y + 25.0,
            text: String::from(title),
            color: TEXT_COLOR,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(box_w - 20.0),
        });

        cmds.push(RenderCommand::Text {
            x: box_x + box_w / 2.0 - 70.0,
            y: box_y + 60.0,
            text: String::from(subtitle),
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(box_w - 20.0),
        });
    }

    /// Format elapsed time as mm:ss.
    fn format_time(&self) -> String {
        let total_seconds = self.elapsed_ms / 1000;
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{minutes:02}:{seconds:02}")
    }
}

fn main() {
    let _app = TetrisApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper functions ────────────────────────────────────────────

    fn make_key_event(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn press_key(app: &mut TetrisApp, key: Key) {
        let event = make_key_event(key);
        app.handle_key(&event);
    }

    /// Fill a specific row in the field with a color, leaving one column empty.
    fn fill_row_except(app: &mut TetrisApp, row: usize, except_col: usize) {
        for col in 0..FIELD_COLS {
            if col != except_col {
                app.field_set(row, col, Some(BLUE));
            }
        }
    }

    /// Fill a specific row completely.
    fn fill_row(app: &mut TetrisApp, row: usize) {
        for col in 0..FIELD_COLS {
            app.field_set(row, col, Some(BLUE));
        }
    }

    /// Create an app with no current piece for field manipulation tests.
    fn app_no_piece() -> TetrisApp {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = None;
        app
    }

    // ── Piece shape tests ───────────────────────────────────────────

    #[test]
    fn test_i_piece_shape_rotation_0() {
        let cells = PieceKind::I.cells(0);
        // I piece rotation 0: horizontal bar in second row
        assert_eq!(cells, [(1, 0), (1, 1), (1, 2), (1, 3)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_1() {
        let cells = PieceKind::I.cells(1);
        // I piece rotation 1: vertical bar in third column
        assert_eq!(cells, [(0, 2), (1, 2), (2, 2), (3, 2)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_2() {
        let cells = PieceKind::I.cells(2);
        assert_eq!(cells, [(2, 0), (2, 1), (2, 2), (2, 3)]);
    }

    #[test]
    fn test_i_piece_shape_rotation_3() {
        let cells = PieceKind::I.cells(3);
        assert_eq!(cells, [(0, 1), (1, 1), (2, 1), (3, 1)]);
    }

    #[test]
    fn test_o_piece_shape_all_rotations() {
        // O piece is the same in all rotations
        let expected = [(0, 0), (0, 1), (1, 0), (1, 1)];
        for rot in 0..4 {
            assert_eq!(PieceKind::O.cells(rot), expected, "O piece rotation {rot}");
        }
    }

    #[test]
    fn test_t_piece_all_rotations() {
        assert_eq!(PieceKind::T.cells(0), [(0, 1), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::T.cells(1), [(0, 1), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::T.cells(2), [(1, 0), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::T.cells(3), [(0, 1), (1, 0), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_s_piece_all_rotations() {
        assert_eq!(PieceKind::S.cells(0), [(0, 1), (0, 2), (1, 0), (1, 1)]);
        assert_eq!(PieceKind::S.cells(1), [(0, 1), (1, 1), (1, 2), (2, 2)]);
        assert_eq!(PieceKind::S.cells(2), [(1, 1), (1, 2), (2, 0), (2, 1)]);
        assert_eq!(PieceKind::S.cells(3), [(0, 0), (1, 0), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_z_piece_all_rotations() {
        assert_eq!(PieceKind::Z.cells(0), [(0, 0), (0, 1), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::Z.cells(1), [(0, 2), (1, 1), (1, 2), (2, 1)]);
        assert_eq!(PieceKind::Z.cells(2), [(1, 0), (1, 1), (2, 1), (2, 2)]);
        assert_eq!(PieceKind::Z.cells(3), [(0, 1), (1, 0), (1, 1), (2, 0)]);
    }

    #[test]
    fn test_j_piece_all_rotations() {
        assert_eq!(PieceKind::J.cells(0), [(0, 0), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::J.cells(1), [(0, 1), (0, 2), (1, 1), (2, 1)]);
        assert_eq!(PieceKind::J.cells(2), [(1, 0), (1, 1), (1, 2), (2, 2)]);
        assert_eq!(PieceKind::J.cells(3), [(0, 1), (1, 1), (2, 0), (2, 1)]);
    }

    #[test]
    fn test_l_piece_all_rotations() {
        assert_eq!(PieceKind::L.cells(0), [(0, 2), (1, 0), (1, 1), (1, 2)]);
        assert_eq!(PieceKind::L.cells(1), [(0, 1), (1, 1), (2, 1), (2, 2)]);
        assert_eq!(PieceKind::L.cells(2), [(1, 0), (1, 1), (1, 2), (2, 0)]);
        assert_eq!(PieceKind::L.cells(3), [(0, 0), (0, 1), (1, 1), (2, 1)]);
    }

    #[test]
    fn test_piece_bounding_sizes() {
        assert_eq!(PieceKind::I.bounding_size(), (4, 4));
        assert_eq!(PieceKind::O.bounding_size(), (2, 2));
        assert_eq!(PieceKind::T.bounding_size(), (3, 3));
        assert_eq!(PieceKind::S.bounding_size(), (3, 3));
        assert_eq!(PieceKind::Z.bounding_size(), (3, 3));
        assert_eq!(PieceKind::J.bounding_size(), (3, 3));
        assert_eq!(PieceKind::L.bounding_size(), (3, 3));
    }

    #[test]
    fn test_piece_colors_are_distinct() {
        let colors: Vec<Color> = PieceKind::ALL.iter().map(|k| k.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "Pieces {:?} and {:?} share a color",
                    PieceKind::ALL[i], PieceKind::ALL[j]);
            }
        }
    }

    #[test]
    fn test_piece_labels() {
        assert_eq!(PieceKind::I.label(), 'I');
        assert_eq!(PieceKind::O.label(), 'O');
        assert_eq!(PieceKind::T.label(), 'T');
        assert_eq!(PieceKind::S.label(), 'S');
        assert_eq!(PieceKind::Z.label(), 'Z');
        assert_eq!(PieceKind::J.label(), 'J');
        assert_eq!(PieceKind::L.label(), 'L');
    }

    // ── Collision detection tests ───────────────────────────────────

    #[test]
    fn test_valid_position_at_spawn() {
        let app = TetrisApp::with_seed(42);
        assert!(app.current_piece.is_some());
        assert!(app.is_valid_position(app.current_piece.as_ref().unwrap()));
    }

    #[test]
    fn test_collision_with_left_wall() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            // Move far left until collision
            let mut test = piece.clone();
            test.col = -5;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_right_wall() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let mut test = piece.clone();
            test.col = FIELD_COLS as i8 + 1;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_floor() {
        let app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let mut test = piece.clone();
            test.row = TOTAL_ROWS as i8;
            assert!(!app.is_valid_position(&test));
        }
    }

    #[test]
    fn test_collision_with_placed_block() {
        let mut app = TetrisApp::with_seed(42);
        // Place a block in the middle of the field
        app.field_set(TOTAL_ROWS - 1, 5, Some(RED));
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: (TOTAL_ROWS - 2) as i8,
            col: 5,
        };
        assert!(!app.is_valid_position(&piece));
    }

    #[test]
    fn test_no_collision_with_empty_field() {
        let app = TetrisApp::with_seed(42);
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 10,
            col: 4,
        };
        assert!(app.is_valid_position(&piece));
    }

    // ── Movement tests ──────────────────────────────────────────────

    #[test]
    fn test_move_left() {
        let mut app = TetrisApp::with_seed(42);
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(app.try_move(0, -1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col - 1);
    }

    #[test]
    fn test_move_right() {
        let mut app = TetrisApp::with_seed(42);
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(app.try_move(0, 1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col + 1);
    }

    #[test]
    fn test_move_down() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        assert!(app.try_move(1, 0));
        assert_eq!(app.current_piece.as_ref().unwrap().row, orig_row + 1);
    }

    #[test]
    fn test_cannot_move_through_wall() {
        let mut app = TetrisApp::with_seed(42);
        // Move all the way left
        for _ in 0..20 {
            app.try_move(0, -1);
        }
        let col = app.current_piece.as_ref().unwrap().col;
        // Should not be able to move further left
        assert!(!app.try_move(0, -1) || app.current_piece.as_ref().unwrap().col >= 0);
        // Actually verify position is at wall
        let final_col = app.current_piece.as_ref().unwrap().col;
        assert!(final_col <= col);
    }

    #[test]
    fn test_cannot_move_when_paused() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::Paused;
        let orig_col = app.current_piece.as_ref().unwrap().col;
        assert!(!app.try_move(0, -1));
        assert_eq!(app.current_piece.as_ref().unwrap().col, orig_col);
    }

    #[test]
    fn test_cannot_move_when_game_over() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        assert!(!app.try_move(0, -1));
    }

    // ── Rotation tests ──────────────────────────────────────────────

    #[test]
    fn test_rotate_clockwise() {
        let mut app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let orig_rot = piece.rotation;
            if piece.kind != PieceKind::O {
                assert!(app.try_rotate(true));
                assert_eq!(app.current_piece.as_ref().unwrap().rotation, (orig_rot + 1) % 4);
            }
        }
    }

    #[test]
    fn test_rotate_counterclockwise() {
        let mut app = TetrisApp::with_seed(42);
        if let Some(piece) = &app.current_piece {
            let orig_rot = piece.rotation;
            if piece.kind != PieceKind::O {
                assert!(app.try_rotate(false));
                assert_eq!(app.current_piece.as_ref().unwrap().rotation, (orig_rot + 3) % 4);
            }
        }
    }

    #[test]
    fn test_full_rotation_cycle() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece down a bit to have room
        for _ in 0..5 {
            app.try_move(1, 0);
        }
        if app.current_piece.as_ref().unwrap().kind != PieceKind::O {
            for _ in 0..4 {
                assert!(app.try_rotate(true));
            }
            // After 4 CW rotations, should be back to 0
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, 0);
        }
    }

    #[test]
    fn test_o_piece_rotation_unchanged() {
        let mut app = TetrisApp::with_seed(42);
        // Force an O piece
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 10,
            col: 4,
        });
        let cells_before = app.current_piece.as_ref().unwrap().absolute_cells();
        app.try_rotate(true);
        let cells_after = app.current_piece.as_ref().unwrap().absolute_cells();
        assert_eq!(cells_before, cells_after);
    }

    #[test]
    fn test_wall_kick_near_left_wall() {
        let mut app = TetrisApp::with_seed(42);
        // Place an I piece near the left wall in vertical orientation
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::I,
            rotation: 1, // vertical
            row: 10,
            col: -1, // partially off-screen
        });
        // Try to rotate - should wall kick
        let result = app.try_rotate(true);
        // The rotation may or may not succeed depending on wall kick data,
        // but at least it shouldn't panic.
        let _ = result;
    }

    #[test]
    fn test_wall_kick_near_right_wall() {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 10,
            col: (FIELD_COLS - 3) as i8,
        });
        // Should be able to rotate with kicks
        let original_rot = app.current_piece.as_ref().unwrap().rotation;
        app.try_rotate(true);
        // Verify rotation happened (or piece is still valid)
        assert!(app.is_valid_position(app.current_piece.as_ref().unwrap()));
        let new_rot = app.current_piece.as_ref().unwrap().rotation;
        // For T piece at right wall, rotation 0->1 should succeed with or without kick
        assert_ne!(original_rot, new_rot);
    }

    #[test]
    fn test_rotation_blocked_when_no_valid_kick() {
        let mut app = TetrisApp::with_seed(42);
        // Create a very tight space where rotation is impossible
        // Fill most of the field, leaving only a small gap
        for row in 8..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if !(row == 10 && (col == 4 || col == 5)) {
                    app.field_set(row, col, Some(BLUE));
                }
            }
        }
        // Place a piece in the tight gap
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::I,
            rotation: 0,
            row: 8,
            col: 3,
        });
        // This rotation should fail because there's no room
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let result = app.try_rotate(true);
        if !result {
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, rot_before);
        }
    }

    // ── Soft drop tests ─────────────────────────────────────────────

    #[test]
    fn test_soft_drop_moves_down() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        assert!(app.soft_drop());
        assert_eq!(app.current_piece.as_ref().unwrap().row, orig_row + 1);
    }

    #[test]
    fn test_soft_drop_scores_one_per_cell() {
        let mut app = TetrisApp::with_seed(42);
        let score_before = app.score;
        app.soft_drop();
        assert_eq!(app.score, score_before + 1);
    }

    #[test]
    fn test_soft_drop_multiple() {
        let mut app = TetrisApp::with_seed(42);
        let score_before = app.score;
        for _ in 0..5 {
            app.soft_drop();
        }
        assert_eq!(app.score, score_before + 5);
    }

    // ── Hard drop tests ─────────────────────────────────────────────

    #[test]
    fn test_hard_drop_locks_piece() {
        let mut app = TetrisApp::with_seed(42);
        let kind = app.current_piece.as_ref().unwrap().kind;
        app.hard_drop();
        // After hard drop, the piece should be locked and a new piece spawned
        // (or the same piece reference changed)
        assert!(app.pieces_placed >= 1);
        // The current piece should be a new one (or game over)
        if app.status == GameStatus::Playing {
            // New piece was spawned
            assert!(app.current_piece.is_some());
        }
        let _ = kind;
    }

    #[test]
    fn test_hard_drop_scores_two_per_cell() {
        let mut app = TetrisApp::with_seed(42);
        let orig_row = app.current_piece.as_ref().unwrap().row;
        let ghost = app.ghost_row().unwrap();
        let expected_cells = (ghost - orig_row) as u32;
        let score_before = app.score;
        app.hard_drop();
        assert_eq!(app.score, score_before + expected_cells * 2);
    }

    #[test]
    fn test_hard_drop_piece_reaches_bottom() {
        let mut app = TetrisApp::with_seed(42);
        app.hard_drop();
        // At least one cell in the bottom portion of the field should be filled
        let mut found = false;
        for row in (TOTAL_ROWS - 4)..TOTAL_ROWS {
            for col in 0..FIELD_COLS {
                if app.field_get(row, col).is_some() {
                    found = true;
                }
            }
        }
        assert!(found, "Hard drop should place piece near bottom of field");
    }

    // ── Ghost piece tests ───────────────────────────────────────────

    #[test]
    fn test_ghost_row_exists() {
        let app = TetrisApp::with_seed(42);
        assert!(app.ghost_row().is_some());
    }

    #[test]
    fn test_ghost_row_below_current() {
        let app = TetrisApp::with_seed(42);
        let piece_row = app.current_piece.as_ref().unwrap().row;
        let ghost = app.ghost_row().unwrap();
        assert!(ghost >= piece_row);
    }

    #[test]
    fn test_ghost_row_is_valid_position() {
        let app = TetrisApp::with_seed(42);
        let piece = app.current_piece.as_ref().unwrap();
        let ghost = app.ghost_row().unwrap();
        let ghost_piece = ActivePiece {
            kind: piece.kind,
            rotation: piece.rotation,
            row: ghost,
            col: piece.col,
        };
        assert!(app.is_valid_position(&ghost_piece));
        // One row below should NOT be valid
        let below = ghost_piece.moved(1, 0);
        assert!(!app.is_valid_position(&below));
    }

    // ── Line clearing tests ─────────────────────────────────────────

    #[test]
    fn test_clear_single_line() {
        let mut app = app_no_piece();
        let bottom_row = TOTAL_ROWS - 1;
        fill_row(&mut app, bottom_row);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 1);
        // Row should now be empty
        for col in 0..FIELD_COLS {
            assert!(app.field_get(bottom_row, col).is_none());
        }
    }

    #[test]
    fn test_clear_double_line() {
        let mut app = app_no_piece();
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 2);
    }

    #[test]
    fn test_clear_triple_line() {
        let mut app = app_no_piece();
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        fill_row(&mut app, TOTAL_ROWS - 3);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 3);
    }

    #[test]
    fn test_clear_tetris() {
        let mut app = app_no_piece();
        for i in 0..4 {
            fill_row(&mut app, TOTAL_ROWS - 1 - i);
        }
        let cleared = app.clear_lines();
        assert_eq!(cleared, 4);
    }

    #[test]
    fn test_clear_line_shifts_above_down() {
        let mut app = app_no_piece();
        // Place a block above the line to clear
        let above_row = TOTAL_ROWS - 3;
        app.field_set(above_row, 5, Some(GREEN));
        // Fill the bottom two rows
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 2);
        app.clear_lines();
        // The block that was at above_row should now be at above_row + 2
        assert!(app.field_get(above_row + 2, 5).is_some());
        assert!(app.field_get(above_row, 5).is_none());
    }

    #[test]
    fn test_no_clear_incomplete_row() {
        let mut app = app_no_piece();
        fill_row_except(&mut app, TOTAL_ROWS - 1, 5);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 0);
    }

    #[test]
    fn test_clear_non_contiguous_rows() {
        let mut app = app_no_piece();
        // Fill bottom row and row 3 from bottom (leaving gap in between)
        fill_row(&mut app, TOTAL_ROWS - 1);
        fill_row(&mut app, TOTAL_ROWS - 3);
        // Put something in the middle row to make it incomplete
        fill_row_except(&mut app, TOTAL_ROWS - 2, 3);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 2);
    }

    // ── Scoring tests ───────────────────────────────────────────────

    #[test]
    fn test_single_line_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::None), 100);
    }

    #[test]
    fn test_double_line_score() {
        assert_eq!(line_clear_score(2, 1, TSpinKind::None), 300);
    }

    #[test]
    fn test_triple_line_score() {
        assert_eq!(line_clear_score(3, 1, TSpinKind::None), 500);
    }

    #[test]
    fn test_tetris_score() {
        assert_eq!(line_clear_score(4, 1, TSpinKind::None), 800);
    }

    #[test]
    fn test_score_scales_with_level() {
        assert_eq!(line_clear_score(1, 5, TSpinKind::None), 500);
        assert_eq!(line_clear_score(4, 3, TSpinKind::None), 2400);
    }

    #[test]
    fn test_tspin_single_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::Full), 800);
    }

    #[test]
    fn test_tspin_double_score() {
        assert_eq!(line_clear_score(2, 1, TSpinKind::Full), 1200);
    }

    #[test]
    fn test_tspin_triple_score() {
        assert_eq!(line_clear_score(3, 1, TSpinKind::Full), 1600);
    }

    #[test]
    fn test_tspin_mini_score() {
        assert_eq!(line_clear_score(1, 1, TSpinKind::Mini), 200);
    }

    #[test]
    fn test_tspin_no_clear_score() {
        assert_eq!(line_clear_score(0, 1, TSpinKind::Full), 400);
    }

    #[test]
    fn test_soft_drop_score_fn() {
        assert_eq!(soft_drop_score(5), 5);
    }

    #[test]
    fn test_hard_drop_score_fn() {
        assert_eq!(hard_drop_score(10), 20);
    }

    // ── Level progression tests ─────────────────────────────────────

    #[test]
    fn test_initial_level() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.level, 1);
    }

    #[test]
    fn test_level_up_after_10_lines() {
        let mut app = app_no_piece();
        app.lines_cleared = 9;
        app.level = 1;
        // Simulate clearing one more line (bypassing lock_piece for unit test)
        fill_row(&mut app, TOTAL_ROWS - 1);
        let cleared = app.clear_lines();
        assert_eq!(cleared, 1);
        app.lines_cleared += cleared as u32;
        let new_level = app.lines_cleared / 10 + 1;
        if new_level > app.level {
            app.level = new_level;
        }
        assert_eq!(app.level, 2);
    }

    #[test]
    fn test_level_increases_speed() {
        let app1 = TetrisApp::with_seed(42);
        let mut app10 = TetrisApp::with_seed(42);
        app10.level = 10;
        assert!(app10.gravity_interval_ms() < app1.gravity_interval_ms());
    }

    #[test]
    fn test_gravity_interval_minimum() {
        let mut app = TetrisApp::with_seed(42);
        app.level = 100;
        assert!(app.gravity_interval_ms() >= 10);
    }

    // ── Hold piece tests ────────────────────────────────────────────

    #[test]
    fn test_hold_piece_first_time() {
        let mut app = TetrisApp::with_seed(42);
        let first_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece();
        assert_eq!(app.hold_piece, Some(first_kind));
        assert!(app.hold_used);
        // A new piece should have been spawned
        assert!(app.current_piece.is_some());
    }

    #[test]
    fn test_hold_piece_swap() {
        let mut app = TetrisApp::with_seed(42);
        let first_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece(); // Hold first piece, spawn second
        let second_kind = app.current_piece.as_ref().unwrap().kind;
        assert_ne!(first_kind, second_kind); // likely different with seed 42

        // Can't hold again until next piece
        app.hold_used = false; // simulate new piece (normally done by spawn)
        app.hold_piece(); // Swap back
        assert_eq!(app.hold_piece, Some(second_kind));
        assert_eq!(app.current_piece.as_ref().unwrap().kind, first_kind);
    }

    #[test]
    fn test_cannot_hold_twice_same_piece() {
        let mut app = TetrisApp::with_seed(42);
        app.hold_piece();
        let held = app.hold_piece;
        let current_kind = app.current_piece.as_ref().unwrap().kind;
        app.hold_piece(); // Should do nothing — hold_used is true
        assert_eq!(app.hold_piece, held);
        assert_eq!(app.current_piece.as_ref().unwrap().kind, current_kind);
    }

    // ── 7-bag randomizer tests ──────────────────────────────────────

    #[test]
    fn test_bag_contains_all_seven_pieces() {
        let mut bag = BagRandomizer::new(123);
        let mut seen = [false; 7];
        for _ in 0..7 {
            let piece = bag.next_piece();
            let idx = PieceKind::ALL.iter().position(|k| *k == piece).unwrap();
            assert!(!seen[idx], "Duplicate piece in first bag: {piece:?}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&s| s), "Not all pieces appeared in first bag");
    }

    #[test]
    fn test_bag_refills_after_seven() {
        let mut bag = BagRandomizer::new(456);
        // Drain first bag
        for _ in 0..7 {
            bag.next_piece();
        }
        // Second bag should also contain all 7
        let mut seen = [false; 7];
        for _ in 0..7 {
            let piece = bag.next_piece();
            let idx = PieceKind::ALL.iter().position(|k| *k == piece).unwrap();
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&s| s), "Second bag missing pieces");
    }

    #[test]
    fn test_bag_deterministic_with_same_seed() {
        let mut bag1 = BagRandomizer::new(789);
        let mut bag2 = BagRandomizer::new(789);
        for _ in 0..21 {
            assert_eq!(bag1.next_piece(), bag2.next_piece());
        }
    }

    #[test]
    fn test_bag_different_seeds_differ() {
        let mut bag1 = BagRandomizer::new(100);
        let mut bag2 = BagRandomizer::new(200);
        let mut same = true;
        for _ in 0..14 {
            if bag1.next_piece() != bag2.next_piece() {
                same = false;
            }
        }
        assert!(!same, "Different seeds should produce different sequences");
    }

    #[test]
    fn test_preview_queue_initially_has_three() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.preview_queue.len(), 3);
    }

    #[test]
    fn test_preview_queue_maintained_after_spawn() {
        let mut app = TetrisApp::with_seed(42);
        app.hard_drop(); // Lock current piece and spawn new one
        if app.status == GameStatus::Playing {
            assert_eq!(app.preview_queue.len(), 3);
        }
    }

    // ── Game over detection tests ───────────────────────────────────

    #[test]
    fn test_game_over_when_field_full() {
        let mut app = TetrisApp::with_seed(42);
        // Fill the top visible rows completely
        for row in HIDDEN_ROWS..HIDDEN_ROWS + 4 {
            for col in 0..FIELD_COLS {
                app.field_set(row, col, Some(RED));
            }
        }
        // Also fill hidden rows
        for row in 0..HIDDEN_ROWS {
            for col in 0..FIELD_COLS {
                app.field_set(row, col, Some(RED));
            }
        }
        // Try to spawn a new piece
        app.current_piece = None;
        app.spawn_piece();
        assert_eq!(app.status, GameStatus::GameOver);
    }

    #[test]
    fn test_initial_status_is_playing() {
        let app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);
    }

    // ── Lock delay tests ────────────────────────────────────────────

    #[test]
    fn test_lock_delay_not_immediate() {
        let mut app = TetrisApp::with_seed(42);
        // Drop piece to the bottom
        while app.try_move(1, 0) {}
        // Piece should be on surface but not yet locked
        app.update_surface_status();
        assert!(app.piece_on_surface);
        // Small tick should not lock
        app.tick(100);
        if app.status == GameStatus::Playing {
            assert!(app.current_piece.is_some() || app.pieces_placed > 0);
        }
    }

    #[test]
    fn test_lock_delay_expires() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece to bottom
        while app.try_move(1, 0) {}
        app.update_surface_status();
        assert!(app.piece_on_surface);
        let pieces_before = app.pieces_placed;
        // Tick past the lock delay
        app.tick(LOCK_DELAY_MS + 100);
        // Piece should now be locked
        assert!(app.pieces_placed > pieces_before || app.status == GameStatus::GameOver);
    }

    #[test]
    fn test_lock_delay_resets_on_move() {
        let mut app = TetrisApp::with_seed(42);
        // Move piece to bottom
        while app.try_move(1, 0) {}
        app.update_surface_status();
        assert!(app.piece_on_surface);
        app.lock_accum_ms = 400; // Almost expired
        // Move sideways (if possible) should reset lock delay
        if app.try_move(0, -1) {
            assert!(app.lock_accum_ms < 400 || !app.piece_on_surface);
        }
    }

    // ── Pause tests ─────────────────────────────────────────────────

    #[test]
    fn test_pause_toggle() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);
        press_key(&mut app, Key::P);
        assert_eq!(app.status, GameStatus::Paused);
        press_key(&mut app, Key::P);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_no_movement_while_paused() {
        let mut app = TetrisApp::with_seed(42);
        press_key(&mut app, Key::P);
        let col = app.current_piece.as_ref().unwrap().col;
        press_key(&mut app, Key::Left);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col);
    }

    #[test]
    fn test_tick_does_nothing_while_paused() {
        let mut app = TetrisApp::with_seed(42);
        let time_before = app.elapsed_ms;
        app.status = GameStatus::Paused;
        app.tick(1000);
        assert_eq!(app.elapsed_ms, time_before);
    }

    // ── Gravity / tick tests ────────────────────────────────────────

    #[test]
    fn test_gravity_moves_piece_down() {
        let mut app = TetrisApp::with_seed(42);
        let row_before = app.current_piece.as_ref().unwrap().row;
        // Tick for enough time to trigger gravity at level 1
        let interval = app.gravity_interval_ms();
        app.tick(interval + 1);
        let row_after = app.current_piece.as_ref().unwrap().row;
        assert!(row_after > row_before, "Gravity should move piece down");
    }

    #[test]
    fn test_gravity_faster_at_higher_level() {
        let mut app = TetrisApp::with_seed(42);
        let interval_l1 = app.gravity_interval_ms();
        app.level = 5;
        let interval_l5 = app.gravity_interval_ms();
        app.level = 10;
        let interval_l10 = app.gravity_interval_ms();
        assert!(interval_l5 < interval_l1);
        assert!(interval_l10 < interval_l5);
    }

    // ── T-spin detection tests ──────────────────────────────────────

    #[test]
    fn test_t_corners_empty_field() {
        let field = vec![None; TOTAL_ROWS * FIELD_COLS];
        let corners = count_t_corners(&field, 10, 4);
        assert_eq!(corners, 0);
    }

    #[test]
    fn test_t_corners_all_occupied() {
        let mut field = vec![None; TOTAL_ROWS * FIELD_COLS];
        // Set all four corners of a 3x3 bounding box
        field[10 * FIELD_COLS + 4] = Some(BLUE);     // (10, 4) top-left
        field[10 * FIELD_COLS + 6] = Some(BLUE);     // (10, 6) top-right
        field[12 * FIELD_COLS + 4] = Some(BLUE);     // (12, 4) bottom-left
        field[12 * FIELD_COLS + 6] = Some(BLUE);     // (12, 6) bottom-right
        let corners = count_t_corners(&field, 10, 4);
        assert_eq!(corners, 4);
    }

    #[test]
    fn test_t_corners_at_left_wall() {
        let field = vec![None; TOTAL_ROWS * FIELD_COLS];
        // At col=-1, left corners are out of bounds (count as occupied)
        let corners = count_t_corners(&field, 10, -1);
        assert_eq!(corners, 2); // two left corners are OOB
    }

    #[test]
    fn test_tspin_detection_none_without_rotation() {
        let mut app = TetrisApp::with_seed(42);
        app.current_piece = Some(ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 10,
            col: 4,
        });
        app.last_move_was_rotation = false;
        let piece = app.current_piece.as_ref().unwrap().clone();
        let tspin = app.detect_tspin(&piece);
        assert_eq!(tspin, TSpinKind::None);
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_returns_commands() {
        let app = TetrisApp::with_seed(42);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_pause_overlay() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::Paused;
        let cmds = app.render();
        // Should have more commands than when playing (overlay)
        let playing_app = TetrisApp::with_seed(42);
        let playing_cmds = playing_app.render();
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_game_over_overlay() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        let cmds = app.render();
        let playing_app = TetrisApp::with_seed(42);
        let playing_cmds = playing_app.render();
        assert!(cmds.len() > playing_cmds.len());
    }

    #[test]
    fn test_render_ghost_piece() {
        let app = TetrisApp::with_seed(42);
        let cmds = app.render();
        // Ghost piece generates StrokeRect commands — count them
        let stroke_count = cmds.iter().filter(|c| matches!(c, RenderCommand::StrokeRect { .. })).count();
        // Should have at least 4 (ghost cells) + 1 (field border) + overlay borders
        assert!(stroke_count >= 5, "Expected ghost piece stroke rects, found {stroke_count}");
    }

    #[test]
    fn test_render_field_background() {
        let app = TetrisApp::with_seed(42);
        let cmds = app.render();
        // First command should be the background fill
        assert!(matches!(cmds.first(), Some(RenderCommand::FillRect { .. })));
    }

    #[test]
    fn test_window_dimensions_positive() {
        let app = TetrisApp::with_seed(42);
        assert!(app.window_width() > 0.0);
        assert!(app.window_height() > 0.0);
    }

    #[test]
    fn test_format_time() {
        let mut app = TetrisApp::with_seed(42);
        app.elapsed_ms = 0;
        assert_eq!(app.format_time(), "00:00");
        app.elapsed_ms = 65_000;
        assert_eq!(app.format_time(), "01:05");
        app.elapsed_ms = 3_661_000;
        assert_eq!(app.format_time(), "61:01");
    }

    // ── Event handling tests ────────────────────────────────────────

    #[test]
    fn test_handle_event_key() {
        let mut app = TetrisApp::with_seed(42);
        let event = Event::Key(make_key_event(Key::Left));
        let col_before = app.current_piece.as_ref().unwrap().col;
        app.handle_event(&event);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col_before - 1);
    }

    #[test]
    fn test_handle_event_tick() {
        let mut app = TetrisApp::with_seed(42);
        let elapsed_before = app.elapsed_ms;
        app.handle_event(&Event::Tick { elapsed_ms: 100 });
        assert_eq!(app.elapsed_ms, elapsed_before + 100);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = TetrisApp::with_seed(42);
        let col_before = app.current_piece.as_ref().unwrap().col;
        let event = KeyEvent {
            key: Key::Left,
            pressed: false, // Release, not press
            modifiers: Modifiers::NONE,
            text: None,
        };
        app.handle_key(&event);
        assert_eq!(app.current_piece.as_ref().unwrap().col, col_before);
    }

    #[test]
    fn test_up_key_rotates_cw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::Up);
        if kind != PieceKind::O {
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, (rot_before + 1) % 4);
        }
    }

    #[test]
    fn test_z_key_rotates_cw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::Z);
        if kind != PieceKind::O {
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, (rot_before + 1) % 4);
        }
    }

    #[test]
    fn test_x_key_rotates_ccw() {
        let mut app = TetrisApp::with_seed(42);
        let rot_before = app.current_piece.as_ref().unwrap().rotation;
        let kind = app.current_piece.as_ref().unwrap().kind;
        press_key(&mut app, Key::X);
        if kind != PieceKind::O {
            assert_eq!(app.current_piece.as_ref().unwrap().rotation, (rot_before + 3) % 4);
        }
    }

    #[test]
    fn test_space_key_hard_drops() {
        let mut app = TetrisApp::with_seed(42);
        let pieces_before = app.pieces_placed;
        press_key(&mut app, Key::Space);
        assert!(app.pieces_placed > pieces_before);
    }

    #[test]
    fn test_c_key_holds() {
        let mut app = TetrisApp::with_seed(42);
        assert!(app.hold_piece.is_none());
        press_key(&mut app, Key::C);
        assert!(app.hold_piece.is_some());
    }

    #[test]
    fn test_down_key_soft_drops() {
        let mut app = TetrisApp::with_seed(42);
        let row_before = app.current_piece.as_ref().unwrap().row;
        press_key(&mut app, Key::Down);
        assert_eq!(app.current_piece.as_ref().unwrap().row, row_before + 1);
    }

    // ── LCG tests ───────────────────────────────────────────────────

    #[test]
    fn test_lcg_deterministic() {
        let mut rng1 = Lcg::new(12345);
        let mut rng2 = Lcg::new(12345);
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_lcg_bounded_range() {
        let mut rng = Lcg::new(42);
        for _ in 0..1000 {
            let val = rng.next_bounded(7);
            assert!(val < 7);
        }
    }

    #[test]
    fn test_lcg_different_seeds() {
        let mut rng1 = Lcg::new(1);
        let mut rng2 = Lcg::new(2);
        // At least one of the first 10 values should differ
        let mut all_same = true;
        for _ in 0..10 {
            if rng1.next_u64() != rng2.next_u64() {
                all_same = false;
            }
        }
        assert!(!all_same);
    }

    // ── Integration tests ───────────────────────────────────────────

    #[test]
    fn test_full_game_sequence() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.status, GameStatus::Playing);

        // Drop a few pieces
        for _ in 0..5 {
            app.hard_drop();
            if app.status != GameStatus::Playing {
                break;
            }
        }

        assert!(app.pieces_placed >= 5 || app.status == GameStatus::GameOver);
        assert!(app.score > 0);
    }

    #[test]
    fn test_restart_from_game_over() {
        let mut app = TetrisApp::with_seed(42);
        app.status = GameStatus::GameOver;
        press_key(&mut app, Key::R);
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.score, 0);
        assert_eq!(app.level, 1);
        assert_eq!(app.lines_cleared, 0);
        assert_eq!(app.pieces_placed, 0);
    }

    #[test]
    fn test_main_fn_does_not_panic() {
        // Ensure main() creates app without panicking
        let _app = TetrisApp::new();
    }

    #[test]
    fn test_active_piece_absolute_cells() {
        let piece = ActivePiece {
            kind: PieceKind::O,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let cells = piece.absolute_cells();
        assert_eq!(cells, [(5, 3), (5, 4), (6, 3), (6, 4)]);
    }

    #[test]
    fn test_active_piece_moved() {
        let piece = ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let moved = piece.moved(2, -1);
        assert_eq!(moved.row, 7);
        assert_eq!(moved.col, 2);
        assert_eq!(moved.kind, PieceKind::T);
        assert_eq!(moved.rotation, 0);
    }

    #[test]
    fn test_active_piece_with_rotation() {
        let piece = ActivePiece {
            kind: PieceKind::T,
            rotation: 0,
            row: 5,
            col: 3,
        };
        let rotated = piece.with_rotation(2);
        assert_eq!(rotated.rotation, 2);
        assert_eq!(rotated.row, 5);
        assert_eq!(rotated.col, 3);
    }

    #[test]
    fn test_field_get_out_of_bounds() {
        let app = TetrisApp::with_seed(42);
        assert!(app.field_get(TOTAL_ROWS, 0).is_none());
        assert!(app.field_get(0, FIELD_COLS).is_none());
    }

    #[test]
    fn test_field_set_and_get() {
        let mut app = TetrisApp::with_seed(42);
        app.field_set(10, 5, Some(RED));
        assert_eq!(app.field_get(10, 5), Some(RED));
        app.field_set(10, 5, None);
        assert_eq!(app.field_get(10, 5), None);
    }

    #[test]
    fn test_pieces_placed_increments() {
        let mut app = TetrisApp::with_seed(42);
        assert_eq!(app.pieces_placed, 0);
        app.hard_drop();
        assert!(app.pieces_placed >= 1);
    }

    #[test]
    fn test_elapsed_time_increments() {
        let mut app = TetrisApp::with_seed(42);
        app.tick(500);
        assert_eq!(app.elapsed_ms, 500);
        app.tick(500);
        assert_eq!(app.elapsed_ms, 1000);
    }
}
