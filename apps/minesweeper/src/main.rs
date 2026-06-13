#![allow(dead_code)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_lossless)]

//! Slate OS Minesweeper — classic mine-clearing puzzle game.
//!
//! Features three difficulty levels (Beginner, Intermediate, Expert),
//! first-click-safe mine placement, flood-fill reveal, flagging,
//! mine counter, timer, win/loss detection, and game-over full reveal.
//! Uses an LCG pseudo-random number generator (no external rand crate).

use guitk::color::Color;
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
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// Colors for the neighbor-count digits 1 through 8.
const NUMBER_COLORS: [Color; 8] = [
    BLUE,    // 1
    GREEN,   // 2
    RED,     // 3
    MAUVE,   // 4
    PEACH,   // 5
    TEAL,    // 6
    YELLOW,  // 7
    TEXT_COLOR, // 8
];

// ── Layout constants ────────────────────────────────────────────────
const CELL_SIZE: f32 = 30.0;
const CELL_GAP: f32 = 2.0;
const HEADER_HEIGHT: f32 = 50.0;
const PADDING: f32 = 12.0;
const CELL_CORNER_RADIUS: f32 = 3.0;
const HEADER_FONT_SIZE: f32 = 18.0;
const CELL_FONT_SIZE: f32 = 16.0;
const TITLE_FONT_SIZE: f32 = 14.0;

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
        // LCG constants from Numerical Recipes
        self.state = self.state.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Returns a value in `0..bound` (exclusive upper bound).
    fn next_bounded(&mut self, bound: usize) -> usize {
        let val = self.next_u64();
        (val % bound as u64) as usize
    }
}

// ── Difficulty presets ──────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Beginner,
    Intermediate,
    Expert,
}

impl Difficulty {
    fn cols(self) -> usize {
        match self {
            Self::Beginner => 9,
            Self::Intermediate => 16,
            Self::Expert => 30,
        }
    }

    fn rows(self) -> usize {
        match self {
            Self::Beginner => 9,
            Self::Intermediate => 16,
            Self::Expert => 16,
        }
    }

    fn mines(self) -> usize {
        match self {
            Self::Beginner => 10,
            Self::Intermediate => 40,
            Self::Expert => 99,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Intermediate => "Intermediate",
            Self::Expert => "Expert",
        }
    }
}

// ── Cell state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellState {
    /// Hidden, not flagged.
    Hidden,
    /// Hidden and flagged by the player.
    Flagged,
    /// Revealed.
    Revealed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    is_mine: bool,
    state: CellState,
    /// Number of adjacent mines (0..=8). Only meaningful for non-mine cells.
    adjacent_mines: u8,
}

impl Cell {
    fn new() -> Self {
        Self {
            is_mine: false,
            state: CellState::Hidden,
            adjacent_mines: 0,
        }
    }
}

// ── Game status ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    /// Mines have not been placed yet (waiting for first click).
    Ready,
    /// Mines placed, game in progress.
    Playing,
    /// Player revealed a mine.
    Lost,
    /// All non-mine cells revealed.
    Won,
}

// ── Main application struct ─────────────────────────────────────────
struct MinesweeperApp {
    difficulty: Difficulty,
    rows: usize,
    cols: usize,
    total_mines: usize,
    cells: Vec<Cell>,
    status: GameStatus,
    flags_placed: usize,
    /// Elapsed seconds since the game started.
    elapsed_seconds: u32,
    /// Seed for the LCG (set once per game for reproducibility in tests).
    rng_seed: u64,
    /// Tracks which cell was the losing click (for rendering the red mine).
    losing_cell: Option<(usize, usize)>,
    /// Number of revealed cells (for quick win-check).
    revealed_count: usize,
}

impl MinesweeperApp {
    /// Create a new Minesweeper game with the given difficulty.
    fn new(difficulty: Difficulty) -> Self {
        Self::with_seed(difficulty, 42)
    }

    /// Create a new game with a specific RNG seed (for deterministic tests).
    fn with_seed(difficulty: Difficulty, seed: u64) -> Self {
        let rows = difficulty.rows();
        let cols = difficulty.cols();
        let total = rows * cols;
        Self {
            difficulty,
            rows,
            cols,
            total_mines: difficulty.mines(),
            cells: vec![Cell::new(); total],
            status: GameStatus::Ready,
            flags_placed: 0,
            elapsed_seconds: 0,
            rng_seed: seed,
            losing_cell: None,
            revealed_count: 0,
        }
    }

    /// Total number of cells in the grid.
    fn total_cells(&self) -> usize {
        self.rows * self.cols
    }

    /// Convert (row, col) to flat index, returning `None` if out of bounds.
    fn index_of(&self, row: usize, col: usize) -> Option<usize> {
        if row < self.rows && col < self.cols {
            Some(row * self.cols + col)
        } else {
            None
        }
    }

    /// Get the cell at (row, col), returning `None` if out of bounds.
    fn cell_at(&self, row: usize, col: usize) -> Option<&Cell> {
        self.index_of(row, col).map(|i| &self.cells[i])
    }

