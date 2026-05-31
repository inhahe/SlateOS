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

//! Sliding Puzzle (15-puzzle) — a classic tile-sliding puzzle.
//!
//! Slide numbered tiles on a grid to arrange them in order.
//! Supports 3x3 (8-puzzle), 4x4 (15-puzzle), and 5x5 (24-puzzle) sizes.
//! Uses keyboard arrows or mouse clicks to move tiles.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ──
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
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

// ── Tile colors (cycle for visual appeal) ──
const TILE_COLORS: [Color; 8] = [BLUE, GREEN, PEACH, MAUVE, TEAL, YELLOW, RED, LAVENDER];

// ── Seeded LCG RNG ──
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
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
        (self.next() % max as u64) as usize
    }
}

// ── Direction ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

// ── Puzzle board ──
#[derive(Clone)]
struct Board {
    size: usize,
    tiles: Vec<u8>, // 0 = empty space
    empty_pos: usize,
}

impl Board {
    fn new(size: usize) -> Self {
        let total = size * size;
        let mut tiles = Vec::with_capacity(total);
        for i in 0..total {
            if i == total - 1 {
                tiles.push(0);
            } else {
                tiles.push((i + 1) as u8);
            }
        }
        Self {
            size,
            tiles,
            empty_pos: total - 1,
        }
    }

    fn is_solved(&self) -> bool {
        let total = self.size * self.size;
        for i in 0..total - 1 {
            if self.tiles[i] != (i + 1) as u8 {
                return false;
            }
        }
        self.tiles[total - 1] == 0
    }

    fn empty_row(&self) -> usize {
        self.empty_pos / self.size
    }

    fn empty_col(&self) -> usize {
        self.empty_pos % self.size
    }

    fn can_move(&self, dir: Direction) -> bool {
        match dir {
            Direction::Up => self.empty_row() < self.size - 1,
            Direction::Down => self.empty_row() > 0,
            Direction::Left => self.empty_col() < self.size - 1,
            Direction::Right => self.empty_col() > 0,
        }
    }

    /// Move a tile into the empty space from the given direction.
    /// Returns true if the move was made.
    fn slide(&mut self, dir: Direction) -> bool {
        if !self.can_move(dir) {
            return false;
        }

        let target = match dir {
            Direction::Up => self.empty_pos + self.size,
            Direction::Down => self.empty_pos - self.size,
            Direction::Left => self.empty_pos + 1,
            Direction::Right => self.empty_pos - 1,
        };

        self.tiles.swap(self.empty_pos, target);
        self.empty_pos = target;
        true
    }

    /// Try to slide a tile at the given position into the empty space.
    /// Returns true if the move was made.
    fn slide_pos(&mut self, row: usize, col: usize) -> bool {
        let pos = row * self.size + col;
        if pos >= self.tiles.len() {
            return false;
        }

        let er = self.empty_row();
        let ec = self.empty_col();

        // The clicked tile must be adjacent to the empty space
        if row == er && col == ec {
            return false;
        }

        // Same row, adjacent column
        if row == er && col.abs_diff(ec) == 1 {
            if col > ec {
                return self.slide(Direction::Left);
            }
            return self.slide(Direction::Right);
        }

        // Same column, adjacent row
        if col == ec && row.abs_diff(er) == 1 {
            if row > er {
                return self.slide(Direction::Up);
            }
            return self.slide(Direction::Down);
        }

        false
    }

    /// Shuffle by making random valid moves (guarantees solvability).
    fn shuffle(&mut self, rng: &mut Rng, move_count: usize) {
        let dirs = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];
        let mut last_dir: Option<Direction> = None;

        for _ in 0..move_count {
            // Pick a random direction, avoid immediately undoing
            loop {
                let idx = rng.next_range(4);
                let dir = dirs[idx];
                if let Some(last) = last_dir {
                    if dir == last.opposite() {
                        continue;
                    }
                }
                if self.slide(dir) {
                    last_dir = Some(dir);
                    break;
                }
            }
        }
    }
}

