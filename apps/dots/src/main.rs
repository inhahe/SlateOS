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

//! Slate OS Dots and Boxes — classic pencil-and-paper strategy game.
//!
//! Two players take turns drawing lines between adjacent dots on a grid.
//! When a player completes the fourth side of a box, that box is claimed and
//! the player gets another turn. The game ends when all boxes are filled.
//! Supports human-vs-AI and two-player modes, configurable grid sizes
//! (3x3, 4x4, 5x5 dots), keyboard and mouse input, and a greedy AI opponent.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seed_from_system};
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

// ── Player colors ───────────────────────────────────────────────────
const PLAYER1_COLOR: Color = BLUE;
const PLAYER2_COLOR: Color = RED;
const PLAYER1_BOX_COLOR: Color = Color::from_hex(0x2A3A5E);
const PLAYER2_BOX_COLOR: Color = Color::from_hex(0x5E2A3A);

// ── Layout constants ────────────────────────────────────────────────
const DOT_RADIUS: f32 = 6.0;
const LINE_THICKNESS: f32 = 5.0;
const LINE_HOVER_THICKNESS: f32 = 7.0;
const DOT_SPACING: f32 = 70.0;
const PADDING: f32 = 20.0;
const HEADER_HEIGHT: f32 = 60.0;
const FOOTER_HEIGHT: f32 = 40.0;
const HEADER_FONT_SIZE: f32 = 18.0;
const TITLE_FONT_SIZE: f32 = 24.0;
const SCORE_FONT_SIZE: f32 = 16.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;

/// Default grid size: 4x4 dots = 3x3 boxes.
const DEFAULT_GRID_SIZE: usize = 4;
const MIN_GRID_SIZE: usize = 3;
const MAX_GRID_SIZE: usize = 5;

// ── Randomness ─────────────────────────────────────────────────────
//
// This file used to carry its own copy of the LCG that `guitk::rng` exists to
// replace, reduced with `val % bound` and seeded with a literal `42`. The AI
// picks a safe line at random when several are equally good, so every match
// against the computer played out the same way from the same position.

/// The seed a session falls back to when the kernel has no entropy to give.
///
/// The AI's tie-breaking may be predictable -- the worst outcome is that the
/// computer plays the same game twice. Refusing to start would be the worse
/// failure; see [`guitk::rng::seeded_from_system`]. "DOTSBOX!" in ASCII.
const FALLBACK_SEED: u64 = 0x444F_5453_424F_5821;

// ── Line orientation ────────────────────────────────────────────────
/// A line can be horizontal (connecting dots in the same row) or vertical
/// (connecting dots in the same column).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Orientation {
    Horizontal,
    Vertical,
}

// ── Line identifier ─────────────────────────────────────────────────
/// Identifies a line segment between two adjacent dots.
///
/// For horizontal lines: `(row, col)` is the left dot; the right dot is at `(row, col+1)`.
/// For vertical lines: `(row, col)` is the top dot; the bottom dot is at `(row+1, col)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LineId {
    orientation: Orientation,
    row: usize,
    col: usize,
}

impl LineId {
    const fn new(orientation: Orientation, row: usize, col: usize) -> Self {
        Self {
            orientation,
            row,
            col,
        }
    }

    fn horizontal(row: usize, col: usize) -> Self {
        Self::new(Orientation::Horizontal, row, col)
    }

    fn vertical(row: usize, col: usize) -> Self {
        Self::new(Orientation::Vertical, row, col)
    }
}

// ── Player ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Player {
    One,
    Two,
}

impl Player {
    fn other(self) -> Self {
        match self {
            Player::One => Player::Two,
            Player::Two => Player::One,
        }
    }

    fn color(self) -> Color {
        match self {
            Player::One => PLAYER1_COLOR,
            Player::Two => PLAYER2_COLOR,
        }
    }

    fn box_color(self) -> Color {
        match self {
            Player::One => PLAYER1_BOX_COLOR,
            Player::Two => PLAYER2_BOX_COLOR,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Player::One => "Player 1",
            Player::Two => "Player 2",
        }
    }
}

// ── Game mode ───────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameMode {
    /// Human vs AI.
    VsAi,
    /// Two human players.
    TwoPlayer,
}

impl GameMode {
    fn label(self) -> &'static str {
        match self {
            GameMode::VsAi => "vs AI",
            GameMode::TwoPlayer => "2-Player",
        }
    }
}

// ── Game state ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePhase {
    Playing,
    GameOver,
}

// ── Cursor for keyboard navigation ─────────────────────────────────
/// The cursor selects a line on the grid. It tracks which line is currently
/// highlighted for keyboard-based play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cursor {
    orientation: Orientation,
    row: usize,
    col: usize,
}

impl Cursor {
    fn to_line_id(self) -> LineId {
        LineId::new(self.orientation, self.row, self.col)
    }
}

// ── Board ───────────────────────────────────────────────────────────
/// The game board tracking all lines and box ownership.
///
/// For an NxN dot grid, there are:
/// - N*(N-1) horizontal lines
/// - (N-1)*N vertical lines
/// - (N-1)*(N-1) boxes
struct Board {
    /// Number of dots per side (e.g. 4 for a 3x3 box grid).
    grid_size: usize,
    /// Horizontal lines: `h_lines[row][col]` is true if the horizontal line
    /// from dot (row, col) to dot (row, col+1) has been drawn.
    h_lines: Vec<Vec<bool>>,
    /// Vertical lines: `v_lines[row][col]` is true if the vertical line
    /// from dot (row, col) to dot (row+1, col) has been drawn.
    v_lines: Vec<Vec<bool>>,
    /// Box ownership: `boxes[row][col]` is the player who completed the box
    /// at grid position (row, col), where the box is bounded by dots
    /// (row, col), (row, col+1), (row+1, col), (row+1, col+1).
    boxes: Vec<Vec<Option<Player>>>,
}

impl Board {
    /// Create a new empty board with the given grid size (number of dots per side).
    fn new(grid_size: usize) -> Self {
        let boxes_per_side = grid_size.saturating_sub(1);
        Self {
            grid_size,
            h_lines: vec![vec![false; boxes_per_side]; grid_size],
            v_lines: vec![vec![false; grid_size]; boxes_per_side],
            boxes: vec![vec![None; boxes_per_side]; boxes_per_side],
        }
    }

    /// Number of boxes per side.
    fn boxes_per_side(&self) -> usize {
        self.grid_size.saturating_sub(1)
    }

    /// Total number of boxes on the board.
    fn total_boxes(&self) -> usize {
        let bps = self.boxes_per_side();
        bps * bps
    }

    /// Total number of lines on the board.
    fn total_lines(&self) -> usize {
        let n = self.grid_size;
        let bps = self.boxes_per_side();
        // Horizontal: n rows, each with (n-1) lines
        // Vertical: (n-1) rows, each with n lines
        n * bps + bps * n
    }

    /// Count how many lines have been drawn.
    fn drawn_line_count(&self) -> usize {
        let mut count = 0;
        for row in &self.h_lines {
            for &drawn in row {
                if drawn {
                    count += 1;
                }
            }
        }
        for row in &self.v_lines {
            for &drawn in row {
                if drawn {
                    count += 1;
                }
            }
        }
        count
    }

    /// Check if a line has been drawn.
    fn is_line_drawn(&self, line: LineId) -> bool {
        match line.orientation {
            Orientation::Horizontal => {
                if line.row < self.h_lines.len()
                    && let Some(&drawn) = self.h_lines[line.row].get(line.col)
                {
                    return drawn;
                }
                false
            }
            Orientation::Vertical => {
                if line.row < self.v_lines.len()
                    && let Some(&drawn) = self.v_lines[line.row].get(line.col)
                {
                    return drawn;
                }
                false
            }
        }
    }

    /// Check if a line ID is valid for this board.
    fn is_valid_line(&self, line: LineId) -> bool {
        let bps = self.boxes_per_side();
        match line.orientation {
            Orientation::Horizontal => line.row < self.grid_size && line.col < bps,
            Orientation::Vertical => line.row < bps && line.col < self.grid_size,
        }
    }

    /// Draw a line and return how many boxes were completed by it.
    /// Completed boxes are assigned to the given player.
    fn draw_line(&mut self, line: LineId, player: Player) -> usize {
        if !self.is_valid_line(line) || self.is_line_drawn(line) {
            return 0;
        }

        match line.orientation {
            Orientation::Horizontal => {
                self.h_lines[line.row][line.col] = true;
            }
            Orientation::Vertical => {
                self.v_lines[line.row][line.col] = true;
            }
        }

        let mut completed = 0;
        let adjacent = self.adjacent_boxes(line);
        for (br, bc) in adjacent {
            if self.is_box_complete(br, bc) && self.boxes[br][bc].is_none() {
                self.boxes[br][bc] = Some(player);
                completed += 1;
            }
        }
        completed
    }

    /// Get the box coordinates adjacent to a line.
    ///
    /// A horizontal line at (row, col) borders:
    /// - box (row-1, col) above (if row > 0)
    /// - box (row, col) below (if row < boxes_per_side)
    ///
    /// A vertical line at (row, col) borders:
    /// - box (row, col-1) to the left (if col > 0)
    /// - box (row, col) to the right (if col < boxes_per_side)
    fn adjacent_boxes(&self, line: LineId) -> Vec<(usize, usize)> {
        let bps = self.boxes_per_side();
        let mut result = Vec::new();
        match line.orientation {
            Orientation::Horizontal => {
                if line.row > 0 {
                    let br = line.row - 1;
                    if br < bps && line.col < bps {
                        result.push((br, line.col));
                    }
                }
                if line.row < bps && line.col < bps {
                    result.push((line.row, line.col));
                }
            }
            Orientation::Vertical => {
                if line.col > 0 {
                    let bc = line.col - 1;
                    if line.row < bps && bc < bps {
                        result.push((line.row, bc));
                    }
                }
                if line.row < bps && line.col < bps {
                    result.push((line.row, line.col));
                }
            }
        }
        result
    }

    /// Check if all four sides of a box are drawn.
    fn is_box_complete(&self, box_row: usize, box_col: usize) -> bool {
        let bps = self.boxes_per_side();
        if box_row >= bps || box_col >= bps {
            return false;
        }
        // Top: horizontal line at (box_row, box_col)
        let top = self.h_lines[box_row][box_col];
        // Bottom: horizontal line at (box_row+1, box_col)
        let bottom = self.h_lines[box_row + 1][box_col];
        // Left: vertical line at (box_row, box_col)
        let left = self.v_lines[box_row][box_col];
        // Right: vertical line at (box_row, box_col+1)
        let right = self.v_lines[box_row][box_col + 1];
        top && bottom && left && right
    }

    /// Count sides drawn for a specific box.
    fn box_side_count(&self, box_row: usize, box_col: usize) -> usize {
        let bps = self.boxes_per_side();
        if box_row >= bps || box_col >= bps {
            return 0;
        }
        let mut count = 0;
        if self.h_lines[box_row][box_col] {
            count += 1;
        }
        if self.h_lines[box_row + 1][box_col] {
            count += 1;
        }
        if self.v_lines[box_row][box_col] {
            count += 1;
        }
        if self.v_lines[box_row][box_col + 1] {
            count += 1;
        }
        count
    }

    /// Return all valid lines that haven't been drawn yet.
    fn available_lines(&self) -> Vec<LineId> {
        let mut lines = Vec::new();
        let bps = self.boxes_per_side();
        for row in 0..self.grid_size {
            for col in 0..bps {
                if !self.h_lines[row][col] {
                    lines.push(LineId::horizontal(row, col));
                }
            }
        }
        for row in 0..bps {
            for col in 0..self.grid_size {
                if !self.v_lines[row][col] {
                    lines.push(LineId::vertical(row, col));
                }
            }
        }
        lines
    }

    /// Check if all lines have been drawn (game over condition).
    fn all_lines_drawn(&self) -> bool {
        for row in &self.h_lines {
            for &drawn in row {
                if !drawn {
                    return false;
                }
            }
        }
        for row in &self.v_lines {
            for &drawn in row {
                if !drawn {
                    return false;
                }
            }
        }
        true
    }