    /// Get a mutable reference to the cell at (row, col).
    fn cell_at_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        self.index_of(row, col).map(|i| &mut self.cells[i])
    }

    /// Iterate over the (at most 8) neighbors of the given cell.
    fn neighbors(&self, row: usize, col: usize) -> Vec<(usize, usize)> {
        let mut result = Vec::new();
        let row_i = row as isize;
        let col_i = col as isize;
        for dr in -1..=1_isize {
            for dc in -1..=1_isize {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = row_i + dr;
                let nc = col_i + dc;
                if nr >= 0 && nr < self.rows as isize && nc >= 0 && nc < self.cols as isize {
                    result.push((nr as usize, nc as usize));
                }
            }
        }
        result
    }

    /// Place mines randomly, avoiding the first-click cell and its neighbors.
    /// This ensures the first click is always safe and reveals a region.
    fn place_mines(&mut self, safe_row: usize, safe_col: usize) {
        let mut rng = Lcg::new(self.rng_seed);

        // Collect the safe zone: the clicked cell + its neighbors.
        let mut safe_zone = vec![(safe_row, safe_col)];
        safe_zone.extend(self.neighbors(safe_row, safe_col));

        let total = self.total_cells();
        let mut placed = 0;

        while placed < self.total_mines {
            let idx = rng.next_bounded(total);
            let r = idx / self.cols;
            let c = idx % self.cols;

            // Skip if already a mine or in the safe zone.
            if self.cells[idx].is_mine {
                continue;
            }
            if safe_zone.iter().any(|&(sr, sc)| sr == r && sc == c) {
                continue;
            }

            self.cells[idx].is_mine = true;
            placed += 1;
        }

        // Compute neighbor counts for every cell.
        self.compute_neighbor_counts();
    }

    /// Recompute the `adjacent_mines` count for every cell.
    fn compute_neighbor_counts(&mut self) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let count = self.neighbors(row, col)
                    .iter()
                    .filter(|&&(nr, nc)| {
                        self.index_of(nr, nc)
                            .is_some_and(|i| self.cells[i].is_mine)
                    })
                    .count() as u8;
                let idx = row * self.cols + col;
                self.cells[idx].adjacent_mines = count;
            }
        }
    }

    /// Remaining mine count: total mines minus flags placed.
    fn mines_remaining(&self) -> i32 {
        self.total_mines as i32 - self.flags_placed as i32
    }

    /// Reveal a cell. On first click, places mines ensuring safety.
    /// Returns `true` if the action was performed (game still active, cell was hidden).
    fn reveal(&mut self, row: usize, col: usize) -> bool {
        if self.status == GameStatus::Lost || self.status == GameStatus::Won {
            return false;
        }

        let idx = match self.index_of(row, col) {
            Some(i) => i,
            None => return false,
        };

        // Cannot reveal a flagged or already-revealed cell.
        if self.cells[idx].state != CellState::Hidden {
            return false;
        }

        // First click: place mines, then start the game.
        if self.status == GameStatus::Ready {
            self.place_mines(row, col);
            self.status = GameStatus::Playing;
        }

        // If it is a mine, game over.
        if self.cells[idx].is_mine {
            self.cells[idx].state = CellState::Revealed;
            self.revealed_count += 1;
            self.status = GameStatus::Lost;
            self.losing_cell = Some((row, col));
            self.reveal_all_mines();
            return true;
        }

        // Flood-fill reveal for safe cells.
        self.flood_reveal(row, col);

        // Check for win: all non-mine cells revealed.
        if self.check_win() {
            self.status = GameStatus::Won;
            // Auto-flag remaining mines on win.
            self.auto_flag_mines();
        }

        true
    }

    /// Flood-fill reveal starting from (row, col). Only expands through
    /// cells with zero adjacent mines.
    fn flood_reveal(&mut self, start_row: usize, start_col: usize) {
        let mut stack = vec![(start_row, start_col)];

        while let Some((row, col)) = stack.pop() {
            let idx = match self.index_of(row, col) {
                Some(i) => i,
                None => continue,
            };

            if self.cells[idx].state != CellState::Hidden {
                continue;
            }
            if self.cells[idx].is_mine {
                continue;
            }

            self.cells[idx].state = CellState::Revealed;
            self.revealed_count += 1;

            // If this cell has zero adjacent mines, expand to neighbors.
            if self.cells[idx].adjacent_mines == 0 {
                for (nr, nc) in self.neighbors(row, col) {
                    let ni = nr * self.cols + nc;
                    if self.cells[ni].state == CellState::Hidden {
                        stack.push((nr, nc));
                    }
                }
            }
        }
    }

    /// Toggle a flag on a hidden cell. Returns `true` if toggled.
    fn toggle_flag(&mut self, row: usize, col: usize) -> bool {
        if self.status == GameStatus::Lost || self.status == GameStatus::Won {
            return false;
        }
        // Cannot flag before game starts (no mines placed yet).
        if self.status == GameStatus::Ready {
            return false;
        }

        let idx = match self.index_of(row, col) {
            Some(i) => i,
            None => return false,
        };

        match self.cells[idx].state {
            CellState::Hidden => {
                self.cells[idx].state = CellState::Flagged;
                self.flags_placed += 1;
                true
            }
            CellState::Flagged => {
                self.cells[idx].state = CellState::Hidden;
                self.flags_placed -= 1;
                true
            }
            CellState::Revealed => false,
        }
    }

    /// Chord-click: if a revealed numbered cell has exactly the right number
    /// of adjacent flags, reveal all hidden neighbors. Returns `true` if
    /// any action was performed.
    fn chord(&mut self, row: usize, col: usize) -> bool {
        if self.status != GameStatus::Playing {
            return false;
        }

        let idx = match self.index_of(row, col) {
            Some(i) => i,
            None => return false,
        };

        if self.cells[idx].state != CellState::Revealed {
            return false;
        }
        if self.cells[idx].is_mine {
            return false;
        }
        let needed = self.cells[idx].adjacent_mines;
        if needed == 0 {
            return false;
        }

        let nbrs = self.neighbors(row, col);
        let flag_count = nbrs.iter()
            .filter(|&&(nr, nc)| {
                self.index_of(nr, nc)
                    .is_some_and(|i| self.cells[i].state == CellState::Flagged)
            })
            .count() as u8;

        if flag_count != needed {
            return false;
        }

        // Collect neighbors to reveal (must copy to avoid borrow issues).
        let to_reveal: Vec<(usize, usize)> = nbrs.iter()
            .filter(|&&(nr, nc)| {
                self.index_of(nr, nc)
                    .is_some_and(|i| self.cells[i].state == CellState::Hidden)
            })
            .copied()
            .collect();

        if to_reveal.is_empty() {
            return false;
        }

        for (nr, nc) in to_reveal {
            self.reveal(nr, nc);
            if self.status == GameStatus::Lost {
                return true;
            }
        }

        true
    }

    /// Check whether all non-mine cells have been revealed.
    fn check_win(&self) -> bool {
        let non_mine_count = self.total_cells() - self.total_mines;
        self.revealed_count >= non_mine_count
    }

    /// On loss, reveal all mines (unflagged ones shown as mines,
    /// incorrectly-flagged cells remain as-is for display).
    fn reveal_all_mines(&mut self) {
        for cell in &mut self.cells {
            if cell.is_mine && cell.state == CellState::Hidden {
                cell.state = CellState::Revealed;
            }
        }
    }

    /// On win, auto-flag all remaining hidden mine cells.
    fn auto_flag_mines(&mut self) {
        for cell in &mut self.cells {
            if cell.is_mine && cell.state == CellState::Hidden {
                cell.state = CellState::Flagged;
            }
        }
        self.flags_placed = self.total_mines;
    }

    /// Restart the game with the same difficulty and a new seed.
    fn restart(&mut self) {
        self.restart_with_seed(self.rng_seed.wrapping_add(1));
    }

    /// Restart with a specific seed.
    fn restart_with_seed(&mut self, seed: u64) {
        let difficulty = self.difficulty;
        *self = Self::with_seed(difficulty, seed);
    }

    /// Change difficulty and reset the game.
    fn set_difficulty(&mut self, difficulty: Difficulty) {
        *self = Self::new(difficulty);
    }

    /// Advance the timer by one second (called by the event loop).
    fn tick(&mut self) {
        if self.status == GameStatus::Playing {
            self.elapsed_seconds = self.elapsed_seconds.saturating_add(1);
        }
    }

    /// Format elapsed time as MM:SS.
    fn format_time(&self) -> String {
        let mins = self.elapsed_seconds / 60;
        let secs = self.elapsed_seconds % 60;
        format!("{mins:02}:{secs:02}")
    }

    /// Count how many mines are in the grid (for testing).
    fn mine_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_mine).count()
    }

    /// Count flagged cells.
    fn flag_count(&self) -> usize {
        self.cells.iter().filter(|c| c.state == CellState::Flagged).count()
    }

    /// Count revealed cells.
    fn count_revealed(&self) -> usize {
        self.cells.iter().filter(|c| c.state == CellState::Revealed).count()
    }

    /// Count hidden cells (not flagged, not revealed).
    fn count_hidden(&self) -> usize {
        self.cells.iter().filter(|c| c.state == CellState::Hidden).count()
    }

    /// Check whether a particular cell is a mine.
    fn is_mine(&self, row: usize, col: usize) -> bool {
        self.cell_at(row, col).is_some_and(|c| c.is_mine)
    }

    /// Check whether a particular cell is revealed.
    fn is_revealed(&self, row: usize, col: usize) -> bool {
        self.cell_at(row, col).is_some_and(|c| c.state == CellState::Revealed)
    }

    /// Check whether a particular cell is flagged.
    fn is_flagged(&self, row: usize, col: usize) -> bool {
        self.cell_at(row, col).is_some_and(|c| c.state == CellState::Flagged)
    }

    /// Get the adjacent mine count for a cell.
    fn adjacent_count(&self, row: usize, col: usize) -> u8 {
        self.cell_at(row, col).map_or(0, |c| c.adjacent_mines)
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Compute the full window width based on grid.
    fn window_width(&self) -> f32 {
        PADDING * 2.0 + self.cols as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Compute the full window height based on grid.
    fn window_height(&self) -> f32 {
        PADDING * 2.0 + HEADER_HEIGHT + self.rows as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    /// Produce the full set of render commands for the current frame.
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
            corner_radii: CornerRadii::all(6.0),
        });

        // Header background
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: win_w - PADDING * 2.0,
            height: HEADER_HEIGHT - 4.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        self.render_header(&mut cmds);
        self.render_grid(&mut cmds);

        cmds
    }

    /// Render the header bar (mine counter, status face, timer).
    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_y = PADDING + 8.0;
        let header_w = self.window_width() - PADDING * 2.0;

        // Mine counter (left side)
        let mine_text = format!("{:03}", self.mines_remaining().clamp(-99, 999));
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: header_y,
            text: mine_text,
            font_size: HEADER_FONT_SIZE,
            color: RED,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });

        // Mine label
        cmds.push(RenderCommand::Text {
            x: PADDING + 55.0,
            y: header_y + 2.0,
            text: String::from("mines"),
            font_size: TITLE_FONT_SIZE,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        // Status indicator (center)
        let status_text = match self.status {
            GameStatus::Ready => "\u{25CB}",   // empty circle
            GameStatus::Playing => "\u{263A}", // smiley
            GameStatus::Won => "\u{2605}",     // star
            GameStatus::Lost => "\u{2716}",    // X mark
        };
        let status_color = match self.status {
            GameStatus::Ready => OVERLAY0,
            GameStatus::Playing => YELLOW,
            GameStatus::Won => GREEN,
            GameStatus::Lost => RED,
        };
        let center_x = PADDING + header_w / 2.0 - 8.0;
        cmds.push(RenderCommand::Text {
            x: center_x,
            y: header_y,
            text: String::from(status_text),
            font_size: HEADER_FONT_SIZE + 2.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: Some(30.0),
        });

        // Difficulty label (just below center icon)
        cmds.push(RenderCommand::Text {
            x: center_x - 20.0,
            y: header_y + 18.0,
            text: String::from(self.difficulty.label()),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });

        // Timer (right side)
        let timer_text = self.format_time();
        let timer_x = PADDING + header_w - 80.0;
        cmds.push(RenderCommand::Text {
            x: timer_x,
            y: header_y,
            text: timer_text,
            font_size: HEADER_FONT_SIZE,
            color: BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });

        // Timer label
        cmds.push(RenderCommand::Text {
            x: timer_x + 45.0,
            y: header_y + 2.0,
            text: String::from("time"),
            font_size: TITLE_FONT_SIZE,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });
    }

    /// Render the mine grid.
    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let grid_y = PADDING + HEADER_HEIGHT;

        for row in 0..self.rows {
            for col in 0..self.cols {
                let x = PADDING + col as f32 * (CELL_SIZE + CELL_GAP);
                let y = grid_y + row as f32 * (CELL_SIZE + CELL_GAP);

                let idx = row * self.cols + col;
                let cell = &self.cells[idx];

                self.render_cell(cmds, cell, row, col, x, y);
            }
        }
    }

    /// Render a single cell.
    fn render_cell(
        &self,
        cmds: &mut Vec<RenderCommand>,
        cell: &Cell,
        row: usize,
        col: usize,
        x: f32,
        y: f32,
    ) {
        let radii = CornerRadii::all(CELL_CORNER_RADIUS);

        match cell.state {
            CellState::Hidden => {
                // Raised-looking hidden cell
                cmds.push(RenderCommand::FillRect {
                    x, y, width: CELL_SIZE, height: CELL_SIZE,
                    color: SURFACE1,
                    corner_radii: radii,
                });
                // Subtle highlight on top-left edges
                cmds.push(RenderCommand::Line {
                    x1: x + 2.0, y1: y + 1.0,
                    x2: x + CELL_SIZE - 2.0, y2: y + 1.0,
                    color: SURFACE2,
                    width: 1.0,
                });
            }
            CellState::Flagged => {
                // Flagged cell background
                cmds.push(RenderCommand::FillRect {
                    x, y, width: CELL_SIZE, height: CELL_SIZE,
                    color: SURFACE1,
                    corner_radii: radii,
                });
                // Flag icon (triangle-ish using text)
                cmds.push(RenderCommand::Text {
                    x: x + CELL_SIZE / 2.0 - 5.0,
                    y: y + CELL_SIZE / 2.0 - 9.0,
                    text: String::from("\u{2691}"), // flag
                    font_size: CELL_FONT_SIZE,
                    color: RED,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(CELL_SIZE),
                });
                // If game is lost and this flag was wrong, show X overlay
                if self.status == GameStatus::Lost && !cell.is_mine {
                    cmds.push(RenderCommand::Line {
                        x1: x + 4.0, y1: y + 4.0,
                        x2: x + CELL_SIZE - 4.0, y2: y + CELL_SIZE - 4.0,
                        color: RED,
                        width: 2.0,
                    });
                    cmds.push(RenderCommand::Line {
                        x1: x + CELL_SIZE - 4.0, y1: y + 4.0,
                        x2: x + 4.0, y2: y + CELL_SIZE - 4.0,
                        color: RED,
                        width: 2.0,
                    });
                }
            }
            CellState::Revealed => {
                if cell.is_mine {
                    // Mine cell
                    let is_losing = self.losing_cell == Some((row, col));
                    let bg = if is_losing { RED } else { SURFACE0 };
                    cmds.push(RenderCommand::FillRect {
                        x, y, width: CELL_SIZE, height: CELL_SIZE,
                        color: bg,
                        corner_radii: radii,
                    });
                    // Mine symbol
                    let mine_color = if is_losing { CRUST } else { TEXT_COLOR };
                    cmds.push(RenderCommand::Text {
                        x: x + CELL_SIZE / 2.0 - 5.0,
                        y: y + CELL_SIZE / 2.0 - 9.0,
                        text: String::from("\u{2739}"), // mine/asterisk
                        font_size: CELL_FONT_SIZE,
                        color: mine_color,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(CELL_SIZE),
                    });
                } else {
                    // Revealed safe cell
                    cmds.push(RenderCommand::FillRect {
                        x, y, width: CELL_SIZE, height: CELL_SIZE,
                        color: SURFACE0,
                        corner_radii: radii,
                    });
                    // Show number if adjacent_mines > 0
                    if cell.adjacent_mines > 0 {
                        let color_idx = (cell.adjacent_mines as usize).saturating_sub(1).min(7);
                        let num_color = NUMBER_COLORS[color_idx];
                        cmds.push(RenderCommand::Text {
                            x: x + CELL_SIZE / 2.0 - 5.0,
                            y: y + CELL_SIZE / 2.0 - 9.0,
                            text: format!("{}", cell.adjacent_mines),
                            font_size: CELL_FONT_SIZE,
                            color: num_color,
                            font_weight: FontWeightHint::Bold,
                            max_width: Some(CELL_SIZE),
                        });
                    }
                }
            }
        }
    }
}