// ── Game state ──
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameState {
    Playing,
    Won,
}

// ── Main app ──
struct SlidingPuzzle {
    board: Board,
    state: GameState,
    moves: u32,
    size: usize,                  // 3, 4, or 5
    best_moves: [Option<u32>; 3], // best for 3x3, 4x4, 5x5
    rng: Rng,
    timer_ticks: u64,   // rough frame counter for animation
    show_numbers: bool, // show tile numbers
    show_help: bool,
}

impl SlidingPuzzle {
    fn new() -> Self {
        let size = 4;
        let mut rng = Rng::new(42);
        let mut board = Board::new(size);
        board.shuffle(&mut rng, 200);

        Self {
            board,
            state: GameState::Playing,
            moves: 0,
            size,
            best_moves: [None; 3],
            rng,
            timer_ticks: 0,
            show_numbers: true,
            show_help: false,
        }
    }

    fn new_game(&mut self) {
        self.board = Board::new(self.size);
        let shuffle_count = match self.size {
            3 => 100,
            4 => 200,
            5 => 400,
            _ => 200,
        };
        self.board.shuffle(&mut self.rng, shuffle_count);
        self.state = GameState::Playing;
        self.moves = 0;
    }

    fn set_size(&mut self, size: usize) {
        if size >= 3 && size <= 5 {
            self.size = size;
            self.new_game();
        }
    }

    fn size_index(&self) -> usize {
        self.size.saturating_sub(3)
    }

    fn handle_move(&mut self, dir: Direction) {
        if self.state != GameState::Playing {
            return;
        }
        if self.board.slide(dir) {
            self.moves = self.moves.saturating_add(1);
            self.check_win();
        }
    }

    fn handle_click(&mut self, row: usize, col: usize) {
        if self.state != GameState::Playing {
            return;
        }
        if self.board.slide_pos(row, col) {
            self.moves = self.moves.saturating_add(1);
            self.check_win();
        }
    }

    fn check_win(&mut self) {
        if self.board.is_solved() {
            self.state = GameState::Won;
            let idx = self.size_index();
            if idx < 3 {
                if let Some(best) = self.best_moves[idx] {
                    if self.moves < best {
                        self.best_moves[idx] = Some(self.moves);
                    }
                } else {
                    self.best_moves[idx] = Some(self.moves);
                }
            }
        }
    }