    /// Count boxes owned by a given player.
    fn score(&self, player: Player) -> usize {
        let mut count = 0;
        for row in &self.boxes {
            for cell in row {
                if *cell == Some(player) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Count how many boxes have been claimed.
    fn claimed_boxes(&self) -> usize {
        let mut count = 0;
        for row in &self.boxes {
            for cell in row {
                if cell.is_some() {
                    count += 1;
                }
            }
        }
        count
    }
}

// ── AI ──────────────────────────────────────────────────────────────
/// Greedy AI strategy:
/// 1. If any line completes a box, take it.
/// 2. Avoid lines that would give the opponent a box (lines that leave a box with 3 sides).
/// 3. Otherwise, pick a random safe line.
/// 4. If forced, pick a line that gives away the fewest boxes.
fn ai_choose_line(board: &Board, rng: &mut SeededRng) -> Option<LineId> {
    let available = board.available_lines();
    if available.is_empty() {
        return None;
    }

    // Phase 1: Find lines that complete boxes (greedy capture).
    let mut completing = Vec::new();
    for &line in &available {
        let adjacent = board.adjacent_boxes(line);
        for (br, bc) in &adjacent {
            if board.box_side_count(*br, *bc) == 3 {
                completing.push(line);
                break;
            }
        }
    }
    if !completing.is_empty() {
        // Prefer completing multiple boxes at once.
        let mut best_line = completing[0];
        let mut best_count = 0;
        for &line in &completing {
            let mut count = 0;
            let adjacent = board.adjacent_boxes(line);
            for (br, bc) in &adjacent {
                if board.box_side_count(*br, *bc) == 3 {
                    count += 1;
                }
            }
            if count > best_count {
                best_count = count;
                best_line = line;
            }
        }
        return Some(best_line);
    }

    // Phase 2: Find safe lines (don't give opponent a box with 3 sides).
    let mut safe = Vec::new();
    for &line in &available {
        let mut gives_away = false;
        let adjacent = board.adjacent_boxes(line);
        for (br, bc) in &adjacent {
            // Drawing this line would bring this box to side_count+1 sides.
            if board.box_side_count(*br, *bc) == 2 {
                gives_away = true;
                break;
            }
        }
        if !gives_away {
            safe.push(line);
        }
    }
    if !safe.is_empty() {
        let idx = rng.below(safe.len());
        return Some(safe[idx]);
    }

    // Phase 3: All moves are dangerous. Pick the one that gives away the fewest boxes.
    let mut best_line = available[0];
    let mut best_damage = usize::MAX;
    for &line in &available {
        let mut damage = 0;
        let adjacent = board.adjacent_boxes(line);
        for (br, bc) in &adjacent {
            if board.box_side_count(*br, *bc) == 2 {
                damage += 1;
            }
        }
        if damage < best_damage {
            best_damage = damage;
            best_line = line;
        }
    }
    Some(best_line)
}

// ── Main app struct ─────────────────────────────────────────────────
struct DotsAndBoxes {
    board: Board,
    current_player: Player,
    phase: GamePhase,
    mode: GameMode,
    cursor: Cursor,
    score_p1: usize,
    score_p2: usize,
    total_moves: usize,
    rng: SeededRng,
    /// Accumulated time for AI delay.
    ai_delay_ms: u64,
    /// Whether the AI is "thinking" (short delay before move).
    ai_pending: bool,
}

/// AI thinking delay in milliseconds.
const AI_DELAY: u64 = 400;

impl DotsAndBoxes {
    fn new() -> Self {
        Self::with_config(
            DEFAULT_GRID_SIZE,
            GameMode::VsAi,
            seed_from_system(FALLBACK_SEED),
        )
    }

    fn with_config(grid_size: usize, mode: GameMode, seed: u64) -> Self {
        let clamped_size = grid_size.clamp(MIN_GRID_SIZE, MAX_GRID_SIZE);
        Self {
            board: Board::new(clamped_size),
            current_player: Player::One,
            phase: GamePhase::Playing,
            mode,
            cursor: Cursor {
                orientation: Orientation::Horizontal,
                row: 0,
                col: 0,
            },
            score_p1: 0,
            score_p2: 0,
            total_moves: 0,
            rng: SeededRng::new(seed),
            ai_delay_ms: 0,
            ai_pending: false,
        }
    }

    /// Start a new game with the current grid size and mode.
    fn new_game(&mut self) {
        let size = self.board.grid_size;
        let mode = self.mode;
        let seed = self.rng.next_u64();
        *self = Self::with_config(size, mode, seed);
    }

    /// Start a new game with a specific grid size.
    fn new_game_with_size(&mut self, grid_size: usize) {
        let mode = self.mode;
        let seed = self.rng.next_u64();
        *self = Self::with_config(grid_size, mode, seed);
    }

    /// Grid size (number of dots per side).
    fn grid_size(&self) -> usize {
        self.board.grid_size
    }

    /// Number of boxes per side.
    fn boxes_per_side(&self) -> usize {
        self.board.boxes_per_side()
    }

    /// Try to place a line at the given position. Returns true if a line was placed.
    fn try_place_line(&mut self, line: LineId) -> bool {
        if self.phase != GamePhase::Playing {
            return false;
        }
        if !self.board.is_valid_line(line) || self.board.is_line_drawn(line) {
            return false;
        }
        if self.ai_pending {
            return false;
        }

        let completed = self.board.draw_line(line, self.current_player);
        self.total_moves += 1;

        match self.current_player {
            Player::One => self.score_p1 += completed,
            Player::Two => self.score_p2 += completed,
        }

        if self.board.all_lines_drawn() {
            self.phase = GamePhase::GameOver;
            return true;
        }

        // If the player completed a box, they get another turn.
        if completed == 0 {
            self.current_player = self.current_player.other();
            // If it's now the AI's turn, start the delay.
            if self.mode == GameMode::VsAi && self.current_player == Player::Two {
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            }
        } else {
            // Player completed a box and gets another turn.
            // If the current player is AI, start another AI move.
            if self.mode == GameMode::VsAi && self.current_player == Player::Two {
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            }
        }
        true
    }

    /// Execute the AI's move.
    fn do_ai_move(&mut self) {
        if self.phase != GamePhase::Playing {
            return;
        }
        if let Some(line) = ai_choose_line(&self.board, &mut self.rng) {
            let completed = self.board.draw_line(line, Player::Two);
            self.total_moves += 1;
            self.score_p2 += completed;

            if self.board.all_lines_drawn() {
                self.phase = GamePhase::GameOver;
                self.ai_pending = false;
                return;
            }

            if completed > 0 {
                // AI gets another turn.
                self.ai_pending = true;
                self.ai_delay_ms = 0;
            } else {
                self.current_player = Player::One;
                self.ai_pending = false;
            }
        } else {
            self.ai_pending = false;
        }
    }

    /// Get the winner, if any. Returns None for a draw.
    fn winner(&self) -> Option<Player> {
        if self.score_p1 > self.score_p2 {
            Some(Player::One)
        } else if self.score_p2 > self.score_p1 {
            Some(Player::Two)
        } else {
            None
        }
    }

    // ── Cursor navigation ──────────────────────────────────────────

    /// Move the cursor in the given direction, wrapping around.
    fn move_cursor(&mut self, key: Key) {
        let bps = self.boxes_per_side();
        let gs = self.grid_size();
        match self.cursor.orientation {
            Orientation::Horizontal => {
                // Horizontal lines: row 0..grid_size, col 0..bps
                match key {
                    Key::Left => {
                        if self.cursor.col > 0 {
                            self.cursor.col -= 1;
                        } else {
                            self.cursor.col = bps.saturating_sub(1);
                        }
                    }
                    Key::Right => {
                        if self.cursor.col + 1 < bps {
                            self.cursor.col += 1;
                        } else {
                            self.cursor.col = 0;
                        }
                    }
                    Key::Up => {
                        if self.cursor.row > 0 {
                            self.cursor.row -= 1;
                        } else {
                            // Switch to vertical, going to a vertical line at bottom.
                            self.cursor.orientation = Orientation::Vertical;
                            self.cursor.row = bps.saturating_sub(1);
                            self.cursor.col = self.cursor.col.min(gs.saturating_sub(1));
                        }
                    }
                    Key::Down => {
                        if self.cursor.row + 1 < gs {
                            self.cursor.row += 1;
                        } else {
                            // Switch to vertical orientation at top.
                            self.cursor.orientation = Orientation::Vertical;
                            self.cursor.row = 0;
                            self.cursor.col = self.cursor.col.min(gs.saturating_sub(1));
                        }
                    }
                    _ => {}
                }
            }
            Orientation::Vertical => {
                // Vertical lines: row 0..bps, col 0..grid_size
                match key {
                    Key::Left => {
                        if self.cursor.col > 0 {
                            self.cursor.col -= 1;
                        } else {
                            self.cursor.col = gs.saturating_sub(1);
                        }
                    }
                    Key::Right => {
                        if self.cursor.col + 1 < gs {
                            self.cursor.col += 1;
                        } else {
                            self.cursor.col = 0;
                        }
                    }
                    Key::Up => {
                        if self.cursor.row > 0 {
                            self.cursor.row -= 1;
                        } else {
                            // Switch to horizontal orientation at bottom.
                            self.cursor.orientation = Orientation::Horizontal;
                            self.cursor.row = gs.saturating_sub(1);
                            self.cursor.col = self.cursor.col.min(bps.saturating_sub(1));
                        }
                    }
                    Key::Down => {
                        if self.cursor.row + 1 < bps {
                            self.cursor.row += 1;
                        } else {
                            // Switch to horizontal at top.
                            self.cursor.orientation = Orientation::Horizontal;
                            self.cursor.row = 0;
                            self.cursor.col = self.cursor.col.min(bps.saturating_sub(1));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Toggle cursor orientation between horizontal and vertical.
    fn toggle_cursor_orientation(&mut self) {
        let bps = self.boxes_per_side();
        let gs = self.grid_size();
        match self.cursor.orientation {
            Orientation::Horizontal => {
                self.cursor.orientation = Orientation::Vertical;
                self.cursor.row = self.cursor.row.min(bps.saturating_sub(1));
                self.cursor.col = self.cursor.col.min(gs.saturating_sub(1));
            }
            Orientation::Vertical => {
                self.cursor.orientation = Orientation::Horizontal;
                self.cursor.row = self.cursor.row.min(gs.saturating_sub(1));
                self.cursor.col = self.cursor.col.min(bps.saturating_sub(1));
            }
        }
    }

    // ── Board geometry ─────────────────────────────────────────────
    //
    // There used to be two hit tests. `handle_mouse` called
    // `hit_test_line_precise`, which measures the perpendicular distance to
    // each line *segment*, so the whole 70px length of a line was clickable.
    // `hit_test_line` measured the distance to each segment's *midpoint*
    // instead, so only a 12px disc around the middle of a line answered at all
    // and the two ends were dead. They were not two spellings of one rule --
    // they were two different rules -- and the app ran one of them while three
    // of the tests were pointed at the other. A test aimed at a function
    // nothing calls is green about code that does not ship; see
    // design-decisions.md §486. The midpoint version is gone.
    //
    // The endpoints of a line were also derived at six sites (twice in each
    // hit test, twice in `render_lines`, twice in `render_cursor`), which is
    // what let a hit test and the picture of the same line disagree in the
    // first place. `line_endpoints` is now the one place that is written.

    /// How far from a line a click may land and still count, in pixels.
    ///
    /// Measured perpendicular to the line, so the whole of a line is equally
    /// clickable -- the alternative, distance to the line's midpoint, makes
    /// the ends of every line dead and is what the deleted `hit_test_line`
    /// did. At `DOT_SPACING` 70 and this threshold, the bands around two
    /// parallel lines never touch, so "nearest line" is never a close call.
    const CLICK_THRESHOLD: f32 = 10.0;

    /// Centre of the dot at grid coordinates (row, col), in pixels.
    ///
    /// The first dot's *centre* used to sit at `PADDING`, which put its left
    /// half inside the padding while the window still reserved a whole dot's
    /// width past the last column -- so the board hung 12px up and to the
    /// left of centre in its own window. Offsetting by `DOT_RADIUS` here
    /// makes the painted board, discs included, sit exactly `PADDING` from
    /// every edge; `board_pixel_span` measures the same discs, so the two
    /// agree by construction.
    fn dot_pos(&self, row: usize, col: usize) -> (f32, f32) {
        let x = PADDING + DOT_RADIUS + col as f32 * DOT_SPACING;
        let y = PADDING + DOT_RADIUS + HEADER_HEIGHT + row as f32 * DOT_SPACING;
        (x, y)
    }

    /// The two dots a line runs between, in pixels.
    ///
    /// A horizontal line joins `(row, col)` to the dot on its right; a
    /// vertical one joins it to the dot below. Both hit-testing and painting
    /// read this, so a line cannot be drawn between one pair of dots and
    /// clicked between another.
    fn line_endpoints(&self, line: LineId) -> ((f32, f32), (f32, f32)) {
        let start = self.dot_pos(line.row, line.col);
        let end = match line.orientation {
            Orientation::Horizontal => (self.dot_pos(line.row, line.col + 1).0, start.1),
            Orientation::Vertical => (start.0, self.dot_pos(line.row + 1, line.col).1),
        };
        (start, end)
    }

    /// Every line on the board, drawn or not.
    fn all_lines(&self) -> impl Iterator<Item = LineId> + use<> {
        let gs = self.grid_size();
        let bps = self.boxes_per_side();
        let horizontals =
            (0..gs).flat_map(move |row| (0..bps).map(move |col| LineId::horizontal(row, col)));
        let verticals =
            (0..bps).flat_map(move |row| (0..gs).map(move |col| LineId::vertical(row, col)));
        horizontals.chain(verticals)
    }

    /// Distance from the far side of the outermost dots to the window edge.
    ///
    /// The dots are drawn as discs of `DOT_RADIUS` centred on the lattice, so
    /// the picture reaches half a dot past the last dot's centre on every
    /// side; the window has to hold that as well as the lattice itself.
    fn board_pixel_span(&self) -> f32 {
        let gs = self.grid_size();
        let (near, _) = self.dot_pos(0, 0);
        let (far, _) = self.dot_pos(0, gs - 1);
        far - near + DOT_RADIUS * 2.0
    }

    /// Window width for current grid size.
    fn window_width(&self) -> f32 {
        PADDING * 2.0 + self.board_pixel_span()
    }

    /// Window height for current grid size.
    fn window_height(&self) -> f32 {
        PADDING * 2.0 + HEADER_HEIGHT + FOOTER_HEIGHT + self.board_pixel_span()
    }

    /// The line a click at `(mx, my)` lands on, or `None` if it is not within
    /// `CLICK_THRESHOLD` of any of them.
    fn hit_test_line(&self, mx: f32, my: f32) -> Option<LineId> {
        let mut best_line: Option<LineId> = None;
        let mut best_dist = Self::CLICK_THRESHOLD;
        for line in self.all_lines() {
            let ((x1, y1), (x2, y2)) = self.line_endpoints(line);
            let dist = point_to_segment_distance(mx, my, x1, y1, x2, y2);
            if dist < best_dist {
                best_dist = dist;
                best_line = Some(line);
            }
        }
        best_line
    }

    // ── Event handling ─────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) if ke.pressed => {
                self.handle_key(ke.key);
            }
            Event::Mouse(me) => {
                self.handle_mouse(me);
            }
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: Key) {
        match key {
            Key::N => {
                self.new_game();
            }
            Key::M => {
                self.mode = match self.mode {
                    GameMode::VsAi => GameMode::TwoPlayer,
                    GameMode::TwoPlayer => GameMode::VsAi,
                };
                self.new_game();
            }
            Key::Num3 => {
                self.new_game_with_size(3);
            }
            Key::Num4 => {
                self.new_game_with_size(4);
            }
            Key::Num5 => {
                self.new_game_with_size(5);
            }
            Key::Left | Key::Right | Key::Up | Key::Down
                if self.phase == GamePhase::Playing && !self.ai_pending =>
            {
                self.move_cursor(key);
            }
            Key::Tab if self.phase == GamePhase::Playing && !self.ai_pending => {
                self.toggle_cursor_orientation();
            }
            Key::Enter | Key::Space if self.phase == GamePhase::Playing && !self.ai_pending => {
                let line = self.cursor.to_line_id();
                self.try_place_line(line);
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, me: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = me.kind
            && self.phase == GamePhase::Playing
            && !self.ai_pending
            && let Some(line) = self.hit_test_line(me.x, me.y)
            && !self.board.is_line_drawn(line)
        {
            self.try_place_line(line);
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.ai_pending && self.phase == GamePhase::Playing {
            self.ai_delay_ms += elapsed_ms;
            if self.ai_delay_ms >= AI_DELAY {
                self.ai_pending = false;
                self.do_ai_move();
            }
        }
    }

    // ── Rendering ──────────────────────────────────────────────────

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Header.
        self.render_header(&mut cmds, width);

        // Boxes (filled areas).
        self.render_boxes(&mut cmds);

        // Drawn lines.
        self.render_lines(&mut cmds);

        // Cursor highlight.
        if self.phase == GamePhase::Playing && !self.ai_pending {
            self.render_cursor(&mut cmds);
        }

        // Dots.
        self.render_dots(&mut cmds);

        // Footer with controls help.
        self.render_footer(&mut cmds, width);

        // Game over overlay.
        if self.phase == GamePhase::GameOver {
            self.render_game_over(&mut cmds, width, height);
        }

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>, win_width: f32) {
        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_width,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 8.0,
            text: String::from("Dots & Boxes"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Mode label.
        let mode_text = format!(
            "{} | {}x{}",
            self.mode.label(),
            self.grid_size(),
            self.grid_size()
        );
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 34.0,
            text: mode_text,
            color: SUBTEXT0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Scores.
        let p1_text = format!("{}: {}", Player::One.label(), self.score_p1);
        let p1_label = if self.mode == GameMode::VsAi {
            format!("You: {}", self.score_p1)
        } else {
            p1_text
        };
        cmds.push(RenderCommand::Text {
            x: win_width - 200.0,
            y: 8.0,
            text: p1_label,
            color: PLAYER1_COLOR,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let p2_text = if self.mode == GameMode::VsAi {
            format!("AI: {}", self.score_p2)
        } else {
            format!("{}: {}", Player::Two.label(), self.score_p2)
        };
        cmds.push(RenderCommand::Text {
            x: win_width - 200.0,
            y: 28.0,
            text: p2_text,
            color: PLAYER2_COLOR,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Current turn indicator.
        let turn_text = if self.phase == GamePhase::GameOver {
            String::from("Game Over!")
        } else if self.ai_pending {
            String::from("AI thinking...")
        } else {
            let name = if self.mode == GameMode::VsAi {
                match self.current_player {
                    Player::One => "Your turn",
                    Player::Two => "AI's turn",
                }
            } else {
                match self.current_player {
                    Player::One => "P1's turn",
                    Player::Two => "P2's turn",
                }
            };
            String::from(name)
        };
        cmds.push(RenderCommand::Text {
            x: win_width - 100.0,
            y: 18.0,
            text: turn_text,
            color: self.current_player.color(),
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_boxes(&self, cmds: &mut Vec<RenderCommand>) {
        let bps = self.boxes_per_side();
        for row in 0..bps {
            for col in 0..bps {
                if let Some(player) = self.board.boxes[row][col] {
                    let (x1, y1) = self.dot_pos(row, col);
                    let (x2, y2) = self.dot_pos(row + 1, col + 1);
                    let margin = 3.0;
                    cmds.push(RenderCommand::FillRect {
                        x: x1 + margin,
                        y: y1 + margin,
                        width: x2 - x1 - margin * 2.0,
                        height: y2 - y1 - margin * 2.0,
                        color: player.box_color(),
                        corner_radii: CornerRadii::all(3.0),
                    });

                    // Player initial in the box.
                    let label = match player {
                        Player::One => {
                            if self.mode == GameMode::VsAi {
                                "Y"
                            } else {
                                "1"
                            }
                        }
                        Player::Two => {
                            if self.mode == GameMode::VsAi {
                                "A"
                            } else {
                                "2"
                            }
                        }
                    };
                    let cx = f32::midpoint(x1, x2) - 5.0;
                    let cy = f32::midpoint(y1, y2) - 8.0;
                    cmds.push(RenderCommand::Text {
                        x: cx,
                        y: cy,
                        text: String::from(label),
                        color: player.color(),
                        font_size: SCORE_FONT_SIZE,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
            }
        }
    }

    fn render_lines(&self, cmds: &mut Vec<RenderCommand>) {
        // One loop over `all_lines`, not one per orientation, and the ends of
        // each line come from `line_endpoints` -- the same call the hit test
        // makes -- so a line is painted exactly where clicking it works.
        for line in self.all_lines() {
            let ((x1, y1), (x2, y2)) = self.line_endpoints(line);
            let drawn = self.board.is_line_drawn(line);
            let color = if drawn { LAVENDER } else { SURFACE0 };
            let w = if drawn { LINE_THICKNESS } else { 2.0 };
            cmds.push(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width: w,
            });
        }
    }

    fn render_cursor(&self, cmds: &mut Vec<RenderCommand>) {
        let line = self.cursor.to_line_id();
        if !self.board.is_valid_line(line) {
            return;
        }
        if self.board.is_line_drawn(line) {
            // Cursor is on an already-drawn line; show it dimmer.
            return;
        }

        let ((x1, y1), (x2, y2)) = self.line_endpoints(line);
        cmds.push(RenderCommand::Line {
            x1,
            y1,
            x2,
            y2,
            color: self.current_player.color(),
            width: LINE_HOVER_THICKNESS,
        });
    }

    fn render_dots(&self, cmds: &mut Vec<RenderCommand>) {
        let gs = self.grid_size();
        for row in 0..gs {
            for col in 0..gs {
                let (x, y) = self.dot_pos(row, col);
                cmds.push(RenderCommand::FillRect {
                    x: x - DOT_RADIUS,
                    y: y - DOT_RADIUS,
                    width: DOT_RADIUS * 2.0,
                    height: DOT_RADIUS * 2.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(DOT_RADIUS),
                });
            }
        }
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>, win_width: f32) {
        let footer_y = self.window_height() - FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: footer_y,
            width: win_width,
            height: FOOTER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y + 12.0,
            text: String::from(
                "Arrows: move | Tab: toggle H/V | Enter: draw | N: new | M: mode | 3/4/5: size",
            ),
            color: OVERLAY0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_game_over(&self, cmds: &mut Vec<RenderCommand>, win_width: f32, win_height: f32) {
        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_width,
            height: win_height,
            color: Color::from_hex(0x11111B),
            corner_radii: CornerRadii::ZERO,
        });

        let box_w = 260.0;
        let box_h = 140.0;
        let box_x = (win_width - box_w) / 2.0;
        let box_y = (win_height - box_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: box_x,
            y: box_y,
            width: box_w,
            height: box_h,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 16.0,
            text: String::from("Game Over!"),
            color: YELLOW,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let result_text = match self.winner() {
            Some(Player::One) => {
                if self.mode == GameMode::VsAi {
                    String::from("You win!")
                } else {
                    String::from("Player 1 wins!")
                }
            }
            Some(Player::Two) => {
                if self.mode == GameMode::VsAi {
                    String::from("AI wins!")
                } else {
                    String::from("Player 2 wins!")
                }
            }
            None => String::from("It's a draw!"),
        };
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 50.0,
            text: result_text,
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let score_text = format!("Score: {} - {}", self.score_p1, self.score_p2);
        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 76.0,
            text: score_text,
            color: SUBTEXT0,
            font_size: SCORE_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: box_x + 20.0,
            y: box_y + 106.0,
            text: String::from("Press N for new game"),
            color: OVERLAY0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

// ── Geometry helper ─────────────────────────────────────────────────
/// Compute the distance from point (px, py) to the line segment from (x1, y1) to (x2, y2).
fn point_to_segment_distance(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 {
        // Degenerate segment (zero length).
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    // Project point onto the line, clamping to [0, 1].
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t_clamped = t.clamp(0.0, 1.0);
    let proj_x = x1 + t_clamped * dx;
    let proj_y = y1 + t_clamped * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

fn main() {
    let _app = DotsAndBoxes::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // ── Helper functions ───────────────────────────────────────────

    fn test_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(4, GameMode::VsAi, 12345)
    }

    fn two_player_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(4, GameMode::TwoPlayer, 12345)
    }

    fn small_app() -> DotsAndBoxes {
        DotsAndBoxes::with_config(3, GameMode::TwoPlayer, 99)
    }

    fn make_key_event(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn make_key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn make_click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn make_tick(ms: u64) -> Event {
        Event::Tick { elapsed_ms: ms }
    }

    // ── Board construction ─────────────────────────────────────────

    #[test]
    fn test_board_new_3x3() {
        let board = Board::new(3);
        assert_eq!(board.grid_size, 3);
        assert_eq!(board.boxes_per_side(), 2);
        assert_eq!(board.total_boxes(), 4);
    }

    #[test]
    fn test_board_new_4x4() {
        let board = Board::new(4);
        assert_eq!(board.grid_size, 4);
        assert_eq!(board.boxes_per_side(), 3);
        assert_eq!(board.total_boxes(), 9);
    }

    #[test]
    fn test_board_new_5x5() {
        let board = Board::new(5);
        assert_eq!(board.grid_size, 5);
        assert_eq!(board.boxes_per_side(), 4);
        assert_eq!(board.total_boxes(), 16);
    }

    #[test]
    fn test_board_total_lines_3x3() {
        let board = Board::new(3);
        // 3*2 horizontal + 2*3 vertical = 12
        assert_eq!(board.total_lines(), 12);
    }

    #[test]
    fn test_board_total_lines_4x4() {
        let board = Board::new(4);
        // 4*3 horizontal + 3*4 vertical = 24
        assert_eq!(board.total_lines(), 24);
    }

    #[test]
    fn test_board_total_lines_5x5() {
        let board = Board::new(5);
        // 5*4 horizontal + 4*5 vertical = 40
        assert_eq!(board.total_lines(), 40);
    }

    #[test]
    fn test_board_initially_empty() {
        let board = Board::new(4);
        assert_eq!(board.drawn_line_count(), 0);
        assert_eq!(board.claimed_boxes(), 0);
    }

    #[test]
    fn test_board_no_lines_drawn_initially() {
        let board = Board::new(4);
        assert!(!board.is_line_drawn(LineId::horizontal(0, 0)));
        assert!(!board.is_line_drawn(LineId::vertical(0, 0)));
    }

    #[test]
    fn test_board_all_lines_not_drawn_initially() {
        let board = Board::new(4);
        assert!(!board.all_lines_drawn());
    }

    // ── Line drawing ───────────────────────────────────────────────

    #[test]
    fn test_draw_horizontal_line() {
        let mut board = Board::new(4);
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        assert!(board.is_line_drawn(LineId::horizontal(0, 0)));
        assert_eq!(board.drawn_line_count(), 1);
    }

    #[test]
    fn test_draw_vertical_line() {
        let mut board = Board::new(4);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        assert!(board.is_line_drawn(LineId::vertical(0, 0)));
        assert_eq!(board.drawn_line_count(), 1);
    }

    #[test]
    fn test_draw_line_returns_zero_incomplete() {
        let mut board = Board::new(4);
        let completed = board.draw_line(LineId::horizontal(0, 0), Player::One);
        assert_eq!(completed, 0);
    }

    #[test]
    fn test_draw_duplicate_line_returns_zero() {
        let mut board = Board::new(4);
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        let completed = board.draw_line(LineId::horizontal(0, 0), Player::Two);
        assert_eq!(completed, 0);
        assert_eq!(board.drawn_line_count(), 1);
    }

    #[test]
    fn test_draw_invalid_line_returns_zero() {
        let mut board = Board::new(4);
        let completed = board.draw_line(LineId::horizontal(99, 99), Player::One);
        assert_eq!(completed, 0);
    }

    // ── Box completion ─────────────────────────────────────────────

    #[test]
    fn test_complete_single_box() {
        let mut board = Board::new(4);
        // Complete box (0,0): top, bottom, left, right.
        board.draw_line(LineId::horizontal(0, 0), Player::One); // top
        board.draw_line(LineId::horizontal(1, 0), Player::One); // bottom
        board.draw_line(LineId::vertical(0, 0), Player::One); // left
        let completed = board.draw_line(LineId::vertical(0, 1), Player::One); // right
        assert_eq!(completed, 1);
        assert_eq!(board.boxes[0][0], Some(Player::One));
    }

    #[test]
    fn test_box_side_count_increments() {
        let mut board = Board::new(4);
        assert_eq!(board.box_side_count(0, 0), 0);
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        assert_eq!(board.box_side_count(0, 0), 1);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        assert_eq!(board.box_side_count(0, 0), 2);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        assert_eq!(board.box_side_count(0, 0), 3);
        board.draw_line(LineId::vertical(0, 1), Player::One);
        assert_eq!(board.box_side_count(0, 0), 4);
    }

    #[test]
    fn test_is_box_complete() {
        let mut board = Board::new(4);
        assert!(!board.is_box_complete(0, 0));
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::vertical(0, 1), Player::One);
        assert!(board.is_box_complete(0, 0));
    }

    #[test]
    fn test_complete_two_boxes_one_line() {
        let mut board = Board::new(4);
        // Set up two adjacent boxes sharing a vertical line at col=1.
        // Box (0,0): top=h(0,0), bottom=h(1,0), left=v(0,0), right=v(0,1)
        // Box (0,1): top=h(0,1), bottom=h(1,1), left=v(0,1), right=v(0,2)
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::horizontal(0, 1), Player::One);
        board.draw_line(LineId::horizontal(1, 1), Player::One);
        board.draw_line(LineId::vertical(0, 2), Player::One);
        // The shared vertical line at (0,1) completes both boxes.
        let completed = board.draw_line(LineId::vertical(0, 1), Player::One);
        assert_eq!(completed, 2);
    }

    #[test]
    fn test_score_tracking() {
        let mut board = Board::new(4);
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::vertical(0, 1), Player::One);
        assert_eq!(board.score(Player::One), 1);
        assert_eq!(board.score(Player::Two), 0);
    }

    #[test]
    fn test_claimed_boxes_count() {
        let mut board = Board::new(3);
        assert_eq!(board.claimed_boxes(), 0);
        // Complete box (0,0).
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::vertical(0, 1), Player::One);
        assert_eq!(board.claimed_boxes(), 1);
    }

    // ── Line validity ──────────────────────────────────────────────

    #[test]
    fn test_valid_horizontal_lines() {
        let board = Board::new(4);
        assert!(board.is_valid_line(LineId::horizontal(0, 0)));
        assert!(board.is_valid_line(LineId::horizontal(3, 2)));
        assert!(!board.is_valid_line(LineId::horizontal(4, 0)));
        assert!(!board.is_valid_line(LineId::horizontal(0, 3)));
    }

    #[test]
    fn test_valid_vertical_lines() {
        let board = Board::new(4);
        assert!(board.is_valid_line(LineId::vertical(0, 0)));
        assert!(board.is_valid_line(LineId::vertical(2, 3)));
        assert!(!board.is_valid_line(LineId::vertical(3, 0)));
        assert!(!board.is_valid_line(LineId::vertical(0, 4)));
    }

    // ── Available lines ────────────────────────────────────────────

    #[test]
    fn test_available_lines_full_board() {
        let board = Board::new(4);
        let available = board.available_lines();
        assert_eq!(available.len(), board.total_lines());
    }

    #[test]
    fn test_available_lines_after_draw() {
        let mut board = Board::new(4);
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        let available = board.available_lines();
        assert_eq!(available.len(), board.total_lines() - 1);
    }

    #[test]
    fn test_available_lines_excludes_drawn() {
        let mut board = Board::new(4);
        let line = LineId::horizontal(0, 0);
        board.draw_line(line, Player::One);
        let available = board.available_lines();
        assert!(!available.contains(&line));
    }

    // ── Adjacent boxes ─────────────────────────────────────────────

    #[test]
    fn test_adjacent_boxes_top_horizontal() {
        let board = Board::new(4);
        // Top edge horizontal line at (0, 0) only borders box (0, 0) below.
        let adj = board.adjacent_boxes(LineId::horizontal(0, 0));
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0], (0, 0));
    }

    #[test]
    fn test_adjacent_boxes_middle_horizontal() {
        let board = Board::new(4);
        // Horizontal line at (1, 0) borders box (0, 0) above and box (1, 0) below.
        let adj = board.adjacent_boxes(LineId::horizontal(1, 0));
        assert_eq!(adj.len(), 2);
    }

    #[test]
    fn test_adjacent_boxes_bottom_horizontal() {
        let board = Board::new(4);
        // Bottom edge horizontal line at (3, 0) only borders box (2, 0) above.
        let adj = board.adjacent_boxes(LineId::horizontal(3, 0));
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0], (2, 0));
    }

    #[test]
    fn test_adjacent_boxes_left_vertical() {
        let board = Board::new(4);
        // Left edge vertical line at (0, 0) only borders box (0, 0) to the right.
        let adj = board.adjacent_boxes(LineId::vertical(0, 0));
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0], (0, 0));
    }

    #[test]
    fn test_adjacent_boxes_middle_vertical() {
        let board = Board::new(4);
        // Middle vertical line at (0, 1) borders box (0, 0) left and box (0, 1) right.
        let adj = board.adjacent_boxes(LineId::vertical(0, 1));
        assert_eq!(adj.len(), 2);
    }

    #[test]
    fn test_adjacent_boxes_right_vertical() {
        let board = Board::new(4);
        // Right edge vertical at (0, 3) only borders box (0, 2) to the left.
        let adj = board.adjacent_boxes(LineId::vertical(0, 3));
        assert_eq!(adj.len(), 1);
        assert_eq!(adj[0], (0, 2));
    }

    // ── All lines drawn ────────────────────────────────────────────

    #[test]
    fn test_all_lines_drawn_small() {
        let mut board = Board::new(3);
        // Draw all 12 lines.
        for row in 0..3 {
            for col in 0..2 {
                board.draw_line(LineId::horizontal(row, col), Player::One);
            }
        }
        for row in 0..2 {
            for col in 0..3 {
                board.draw_line(LineId::vertical(row, col), Player::One);
            }
        }
        assert!(board.all_lines_drawn());
    }

    // ── DotsAndBoxes construction ──────────────────────────────────

    #[test]
    fn test_app_default_grid_size() {
        let app = DotsAndBoxes::new();
        assert_eq!(app.grid_size(), DEFAULT_GRID_SIZE);
    }

    #[test]
    fn test_app_initial_scores_zero() {
        let app = test_app();
        assert_eq!(app.score_p1, 0);
        assert_eq!(app.score_p2, 0);
    }

    #[test]
    fn test_app_initial_phase_playing() {
        let app = test_app();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_app_initial_player_one() {
        let app = test_app();
        assert_eq!(app.current_player, Player::One);
    }

    #[test]
    fn test_app_initial_total_moves() {
        let app = test_app();
        assert_eq!(app.total_moves, 0);
    }

    #[test]
    fn test_app_vs_ai_mode() {
        let app = DotsAndBoxes::with_config(4, GameMode::VsAi, 42);
        assert_eq!(app.mode, GameMode::VsAi);
    }

    #[test]
    fn test_app_two_player_mode() {
        let app = DotsAndBoxes::with_config(4, GameMode::TwoPlayer, 42);
        assert_eq!(app.mode, GameMode::TwoPlayer);
    }

    #[test]
    fn test_grid_size_clamping_min() {
        let app = DotsAndBoxes::with_config(1, GameMode::VsAi, 42);
        assert_eq!(app.grid_size(), MIN_GRID_SIZE);
    }

    #[test]
    fn test_grid_size_clamping_max() {
        let app = DotsAndBoxes::with_config(10, GameMode::VsAi, 42);
        assert_eq!(app.grid_size(), MAX_GRID_SIZE);
    }

    // ── Line placement ─────────────────────────────────────────────

    #[test]
    fn test_place_line_success() {
        let mut app = two_player_app();
        let result = app.try_place_line(LineId::horizontal(0, 0));
        assert!(result);
        assert!(app.board.is_line_drawn(LineId::horizontal(0, 0)));
    }

    #[test]
    fn test_place_line_switches_player() {
        let mut app = two_player_app();
        assert_eq!(app.current_player, Player::One);
        app.try_place_line(LineId::horizontal(0, 0));
        assert_eq!(app.current_player, Player::Two);
    }

    #[test]
    fn test_place_line_increments_moves() {
        let mut app = two_player_app();
        app.try_place_line(LineId::horizontal(0, 0));
        assert_eq!(app.total_moves, 1);
    }

    #[test]
    fn test_place_duplicate_line_fails() {
        let mut app = two_player_app();
        app.try_place_line(LineId::horizontal(0, 0));
        let result = app.try_place_line(LineId::horizontal(0, 0));
        assert!(!result);
        assert_eq!(app.total_moves, 1);
    }

    #[test]
    fn test_place_invalid_line_fails() {
        let mut app = two_player_app();
        let result = app.try_place_line(LineId::horizontal(99, 99));
        assert!(!result);
    }

    #[test]
    fn test_completing_box_keeps_same_player() {
        let mut app = two_player_app();
        // Player 1 draws 3 sides.
        app.try_place_line(LineId::horizontal(0, 0));
        // Now P2's turn.
        app.try_place_line(LineId::horizontal(1, 0));
        // P1 again.
        app.try_place_line(LineId::vertical(0, 0));
        // P2 again.
        // The last side completes the box - P2 gets another turn.
        app.try_place_line(LineId::vertical(0, 1));
        assert_eq!(app.score_p2, 1);
        assert_eq!(app.current_player, Player::Two);
    }

    #[test]
    fn test_cant_place_when_game_over() {
        let mut app = two_player_app();
        app.phase = GamePhase::GameOver;
        let result = app.try_place_line(LineId::horizontal(0, 0));
        assert!(!result);
    }

    // ── Score tracking ─────────────────────────────────────────────

    #[test]
    fn test_score_after_completing_box() {
        let mut app = two_player_app();
        // P1: top, P2: bottom, P1: left, P2: right (completes box).
        app.try_place_line(LineId::horizontal(0, 0));
        app.try_place_line(LineId::horizontal(1, 0));
        app.try_place_line(LineId::vertical(0, 0));
        app.try_place_line(LineId::vertical(0, 1));
        assert_eq!(app.score_p2, 1);
    }

    // ── Game over ──────────────────────────────────────────────────

    #[test]
    fn test_game_over_when_all_lines_drawn() {
        let mut app = DotsAndBoxes::with_config(3, GameMode::TwoPlayer, 42);
        let bps = app.boxes_per_side();
        let gs = app.grid_size();
        // Draw all lines.
        for row in 0..gs {
            for col in 0..bps {
                app.try_place_line(LineId::horizontal(row, col));
            }
        }
        for row in 0..bps {
            for col in 0..gs {
                app.try_place_line(LineId::vertical(row, col));
            }
        }
        assert_eq!(app.phase, GamePhase::GameOver);
    }

    #[test]
    fn test_total_scores_equal_total_boxes() {
        let mut app = DotsAndBoxes::with_config(3, GameMode::TwoPlayer, 42);
        let total_boxes = app.board.total_boxes();
        // Draw all lines.
        let bps = app.boxes_per_side();
        let gs = app.grid_size();
        for row in 0..gs {
            for col in 0..bps {
                app.try_place_line(LineId::horizontal(row, col));
            }
        }
        for row in 0..bps {
            for col in 0..gs {
                app.try_place_line(LineId::vertical(row, col));
            }
        }
        assert_eq!(app.score_p1 + app.score_p2, total_boxes);
    }

    // ── Winner ─────────────────────────────────────────────────────

    #[test]
    fn test_winner_p1() {
        let mut app = two_player_app();
        app.score_p1 = 5;
        app.score_p2 = 3;
        assert_eq!(app.winner(), Some(Player::One));
    }

    #[test]
    fn test_winner_p2() {
        let mut app = two_player_app();
        app.score_p1 = 2;
        app.score_p2 = 7;
        assert_eq!(app.winner(), Some(Player::Two));
    }

    #[test]
    fn test_winner_draw() {
        let mut app = two_player_app();
        app.score_p1 = 4;
        app.score_p2 = 4;
        assert_eq!(app.winner(), None);
    }

    // ── Player ─────────────────────────────────────────────────────

    #[test]
    fn test_player_other() {
        assert_eq!(Player::One.other(), Player::Two);
        assert_eq!(Player::Two.other(), Player::One);
    }

    #[test]
    fn test_player_labels() {
        assert_eq!(Player::One.label(), "Player 1");
        assert_eq!(Player::Two.label(), "Player 2");
    }

    #[test]
    fn test_player_colors_different() {
        assert_ne!(Player::One.color(), Player::Two.color());
    }

    #[test]
    fn test_player_box_colors_different() {
        assert_ne!(Player::One.box_color(), Player::Two.box_color());
    }

    // ── GameMode ───────────────────────────────────────────────────

    #[test]
    fn test_game_mode_labels() {
        assert_eq!(GameMode::VsAi.label(), "vs AI");
        assert_eq!(GameMode::TwoPlayer.label(), "2-Player");
    }

    // ── Cursor navigation ──────────────────────────────────────────

    #[test]
    fn test_cursor_initial_position() {
        let app = test_app();
        assert_eq!(app.cursor.orientation, Orientation::Horizontal);
        assert_eq!(app.cursor.row, 0);
        assert_eq!(app.cursor.col, 0);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut app = test_app();
        app.move_cursor(Key::Right);
        assert_eq!(app.cursor.col, 1);
    }

    #[test]
    fn test_cursor_move_left_wrap() {
        let mut app = test_app();
        app.move_cursor(Key::Left);
        assert_eq!(app.cursor.col, app.boxes_per_side() - 1);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut app = test_app();
        app.move_cursor(Key::Down);
        assert_eq!(app.cursor.row, 1);
    }

    #[test]
    fn test_cursor_move_up_switches_to_vertical() {
        let mut app = test_app();
        // At row 0 horizontal, moving up switches to vertical.
        app.move_cursor(Key::Up);
        assert_eq!(app.cursor.orientation, Orientation::Vertical);
    }

    #[test]
    fn test_cursor_toggle_orientation() {
        let mut app = test_app();
        assert_eq!(app.cursor.orientation, Orientation::Horizontal);
        app.toggle_cursor_orientation();
        assert_eq!(app.cursor.orientation, Orientation::Vertical);
        app.toggle_cursor_orientation();
        assert_eq!(app.cursor.orientation, Orientation::Horizontal);
    }

    #[test]
    fn test_cursor_to_line_id() {
        let app = test_app();
        let line = app.cursor.to_line_id();
        assert_eq!(line.orientation, Orientation::Horizontal);
        assert_eq!(line.row, 0);
        assert_eq!(line.col, 0);
    }

    #[test]
    fn test_cursor_right_wraps() {
        let mut app = test_app();
        let bps = app.boxes_per_side();
        for _ in 0..bps {
            app.move_cursor(Key::Right);
        }
        assert_eq!(app.cursor.col, 0);
    }

    #[test]
    fn test_cursor_down_past_grid_switches_to_vertical() {
        let mut app = test_app();
        let gs = app.grid_size();
        for _ in 0..gs {
            app.move_cursor(Key::Down);
        }
        assert_eq!(app.cursor.orientation, Orientation::Vertical);
    }

    // ── Keyboard event handling ────────────────────────────────────

    #[test]
    fn test_key_n_new_game() {
        let mut app = two_player_app();
        app.try_place_line(LineId::horizontal(0, 0));
        app.handle_event(&make_key_event(Key::N));
        assert_eq!(app.total_moves, 0);
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_key_m_toggles_mode() {
        let mut app = test_app();
        assert_eq!(app.mode, GameMode::VsAi);
        app.handle_event(&make_key_event(Key::M));
        assert_eq!(app.mode, GameMode::TwoPlayer);
        app.handle_event(&make_key_event(Key::M));
        assert_eq!(app.mode, GameMode::VsAi);
    }

    #[test]
    fn test_key_3_sets_grid_3() {
        let mut app = test_app();
        app.handle_event(&make_key_event(Key::Num3));
        assert_eq!(app.grid_size(), 3);
    }

    #[test]
    fn test_key_4_sets_grid_4() {
        let mut app = test_app();
        app.handle_event(&make_key_event(Key::Num4));
        assert_eq!(app.grid_size(), 4);
    }

    #[test]
    fn test_key_5_sets_grid_5() {
        let mut app = test_app();
        app.handle_event(&make_key_event(Key::Num5));
        assert_eq!(app.grid_size(), 5);
    }

    #[test]
    fn test_arrow_keys_move_cursor() {
        let mut app = two_player_app();
        app.handle_event(&make_key_event(Key::Right));
        assert_eq!(app.cursor.col, 1);
    }

    #[test]
    fn test_tab_toggles_orientation() {
        let mut app = two_player_app();
        app.handle_event(&make_key_event(Key::Tab));
        assert_eq!(app.cursor.orientation, Orientation::Vertical);
    }

    #[test]
    fn test_enter_places_line() {
        let mut app = two_player_app();
        app.handle_event(&make_key_event(Key::Enter));
        assert!(app.board.is_line_drawn(LineId::horizontal(0, 0)));
    }

    #[test]
    fn test_space_places_line() {
        let mut app = two_player_app();
        app.handle_event(&make_key_event(Key::Space));
        assert!(app.board.is_line_drawn(LineId::horizontal(0, 0)));
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = two_player_app();
        app.handle_event(&make_key_release(Key::Enter));
        assert!(!app.board.is_line_drawn(LineId::horizontal(0, 0)));
    }

    // ── Mouse click ────────────────────────────────────────────────

    #[test]
    fn test_mouse_click_on_line() {
        let mut app = two_player_app();
        // Click near the midpoint of the top-left horizontal line.
        let (x1, y1) = app.dot_pos(0, 0);
        let (x2, _y2) = app.dot_pos(0, 1);
        let mid_x = f32::midpoint(x1, x2);
        app.handle_event(&make_click(mid_x, y1));
        assert!(app.board.is_line_drawn(LineId::horizontal(0, 0)));
    }

    #[test]
    fn test_mouse_click_on_vertical_line() {
        let mut app = two_player_app();
        let (x1, y1) = app.dot_pos(0, 0);
        let (_x2, y2) = app.dot_pos(1, 0);
        let mid_y = f32::midpoint(y1, y2);
        app.handle_event(&make_click(x1, mid_y));
        assert!(app.board.is_line_drawn(LineId::vertical(0, 0)));
    }

    #[test]
    fn test_mouse_click_far_away_no_effect() {
        let mut app = two_player_app();
        app.handle_event(&make_click(9999.0, 9999.0));
        assert_eq!(app.board.drawn_line_count(), 0);
    }

    #[test]
    fn test_mouse_click_on_drawn_line_no_effect() {
        let mut app = two_player_app();
        let (x1, y1) = app.dot_pos(0, 0);
        let (x2, _) = app.dot_pos(0, 1);
        let mid_x = f32::midpoint(x1, x2);
        app.handle_event(&make_click(mid_x, y1));
        assert_eq!(app.board.drawn_line_count(), 1);
        // Click same line again.
        app.handle_event(&make_click(mid_x, y1));
        assert_eq!(app.board.drawn_line_count(), 1);
    }

    // ── AI ─────────────────────────────────────────────────────────

    #[test]
    fn test_ai_triggers_after_player_turn() {
        let mut app = test_app();
        // Place a line as P1. Since it's VsAi, AI should be pending.
        app.try_place_line(LineId::horizontal(0, 0));
        assert!(app.ai_pending);
        assert_eq!(app.current_player, Player::Two);
    }

    #[test]
    fn test_ai_executes_after_delay() {
        let mut app = test_app();
        app.try_place_line(LineId::horizontal(0, 0));
        assert!(app.ai_pending);
        // Simulate enough time for AI to act.
        app.handle_event(&make_tick(AI_DELAY + 50));
        // AI should have made a move.
        assert!(app.board.drawn_line_count() >= 2);
    }

    #[test]
    fn test_ai_no_move_during_delay() {
        let mut app = test_app();
        app.try_place_line(LineId::horizontal(0, 0));
        assert!(app.ai_pending);
        // Not enough time.
        app.handle_event(&make_tick(100));
        assert!(app.ai_pending);
        assert_eq!(app.board.drawn_line_count(), 1);
    }

    #[test]
    fn test_ai_choose_completing_move() {
        let mut board = Board::new(4);
        // Set up box (0,0) with 3 sides drawn.
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        let mut rng = SeededRng::new(42);
        let line = ai_choose_line(&board, &mut rng);
        // AI should pick the completing line.
        assert_eq!(line, Some(LineId::vertical(0, 1)));
    }

    #[test]
    fn test_ai_avoids_giving_away_boxes() {
        let mut board = Board::new(4);
        // Set up box (0,0) with 2 sides. AI should avoid the third side.
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        let mut rng = SeededRng::new(42);
        let line = ai_choose_line(&board, &mut rng);
        // AI should not pick v(0,0) or v(0,1) because both would leave box (0,0) with 3 sides.
        if let Some(chosen) = line {
            let adj = board.adjacent_boxes(chosen);
            for (br, bc) in &adj {
                // After drawing the chosen line, no adjacent box should have 3 sides
                // (unless AI is forced to).
                let current_sides = board.box_side_count(*br, *bc);
                assert_ne!(
                    current_sides, 2,
                    "AI should avoid lines that give box 3 sides when possible"
                );
            }
        }
    }

    #[test]
    fn test_ai_returns_none_empty_board_is_some() {
        let board = Board::new(4);
        let mut rng = SeededRng::new(42);
        assert!(ai_choose_line(&board, &mut rng).is_some());
    }

    #[test]
    fn test_ai_returns_none_full_board() {
        let mut board = Board::new(3);
        // Draw all lines.
        for row in 0..3 {
            for col in 0..2 {
                board.draw_line(LineId::horizontal(row, col), Player::One);
            }
        }
        for row in 0..2 {
            for col in 0..3 {
                board.draw_line(LineId::vertical(row, col), Player::One);
            }
        }
        let mut rng = SeededRng::new(42);
        assert!(ai_choose_line(&board, &mut rng).is_none());
    }

    #[test]
    fn test_ai_prefers_double_completion() {
        let mut board = Board::new(4);
        // Set up two adjacent boxes both with 3 sides, sharing the missing vertical.
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::horizontal(0, 1), Player::One);
        board.draw_line(LineId::horizontal(1, 1), Player::One);
        board.draw_line(LineId::vertical(0, 2), Player::One);
        // Both boxes (0,0) and (0,1) need v(0,1).
        let mut rng = SeededRng::new(42);
        let line = ai_choose_line(&board, &mut rng);
        assert_eq!(line, Some(LineId::vertical(0, 1)));
    }

    // ── New game ───────────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_scores() {
        let mut app = two_player_app();
        app.score_p1 = 5;
        app.score_p2 = 3;
        app.new_game();
        assert_eq!(app.score_p1, 0);
        assert_eq!(app.score_p2, 0);
    }

    #[test]
    fn test_new_game_resets_phase() {
        let mut app = two_player_app();
        app.phase = GamePhase::GameOver;
        app.new_game();
        assert_eq!(app.phase, GamePhase::Playing);
    }

    #[test]
    fn test_new_game_resets_board() {
        let mut app = two_player_app();
        app.try_place_line(LineId::horizontal(0, 0));
        app.new_game();
        assert_eq!(app.board.drawn_line_count(), 0);
    }

    #[test]
    fn test_new_game_preserves_grid_size() {
        let mut app = DotsAndBoxes::with_config(5, GameMode::TwoPlayer, 42);
        app.new_game();
        assert_eq!(app.grid_size(), 5);
    }

    #[test]
    fn test_new_game_preserves_mode() {
        let mut app = two_player_app();
        app.new_game();
        assert_eq!(app.mode, GameMode::TwoPlayer);
    }

    #[test]
    fn test_new_game_with_size() {
        let mut app = test_app();
        app.new_game_with_size(5);
        assert_eq!(app.grid_size(), 5);
    }

    // ── Rendering ──────────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = test_app();
        let cmds = app.render(400.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_game_over_produces_commands() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        let cmds = app.render(400.0, 400.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_completed_box() {
        let mut app = two_player_app();
        app.board.draw_line(LineId::horizontal(0, 0), Player::One);
        app.board.draw_line(LineId::horizontal(1, 0), Player::One);
        app.board.draw_line(LineId::vertical(0, 0), Player::One);
        app.board.draw_line(LineId::vertical(0, 1), Player::One);
        let cmds = app.render(400.0, 400.0);
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_different_grid_sizes() {
        for size in [3, 4, 5] {
            let app = DotsAndBoxes::with_config(size, GameMode::TwoPlayer, 42);
            let cmds = app.render(600.0, 600.0);
            assert!(!cmds.is_empty(), "render failed for grid size {size}");
        }
    }

    // ── Window dimensions ──────────────────────────────────────────

    #[test]
    fn test_window_width_positive() {
        let app = test_app();
        assert!(app.window_width() > 0.0);
    }

    #[test]
    fn test_window_height_positive() {
        let app = test_app();
        assert!(app.window_height() > 0.0);
    }

    #[test]
    fn test_window_larger_for_bigger_grid() {
        let app3 = DotsAndBoxes::with_config(3, GameMode::VsAi, 42);
        let app5 = DotsAndBoxes::with_config(5, GameMode::VsAi, 42);
        assert!(app5.window_width() > app3.window_width());
        assert!(app5.window_height() > app3.window_height());
    }

    // ── Dot positions ──────────────────────────────────────────────

    #[test]
    fn test_dot_pos_origin() {
        // Stated as a property of the picture rather than as a copy of the
        // arithmetic: the first dot's *disc* -- not its centre -- begins one
        // PADDING in from the window's left edge and from under the header.
        // Restating `PADDING + HEADER_HEIGHT` here would have passed happily
        // while the board hung 12px off-centre, which is what it did.
        let app = test_app();
        let (x, y) = app.dot_pos(0, 0);
        assert_eq!(x - DOT_RADIUS, PADDING);
        assert_eq!(y - DOT_RADIUS, PADDING + HEADER_HEIGHT);
    }

    #[test]
    fn test_dot_pos_spacing() {
        let app = test_app();
        let (x0, y0) = app.dot_pos(0, 0);
        let (x1, y1) = app.dot_pos(0, 1);
        assert!((x1 - x0 - DOT_SPACING).abs() < 0.01);
        assert!((y1 - y0).abs() < 0.01);
    }

    #[test]
    fn test_dot_pos_row_spacing() {
        let app = test_app();
        let (_x0, y0) = app.dot_pos(0, 0);
        let (_x1, y1) = app.dot_pos(1, 0);
        assert!((y1 - y0 - DOT_SPACING).abs() < 0.01);
    }

    // ── Hit testing ────────────────────────────────────────────────

    #[test]
    fn test_hit_test_on_horizontal_line() {
        let app = test_app();
        let (x1, y1) = app.dot_pos(0, 0);
        let (x2, _) = app.dot_pos(0, 1);
        let mid_x = f32::midpoint(x1, x2);
        let result = app.hit_test_line(mid_x, y1);
        assert_eq!(result, Some(LineId::horizontal(0, 0)));
    }

    #[test]
    fn test_hit_test_on_vertical_line() {
        let app = test_app();
        let (x1, y1) = app.dot_pos(0, 0);
        let (_, y2) = app.dot_pos(1, 0);
        let mid_y = f32::midpoint(y1, y2);
        let result = app.hit_test_line(x1, mid_y);
        assert_eq!(result, Some(LineId::vertical(0, 0)));
    }

    #[test]
    fn test_hit_test_far_away_returns_none() {
        let app = test_app();
        let result = app.hit_test_line(9999.0, 9999.0);
        assert!(result.is_none());
    }

    // The three tests that used to follow here were `_precise` copies of the
    // three above, aimed at the second hit test. With one hit test left they
    // were the same three tests written twice, so they are gone; what they
    // were really covering -- that a click anywhere along a line, not just at
    // its middle, finds it -- is now covered properly by
    // `every_point_along_a_line_is_clickable`.

    // ── Board geometry (design-decisions.md §485 checklist) ────────

    /// Distance between two points, for comparing painted geometry.
    fn apart(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    fn same_point(a: (f32, f32), b: (f32, f32)) -> bool {
        apart(a, b) < 0.01
    }

    /// The centre of every dot the renderer paints.
    ///
    /// Dots are the only `FillRect` drawn `DOT_RADIUS * 2` square in
    /// `TEXT_COLOR`; the completed-box fills are far larger and a different
    /// colour.
    fn painted_dots(app: &DotsAndBoxes) -> Vec<(f32, f32)> {
        app.render(app.window_width(), app.window_height())
            .iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if (*width - DOT_RADIUS * 2.0).abs() < 0.01
                    && (*height - DOT_RADIUS * 2.0).abs() < 0.01
                    && *color == TEXT_COLOR =>
                {
                    Some((x + DOT_RADIUS, y + DOT_RADIUS))
                }
                _ => None,
            })
            .collect()
    }

    /// Every line segment the renderer paints for the board itself.
    ///
    /// The cursor is drawn as a line too, at `LINE_HOVER_THICKNESS`, and it
    /// sits on top of a board line rather than being one; excluding it by
    /// width keeps the two apart.
    fn painted_board_lines(app: &DotsAndBoxes) -> Vec<((f32, f32), (f32, f32))> {
        app.render(app.window_width(), app.window_height())
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    ..
                } if (*width - LINE_HOVER_THICKNESS).abs() >= 0.01 => {
                    Some(((*x1, *y1), (*x2, *y2)))
                }
                _ => None,
            })
            .collect()
    }

    /// The cursor's hover line, if one is painted.
    fn painted_cursor_line(app: &DotsAndBoxes) -> Option<((f32, f32), (f32, f32))> {
        app.render(app.window_width(), app.window_height())
            .iter()
            .find_map(|c| match c {
                RenderCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    ..
                } if (*width - LINE_HOVER_THICKNESS).abs() < 0.01 => Some(((*x1, *y1), (*x2, *y2))),
                _ => None,
            })
    }

    /// §485 item 1 and 6: the lattice is square and evenly spaced, and the
    /// frame around it is positive and even on all four sides.
    ///
    /// "Inside the window" on its own would pass with any amount of slack on
    /// one side, which is how a board measured with a spacing it does not
    /// have stays invisible.
    #[test]
    fn the_painted_dots_are_a_square_even_lattice_centred_in_the_window() {
        for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let app = DotsAndBoxes::with_config(gs, GameMode::TwoPlayer, 7);
            let dots = painted_dots(&app);
            assert_eq!(
                dots.len(),
                gs * gs,
                "a {gs}x{gs} board should paint {} dots, painted {}",
                gs * gs,
                dots.len()
            );

            let mut xs: Vec<f32> = dots.iter().map(|d| d.0).collect();
            let mut ys: Vec<f32> = dots.iter().map(|d| d.1).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
            xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            assert_eq!(xs.len(), gs, "expected {gs} distinct dot columns");
            assert_eq!(ys.len(), gs, "expected {gs} distinct dot rows");

            for i in 1..gs {
                assert!(
                    (xs[i] - xs[i - 1] - DOT_SPACING).abs() < 0.01,
                    "columns {} and {i} are {} apart, not DOT_SPACING",
                    i - 1,
                    xs[i] - xs[i - 1]
                );
                assert!(
                    (ys[i] - ys[i - 1] - DOT_SPACING).abs() < 0.01,
                    "rows {} and {i} are {} apart, not DOT_SPACING",
                    i - 1,
                    ys[i] - ys[i - 1]
                );
            }
            assert!(
                ((xs[gs - 1] - xs[0]) - (ys[gs - 1] - ys[0])).abs() < 0.01,
                "the lattice is {} wide and {} tall -- not square",
                xs[gs - 1] - xs[0],
                ys[gs - 1] - ys[0]
            );

            let (win_w, win_h) = (app.window_width(), app.window_height());
            let left = xs[0] - DOT_RADIUS;
            let right = win_w - (xs[gs - 1] + DOT_RADIUS);
            assert!(left > 0.0, "the leftmost dot is off the left edge");
            assert!(right > 0.0, "the rightmost dot is off the right edge");
            assert!(
                (left - right).abs() < 0.01,
                "the board sits {left} from the left and {right} from the right"
            );

            let top = ys[0] - DOT_RADIUS - HEADER_HEIGHT;
            let bottom = win_h - FOOTER_HEIGHT - (ys[gs - 1] + DOT_RADIUS);
            assert!(top > 0.0, "the top row of dots is under the header");
            assert!(bottom > 0.0, "the bottom row of dots is under the footer");
            assert!(
                (top - bottom).abs() < 0.01,
                "the board sits {top} below the header and {bottom} above the footer"
            );
        }
    }

    /// §485 item 5: a line is painted between exactly the two dots it joins.
    #[test]
    fn every_line_is_painted_between_the_two_dots_it_joins() {
        for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let app = DotsAndBoxes::with_config(gs, GameMode::TwoPlayer, 7);
            let dots = painted_dots(&app);
            for line in app.all_lines() {
                let (a, b) = app.line_endpoints(line);
                assert!(
                    dots.iter().any(|d| same_point(*d, a)),
                    "{line:?} starts at {a:?}, where no dot is painted"
                );
                assert!(
                    dots.iter().any(|d| same_point(*d, b)),
                    "{line:?} ends at {b:?}, where no dot is painted"
                );
                assert!(
                    (apart(a, b) - DOT_SPACING).abs() < 0.01,
                    "{line:?} spans {}, not one lattice step",
                    apart(a, b)
                );
            }
        }
    }

    /// §485 item 2: the whole length of a line is clickable, not just a disc
    /// around its middle.
    ///
    /// This is the test the old midpoint hit test would have failed: it
    /// answered only within 12px of a line's centre, leaving roughly the
    /// outer two-thirds of every 70px line dead. The dots themselves are
    /// excluded here because up to four lines meet at one, so a click exactly
    /// on a dot has not said which line was meant -- see
    /// `a_click_on_a_dot_still_lands_on_a_line_meeting_there`.
    #[test]
    fn every_point_along_a_line_is_clickable() {
        for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let app = DotsAndBoxes::with_config(gs, GameMode::TwoPlayer, 7);
            for line in app.all_lines() {
                let ((x1, y1), (x2, y2)) = app.line_endpoints(line);
                for step in 1..40 {
                    let t = step as f32 / 40.0;
                    let x = x1 + (x2 - x1) * t;
                    let y = y1 + (y2 - y1) * t;
                    assert_eq!(
                        app.hit_test_line(x, y),
                        Some(line),
                        "a click at ({x}, {y}), {}% of the way along {line:?}, missed it",
                        t * 100.0
                    );
                }
            }
        }
    }

    /// A dot is a 12px target where up to four lines meet. Whichever line the
    /// tie is broken toward, it must be one of them -- a dot must not be a
    /// dead spot in the middle of the board.
    #[test]
    fn a_click_on_a_dot_still_lands_on_a_line_meeting_there() {
        let app = test_app();
        for (row, col) in
            (0..app.grid_size()).flat_map(|r| (0..app.grid_size()).map(move |c| (r, c)))
        {
            let dot = app.dot_pos(row, col);
            let hit = app
                .hit_test_line(dot.0, dot.1)
                .unwrap_or_else(|| panic!("the dot at ({row}, {col}) is a dead spot"));
            let (a, b) = app.line_endpoints(hit);
            assert!(
                same_point(a, dot) || same_point(b, dot),
                "clicking the dot at ({row}, {col}) selected {hit:?}, which does not touch it"
            );
        }
    }

    /// The clickable band around a line is wide enough to cover the line the
    /// player can see, and narrow enough not to reach the next line over.
    ///
    /// Deliberately stated in `LINE_HOVER_THICKNESS` and `DOT_SPACING` rather
    /// than in `CLICK_THRESHOLD`: every other slop test here measures against
    /// the threshold constant, so all of them move when it does and none of
    /// them can object to a bad value for it. A mutation that cut the
    /// threshold to a tenth survived the whole sweep for exactly that reason
    /// -- the tests restated the number instead of the requirement, which is
    /// design-decisions.md §487 in miniature. These two bounds are the
    /// requirement: below the first, clicks that visibly land on a line miss
    /// it; above the second, the bands of two parallel lines overlap and
    /// "nearest line" becomes a coin flip in the empty space between them.
    #[test]
    fn the_clickable_band_covers_the_painted_line_but_not_its_neighbour() {
        let app = test_app();
        let h = LineId::horizontal(1, 1);
        let ((x1, y), (x2, _)) = app.line_endpoints(h);
        let mid = f32::midpoint(x1, x2);

        let visible = LINE_HOVER_THICKNESS / 2.0;
        assert_eq!(
            app.hit_test_line(mid, y - visible),
            Some(h),
            "a click on the top edge of the painted line misses it"
        );
        assert_eq!(
            app.hit_test_line(mid, y + visible),
            Some(h),
            "a click on the bottom edge of the painted line misses it"
        );

        let halfway = DOT_SPACING / 2.0;
        assert!(
            app.hit_test_line(mid, y + halfway - 0.05).is_none(),
            "a click in the dead centre between two parallel lines picked one of them"
        );
    }

    /// §485 item 3: the slop is the same on both sides of a line.
    ///
    /// Measured on an interior line, where there is nothing else within
    /// reach, so what is being measured is the threshold and not a
    /// neighbour winning the tie.
    #[test]
    fn a_line_takes_the_same_slop_on_both_sides_of_it() {
        let app = test_app();
        let eps = 0.05;
        let t = DotsAndBoxes::CLICK_THRESHOLD;

        let h = LineId::horizontal(1, 1);
        let ((hx1, hy), (hx2, _)) = app.line_endpoints(h);
        let hmid = f32::midpoint(hx1, hx2);
        assert_eq!(app.hit_test_line(hmid, hy - t + eps), Some(h), "above");
        assert_eq!(app.hit_test_line(hmid, hy + t - eps), Some(h), "below");
        assert_ne!(app.hit_test_line(hmid, hy - t - eps), Some(h), "too high");
        assert_ne!(app.hit_test_line(hmid, hy + t + eps), Some(h), "too low");

        let v = LineId::vertical(1, 1);
        let ((vx, vy1), (_, vy2)) = app.line_endpoints(v);
        let vmid = f32::midpoint(vy1, vy2);
        assert_eq!(app.hit_test_line(vx - t + eps, vmid), Some(v), "left");
        assert_eq!(app.hit_test_line(vx + t - eps, vmid), Some(v), "right");
        assert_ne!(
            app.hit_test_line(vx - t - eps, vmid),
            Some(v),
            "too far left"
        );
        assert_ne!(
            app.hit_test_line(vx + t + eps, vmid),
            Some(v),
            "too far right"
        );
    }

    /// §485 item 3 again, at the four outer edges: the outside of the board
    /// is as forgiving as the inside, and equally so on all four sides.
    #[test]
    fn the_board_edges_take_the_same_slop_on_all_four_sides() {
        let app = test_app();
        let gs = app.grid_size();
        let eps = 0.05;
        let t = DotsAndBoxes::CLICK_THRESHOLD;
        let (min_x, min_y) = app.dot_pos(0, 0);
        let (max_x, max_y) = app.dot_pos(gs - 1, gs - 1);
        let mid_x = f32::midpoint(min_x, max_x);
        let mid_y = f32::midpoint(min_y, max_y);

        // Just outside each edge, still on the outermost line.
        assert!(
            app.hit_test_line(mid_x, min_y - t + eps).is_some(),
            "just above the top edge is dead"
        );
        assert!(
            app.hit_test_line(mid_x, max_y + t - eps).is_some(),
            "just below the bottom edge is dead"
        );
        assert!(
            app.hit_test_line(min_x - t + eps, mid_y).is_some(),
            "just left of the left edge is dead"
        );
        assert!(
            app.hit_test_line(max_x + t - eps, mid_y).is_some(),
            "just right of the right edge is dead"
        );

        // A hair further out, on none of them -- the same distance on each
        // side, which is what makes the four edges reflections of each other.
        assert!(
            app.hit_test_line(mid_x, min_y - t - eps).is_none(),
            "the top edge reaches further out than the others"
        );
        assert!(
            app.hit_test_line(mid_x, max_y + t + eps).is_none(),
            "the bottom edge reaches further out than the others"
        );
        assert!(
            app.hit_test_line(min_x - t - eps, mid_y).is_none(),
            "the left edge reaches further out than the others"
        );
        assert!(
            app.hit_test_line(max_x + t + eps, mid_y).is_none(),
            "the right edge reaches further out than the others"
        );
    }

    /// §485 item 4: sweep well past the board on all four sides and find
    /// nothing, so an off-board click can never place a line.
    #[test]
    fn nothing_far_outside_the_painted_board_lands_on_a_line() {
        let app = test_app();
        let gs = app.grid_size();
        let far = DotsAndBoxes::CLICK_THRESHOLD + 1.0;
        let (min_x, min_y) = app.dot_pos(0, 0);
        let (max_x, max_y) = app.dot_pos(gs - 1, gs - 1);

        for step in 0..120 {
            // Sweep the full height of the window and past it, on both sides.
            let y = step as f32 * 10.0 - 50.0;
            assert!(
                app.hit_test_line(min_x - far, y).is_none(),
                "a click at ({}, {y}) -- left of the whole board -- hit a line",
                min_x - far
            );
            assert!(
                app.hit_test_line(max_x + far, y).is_none(),
                "a click at ({}, {y}) -- right of the whole board -- hit a line",
                max_x + far
            );

            let x = step as f32 * 10.0 - 50.0;
            assert!(
                app.hit_test_line(x, min_y - far).is_none(),
                "a click at ({x}, {}) -- above the whole board -- hit a line",
                min_y - far
            );
            assert!(
                app.hit_test_line(x, max_y + far).is_none(),
                "a click at ({x}, {}) -- below the whole board -- hit a line",
                max_y + far
            );
        }
    }

    /// The picture and the hit test are the same geometry: every line the
    /// renderer paints answers to a click at its own midpoint, with the same
    /// endpoints it was painted with.
    #[test]
    fn every_painted_line_is_clickable_where_it_is_painted() {
        for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let app = DotsAndBoxes::with_config(gs, GameMode::TwoPlayer, 7);
            for (a, b) in painted_board_lines(&app) {
                let mid = (f32::midpoint(a.0, b.0), f32::midpoint(a.1, b.1));
                let hit = app
                    .hit_test_line(mid.0, mid.1)
                    .unwrap_or_else(|| panic!("the line painted {a:?}..{b:?} is not clickable"));
                let (ha, hb) = app.line_endpoints(hit);
                assert!(
                    same_point(ha, a) && same_point(hb, b),
                    "the line painted {a:?}..{b:?} answers as {hit:?}, painted {ha:?}..{hb:?}"
                );
            }
        }
    }

    /// §485 item 7: the board carries exactly the lines the rules say it
    /// does -- `gs * (gs - 1)` in each direction -- each painted once.
    #[test]
    fn the_renderer_paints_every_line_on_the_board_exactly_once() {
        for gs in MIN_GRID_SIZE..=MAX_GRID_SIZE {
            let app = DotsAndBoxes::with_config(gs, GameMode::TwoPlayer, 7);
            let expected = 2 * gs * (gs - 1);
            assert_eq!(
                app.all_lines().count(),
                expected,
                "a {gs}x{gs} board has {expected} lines"
            );

            let painted = painted_board_lines(&app);
            assert_eq!(
                painted.len(),
                expected,
                "a {gs}x{gs} board painted {} lines, not {expected}",
                painted.len()
            );
            for line in app.all_lines() {
                let (a, b) = app.line_endpoints(line);
                let hits = painted
                    .iter()
                    .filter(|(pa, pb)| same_point(*pa, a) && same_point(*pb, b))
                    .count();
                assert_eq!(hits, 1, "{line:?} was painted {hits} times, not once");
            }
        }
    }

    /// The cursor is drawn on the line it points at, not the one next door.
    #[test]
    fn the_cursor_is_drawn_on_the_line_it_points_at() {
        let mut app = two_player_app();
        for line in app.all_lines() {
            app.cursor = Cursor {
                orientation: line.orientation,
                row: line.row,
                col: line.col,
            };
            let (a, b) = app.line_endpoints(line);
            let drawn = painted_cursor_line(&app)
                .unwrap_or_else(|| panic!("no cursor is drawn while it points at {line:?}"));
            assert!(
                same_point(drawn.0, a) && same_point(drawn.1, b),
                "the cursor on {line:?} ({a:?}..{b:?}) is drawn at {drawn:?}"
            );
        }
    }

    // ── Geometry helper ────────────────────────────────────────────

    #[test]
    fn test_point_on_segment() {
        let dist = point_to_segment_distance(5.0, 0.0, 0.0, 0.0, 10.0, 0.0);
        assert!(dist < 0.01);
    }

    #[test]
    fn test_point_perpendicular_to_segment() {
        let dist = point_to_segment_distance(5.0, 3.0, 0.0, 0.0, 10.0, 0.0);
        assert!((dist - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_point_beyond_segment_end() {
        let dist = point_to_segment_distance(15.0, 0.0, 0.0, 0.0, 10.0, 0.0);
        assert!((dist - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_point_before_segment_start() {
        let dist = point_to_segment_distance(-3.0, 4.0, 0.0, 0.0, 10.0, 0.0);
        // Distance from (-3, 4) to (0, 0) = 5.
        assert!((dist - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_degenerate_segment() {
        let dist = point_to_segment_distance(3.0, 4.0, 0.0, 0.0, 0.0, 0.0);
        assert!((dist - 5.0).abs() < 0.01);
    }

    // ── Orientation ────────────────────────────────────────────────

    #[test]
    fn test_orientation_equality() {
        assert_eq!(Orientation::Horizontal, Orientation::Horizontal);
        assert_ne!(Orientation::Horizontal, Orientation::Vertical);
    }

    // ── LineId ─────────────────────────────────────────────────────

    #[test]
    fn test_line_id_equality() {
        let a = LineId::horizontal(0, 0);
        let b = LineId::horizontal(0, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_line_id_inequality() {
        let a = LineId::horizontal(0, 0);
        let b = LineId::vertical(0, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn test_line_id_constructors() {
        let h = LineId::horizontal(1, 2);
        assert_eq!(h.orientation, Orientation::Horizontal);
        assert_eq!(h.row, 1);
        assert_eq!(h.col, 2);

        let v = LineId::vertical(3, 4);
        assert_eq!(v.orientation, Orientation::Vertical);
        assert_eq!(v.row, 3);
        assert_eq!(v.col, 4);
    }

    // ── Randomness, as the game uses it ────────────────────────────
    //
    // The three tests that were here checked that the private generator was
    // deterministic, differed by seed, and stayed inside its bound. That is
    // `randrange`'s contract now, tested there against the real hazards. These
    // test what *this game* needs.

    /// `new()` must take its seed from the system rather than a literal.
    ///
    /// Checked by *which* seed, not by variety: the host test toolchain has no
    /// SlateOS kernel, so `seed_from_system` correctly returns the fallback and
    /// two fresh games agree there -- exactly as they did under the old
    /// hardcoded `42`. On real hardware the same line reaches the kernel.
    #[test]
    #[cfg(not(unix))]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let mut fresh = DotsAndBoxes::new();
        assert_eq!(
            fresh.rng.next_u64(),
            SeededRng::new(FALLBACK_SEED).next_u64()
        );
        assert_ne!(
            DotsAndBoxes::new().rng.next_u64(),
            SeededRng::new(42).next_u64(),
            "back on a hardcoded seed"
        );
    }

    /// The AI's tie-breaking must actually vary with the generator, which is
    /// the property the fixed seed made unobservable.
    #[test]
    fn the_ai_breaks_ties_differently_under_different_seeds() {
        let board = Board::new(DEFAULT_GRID_SIZE);
        let mut seen: Vec<LineId> = Vec::new();
        for seed in 0..30 {
            let mut rng = SeededRng::new(seed);
            if let Some(line) = ai_choose_line(&board, &mut rng)
                && !seen.contains(&line)
            {
                seen.push(line);
            }
        }
        assert!(
            seen.len() > 1,
            "the AI opened with one line under all 30 seeds"
        );
    }

    /// A new game must not replay the one before it. `new_game` reseeds from
    /// the live generator, so consecutive games differ by construction.
    #[test]
    fn a_new_game_does_not_replay_the_last_one() {
        let mut game = DotsAndBoxes::with_config(DEFAULT_GRID_SIZE, GameMode::VsAi, 7);
        let first = game.rng.next_u64();
        game.new_game();
        assert_ne!(first, game.rng.next_u64());
    }

    // ── Full game simulation ───────────────────────────────────────

    #[test]
    fn test_full_game_two_player_3x3() {
        let mut app = DotsAndBoxes::with_config(3, GameMode::TwoPlayer, 7);
        let gs = app.grid_size();
        let bps = app.boxes_per_side();
        // Draw all horizontal then all vertical lines.
        for row in 0..gs {
            for col in 0..bps {
                app.try_place_line(LineId::horizontal(row, col));
            }
        }
        for row in 0..bps {
            for col in 0..gs {
                app.try_place_line(LineId::vertical(row, col));
            }
        }
        assert_eq!(app.phase, GamePhase::GameOver);
        assert_eq!(app.score_p1 + app.score_p2, app.board.total_boxes());
    }

    #[test]
    fn test_full_game_vs_ai() {
        let mut app = DotsAndBoxes::with_config(3, GameMode::VsAi, 42);
        // Simulate a game: player draws some lines, AI fills rest.
        let mut max_turns = 100;
        while app.phase != GamePhase::GameOver && max_turns > 0 {
            if app.ai_pending {
                app.handle_event(&make_tick(AI_DELAY + 50));
            } else {
                let available = app.board.available_lines();
                if let Some(&line) = available.first() {
                    app.try_place_line(line);
                } else {
                    break;
                }
            }
            max_turns -= 1;
        }
        assert_eq!(app.phase, GamePhase::GameOver);
        assert_eq!(app.score_p1 + app.score_p2, app.board.total_boxes());
    }

    // ── Edge cases ─────────────────────────────────────────────────

    #[test]
    fn test_board_3x3_structure() {
        let board = Board::new(3);
        assert_eq!(board.h_lines.len(), 3);
        assert_eq!(board.h_lines[0].len(), 2);
        assert_eq!(board.v_lines.len(), 2);
        assert_eq!(board.v_lines[0].len(), 3);
        assert_eq!(board.boxes.len(), 2);
        assert_eq!(board.boxes[0].len(), 2);
    }

    #[test]
    fn test_board_5x5_structure() {
        let board = Board::new(5);
        assert_eq!(board.h_lines.len(), 5);
        assert_eq!(board.h_lines[0].len(), 4);
        assert_eq!(board.v_lines.len(), 4);
        assert_eq!(board.v_lines[0].len(), 5);
        assert_eq!(board.boxes.len(), 4);
        assert_eq!(board.boxes[0].len(), 4);
    }

    #[test]
    fn test_out_of_bounds_box_side_count() {
        let board = Board::new(4);
        assert_eq!(board.box_side_count(99, 99), 0);
    }

    #[test]
    fn test_out_of_bounds_is_box_complete() {
        let board = Board::new(4);
        assert!(!board.is_box_complete(99, 99));
    }

    #[test]
    fn test_place_line_while_ai_pending() {
        let mut app = test_app();
        app.ai_pending = true;
        let result = app.try_place_line(LineId::horizontal(0, 0));
        assert!(!result);
    }

    #[test]
    fn test_cursor_not_movable_when_ai_pending() {
        let mut app = test_app();
        app.ai_pending = true;
        let old_col = app.cursor.col;
        app.handle_event(&make_key_event(Key::Right));
        assert_eq!(app.cursor.col, old_col);
    }

    #[test]
    fn test_cursor_not_movable_when_game_over() {
        let mut app = two_player_app();
        app.phase = GamePhase::GameOver;
        let old_col = app.cursor.col;
        app.handle_event(&make_key_event(Key::Right));
        assert_eq!(app.cursor.col, old_col);
    }

    #[test]
    fn test_tick_no_ai_no_effect() {
        let mut app = two_player_app();
        app.handle_event(&make_tick(1000));
        assert_eq!(app.board.drawn_line_count(), 0);
    }

    // ── Render cursor visibility ───────────────────────────────────

    #[test]
    fn test_render_no_cursor_when_ai_pending() {
        let mut app = test_app();
        app.ai_pending = true;
        let cmds = app.render(400.0, 400.0);
        // Should still render, just without cursor highlight.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_no_cursor_when_game_over() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        let cmds = app.render(400.0, 400.0);
        assert!(!cmds.is_empty());
    }

    // ── Do AI move directly ────────────────────────────────────────

    #[test]
    fn test_do_ai_move_draws_line() {
        let mut app = DotsAndBoxes::with_config(4, GameMode::VsAi, 42);
        app.current_player = Player::Two;
        app.do_ai_move();
        assert!(app.board.drawn_line_count() >= 1);
    }

    #[test]
    fn test_do_ai_move_on_game_over_noop() {
        let mut app = test_app();
        app.phase = GamePhase::GameOver;
        app.do_ai_move();
        assert_eq!(app.board.drawn_line_count(), 0);
    }

    #[test]
    fn test_ai_extra_turn_on_box_completion() {
        let mut app = DotsAndBoxes::with_config(4, GameMode::VsAi, 42);
        // Set up box (0,0) with 3 sides.
        app.board.draw_line(LineId::horizontal(0, 0), Player::One);
        app.board.draw_line(LineId::horizontal(1, 0), Player::One);
        app.board.draw_line(LineId::vertical(0, 0), Player::One);
        // AI should complete the box and get another turn.
        app.current_player = Player::Two;
        app.do_ai_move();
        assert_eq!(app.score_p2, 1);
        // AI completed a box, so ai_pending should be true for another move.
        assert!(app.ai_pending);
    }

    // ── Board score ────────────────────────────────────────────────

    #[test]
    fn test_board_score_empty() {
        let board = Board::new(4);
        assert_eq!(board.score(Player::One), 0);
        assert_eq!(board.score(Player::Two), 0);
    }

    #[test]
    fn test_board_score_mixed() {
        let mut board = Board::new(3);
        // Complete box (0,0) for P1.
        board.draw_line(LineId::horizontal(0, 0), Player::One);
        board.draw_line(LineId::horizontal(1, 0), Player::One);
        board.draw_line(LineId::vertical(0, 0), Player::One);
        board.draw_line(LineId::vertical(0, 1), Player::One);
        // Complete box (0,1) for P2.
        board.draw_line(LineId::horizontal(0, 1), Player::Two);
        board.draw_line(LineId::horizontal(1, 1), Player::Two);
        board.draw_line(LineId::vertical(0, 2), Player::Two);
        // v(0,1) already drawn by P1's box completion.
        assert_eq!(board.score(Player::One), 1);
        assert_eq!(board.score(Player::Two), 1);
    }

    // ── Vertical cursor navigation ─────────────────────────────────

    #[test]
    fn test_vertical_cursor_move_left() {
        let mut app = test_app();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.col = 1;
        app.move_cursor(Key::Left);
        assert_eq!(app.cursor.col, 0);
    }

    #[test]
    fn test_vertical_cursor_move_right() {
        let mut app = test_app();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.col = 0;
        app.move_cursor(Key::Right);
        assert_eq!(app.cursor.col, 1);
    }

    #[test]
    fn test_vertical_cursor_move_down() {
        let mut app = test_app();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.row = 0;
        app.move_cursor(Key::Down);
        assert_eq!(app.cursor.row, 1);
    }

    #[test]
    fn test_vertical_cursor_up_wrap() {
        let mut app = test_app();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.row = 0;
        app.move_cursor(Key::Up);
        // Should switch to horizontal.
        assert_eq!(app.cursor.orientation, Orientation::Horizontal);
    }

    #[test]
    fn test_vertical_cursor_down_wrap() {
        let mut app = test_app();
        let bps = app.boxes_per_side();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.row = bps - 1;
        app.move_cursor(Key::Down);
        assert_eq!(app.cursor.orientation, Orientation::Horizontal);
    }

    #[test]
    fn test_vertical_cursor_left_wrap() {
        let mut app = test_app();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.col = 0;
        app.move_cursor(Key::Left);
        assert_eq!(app.cursor.col, app.grid_size() - 1);
    }

    #[test]
    fn test_vertical_cursor_right_wrap() {
        let mut app = test_app();
        let gs = app.grid_size();
        app.cursor.orientation = Orientation::Vertical;
        app.cursor.col = gs - 1;
        app.move_cursor(Key::Right);
        assert_eq!(app.cursor.col, 0);
    }
}