fn main() {
    let _app = MinesweeperApp::new(Difficulty::Beginner);
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction & difficulty ────────────────────────────────────

    #[test]
    fn test_beginner_dimensions() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.rows, 9);
        assert_eq!(app.cols, 9);
        assert_eq!(app.total_mines, 10);
    }

    #[test]
    fn test_intermediate_dimensions() {
        let app = MinesweeperApp::new(Difficulty::Intermediate);
        assert_eq!(app.rows, 16);
        assert_eq!(app.cols, 16);
        assert_eq!(app.total_mines, 40);
    }

    #[test]
    fn test_expert_dimensions() {
        let app = MinesweeperApp::new(Difficulty::Expert);
        assert_eq!(app.rows, 16);
        assert_eq!(app.cols, 30);
        assert_eq!(app.total_mines, 99);
    }

    #[test]
    fn test_initial_status_is_ready() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.status, GameStatus::Ready);
    }

    #[test]
    fn test_initial_no_mines_placed() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.mine_count(), 0);
    }

    #[test]
    fn test_initial_no_flags() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.flags_placed, 0);
        assert_eq!(app.flag_count(), 0);
    }

    #[test]
    fn test_initial_no_revealed() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.count_revealed(), 0);
        assert_eq!(app.revealed_count, 0);
    }

    #[test]
    fn test_initial_timer_zero() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.elapsed_seconds, 0);
    }

    #[test]
    fn test_total_cells_beginner() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.total_cells(), 81);
    }

    #[test]
    fn test_total_cells_expert() {
        let app = MinesweeperApp::new(Difficulty::Expert);
        assert_eq!(app.total_cells(), 480);
    }

    // ── Index and cell access ───────────────────────────────────────

    #[test]
    fn test_index_of_valid() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.index_of(0, 0), Some(0));
        assert_eq!(app.index_of(0, 8), Some(8));
        assert_eq!(app.index_of(8, 8), Some(80));
    }

    #[test]
    fn test_index_of_out_of_bounds() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.index_of(9, 0), None);
        assert_eq!(app.index_of(0, 9), None);
        assert_eq!(app.index_of(100, 100), None);
    }

    #[test]
    fn test_cell_at_valid() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert!(app.cell_at(0, 0).is_some());
        assert!(app.cell_at(4, 4).is_some());
    }

    #[test]
    fn test_cell_at_out_of_bounds() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert!(app.cell_at(9, 9).is_none());
    }

    // ── Neighbors ───────────────────────────────────────────────────

    #[test]
    fn test_neighbors_corner() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let nbrs = app.neighbors(0, 0);
        assert_eq!(nbrs.len(), 3);
    }

    #[test]
    fn test_neighbors_edge() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let nbrs = app.neighbors(0, 4);
        assert_eq!(nbrs.len(), 5);
    }

    #[test]
    fn test_neighbors_center() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let nbrs = app.neighbors(4, 4);
        assert_eq!(nbrs.len(), 8);
    }

    #[test]
    fn test_neighbors_bottom_right_corner() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let nbrs = app.neighbors(8, 8);
        assert_eq!(nbrs.len(), 3);
    }

    #[test]
    fn test_neighbors_bottom_edge() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let nbrs = app.neighbors(8, 4);
        assert_eq!(nbrs.len(), 5);
    }

    // ── Mine placement ──────────────────────────────────────────────

    #[test]
    fn test_first_click_places_mines() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        assert_eq!(app.mine_count(), 10);
    }

    #[test]
    fn test_first_click_safe() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        assert!(!app.is_mine(4, 4));
    }

    #[test]
    fn test_first_click_neighbors_safe() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        for (nr, nc) in app.neighbors(4, 4) {
            assert!(!app.is_mine(nr, nc), "Neighbor ({nr}, {nc}) should not be a mine");
        }
    }

    #[test]
    fn test_first_click_corner_safe() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(0, 0);
        assert!(!app.is_mine(0, 0));
        for (nr, nc) in app.neighbors(0, 0) {
            assert!(!app.is_mine(nr, nc));
        }
    }

    #[test]
    fn test_mine_placement_deterministic() {
        let mut a = MinesweeperApp::with_seed(Difficulty::Beginner, 12345);
        let mut b = MinesweeperApp::with_seed(Difficulty::Beginner, 12345);
        a.reveal(0, 0);
        b.reveal(0, 0);
        for i in 0..a.total_cells() {
            assert_eq!(a.cells[i].is_mine, b.cells[i].is_mine);
        }
    }

    #[test]
    fn test_different_seeds_different_layouts() {
        let mut a = MinesweeperApp::with_seed(Difficulty::Beginner, 111);
        let mut b = MinesweeperApp::with_seed(Difficulty::Beginner, 222);
        a.reveal(4, 4);
        b.reveal(4, 4);
        // With overwhelming probability, different seeds produce different layouts
        let same = (0..a.total_cells())
            .all(|i| a.cells[i].is_mine == b.cells[i].is_mine);
        assert!(!same);
    }

    #[test]
    fn test_neighbor_counts_consistent() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 99);
        app.reveal(4, 4);
        for row in 0..app.rows {
            for col in 0..app.cols {
                if !app.is_mine(row, col) {
                    let expected = app.neighbors(row, col)
                        .iter()
                        .filter(|&&(r, c)| app.is_mine(r, c))
                        .count() as u8;
                    assert_eq!(app.adjacent_count(row, col), expected,
                        "Wrong count at ({row}, {col})");
                }
            }
        }
    }

    // ── Reveal mechanics ────────────────────────────────────────────

    #[test]
    fn test_reveal_changes_status_to_playing() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn test_reveal_marks_cell_revealed() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        assert!(app.is_revealed(4, 4));
    }

    #[test]
    fn test_reveal_returns_false_on_out_of_bounds() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        assert!(!app.reveal(100, 100));
    }

    #[test]
    fn test_reveal_already_revealed_returns_false() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.reveal(4, 4);
        // Cell (4,4) is now revealed
        assert!(!app.reveal(4, 4));
    }

    #[test]
    fn test_reveal_flagged_cell_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        // First click to place mines
        app.reveal(4, 4);
        // Find a hidden cell to flag
        let (fr, fc) = find_hidden_cell(&app);
        app.toggle_flag(fr, fc);
        assert!(!app.reveal(fr, fc));
    }

    #[test]
    fn test_flood_fill_reveals_multiple() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Flood fill should reveal more than one cell
        assert!(app.count_revealed() > 1);
    }

    #[test]
    fn test_reveal_numbered_cell_no_flood() {
        // Find a seed/position where the first click reveals a numbered cell
        // by clicking on a cell that has neighbors with mines.
        let mut app = MinesweeperApp::with_seed(Difficulty::Intermediate, 777);
        app.reveal(0, 0);
        // Find a revealed cell with a nonzero count that borders hidden cells.
        // Revealing such a cell's hidden neighbor (if numbered) should not flood.
        let mut found = false;
        for row in 0..app.rows {
            for col in 0..app.cols {
                if app.is_revealed(row, col) && app.adjacent_count(row, col) > 0 {
                    // Check a neighbor that is hidden and also numbered
                    for (nr, nc) in app.neighbors(row, col) {
                        if !app.is_revealed(nr, nc)
                            && !app.is_flagged(nr, nc)
                            && !app.is_mine(nr, nc)
                        {
                            let before = app.count_revealed();
                            app.reveal(nr, nc);
                            if app.adjacent_count(nr, nc) > 0 {
                                // Numbered cell: should reveal exactly 1 more
                                assert_eq!(app.count_revealed(), before + 1);
                                found = true;
                                break;
                            }
                        }
                    }
                    if found { break; }
                }
            }
            if found { break; }
        }
        assert!(found, "Could not find a numbered hidden cell to test");
    }

    // ── Flagging ────────────────────────────────────────────────────

    #[test]
    fn test_flag_before_game_starts_returns_false() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        assert!(!app.toggle_flag(0, 0));
    }

    #[test]
    fn test_flag_hidden_cell() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (fr, fc) = find_hidden_cell(&app);
        assert!(app.toggle_flag(fr, fc));
        assert!(app.is_flagged(fr, fc));
        assert_eq!(app.flags_placed, 1);
    }

    #[test]
    fn test_unflag_cell() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (fr, fc) = find_hidden_cell(&app);
        app.toggle_flag(fr, fc);
        assert!(app.toggle_flag(fr, fc));
        assert!(!app.is_flagged(fr, fc));
        assert_eq!(app.flags_placed, 0);
    }

    #[test]
    fn test_flag_revealed_cell_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // (4,4) is revealed
        assert!(!app.toggle_flag(4, 4));
    }

    #[test]
    fn test_flag_out_of_bounds_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        assert!(!app.toggle_flag(100, 100));
    }

    #[test]
    fn test_mines_remaining_decreases_with_flags() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        assert_eq!(app.mines_remaining(), 10);
        let (fr, fc) = find_hidden_cell(&app);
        app.toggle_flag(fr, fc);
        assert_eq!(app.mines_remaining(), 9);
    }

    #[test]
    fn test_flag_after_game_over_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        let (fr, fc) = find_hidden_non_mine_cell(&app);
        assert!(!app.toggle_flag(fr, fc));
    }

    // ── Win detection ───────────────────────────────────────────────

    #[test]
    fn test_win_by_revealing_all_safe_cells() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        reveal_all_safe_cells(&mut app);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn test_win_auto_flags_mines() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        reveal_all_safe_cells(&mut app);
        assert_eq!(app.flag_count(), app.total_mines);
    }

    #[test]
    fn test_reveal_after_win_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        reveal_all_safe_cells(&mut app);
        assert!(!app.reveal(0, 0));
    }

    // ── Loss detection ──────────────────────────────────────────────

    #[test]
    fn test_loss_on_mine_reveal() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        assert_eq!(app.status, GameStatus::Lost);
    }

    #[test]
    fn test_loss_records_losing_cell() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        let mine_pos = force_loss(&mut app);
        assert_eq!(app.losing_cell, Some(mine_pos));
    }

    #[test]
    fn test_loss_reveals_all_mines() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        for i in 0..app.total_cells() {
            if app.cells[i].is_mine {
                // Mines should be revealed or flagged (if user flagged them)
                assert_ne!(app.cells[i].state, CellState::Hidden,
                    "Mine at index {i} should be revealed after loss");
            }
        }
    }

    #[test]
    fn test_reveal_after_loss_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        assert!(!app.reveal(0, 0));
    }

    // ── Chord ───────────────────────────────────────────────────────

    #[test]
    fn test_chord_on_unrevealed_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (hr, hc) = find_hidden_cell(&app);
        assert!(!app.chord(hr, hc));
    }

    #[test]
    fn test_chord_on_zero_cell_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Find a revealed cell with count 0
        for row in 0..app.rows {
            for col in 0..app.cols {
                if app.is_revealed(row, col) && app.adjacent_count(row, col) == 0 {
                    assert!(!app.chord(row, col));
                    return;
                }
            }
        }
    }

    #[test]
    fn test_chord_insufficient_flags_returns_false() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Find a revealed numbered cell
        if let Some((row, col)) = find_revealed_numbered_cell(&app) {
            // Don't flag any neighbors — chord should fail
            assert!(!app.chord(row, col));
        }
    }

    #[test]
    fn test_chord_with_correct_flags_reveals_neighbors() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Find a revealed numbered cell, flag its mine neighbors, then chord
        if let Some((row, col)) = find_revealed_numbered_cell(&app) {
            let nbrs = app.neighbors(row, col);
            let mine_nbrs: Vec<(usize, usize)> = nbrs.iter()
                .filter(|&&(r, c)| app.is_mine(r, c))
                .copied()
                .collect();
            for &(mr, mc) in &mine_nbrs {
                if app.cells[mr * app.cols + mc].state == CellState::Hidden {
                    app.toggle_flag(mr, mc);
                }
            }
            let before = app.count_revealed();
            let did_chord = app.chord(row, col);
            if did_chord {
                assert!(app.count_revealed() > before);
            }
        }
    }

    #[test]
    fn test_chord_before_game_starts_returns_false() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        assert!(!app.chord(4, 4));
    }

    // ── Timer ───────────────────────────────────────────────────────

    #[test]
    fn test_tick_increments_during_playing() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        assert_eq!(app.elapsed_seconds, 0);
        app.tick();
        assert_eq!(app.elapsed_seconds, 1);
        app.tick();
        assert_eq!(app.elapsed_seconds, 2);
    }

    #[test]
    fn test_tick_does_not_increment_when_ready() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.tick();
        assert_eq!(app.elapsed_seconds, 0);
    }

    #[test]
    fn test_tick_does_not_increment_after_loss() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        app.tick();
        assert_eq!(app.elapsed_seconds, 0);
    }

    #[test]
    fn test_tick_does_not_increment_after_win() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        reveal_all_safe_cells(&mut app);
        let t = app.elapsed_seconds;
        app.tick();
        assert_eq!(app.elapsed_seconds, t);
    }

    #[test]
    fn test_format_time_zero() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        assert_eq!(app.format_time(), "00:00");
    }

    #[test]
    fn test_format_time_minutes_and_seconds() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        app.elapsed_seconds = 125;
        assert_eq!(app.format_time(), "02:05");
    }

    #[test]
    fn test_timer_saturates() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        app.elapsed_seconds = u32::MAX;
        app.tick();
        assert_eq!(app.elapsed_seconds, u32::MAX);
    }

    // ── Restart ─────────────────────────────────────────────────────

    #[test]
    fn test_restart_resets_state() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        app.tick();
        app.restart();
        assert_eq!(app.status, GameStatus::Ready);
        assert_eq!(app.mine_count(), 0);
        assert_eq!(app.elapsed_seconds, 0);
        assert_eq!(app.flags_placed, 0);
        assert_eq!(app.revealed_count, 0);
    }

    #[test]
    fn test_restart_preserves_difficulty() {
        let mut app = MinesweeperApp::new(Difficulty::Expert);
        app.reveal(4, 4);
        app.restart();
        assert_eq!(app.difficulty, Difficulty::Expert);
        assert_eq!(app.rows, 16);
        assert_eq!(app.cols, 30);
    }

    #[test]
    fn test_restart_with_seed() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.restart_with_seed(999);
        assert_eq!(app.rng_seed, 999);
        assert_eq!(app.status, GameStatus::Ready);
    }

    // ── Set difficulty ──────────────────────────────────────────────

    #[test]
    fn test_set_difficulty_changes_grid() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        app.set_difficulty(Difficulty::Expert);
        assert_eq!(app.rows, 16);
        assert_eq!(app.cols, 30);
        assert_eq!(app.total_mines, 99);
        assert_eq!(app.status, GameStatus::Ready);
    }

    #[test]
    fn test_set_difficulty_resets_game() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        app.set_difficulty(Difficulty::Intermediate);
        assert_eq!(app.mine_count(), 0);
        assert_eq!(app.count_revealed(), 0);
    }

    // ── Rendering ───────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_after_reveal_produces_commands() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_after_loss_produces_commands() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_after_win_produces_commands() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        reveal_all_safe_cells(&mut app);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_contains_background_rect() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        // First command should be the full background fill
        let has_bg = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::FillRect { x, y, color, .. }
            if *x == 0.0 && *y == 0.0 && *color == BASE
        ));
        assert!(has_bg, "Should have a background fill rect");
    }

    #[test]
    fn test_render_contains_header() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        // Should have the mantle-colored header bar
        let has_header = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::FillRect { color, .. } if *color == MANTLE
        ));
        assert!(has_header, "Should have a header bar");
    }

    #[test]
    fn test_render_contains_mine_counter_text() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        let has_counter = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::Text { text, color, .. }
            if *color == RED && text.contains("010")
        ));
        assert!(has_counter, "Should have mine counter text");
    }

    #[test]
    fn test_render_contains_timer_text() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        let has_timer = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::Text { text, color, .. }
            if *color == BLUE && text == "00:00"
        ));
        assert!(has_timer, "Should have timer text");
    }

    #[test]
    fn test_render_cell_count_matches_grid() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let cmds = app.render();
        // Each hidden cell produces a FillRect + a Line (highlight)
        let surface1_rects = cmds.iter().filter(|cmd| matches!(cmd,
            RenderCommand::FillRect { color, .. } if *color == SURFACE1
        )).count();
        // Should have one FillRect per cell (81 for beginner)
        assert_eq!(surface1_rects, 81);
    }

    #[test]
    fn test_render_revealed_cells_use_surface0() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let cmds = app.render();
        let surface0_rects = cmds.iter().filter(|cmd| matches!(cmd,
            RenderCommand::FillRect { color, .. } if *color == SURFACE0
        )).count();
        // At least one revealed cell (flood fill)
        assert!(surface0_rects > 0);
    }

    #[test]
    fn test_render_flagged_cell_has_flag_symbol() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (fr, fc) = find_hidden_cell(&app);
        app.toggle_flag(fr, fc);
        let cmds = app.render();
        let has_flag = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::Text { text, color, .. }
            if *color == RED && text == "\u{2691}"
        ));
        assert!(has_flag, "Should have a flag symbol");
    }

    #[test]
    fn test_render_number_colors() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let cmds = app.render();
        // Check that at least one numbered cell uses the correct color
        let has_numbered = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::Text { text, color, .. }
            if (*color == BLUE && text == "1")
                || (*color == GREEN && text == "2")
                || (*color == RED && text == "3")
        ));
        assert!(has_numbered, "Should have colored number text");
    }

    #[test]
    fn test_render_loss_shows_mine_symbols() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        let cmds = app.render();
        let mine_symbols = cmds.iter().filter(|cmd| matches!(cmd,
            RenderCommand::Text { text, .. } if text == "\u{2739}"
        )).count();
        assert!(mine_symbols > 0, "Should show mine symbols after loss");
    }

    #[test]
    fn test_render_losing_cell_has_red_background() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        force_loss(&mut app);
        let cmds = app.render();
        let has_red_mine_bg = cmds.iter().any(|cmd| matches!(cmd,
            RenderCommand::FillRect { color, width, height, .. }
            if *color == RED && *width == CELL_SIZE && *height == CELL_SIZE
        ));
        assert!(has_red_mine_bg, "Losing cell should have red background");
    }

    #[test]
    fn test_window_width_beginner() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let expected = PADDING * 2.0 + 9.0 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        assert!((app.window_width() - expected).abs() < 0.01);
    }

    #[test]
    fn test_window_height_beginner() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        let expected = PADDING * 2.0 + HEADER_HEIGHT + 9.0 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        assert!((app.window_height() - expected).abs() < 0.01);
    }

    #[test]
    fn test_window_width_expert() {
        let app = MinesweeperApp::new(Difficulty::Expert);
        let expected = PADDING * 2.0 + 30.0 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
        assert!((app.window_width() - expected).abs() < 0.01);
    }

    // ── LCG tests ───────────────────────────────────────────────────

    #[test]
    fn test_lcg_deterministic() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn test_lcg_different_seeds_differ() {
        let mut a = Lcg::new(1);
        let mut b = Lcg::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn test_lcg_bounded_in_range() {
        let mut rng = Lcg::new(42);
        for _ in 0..1000 {
            let v = rng.next_bounded(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn test_lcg_bounded_distribution() {
        // Rough check: each bucket should get some hits in 10000 draws
        let mut rng = Lcg::new(42);
        let mut counts = [0u32; 10];
        for _ in 0..10_000 {
            let v = rng.next_bounded(10);
            counts[v] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            assert!(c > 500, "Bucket {i} only got {c} hits — distribution seems broken");
        }
    }

    // ── Difficulty labels ───────────────────────────────────────────

    #[test]
    fn test_difficulty_labels() {
        assert_eq!(Difficulty::Beginner.label(), "Beginner");
        assert_eq!(Difficulty::Intermediate.label(), "Intermediate");
        assert_eq!(Difficulty::Expert.label(), "Expert");
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn test_multiple_restarts() {
        let mut app = MinesweeperApp::new(Difficulty::Beginner);
        for _ in 0..5 {
            app.reveal(4, 4);
            app.restart();
            assert_eq!(app.status, GameStatus::Ready);
            assert_eq!(app.mine_count(), 0);
        }
    }

    #[test]
    fn test_flag_then_restart_clears_flags() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (fr, fc) = find_hidden_cell(&app);
        app.toggle_flag(fr, fc);
        app.restart();
        assert_eq!(app.flags_placed, 0);
        assert_eq!(app.flag_count(), 0);
    }

    #[test]
    fn test_mines_remaining_can_go_negative() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Flag more cells than there are mines
        let mut flagged = 0;
        for row in 0..app.rows {
            for col in 0..app.cols {
                if flagged >= 12 { break; }
                if app.cells[row * app.cols + col].state == CellState::Hidden {
                    app.toggle_flag(row, col);
                    flagged += 1;
                }
            }
            if flagged >= 12 { break; }
        }
        assert!(app.mines_remaining() < 0);
    }

    #[test]
    fn test_all_cells_initially_hidden() {
        let app = MinesweeperApp::new(Difficulty::Beginner);
        for cell in &app.cells {
            assert_eq!(cell.state, CellState::Hidden);
        }
    }

    #[test]
    fn test_no_mines_on_brand_new_grid() {
        let app = MinesweeperApp::new(Difficulty::Expert);
        for cell in &app.cells {
            assert!(!cell.is_mine);
        }
    }

    #[test]
    fn test_cell_new_defaults() {
        let c = Cell::new();
        assert!(!c.is_mine);
        assert_eq!(c.state, CellState::Hidden);
        assert_eq!(c.adjacent_mines, 0);
    }

    #[test]
    fn test_expert_mine_count_after_first_click() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Expert, 42);
        app.reveal(8, 15);
        assert_eq!(app.mine_count(), 99);
    }

    #[test]
    fn test_intermediate_first_click_center() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Intermediate, 42);
        app.reveal(8, 8);
        assert!(!app.is_mine(8, 8));
        assert_eq!(app.mine_count(), 40);
    }

    #[test]
    fn test_wrong_flag_shown_on_loss() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        // Flag a non-mine hidden cell
        let (fr, fc) = find_hidden_non_mine_cell(&app);
        app.toggle_flag(fr, fc);
        force_loss(&mut app);
        // The wrongly-flagged cell should still be Flagged (render shows X overlay)
        assert!(app.is_flagged(fr, fc));
        assert!(!app.is_mine(fr, fc));
    }

    #[test]
    fn test_render_wrong_flag_shows_x_lines() {
        let mut app = MinesweeperApp::with_seed(Difficulty::Beginner, 42);
        app.reveal(4, 4);
        let (fr, fc) = find_hidden_non_mine_cell(&app);
        app.toggle_flag(fr, fc);
        force_loss(&mut app);
        let cmds = app.render();
        // Should have Line commands for the X overlay on wrong flags
        let line_count = cmds.iter().filter(|cmd| matches!(cmd, RenderCommand::Line { color, width, .. }
            if *color == RED && *width == 2.0
        )).count();
        // At least 2 lines (the X) for the wrong flag
        assert!(line_count >= 2, "Should have X overlay lines for wrong flag");
    }

    // ── Helper functions for tests ──────────────────────────────────

    /// Find any hidden (non-flagged, non-revealed) cell.
    fn find_hidden_cell(app: &MinesweeperApp) -> (usize, usize) {
        for row in 0..app.rows {
            for col in 0..app.cols {
                if app.cells[row * app.cols + col].state == CellState::Hidden {
                    return (row, col);
                }
            }
        }
        panic!("No hidden cell found");
    }

    /// Find a hidden cell that is not a mine.
    fn find_hidden_non_mine_cell(app: &MinesweeperApp) -> (usize, usize) {
        for row in 0..app.rows {
            for col in 0..app.cols {
                let idx = row * app.cols + col;
                if app.cells[idx].state == CellState::Hidden && !app.cells[idx].is_mine {
                    return (row, col);
                }
            }
        }
        panic!("No hidden non-mine cell found");
    }

    /// Find a revealed cell with a nonzero neighbor count.
    fn find_revealed_numbered_cell(app: &MinesweeperApp) -> Option<(usize, usize)> {
        for row in 0..app.rows {
            for col in 0..app.cols {
                let idx = row * app.cols + col;
                if app.cells[idx].state == CellState::Revealed
                    && !app.cells[idx].is_mine
                    && app.cells[idx].adjacent_mines > 0
                {
                    return Some((row, col));
                }
            }
        }
        None
    }

    /// Force a loss by finding and revealing a mine.
    /// Returns the (row, col) of the mine that was clicked.
    fn force_loss(app: &mut MinesweeperApp) -> (usize, usize) {
        // First, ensure mines are placed
        if app.status == GameStatus::Ready {
            app.reveal(4, 4);
        }
        // Now find a mine and reveal it
        for row in 0..app.rows {
            for col in 0..app.cols {
                let idx = row * app.cols + col;
                if app.cells[idx].is_mine && app.cells[idx].state == CellState::Hidden {
                    app.cells[idx].state = CellState::Hidden; // ensure it is hidden
                    app.reveal(row, col);
                    return (row, col);
                }
            }
        }
        panic!("No hidden mine found to force loss");
    }

    /// Reveal all safe (non-mine) cells to trigger a win.
    fn reveal_all_safe_cells(app: &mut MinesweeperApp) {
        // First click to place mines
        if app.status == GameStatus::Ready {
            app.reveal(4, 4);
        }
        // Reveal every non-mine cell
        loop {
            let mut found = false;
            for row in 0..app.rows {
                for col in 0..app.cols {
                    let idx = row * app.cols + col;
                    if !app.cells[idx].is_mine && app.cells[idx].state == CellState::Hidden {
                        app.reveal(row, col);
                        found = true;
                    }
                }
            }
            if !found { break; }
        }
    }
}
