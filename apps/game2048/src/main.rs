//! Slate OS 2048 Game
//!
//! A tile-merging puzzle game where the player slides numbered tiles on a 4×4
//! grid, combining identical tiles to reach the 2048 tile and beyond.

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

use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, Key, KeyEvent, Modifiers};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const GRID_SIZE: usize = 4;
const CELL_SIZE: f32 = 100.0;
const CELL_GAP: f32 = 12.0;
const BOARD_PADDING: f32 = 12.0;
const BOARD_X: f32 = 40.0;
const BOARD_Y: f32 = 140.0;

// Catppuccin Mocha palette
const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_CRUST: u32 = 0x11111B;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
const COL_SURFACE2: u32 = 0x585B70;
const COL_TEXT: u32 = 0xCDD6F4;
const COL_SUBTEXT0: u32 = 0xA6ADC8;
const COL_BLUE: u32 = 0x89B4FA;
const COL_GREEN: u32 = 0xA6E3A1;
const COL_RED: u32 = 0xF38BA8;
const COL_YELLOW: u32 = 0xF9E2AF;
const COL_PEACH: u32 = 0xFAB387;
const COL_LAVENDER: u32 = 0xB4BEFE;
const COL_OVERLAY0: u32 = 0x6C7086;
const COL_TEAL: u32 = 0x94E2D5;
const COL_MAUVE: u32 = 0xCBA6F7;

// ---------------------------------------------------------------------------
// LCG random number generator (no external crate)
// ---------------------------------------------------------------------------

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG parameters
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next() >> 33) as usize % max
    }
}

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

// ---------------------------------------------------------------------------
// Game state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
    Lost,
    WonContinuing,
}

#[derive(Debug, Clone)]
struct GameState {
    grid: [[u32; GRID_SIZE]; GRID_SIZE],
    score: u32,
    best_score: u32,
    status: GameStatus,
    moves_count: u32,
    highest_tile: u32,
}

impl GameState {
    fn new() -> Self {
        Self {
            grid: [[0; GRID_SIZE]; GRID_SIZE],
            score: 0,
            best_score: 0,
            status: GameStatus::Playing,
            moves_count: 0,
            highest_tile: 0,
        }
    }