    fn tile_color(&self, value: u8) -> Color {
        if value == 0 {
            return BASE;
        }
        let idx = (value as usize).saturating_sub(1) % TILE_COLORS.len();
        TILE_COLORS[idx]
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if *modifiers == Modifiers::NONE {
                    match key {
                        Key::Up => self.handle_move(Direction::Up),
                        Key::Down => self.handle_move(Direction::Down),
                        Key::Left => self.handle_move(Direction::Left),
                        Key::Right => self.handle_move(Direction::Right),
                        Key::N => self.new_game(),
                        Key::H => self.show_help = !self.show_help,
                        Key::Num3 => self.set_size(3),
                        Key::Num4 => self.set_size(4),
                        Key::Num5 => self.set_size(5),
                        Key::T => self.show_numbers = !self.show_numbers,
                        _ => {}
                    }
                }
            }
            Event::Mouse(MouseEvent { x, y, kind }) => {
                if matches!(kind, MouseEventKind::Press(MouseButton::Left)) {
                    self.handle_mouse_click(*x, *y);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_click(&mut self, mx: f32, my: f32) {
        // Grid layout constants
        let grid_offset_x: f32 = 50.0;
        let grid_offset_y: f32 = 80.0;
        let cell_size: f32 = 80.0;
        let _padding: f32 = 4.0;

        let grid_total = cell_size * self.size as f32;

        // Check if click is within grid
        if mx < grid_offset_x || my < grid_offset_y {
            return;
        }
        if mx > grid_offset_x + grid_total || my > grid_offset_y + grid_total {
            return;
        }

        let col = ((mx - grid_offset_x) / cell_size) as usize;
        let row = ((my - grid_offset_y) / cell_size) as usize;

        if row < self.size && col < self.size {
            self.handle_click(row, col);
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
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        let title = match self.size {
            3 => "8-Puzzle",
            4 => "15-Puzzle",
            5 => "24-Puzzle",
            _ => "Sliding Puzzle",
        };
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 30.0,
            text: title.into(),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Move counter
        let move_text = format!("Moves: {}", self.moves);
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 58.0,
            text: move_text,
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Size selector
        let size_text = format!("Size: {}x{}", self.size, self.size);
        cmds.push(RenderCommand::Text {
            x: 250.0,
            y: 58.0,
            text: size_text,
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid
        let grid_x: f32 = 50.0;
        let grid_y: f32 = 80.0;
        let cell_size: f32 = 80.0;
        let padding: f32 = 4.0;
        let grid_total = cell_size * self.size as f32;

        // Grid background
        cmds.push(RenderCommand::FillRect {
            x: grid_x - 4.0,
            y: grid_y - 4.0,
            width: grid_total + 8.0,
            height: grid_total + 8.0,
            color: CRUST,
            corner_radii: CornerRadii::all(8.0),
        });

        // Tiles
        for row in 0..self.size {
            for col in 0..self.size {
                let idx = row * self.size + col;
                let value = self.board.tiles[idx];

                let tx = grid_x + col as f32 * cell_size + padding;
                let ty = grid_y + row as f32 * cell_size + padding;
                let tw = cell_size - padding * 2.0;
                let th = cell_size - padding * 2.0;

                if value == 0 {
                    // Empty space — dark background
                    cmds.push(RenderCommand::FillRect {
                        x: tx,
                        y: ty,
                        width: tw,
                        height: th,
                        color: MANTLE,
                        corner_radii: CornerRadii::all(6.0),
                    });
                } else {
                    // Tile background
                    let tile_color = self.tile_color(value);
                    cmds.push(RenderCommand::FillRect {
                        x: tx,
                        y: ty,
                        width: tw,
                        height: th,
                        color: tile_color,
                        corner_radii: CornerRadii::all(6.0),
                    });

                    // Tile number
                    if self.show_numbers {
                        let num_str = format!("{}", value);
                        let font_size = if self.size <= 3 {
                            28.0
                        } else if self.size == 4 {
                            24.0
                        } else {
                            20.0
                        };
                        let text_x = tx + tw / 2.0 - if value >= 10 { 12.0 } else { 7.0 };
                        let text_y = ty + th / 2.0 - font_size / 2.0 + 4.0;
                        cmds.push(RenderCommand::Text {
                            x: text_x,
                            y: text_y,
                            text: num_str,
                            color: CRUST,
                            font_size,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }
                }
            }
        }

        // Win message
        if self.state == GameState::Won {
            let win_y = grid_y + grid_total + 30.0;
            cmds.push(RenderCommand::Text {
                x: grid_x,
                y: win_y,
                text: format!("Solved in {} moves!", self.moves),
                color: GREEN,
                font_size: 20.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: grid_x,
                y: win_y + 28.0,
                text: "Press N for new game".into(),
                color: SUBTEXT0,
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Best scores panel
        let panel_x = grid_x + grid_total + 40.0;
        let panel_y = grid_y;

        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: 180.0,
            height: 140.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: panel_y + 16.0,
            text: "Best Scores".into(),
            color: YELLOW,
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let labels = ["3x3 (8)", "4x4 (15)", "5x5 (24)"];
        for (i, label) in labels.iter().enumerate() {
            let sy = panel_y + 44.0 + i as f32 * 28.0;
            let score_str = match self.best_moves[i] {
                Some(m) => format!("{}: {} moves", label, m),
                None => format!("{}: ---", label),
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: sy,
                text: score_str,
                color: TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help / controls
        if self.show_help {
            let help_y = panel_y + 160.0;
            cmds.push(RenderCommand::FillRect {
                x: panel_x,
                y: help_y,
                width: 180.0,
                height: 200.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            let help_lines = [
                ("Arrows", "Slide tiles"),
                ("Click", "Slide tile"),
                ("3/4/5", "Change size"),
                ("N", "New game"),
                ("T", "Toggle numbers"),
                ("H", "Toggle help"),
            ];
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: help_y + 16.0,
                text: "Controls".into(),
                color: YELLOW,
                font_size: 16.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            for (i, (key, desc)) in help_lines.iter().enumerate() {
                let ly = help_y + 44.0 + i as f32 * 24.0;
                cmds.push(RenderCommand::Text {
                    x: panel_x + 12.0,
                    y: ly,
                    text: (*key).into(),
                    color: BLUE,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: panel_x + 70.0,
                    y: ly,
                    text: (*desc).into(),
                    color: SUBTEXT0,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        } else {
            let hint_y = panel_y + 160.0;
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: hint_y,
                text: "Press H for help".into(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

fn main() {
    let _app = SlidingPuzzle::new();
}

// ── Tests ──
#[cfg(test)]
mod tests {
    use super::*;

    // ── Board basics ──

    #[test]
    fn test_new_board_3x3() {
        let b = Board::new(3);
        assert_eq!(b.size, 3);
        assert_eq!(b.tiles, vec![1, 2, 3, 4, 5, 6, 7, 8, 0]);
        assert_eq!(b.empty_pos, 8);
    }

    #[test]
    fn test_new_board_4x4() {
        let b = Board::new(4);
        assert_eq!(b.size, 4);
        assert_eq!(b.tiles.len(), 16);
        assert_eq!(b.tiles[0], 1);
        assert_eq!(b.tiles[14], 15);
        assert_eq!(b.tiles[15], 0);
        assert_eq!(b.empty_pos, 15);
    }

    #[test]
    fn test_new_board_5x5() {
        let b = Board::new(5);
        assert_eq!(b.tiles.len(), 25);
        assert_eq!(b.tiles[23], 24);
        assert_eq!(b.tiles[24], 0);
    }

    #[test]
    fn test_new_board_is_solved() {
        let b = Board::new(4);
        assert!(b.is_solved());
    }

    #[test]
    fn test_empty_position() {
        let b = Board::new(4);
        assert_eq!(b.empty_row(), 3);
        assert_eq!(b.empty_col(), 3);
    }

    // ── Movement ──

    #[test]
    fn test_slide_up() {
        let mut b = Board::new(3);
        // Empty is at (2,2), slide Up brings (2,2-1)=(1,2) tile down
        // Actually Up means tile from below moves up into empty
        // Up: target = empty_pos + size
        // empty at 8 (2,2), target = 8+3=11 — out of bounds, can_move=false
        // Wait: can_move(Up) = empty_row < size-1 → 2 < 2 = false
        assert!(!b.can_move(Direction::Up));
    }

    #[test]
    fn test_slide_down() {
        let mut b = Board::new(3);
        // Empty at (2,2), Down: empty_row > 0 → 2 > 0 = true
        // target = 8 - 3 = 5, tile at pos 5 is value 6
        assert!(b.can_move(Direction::Down));
        assert!(b.slide(Direction::Down));
        assert_eq!(b.tiles[8], 6);
        assert_eq!(b.tiles[5], 0);
        assert_eq!(b.empty_pos, 5);
    }

    #[test]
    fn test_slide_left() {
        let mut b = Board::new(3);
        // Empty at (2,2), Left: empty_col < size-1 → 2 < 2 = false
        assert!(!b.can_move(Direction::Left));
    }

    #[test]
    fn test_slide_right() {
        let mut b = Board::new(3);
        // Empty at (2,2), Right: empty_col > 0 → 2 > 0 = true
        // target = 8 - 1 = 7, tile at pos 7 is value 8
        assert!(b.can_move(Direction::Right));
        assert!(b.slide(Direction::Right));
        assert_eq!(b.tiles[8], 8);
        assert_eq!(b.tiles[7], 0);
        assert_eq!(b.empty_pos, 7);
    }

    #[test]
    fn test_slide_sequence() {
        let mut b = Board::new(3);
        // Start: [1,2,3,4,5,6,7,8,0] empty at 8
        b.slide(Direction::Down); // [1,2,3,4,5,0,7,8,6] empty at 5
        b.slide(Direction::Right); // [1,2,3,4,0,5,7,8,6] empty at 4
        assert_eq!(b.empty_pos, 4);
        assert_eq!(b.tiles[5], 5);
        assert_eq!(b.tiles[4], 0);
    }

    #[test]
    fn test_slide_and_back() {
        let mut b = Board::new(3);
        let original = b.tiles.clone();
        b.slide(Direction::Down);
        b.slide(Direction::Up);
        assert_eq!(b.tiles, original);
    }

    #[test]
    fn test_cannot_slide_invalid() {
        let b = Board::new(3);
        // Empty at bottom-right, can't go Up or Left
        assert!(!b.can_move(Direction::Up));
        assert!(!b.can_move(Direction::Left));
    }

    // ── slide_pos ──

    #[test]
    fn test_slide_pos_adjacent() {
        let mut b = Board::new(3);
        // Empty at (2,2), click (1,2) — same col, row diff 1
        assert!(b.slide_pos(1, 2));
        assert_eq!(b.empty_pos, 5); // moved to row 1, col 2
    }

    #[test]
    fn test_slide_pos_same_row() {
        let mut b = Board::new(3);
        // Empty at (2,2), click (2,1) — same row, col diff 1
        assert!(b.slide_pos(2, 1));
        assert_eq!(b.empty_pos, 7);
    }

    #[test]
    fn test_slide_pos_not_adjacent() {
        let mut b = Board::new(3);
        // Empty at (2,2), click (0,0) — not adjacent
        assert!(!b.slide_pos(0, 0));
    }

    #[test]
    fn test_slide_pos_diagonal() {
        let mut b = Board::new(3);
        // Diagonal — not allowed
        assert!(!b.slide_pos(1, 1));
    }

    #[test]
    fn test_slide_pos_empty_cell() {
        let mut b = Board::new(3);
        // Click on empty space itself
        assert!(!b.slide_pos(2, 2));
    }

    #[test]
    fn test_slide_pos_out_of_bounds() {
        let mut b = Board::new(3);
        assert!(!b.slide_pos(5, 5));
    }

    // ── Shuffle ──

    #[test]
    fn test_shuffle_changes_board() {
        let mut rng = Rng::new(12345);
        let mut b = Board::new(4);
        b.shuffle(&mut rng, 100);
        assert!(!b.is_solved());
    }

    #[test]
    fn test_shuffle_preserves_tiles() {
        let mut rng = Rng::new(99);
        let mut b = Board::new(4);
        b.shuffle(&mut rng, 200);
        // All values 0..16 should still be present
        let mut sorted = b.tiles.clone();
        sorted.sort();
        let expected: Vec<u8> = (0..16).collect();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn test_shuffle_deterministic() {
        let mut rng1 = Rng::new(42);
        let mut b1 = Board::new(4);
        b1.shuffle(&mut rng1, 100);

        let mut rng2 = Rng::new(42);
        let mut b2 = Board::new(4);
        b2.shuffle(&mut rng2, 100);

        assert_eq!(b1.tiles, b2.tiles);
    }

    #[test]
    fn test_shuffle_different_seeds() {
        let mut rng1 = Rng::new(1);
        let mut b1 = Board::new(4);
        b1.shuffle(&mut rng1, 100);

        let mut rng2 = Rng::new(2);
        let mut b2 = Board::new(4);
        b2.shuffle(&mut rng2, 100);

        assert_ne!(b1.tiles, b2.tiles);
    }

    // ── Solved detection ──

    #[test]
    fn test_solved_after_undo() {
        let mut b = Board::new(3);
        assert!(b.is_solved());
        b.slide(Direction::Down);
        assert!(!b.is_solved());
        b.slide(Direction::Up);
        assert!(b.is_solved());
    }

    #[test]
    fn test_not_solved_one_swap() {
        let mut b = Board::new(3);
        // Manually swap two tiles
        b.tiles.swap(0, 1);
        assert!(!b.is_solved());
    }

    // ── Direction ──

    #[test]
    fn test_direction_opposite() {
        assert_eq!(Direction::Up.opposite(), Direction::Down);
        assert_eq!(Direction::Down.opposite(), Direction::Up);
        assert_eq!(Direction::Left.opposite(), Direction::Right);
        assert_eq!(Direction::Right.opposite(), Direction::Left);
    }

    // ── RNG ──

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        assert_ne!(r1.next(), r2.next());
    }

    #[test]
    fn test_rng_range() {
        let mut r = Rng::new(123);
        for _ in 0..1000 {
            let v = r.next_range(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn test_rng_range_zero() {
        let mut r = Rng::new(1);
        assert_eq!(r.next_range(0), 0);
    }

    // ── Game state ──

    #[test]
    fn test_initial_game_not_solved() {
        let app = SlidingPuzzle::new();
        assert_eq!(app.state, GameState::Playing);
        assert!(!app.board.is_solved());
    }

    #[test]
    fn test_initial_size() {
        let app = SlidingPuzzle::new();
        assert_eq!(app.size, 4);
    }

    #[test]
    fn test_initial_moves() {
        let app = SlidingPuzzle::new();
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_set_size_3() {
        let mut app = SlidingPuzzle::new();
        app.set_size(3);
        assert_eq!(app.size, 3);
        assert_eq!(app.board.size, 3);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_set_size_5() {
        let mut app = SlidingPuzzle::new();
        app.set_size(5);
        assert_eq!(app.size, 5);
        assert_eq!(app.board.size, 5);
    }

    #[test]
    fn test_set_size_invalid() {
        let mut app = SlidingPuzzle::new();
        app.set_size(2);
        assert_eq!(app.size, 4); // unchanged
        app.set_size(6);
        assert_eq!(app.size, 4); // unchanged
    }

    #[test]
    fn test_new_game_resets() {
        let mut app = SlidingPuzzle::new();
        app.moves = 50;
        app.state = GameState::Won;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_move_increments_counter() {
        let mut app = SlidingPuzzle::new();
        let dirs = [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];
        let mut moved = false;
        for d in &dirs {
            if app.board.can_move(*d) {
                app.handle_move(*d);
                moved = true;
                break;
            }
        }
        if moved {
            assert_eq!(app.moves, 1);
        }
    }

    #[test]
    fn test_move_when_won_does_nothing() {
        let mut app = SlidingPuzzle::new();
        app.state = GameState::Won;
        app.handle_move(Direction::Up);
        assert_eq!(app.moves, 0); // no change
    }

    #[test]
    fn test_click_when_won_does_nothing() {
        let mut app = SlidingPuzzle::new();
        app.state = GameState::Won;
        app.handle_click(0, 0);
        assert_eq!(app.moves, 0);
    }

    // ── Win detection in game ──

    #[test]
    fn test_win_detection() {
        let mut app = SlidingPuzzle::new();
        // Force board to solved state
        app.board = Board::new(4);
        // Make one move then undo
        app.board.slide(Direction::Down);
        app.moves = 0;
        app.handle_move(Direction::Up);
        assert_eq!(app.state, GameState::Won);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_best_score_tracking() {
        let mut app = SlidingPuzzle::new();
        app.board = Board::new(4);
        app.board.slide(Direction::Down);
        app.moves = 0;
        app.handle_move(Direction::Up);
        // Should have best score of 1 for 4x4
        assert_eq!(app.best_moves[1], Some(1));
    }

    #[test]
    fn test_best_score_only_improves() {
        let mut app = SlidingPuzzle::new();
        // Set initial best
        app.best_moves[1] = Some(5);
        // Win with 3 moves
        app.board = Board::new(4);
        app.board.slide(Direction::Down);
        app.moves = 2;
        app.handle_move(Direction::Up);
        assert_eq!(app.best_moves[1], Some(3));
    }

    #[test]
    fn test_best_score_not_worsen() {
        let mut app = SlidingPuzzle::new();
        app.best_moves[1] = Some(2);
        app.board = Board::new(4);
        app.board.slide(Direction::Down);
        app.moves = 9;
        app.handle_move(Direction::Up);
        assert_eq!(app.best_moves[1], Some(2)); // stays at 2
    }

    // ── Tile colors ──

    #[test]
    fn test_tile_color_zero() {
        let app = SlidingPuzzle::new();
        assert_eq!(app.tile_color(0).r, BASE.r);
    }

    #[test]
    fn test_tile_color_nonzero() {
        let app = SlidingPuzzle::new();
        let c1 = app.tile_color(1);
        // Tile 1 should be TILE_COLORS[0] = BLUE
        assert_eq!(c1.r, BLUE.r);
        assert_eq!(c1.g, BLUE.g);
    }

    #[test]
    fn test_tile_color_wraps() {
        let app = SlidingPuzzle::new();
        // Tile 9 = (9-1) % 8 = 0 = BLUE
        let c = app.tile_color(9);
        assert_eq!(c.r, BLUE.r);
    }

    // ── Size index ──

    #[test]
    fn test_size_index() {
        let mut app = SlidingPuzzle::new();
        app.size = 3;
        assert_eq!(app.size_index(), 0);
        app.size = 4;
        assert_eq!(app.size_index(), 1);
        app.size = 5;
        assert_eq!(app.size_index(), 2);
    }

    // ── Keyboard events ──

    #[test]
    fn test_key_n_new_game() {
        let mut app = SlidingPuzzle::new();
        app.moves = 50;
        let evt = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_key_3_changes_size() {
        let mut app = SlidingPuzzle::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Num3,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.size, 3);
    }

    #[test]
    fn test_key_5_changes_size() {
        let mut app = SlidingPuzzle::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Num5,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert_eq!(app.size, 5);
    }

    #[test]
    fn test_key_h_toggles_help() {
        let mut app = SlidingPuzzle::new();
        assert!(!app.show_help);
        let evt = Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert!(app.show_help);
        app.event(&evt);
        assert!(!app.show_help);
    }

    #[test]
    fn test_key_t_toggles_numbers() {
        let mut app = SlidingPuzzle::new();
        assert!(app.show_numbers);
        let evt = Event::Key(KeyEvent {
            key: Key::T,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: None,
        });
        app.event(&evt);
        assert!(!app.show_numbers);
    }

    // ── Render ──

    #[test]
    fn test_render_returns_commands() {
        let app = SlidingPuzzle::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_help() {
        let mut app = SlidingPuzzle::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_won_state() {
        let mut app = SlidingPuzzle::new();
        app.state = GameState::Won;
        app.moves = 42;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_3x3() {
        let mut app = SlidingPuzzle::new();
        app.set_size(3);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_5x5() {
        let mut app = SlidingPuzzle::new();
        app.set_size(5);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_no_numbers() {
        let mut app = SlidingPuzzle::new();
        app.show_numbers = false;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    // ── Mouse click ──

    #[test]
    fn test_mouse_click_outside_grid() {
        let mut app = SlidingPuzzle::new();
        app.handle_mouse_click(5.0, 5.0); // too far left/up
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_mouse_click_beyond_grid() {
        let mut app = SlidingPuzzle::new();
        app.handle_mouse_click(500.0, 500.0); // beyond grid
        assert_eq!(app.moves, 0);
    }

    // ── Board 4x4 movement ──

    #[test]
    fn test_4x4_can_move_from_corner() {
        let b = Board::new(4);
        // Empty at (3,3)
        assert!(b.can_move(Direction::Down)); // row > 0
        assert!(b.can_move(Direction::Right)); // col > 0
        assert!(!b.can_move(Direction::Up)); // row == size-1
        assert!(!b.can_move(Direction::Left)); // col == size-1
    }

    #[test]
    fn test_4x4_slide_to_center() {
        let mut b = Board::new(4);
        // Move empty from (3,3) to (2,2) via Down then Right
        b.slide(Direction::Down); // empty to (2,3)
        b.slide(Direction::Right); // empty to (2,2)
        assert_eq!(b.empty_row(), 2);
        assert_eq!(b.empty_col(), 2);
    }

    // ── 3x3 specific ──

    #[test]
    fn test_3x3_total_tiles() {
        let b = Board::new(3);
        assert_eq!(b.tiles.len(), 9);
        let mut sorted = b.tiles.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    // ── 5x5 specific ──

    #[test]
    fn test_5x5_total_tiles() {
        let b = Board::new(5);
        assert_eq!(b.tiles.len(), 25);
        let mut sorted = b.tiles.clone();
        sorted.sort();
        let expected: Vec<u8> = (0..25).collect();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn test_5x5_shuffle() {
        let mut rng = Rng::new(7);
        let mut b = Board::new(5);
        b.shuffle(&mut rng, 400);
        assert!(!b.is_solved());
        let mut sorted = b.tiles.clone();
        sorted.sort();
        let expected: Vec<u8> = (0..25).collect();
        assert_eq!(sorted, expected);
    }

    // ── Edge cases ──

    #[test]
    fn test_multiple_slides_same_direction() {
        let mut b = Board::new(4);
        // Slide down 3 times from (3,3)
        b.slide(Direction::Down);
        b.slide(Direction::Down);
        b.slide(Direction::Down);
        assert_eq!(b.empty_row(), 0);
        assert_eq!(b.empty_col(), 3);
    }

    #[test]
    fn test_full_row_slide() {
        let mut b = Board::new(4);
        // Slide right 3 times from (3,3)
        b.slide(Direction::Right);
        b.slide(Direction::Right);
        b.slide(Direction::Right);
        assert_eq!(b.empty_row(), 3);
        assert_eq!(b.empty_col(), 0);
    }

    #[test]
    fn test_board_clone() {
        let b = Board::new(4);
        let b2 = b.clone();
        assert_eq!(b.tiles, b2.tiles);
        assert_eq!(b.empty_pos, b2.empty_pos);
    }

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_eq!(GameState::Won, GameState::Won);
        assert_ne!(GameState::Playing, GameState::Won);
    }

    // ── Best scores across sizes ──

    #[test]
    fn test_best_scores_independent() {
        let mut app = SlidingPuzzle::new();
        // Win 4x4
        app.board = Board::new(4);
        app.board.slide(Direction::Down);
        app.moves = 0;
        app.handle_move(Direction::Up);
        assert_eq!(app.best_moves[1], Some(1));
        assert_eq!(app.best_moves[0], None); // 3x3 unchanged
        assert_eq!(app.best_moves[2], None); // 5x5 unchanged
    }

    // ── Handle move uses correct direction ──

    #[test]
    fn test_handle_move_up_when_possible() {
        let mut app = SlidingPuzzle::new();
        // Move empty to center first
        app.board = Board::new(3);
        app.board.slide(Direction::Down); // empty to (1,2)
        app.board.slide(Direction::Right); // empty to (1,1) center
        app.moves = 0;
        // Now Up should work: empty_row=1 < 2
        app.handle_move(Direction::Up);
        assert_eq!(app.moves, 1);
        assert_eq!(app.board.empty_row(), 2);
    }

    #[test]
    fn test_handle_move_left_when_possible() {
        let mut app = SlidingPuzzle::new();
        app.board = Board::new(3);
        app.board.slide(Direction::Down);
        app.board.slide(Direction::Right);
        app.moves = 0;
        // empty at (1,1), Left: empty_col < size-1 → 1 < 2 = true
        app.handle_move(Direction::Left);
        assert_eq!(app.moves, 1);
        assert_eq!(app.board.empty_col(), 2);
    }

    // ── Show numbers affects render ──

    #[test]
    fn test_show_numbers_default_on() {
        let app = SlidingPuzzle::new();
        assert!(app.show_numbers);
    }
}