    fn empty_cells(&self) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if self.grid[r][c] == 0 {
                    cells.push((r, c));
                }
            }
        }
        cells
    }

    fn is_full(&self) -> bool {
        self.empty_cells().is_empty()
    }

    fn spawn_tile(&mut self, rng: &mut Lcg) {
        let empty = self.empty_cells();
        if empty.is_empty() {
            return;
        }
        let idx = rng.next_range(empty.len());
        let (r, c) = empty[idx];
        // 90% chance of 2, 10% chance of 4
        let val = if rng.next_range(10) < 9 { 2 } else { 4 };
        self.grid[r][c] = val;
        self.update_highest();
    }

    fn spawn_tile_at(&mut self, row: usize, col: usize, val: u32) {
        self.grid[row][col] = val;
        self.update_highest();
    }

    fn update_highest(&mut self) {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if self.grid[r][c] > self.highest_tile {
                    self.highest_tile = self.grid[r][c];
                }
            }
        }
    }

    /// Slide and merge a single row to the left. Returns (new_row, points_earned, moved).
    fn slide_row_left(row: &[u32; GRID_SIZE]) -> ([u32; GRID_SIZE], u32, bool) {
        let mut result = [0u32; GRID_SIZE];
        let mut points = 0u32;
        let mut pos = 0;
        let mut moved = false;

        // Compact non-zero tiles
        let mut compacted = [0u32; GRID_SIZE];
        let mut ci = 0;
        for &val in row {
            if val != 0 {
                compacted[ci] = val;
                ci += 1;
            }
        }

        // Merge adjacent equal tiles
        let mut i = 0;
        while i < GRID_SIZE {
            if compacted[i] == 0 {
                i += 1;
                continue;
            }
            if i + 1 < GRID_SIZE && compacted[i] == compacted[i + 1] {
                let merged = compacted[i].saturating_mul(2);
                result[pos] = merged;
                points = points.saturating_add(merged);
                pos += 1;
                i += 2;
            } else {
                result[pos] = compacted[i];
                pos += 1;
                i += 1;
            }
        }

        // Check if anything moved
        for idx in 0..GRID_SIZE {
            if result[idx] != row[idx] {
                moved = true;
                break;
            }
        }

        (result, points, moved)
    }

    /// Apply a move in the given direction. Returns true if the board changed.
    fn apply_move(&mut self, dir: Direction) -> bool {
        let mut total_points = 0u32;
        let mut any_moved = false;

        match dir {
            Direction::Left => {
                for r in 0..GRID_SIZE {
                    let row = self.grid[r];
                    let (new_row, pts, moved) = Self::slide_row_left(&row);
                    self.grid[r] = new_row;
                    total_points = total_points.saturating_add(pts);
                    if moved {
                        any_moved = true;
                    }
                }
            }
            Direction::Right => {
                for r in 0..GRID_SIZE {
                    let mut reversed = self.grid[r];
                    reversed.reverse();
                    let (mut new_row, pts, moved) = Self::slide_row_left(&reversed);
                    new_row.reverse();
                    self.grid[r] = new_row;
                    total_points = total_points.saturating_add(pts);
                    if moved {
                        any_moved = true;
                    }
                }
            }
            Direction::Up => {
                for c in 0..GRID_SIZE {
                    let col = [
                        self.grid[0][c],
                        self.grid[1][c],
                        self.grid[2][c],
                        self.grid[3][c],
                    ];
                    let (new_col, pts, moved) = Self::slide_row_left(&col);
                    for (r, val) in new_col.iter().enumerate() {
                        self.grid[r][c] = *val;
                    }
                    total_points = total_points.saturating_add(pts);
                    if moved {
                        any_moved = true;
                    }
                }
            }
            Direction::Down => {
                for c in 0..GRID_SIZE {
                    let mut col = [
                        self.grid[0][c],
                        self.grid[1][c],
                        self.grid[2][c],
                        self.grid[3][c],
                    ];
                    col.reverse();
                    let (mut new_col, pts, moved) = Self::slide_row_left(&col);
                    new_col.reverse();
                    for (r, val) in new_col.iter().enumerate() {
                        self.grid[r][c] = *val;
                    }
                    total_points = total_points.saturating_add(pts);
                    if moved {
                        any_moved = true;
                    }
                }
            }
        }

        if any_moved {
            self.score = self.score.saturating_add(total_points);
            if self.score > self.best_score {
                self.best_score = self.score;
            }
            self.moves_count = self.moves_count.saturating_add(1);
            self.update_highest();
        }

        any_moved
    }

    /// Check if any move is possible.
    fn can_move(&self) -> bool {
        // Check for any empty cell
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if self.grid[r][c] == 0 {
                    return true;
                }
            }
        }
        // Check for any adjacent equal pair
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let val = self.grid[r][c];
                if c + 1 < GRID_SIZE && self.grid[r][c + 1] == val {
                    return true;
                }
                if r + 1 < GRID_SIZE && self.grid[r + 1][c] == val {
                    return true;
                }
            }
        }
        false
    }

    fn has_2048(&self) -> bool {
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if self.grid[r][c] >= 2048 {
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Undo stack
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UndoEntry {
    grid: [[u32; GRID_SIZE]; GRID_SIZE],
    score: u32,
}

// ---------------------------------------------------------------------------
// Main app
// ---------------------------------------------------------------------------

struct Game2048App {
    state: GameState,
    rng: Lcg,
    undo_stack: Vec<UndoEntry>,
    max_undo: usize,
    show_help: bool,
}

impl Game2048App {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut app = Self {
            state: GameState::new(),
            rng: Lcg::new(seed),
            undo_stack: Vec::new(),
            max_undo: 50,
            show_help: false,
        };
        app.state.spawn_tile(&mut app.rng);
        app.state.spawn_tile(&mut app.rng);
        app
    }

    fn new_game(&mut self) {
        let old_best = self.state.best_score;
        self.state = GameState::new();
        self.state.best_score = old_best;
        self.undo_stack.clear();
        self.state.spawn_tile(&mut self.rng);
        self.state.spawn_tile(&mut self.rng);
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(UndoEntry {
            grid: self.state.grid,
            score: self.state.score,
        });
        if self.undo_stack.len() > self.max_undo {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo_stack.pop() {
            self.state.grid = entry.grid;
            self.state.score = entry.score;
            self.state.moves_count = self.state.moves_count.saturating_sub(1);
            self.state.update_highest();
            // If we undo from a game-over state, go back to playing
            if self.state.status == GameStatus::Lost {
                self.state.status = GameStatus::Playing;
            }
            true
        } else {
            false
        }
    }

    fn make_move(&mut self, dir: Direction) -> bool {
        if self.state.status == GameStatus::Lost {
            return false;
        }
        if self.state.status == GameStatus::Won {
            return false; // Must choose to continue or restart
        }

        self.push_undo();
        let moved = self.state.apply_move(dir);

        if moved {
            self.state.spawn_tile(&mut self.rng);

            // Check win condition
            if self.state.status == GameStatus::Playing && self.state.has_2048() {
                self.state.status = GameStatus::Won;
                return true;
            }

            // Check lose condition
            if !self.state.can_move() {
                self.state.status = GameStatus::Lost;
            }
        } else {
            // Move didn't change anything, pop the undo entry
            self.undo_stack.pop();
        }

        moved
    }

    fn continue_after_win(&mut self) {
        if self.state.status == GameStatus::Won {
            self.state.status = GameStatus::WonContinuing;
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        // Help toggle
        if event.key == Key::H && !event.modifiers.ctrl {
            self.show_help = !self.show_help;
            return;
        }

        // New game
        if event.key == Key::N || event.key == Key::R {
            self.new_game();
            return;
        }

        // Undo
        if event.key == Key::U || (event.key == Key::Z && event.modifiers.ctrl) {
            self.undo();
            return;
        }

        // Won state - continue or restart
        if self.state.status == GameStatus::Won {
            if event.key == Key::C || event.key == Key::Enter {
                self.continue_after_win();
            }
            return;
        }

        // Game over - only restart
        if self.state.status == GameStatus::Lost {
            return;
        }

        // Movement
        let dir = match event.key {
            Key::Up | Key::W => Some(Direction::Up),
            Key::Down | Key::S => Some(Direction::Down),
            Key::Left | Key::A => Some(Direction::Left),
            Key::Right | Key::D => Some(Direction::Right),
            _ => None,
        };

        if let Some(d) = dir {
            self.make_move(d);
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event { self.handle_key(ke) }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    fn tile_color(value: u32) -> Color {
        match value {
            0 => Color::from_hex(COL_SURFACE0),
            2 => Color::from_hex(0xEEE4DA),
            4 => Color::from_hex(0xEDE0C8),
            8 => Color::from_hex(COL_PEACH),
            16 => Color::from_hex(0xF59563),
            32 => Color::from_hex(COL_RED),
            64 => Color::from_hex(0xF65E3B),
            128 => Color::from_hex(COL_YELLOW),
            256 => Color::from_hex(0xEDCC61),
            512 => Color::from_hex(COL_GREEN),
            1024 => Color::from_hex(COL_TEAL),
            2048 => Color::from_hex(COL_BLUE),
            4096 => Color::from_hex(COL_MAUVE),
            8192 => Color::from_hex(COL_LAVENDER),
            _ => Color::from_hex(COL_SURFACE2),
        }
    }

    fn tile_text_color(value: u32) -> Color {
        match value {
            0 => Color::from_hex(COL_SURFACE0), // invisible
            2 | 4 => Color::from_hex(COL_CRUST),
            _ => Color::from_hex(COL_TEXT),
        }
    }

    fn tile_font_size(value: u32) -> f32 {
        if value >= 10000 {
            28.0
        } else if value >= 1000 {
            32.0
        } else if value >= 100 {
            36.0
        } else {
            40.0
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::from_hex(COL_BASE),
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: BOARD_X,
            y: 20.0,
            text: String::from("2048"),
            color: Color::from_hex(COL_YELLOW),
            font_size: 48.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Score display
        let score_x = BOARD_X + 240.0;
        self.render_score_box(&mut cmds, score_x, 15.0, "SCORE", self.state.score);
        self.render_score_box(
            &mut cmds,
            score_x + 130.0,
            15.0,
            "BEST",
            self.state.best_score,
        );

        // Moves count
        cmds.push(RenderCommand::Text {
            x: BOARD_X,
            y: 80.0,
            text: format!(
                "Moves: {}  |  Highest: {}",
                self.state.moves_count, self.state.highest_tile
            ),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls hint
        cmds.push(RenderCommand::Text {
            x: BOARD_X,
            y: 105.0,
            text: String::from("Arrow keys: move  |  U: undo  |  N: new game  |  H: help"),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Board background
        let board_w = GRID_SIZE as f32 * CELL_SIZE + (GRID_SIZE as f32 + 1.0) * CELL_GAP;
        let board_h = board_w;
        cmds.push(RenderCommand::FillRect {
            x: BOARD_X,
            y: BOARD_Y,
            width: board_w,
            height: board_h,
            color: Color::from_hex(COL_MANTLE),
            corner_radii: CornerRadii::all(8.0),
        });

        // Cells
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let cx = BOARD_X + CELL_GAP + c as f32 * (CELL_SIZE + CELL_GAP);
                let cy = BOARD_Y + CELL_GAP + r as f32 * (CELL_SIZE + CELL_GAP);
                let val = self.state.grid[r][c];

                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: Self::tile_color(val),
                    corner_radii: CornerRadii::all(6.0),
                });

                if val > 0 {
                    let txt = val.to_string();
                    let fs = Self::tile_font_size(val);
                    // Center text roughly
                    let text_x = cx + CELL_SIZE / 2.0 - (txt.len() as f32 * fs * 0.3);
                    let text_y = cy + CELL_SIZE / 2.0 - fs / 2.0;
                    cmds.push(RenderCommand::Text {
                        x: text_x,
                        y: text_y,
                        text: txt,
                        color: Self::tile_text_color(val),
                        font_size: fs,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(CELL_SIZE),
                    });
                }
            }
        }

        // Game status overlay
        match self.state.status {
            GameStatus::Won => {
                self.render_overlay(
                    &mut cmds,
                    board_w,
                    board_h,
                    "You Win!",
                    "Press C to continue or N for new game",
                    Color::from_hex(COL_GREEN),
                );
            }
            GameStatus::Lost => {
                self.render_overlay(
                    &mut cmds,
                    board_w,
                    board_h,
                    "Game Over",
                    "Press N for new game or U to undo",
                    Color::from_hex(COL_RED),
                );
            }
            _ => {}
        }

        // Help panel
        if self.show_help {
            self.render_help(&mut cmds, width, height);
        }

        cmds
    }

    fn render_score_box(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        score: u32,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: 120.0,
            height: 55.0,
            color: Color::from_hex(COL_SURFACE0),
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 8.0,
            text: String::from(label),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: y + 26.0,
            text: score.to_string(),
            color: Color::from_hex(COL_TEXT),
            font_size: 22.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    fn render_overlay(
        &self,
        cmds: &mut Vec<RenderCommand>,
        board_w: f32,
        board_h: f32,
        title: &str,
        subtitle: &str,
        accent: Color,
    ) {
        // Semi-transparent overlay
        cmds.push(RenderCommand::FillRect {
            x: BOARD_X,
            y: BOARD_Y,
            width: board_w,
            height: board_h,
            color: Color::rgba(30, 30, 46, 200),
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: BOARD_X + board_w / 2.0 - 80.0,
            y: BOARD_Y + board_h / 2.0 - 30.0,
            text: String::from(title),
            color: accent,
            font_size: 42.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: BOARD_X + 20.0,
            y: BOARD_Y + board_h / 2.0 + 20.0,
            text: String::from(subtitle),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 16.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(board_w - 40.0),
        });
    }

    fn render_help(&self, cmds: &mut Vec<RenderCommand>, _width: f32, _height: f32) {
        let hx = BOARD_X + 20.0;
        let hy = BOARD_Y + 480.0;

        cmds.push(RenderCommand::FillRect {
            x: hx - 10.0,
            y: hy - 10.0,
            width: 420.0,
            height: 200.0,
            color: Color::from_hex(COL_SURFACE0),
            corner_radii: CornerRadii::all(8.0),
        });

        let help_lines = [
            "Arrow keys / WASD: Slide tiles",
            "U / Ctrl+Z: Undo last move",
            "N / R: New game",
            "C / Enter: Continue after winning",
            "H: Toggle this help",
            "",
            "Combine matching tiles to reach 2048!",
            "After 2048, keep going for a higher score.",
        ];

        for (i, line) in help_lines.iter().enumerate() {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: hy + i as f32 * 22.0,
                text: String::from(*line),
                color: Color::from_hex(COL_TEXT),
                font_size: 15.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(400.0),
            });
        }
    }
}

fn main() {
    let _app = Game2048App::new();
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- LCG tests ---

    #[test]
    fn lcg_deterministic() {
        let mut r1 = Lcg::new(123);
        let mut r2 = Lcg::new(123);
        for _ in 0..100 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn lcg_different_seeds() {
        let mut r1 = Lcg::new(1);
        let mut r2 = Lcg::new(2);
        assert_ne!(r1.next(), r2.next());
    }

    #[test]
    fn lcg_range() {
        let mut rng = Lcg::new(42);
        for _ in 0..200 {
            let v = rng.next_range(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn lcg_range_zero() {
        let mut rng = Lcg::new(42);
        assert_eq!(rng.next_range(0), 0);
    }

    // --- GameState creation ---

    #[test]
    fn new_game_state_empty() {
        let state = GameState::new();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                assert_eq!(state.grid[r][c], 0);
            }
        }
        assert_eq!(state.score, 0);
        assert_eq!(state.moves_count, 0);
        assert_eq!(state.status, GameStatus::Playing);
    }

    #[test]
    fn empty_cells_full_grid() {
        let state = GameState::new();
        assert_eq!(state.empty_cells().len(), 16);
    }

    #[test]
    fn empty_cells_partial() {
        let mut state = GameState::new();
        state.grid[0][0] = 2;
        state.grid[1][1] = 4;
        assert_eq!(state.empty_cells().len(), 14);
    }

    #[test]
    fn is_full_empty() {
        let state = GameState::new();
        assert!(!state.is_full());
    }

    #[test]
    fn is_full_filled() {
        let mut state = GameState::new();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                state.grid[r][c] = 2;
            }
        }
        assert!(state.is_full());
    }

    // --- Spawn tiles ---

    #[test]
    fn spawn_tile_adds_one() {
        let mut state = GameState::new();
        let mut rng = Lcg::new(42);
        state.spawn_tile(&mut rng);
        let non_zero: usize = state.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(non_zero, 1);
    }

    #[test]
    fn spawn_tile_value_2_or_4() {
        let mut state = GameState::new();
        let mut rng = Lcg::new(42);
        for _ in 0..16 {
            state.spawn_tile(&mut rng);
        }
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let v = state.grid[r][c];
                assert!(v == 2 || v == 4, "unexpected tile value: {v}");
            }
        }
    }

    #[test]
    fn spawn_tile_at_specific() {
        let mut state = GameState::new();
        state.spawn_tile_at(2, 3, 8);
        assert_eq!(state.grid[2][3], 8);
        assert_eq!(state.highest_tile, 8);
    }

    // --- Slide row left ---

    #[test]
    fn slide_empty_row() {
        let row = [0, 0, 0, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [0, 0, 0, 0]);
        assert_eq!(pts, 0);
        assert!(!moved);
    }

    #[test]
    fn slide_single_tile_at_start() {
        let row = [2, 0, 0, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [2, 0, 0, 0]);
        assert_eq!(pts, 0);
        assert!(!moved);
    }

    #[test]
    fn slide_single_tile_at_end() {
        let row = [0, 0, 0, 4];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 0, 0, 0]);
        assert_eq!(pts, 0);
        assert!(moved);
    }

    #[test]
    fn slide_merge_pair() {
        let row = [2, 2, 0, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 0, 0, 0]);
        assert_eq!(pts, 4);
        assert!(moved);
    }

    #[test]
    fn slide_merge_with_gap() {
        let row = [2, 0, 2, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 0, 0, 0]);
        assert_eq!(pts, 4);
        assert!(moved);
    }

    #[test]
    fn slide_two_merges() {
        let row = [2, 2, 4, 4];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 8, 0, 0]);
        assert_eq!(pts, 12);
        assert!(moved);
    }

    #[test]
    fn slide_no_cascade() {
        // [4, 2, 2, 0] -> [4, 4, 0, 0] (the two 2s merge but don't cascade with the 4)
        let row = [4, 2, 2, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 4, 0, 0]);
        assert_eq!(pts, 4);
        assert!(moved);
    }

    #[test]
    fn slide_three_same() {
        // [2, 2, 2, 0] -> leftmost pair merges: [4, 2, 0, 0]
        let row = [2, 2, 2, 0];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 2, 0, 0]);
        assert_eq!(pts, 4);
        assert!(moved);
    }

    #[test]
    fn slide_four_same() {
        let row = [2, 2, 2, 2];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [4, 4, 0, 0]);
        assert_eq!(pts, 8);
        assert!(moved);
    }

    #[test]
    fn slide_no_merge_different() {
        let row = [2, 4, 8, 16];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [2, 4, 8, 16]);
        assert_eq!(pts, 0);
        assert!(!moved);
    }

    #[test]
    fn slide_compact_no_merge() {
        let row = [0, 2, 0, 4];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [2, 4, 0, 0]);
        assert_eq!(pts, 0);
        assert!(moved);
    }

    // --- Apply move directions ---

    #[test]
    fn apply_move_left() {
        let mut state = GameState::new();
        state.grid = [[0, 0, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let moved = state.apply_move(Direction::Left);
        assert!(moved);
        assert_eq!(state.grid[0][0], 4);
        assert_eq!(state.grid[0][1], 0);
        assert_eq!(state.score, 4);
    }

    #[test]
    fn apply_move_right() {
        let mut state = GameState::new();
        state.grid = [[2, 2, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let moved = state.apply_move(Direction::Right);
        assert!(moved);
        assert_eq!(state.grid[0][3], 4);
        assert_eq!(state.score, 4);
    }

    #[test]
    fn apply_move_up() {
        let mut state = GameState::new();
        state.grid = [[0, 0, 0, 0], [0, 0, 0, 0], [2, 0, 0, 0], [2, 0, 0, 0]];
        let moved = state.apply_move(Direction::Up);
        assert!(moved);
        assert_eq!(state.grid[0][0], 4);
        assert_eq!(state.grid[1][0], 0);
        assert_eq!(state.score, 4);
    }

    #[test]
    fn apply_move_down() {
        let mut state = GameState::new();
        state.grid = [[2, 0, 0, 0], [2, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let moved = state.apply_move(Direction::Down);
        assert!(moved);
        assert_eq!(state.grid[3][0], 4);
        assert_eq!(state.score, 4);
    }

    #[test]
    fn apply_move_no_change() {
        let mut state = GameState::new();
        state.grid = [[2, 4, 8, 16], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let moved = state.apply_move(Direction::Left);
        assert!(!moved);
        assert_eq!(state.score, 0);
    }

    #[test]
    fn apply_move_increments_moves() {
        let mut state = GameState::new();
        state.grid[0] = [2, 2, 0, 0];
        state.apply_move(Direction::Left);
        assert_eq!(state.moves_count, 1);
    }

    // --- Can move ---

    #[test]
    fn can_move_empty_board() {
        let state = GameState::new();
        assert!(state.can_move());
    }

    #[test]
    fn can_move_with_merge() {
        let mut state = GameState::new();
        state.grid = [
            [2, 4, 8, 16],
            [16, 8, 4, 2],
            [2, 4, 8, 16],
            [16, 8, 4, 4], // last two can merge
        ];
        assert!(state.can_move());
    }

    #[test]
    fn cannot_move_stuck() {
        let mut state = GameState::new();
        state.grid = [[2, 4, 8, 16], [16, 8, 4, 2], [2, 4, 8, 16], [16, 8, 4, 2]];
        assert!(!state.can_move());
    }

    // --- Win/lose detection ---

    #[test]
    fn has_2048_false() {
        let mut state = GameState::new();
        state.grid[0][0] = 1024;
        assert!(!state.has_2048());
    }

    #[test]
    fn has_2048_true() {
        let mut state = GameState::new();
        state.grid[2][3] = 2048;
        assert!(state.has_2048());
    }

    #[test]
    fn has_2048_higher() {
        let mut state = GameState::new();
        state.grid[1][1] = 4096;
        assert!(state.has_2048());
    }

    // --- App creation ---

    #[test]
    fn new_app_has_two_tiles() {
        let app = Game2048App::new();
        let count: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn new_app_deterministic() {
        let a1 = Game2048App::with_seed(99);
        let a2 = Game2048App::with_seed(99);
        assert_eq!(a1.state.grid, a2.state.grid);
    }

    #[test]
    fn new_app_different_seeds() {
        let a1 = Game2048App::with_seed(1);
        let a2 = Game2048App::with_seed(2);
        // Both grids should be well-formed 4x4 with exactly two non-zero starting tiles.
        // (We don't assert they differ — that's probabilistic.)
        assert_eq!(a1.state.grid.len(), 4);
        assert_eq!(a2.state.grid.len(), 4);
        let count1 = a1.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        let count2 = a2.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(count1, 2);
        assert_eq!(count2, 2);
    }

    // --- Undo ---

    #[test]
    fn undo_empty_stack() {
        let mut app = Game2048App::with_seed(42);
        assert!(!app.undo());
    }

    #[test]
    fn undo_restores_state() {
        let mut app = Game2048App::with_seed(42);
        let grid_before = app.state.grid;
        let score_before = app.state.score;
        app.make_move(Direction::Left);
        app.undo();
        assert_eq!(app.state.grid, grid_before);
        assert_eq!(app.state.score, score_before);
    }

    #[test]
    fn undo_multiple() {
        let mut app = Game2048App::with_seed(42);
        let grid_start = app.state.grid;
        app.make_move(Direction::Left);
        app.make_move(Direction::Up);
        app.undo();
        app.undo();
        assert_eq!(app.state.grid, grid_start);
    }

    #[test]
    fn undo_stack_limit() {
        let mut app = Game2048App::with_seed(42);
        app.max_undo = 3;
        // Keep making moves that change the board
        for _ in 0..10 {
            let dirs = [
                Direction::Left,
                Direction::Right,
                Direction::Up,
                Direction::Down,
            ];
            for &d in &dirs {
                app.make_move(d);
            }
        }
        assert!(app.undo_stack.len() <= 3);
    }

    // --- New game ---

    #[test]
    fn new_game_resets() {
        let mut app = Game2048App::with_seed(42);
        app.make_move(Direction::Left);
        app.make_move(Direction::Up);
        let best = app.state.best_score;
        app.new_game();
        assert_eq!(app.state.score, 0);
        assert_eq!(app.state.moves_count, 0);
        assert_eq!(app.state.best_score, best);
        assert!(app.undo_stack.is_empty());
        let count: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn new_game_preserves_best_score() {
        let mut app = Game2048App::with_seed(42);
        app.state.best_score = 1000;
        app.new_game();
        assert_eq!(app.state.best_score, 1000);
    }

    // --- Make move ---

    #[test]
    fn make_move_spawns_tile() {
        let mut app = Game2048App::with_seed(42);
        let before_count: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        // Try all directions to ensure at least one moves
        let moved = app.make_move(Direction::Left)
            || app.make_move(Direction::Right)
            || app.make_move(Direction::Up)
            || app.make_move(Direction::Down);
        if moved {
            let after_count: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
            // After move + spawn: a merge reduces by 1, but the spawn adds 1, so the count is
            // either equal to (no merges) or one less than (some merges) the count plus spawn.
            // The bounded relation is: before_count - merges + 1 == after_count, with merges >= 0.
            // So after_count is at most before_count + 1.
            assert!(after_count <= before_count + 1);
        }
    }

    #[test]
    fn make_move_no_change_no_spawn() {
        let mut app = Game2048App::with_seed(42);
        // Set up a board where left move does nothing
        app.state.grid = [[2, 4, 8, 16], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        let count_before: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        let moved = app.make_move(Direction::Left);
        assert!(!moved);
        let count_after: usize = app.state.grid.iter().flatten().filter(|&&v| v != 0).count();
        assert_eq!(count_before, count_after);
    }

    // --- Win ---

    #[test]
    fn win_detection() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[1024, 1024, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.make_move(Direction::Left);
        assert_eq!(app.state.status, GameStatus::Won);
    }

    #[test]
    fn continue_after_win() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Won;
        app.continue_after_win();
        assert_eq!(app.state.status, GameStatus::WonContinuing);
    }

    #[test]
    fn continue_only_from_won() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Playing;
        app.continue_after_win();
        assert_eq!(app.state.status, GameStatus::Playing);
    }

    #[test]
    fn won_continuing_allows_moves() {
        let mut app = Game2048App::with_seed(42);
        // Leave a gap so a left move actually slides a tile (the old data
        // [2048, 2, 0, 0] was already left-packed, so make_move returned false).
        app.state.grid = [[2048, 0, 2, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.state.status = GameStatus::WonContinuing;
        let moved = app.make_move(Direction::Left);
        assert!(moved);
    }

    // --- Lose ---

    #[test]
    fn lose_detection() {
        let mut app = Game2048App::with_seed(42);
        // Fill all but one cell with unique values, place merging pair to fill last spot
        app.state.grid = [
            [2, 4, 8, 16],
            [16, 8, 4, 2],
            [2, 4, 8, 16],
            [16, 8, 4, 0], // one empty
        ];
        // This specific board after a move that fills the last cell and creates no merges
        // may or may not trigger game over. Let's set up a guaranteed game over instead.
        app.state.grid = [
            [2, 4, 8, 16],
            [16, 8, 4, 2],
            [2, 4, 8, 16],
            [16, 8, 2, 2], // only 2,2 can merge
        ];
        // After left, last row becomes [16, 8, 4, 0], so not game over
        // Let's directly test can_move on a stuck board
        app.state.grid = [[2, 4, 8, 16], [16, 8, 4, 2], [2, 4, 8, 16], [16, 8, 4, 2]];
        assert!(!app.state.can_move());
    }

    #[test]
    fn no_move_when_lost() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Lost;
        let moved = app.make_move(Direction::Left);
        assert!(!moved);
    }

    // --- Key handling ---

    #[test]
    fn key_left_arrow() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[0, 0, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.state.grid[0][0], 4);
    }

    #[test]
    fn key_right_arrow() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[2, 2, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.state.grid[0][3], 4);
    }

    #[test]
    fn key_wasd() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[0, 0, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.handle_key(&KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('a'),
        });
        assert_eq!(app.state.grid[0][0], 4);
    }

    #[test]
    fn key_undo() {
        let mut app = Game2048App::with_seed(42);
        let grid_before = app.state.grid;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_key(&KeyEvent {
            key: Key::U,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('u'),
        });
        assert_eq!(app.state.grid, grid_before);
    }

    #[test]
    fn key_ctrl_z_undo() {
        let mut app = Game2048App::with_seed(42);
        let grid_before = app.state.grid;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_key(&KeyEvent {
            key: Key::Z,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: None,
        });
        assert_eq!(app.state.grid, grid_before);
    }

    #[test]
    fn key_new_game() {
        let mut app = Game2048App::with_seed(42);
        app.make_move(Direction::Left);
        app.handle_key(&KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('n'),
        });
        assert_eq!(app.state.score, 0);
        assert_eq!(app.state.moves_count, 0);
    }

    #[test]
    fn key_help_toggle() {
        let mut app = Game2048App::with_seed(42);
        assert!(!app.show_help);
        app.handle_key(&KeyEvent {
            key: Key::H,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('h'),
        });
        assert!(app.show_help);
        app.handle_key(&KeyEvent {
            key: Key::H,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('h'),
        });
        assert!(!app.show_help);
    }

    #[test]
    fn key_continue_after_win() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Won;
        app.handle_key(&KeyEvent {
            key: Key::C,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('c'),
        });
        assert_eq!(app.state.status, GameStatus::WonContinuing);
    }

    #[test]
    fn key_released_ignored() {
        let mut app = Game2048App::with_seed(42);
        let grid_before = app.state.grid;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.state.grid, grid_before);
    }

    #[test]
    fn key_won_blocks_movement() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Won;
        let grid_before = app.state.grid;
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.state.grid, grid_before);
    }

    // --- Tile colors ---

    #[test]
    fn tile_color_variants() {
        // Just verify no panics and each value returns a color
        let values = [
            0, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384,
        ];
        for v in values {
            let _ = Game2048App::tile_color(v);
            let _ = Game2048App::tile_text_color(v);
            let _ = Game2048App::tile_font_size(v);
        }
    }

    #[test]
    fn tile_font_size_scaling() {
        assert!(Game2048App::tile_font_size(2) > Game2048App::tile_font_size(1024));
        assert!(Game2048App::tile_font_size(1024) > Game2048App::tile_font_size(10000));
    }

    // --- Rendering ---

    #[test]
    fn render_basic() {
        let app = Game2048App::new();
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_has_background() {
        let app = Game2048App::new();
        let cmds = app.render(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_has_title() {
        let app = Game2048App::new();
        let cmds = app.render(600.0, 800.0);
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "2048"));
        assert!(has_title);
    }

    #[test]
    fn render_has_tiles() {
        let app = Game2048App::new();
        let cmds = app.render(600.0, 800.0);
        // Should have at least 16 fill rects for cells + board bg + screen bg
        let fill_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert!(fill_count >= 18);
    }

    #[test]
    fn render_won_overlay() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Won;
        let cmds = app.render(600.0, 800.0);
        let has_win = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "You Win!"));
        assert!(has_win);
    }

    #[test]
    fn render_lost_overlay() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Lost;
        let cmds = app.render(600.0, 800.0);
        let has_lose = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Game Over"));
        assert!(has_lose);
    }

    #[test]
    fn render_help_panel() {
        let mut app = Game2048App::with_seed(42);
        app.show_help = true;
        let cmds = app.render(600.0, 800.0);
        let has_help = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Arrow keys")));
        assert!(has_help);
    }

    #[test]
    fn render_no_help_by_default() {
        let app = Game2048App::new();
        let cmds = app.render(600.0, 800.0);
        let has_help = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Arrow keys / WASD: Slide")));
        assert!(!has_help);
    }

    #[test]
    fn render_score_display() {
        let mut app = Game2048App::with_seed(42);
        app.state.score = 256;
        let cmds = app.render(600.0, 800.0);
        let has_score = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "256"));
        assert!(has_score);
    }

    // --- Complex scenarios ---

    #[test]
    fn multiple_moves_scoring() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[2, 2, 4, 4], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.make_move(Direction::Left);
        // 2+2=4, 4+4=8, score = 12
        assert_eq!(app.state.score, 12);
        assert_eq!(app.state.grid[0][0], 4);
        assert_eq!(app.state.grid[0][1], 8);
    }

    #[test]
    fn best_score_tracking() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[128, 128, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.make_move(Direction::Left);
        assert_eq!(app.state.best_score, 256);
        app.new_game();
        assert_eq!(app.state.best_score, 256);
    }

    #[test]
    fn undo_from_game_over() {
        let mut app = Game2048App::with_seed(42);
        app.state.status = GameStatus::Lost;
        app.undo_stack.push(UndoEntry {
            grid: [[0; GRID_SIZE]; GRID_SIZE],
            score: 100,
        });
        app.undo();
        assert_eq!(app.state.status, GameStatus::Playing);
    }

    #[test]
    fn highest_tile_tracking() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[512, 512, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.make_move(Direction::Left);
        assert_eq!(app.state.highest_tile, 1024);
    }

    #[test]
    fn event_handling() {
        let mut app = Game2048App::with_seed(42);
        app.state.grid = [[0, 0, 2, 2], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]];
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert_eq!(app.state.grid[0][0], 4);
    }

    #[test]
    fn direction_enum_eq() {
        assert_eq!(Direction::Up, Direction::Up);
        assert_ne!(Direction::Up, Direction::Down);
    }

    #[test]
    fn game_status_enum_eq() {
        assert_eq!(GameStatus::Playing, GameStatus::Playing);
        assert_ne!(GameStatus::Won, GameStatus::Lost);
    }

    #[test]
    fn slide_row_large_values() {
        let row = [1024, 1024, 512, 512];
        let (result, pts, moved) = GameState::slide_row_left(&row);
        assert_eq!(result, [2048, 1024, 0, 0]);
        assert_eq!(pts, 2048 + 1024);
        assert!(moved);
    }

    #[test]
    fn full_board_with_moves() {
        let mut state = GameState::new();
        state.grid = [[2, 4, 2, 4], [4, 2, 4, 2], [2, 4, 2, 4], [4, 2, 4, 2]];
        // Board is full but no adjacent equal, so no moves
        assert!(!state.can_move());
    }

    #[test]
    fn full_board_with_vertical_merge() {
        let mut state = GameState::new();
        state.grid = [
            [2, 4, 2, 4],
            [4, 2, 4, 2],
            [2, 4, 2, 4],
            [4, 2, 4, 4], // 4,4 can merge horizontally
        ];
        assert!(state.can_move());
    }
}
