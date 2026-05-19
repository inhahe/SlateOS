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

//! Flood It — a color-flooding puzzle game.
//!
//! Starting from the top-left corner, choose a color to flood-fill.
//! Connected cells of the same color merge into your region.
//! Fill the entire board before running out of moves.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
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

const NUM_COLORS: usize = 6;
const PALETTE: [Color; NUM_COLORS] = [RED, PEACH, YELLOW, GREEN, TEAL, MAUVE];
const PALETTE_LABELS: [&str; NUM_COLORS] = ["R", "O", "Y", "G", "T", "M"];

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next(&mut self) -> u64 {
        self.state = self.state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 { return 0; }
        (self.next() % max as u64) as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameState {
    Playing,
    Won,
    Lost,
}

struct FloodIt {
    grid: Vec<Vec<u8>>,
    size: usize,
    moves: u32,
    max_moves: u32,
    state: GameState,
    selected_color: usize,
    rng: Rng,
    show_help: bool,
}

impl FloodIt {
    fn new() -> Self {
        let size = 14;
        let mut rng = Rng::new(42);
        let grid = Self::generate_grid(size, &mut rng);
        let max_moves = Self::max_moves_for_size(size);
        Self {
            grid,
            size,
            moves: 0,
            max_moves,
            state: GameState::Playing,
            selected_color: 0,
            rng,
            show_help: false,
        }
    }

    fn generate_grid(size: usize, rng: &mut Rng) -> Vec<Vec<u8>> {
        let mut grid = vec![vec![0u8; size]; size];
        for row in &mut grid {
            for cell in row.iter_mut() {
                *cell = rng.next_range(NUM_COLORS) as u8;
            }
        }
        grid
    }

    fn max_moves_for_size(size: usize) -> u32 {
        match size {
            8 => 14,
            10 => 20,
            14 => 25,
            18 => 35,
            _ => (size as u32) * 2,
        }
    }

    fn set_size(&mut self, size: usize) {
        if size == 8 || size == 10 || size == 14 || size == 18 {
            self.size = size;
            self.new_game();
        }
    }

    fn new_game(&mut self) {
        self.grid = Self::generate_grid(self.size, &mut self.rng);
        self.max_moves = Self::max_moves_for_size(self.size);
        self.moves = 0;
        self.state = GameState::Playing;
    }

    fn flood_fill(&mut self, new_color: u8) {
        if self.state != GameState::Playing {
            return;
        }
        let old_color = self.grid[0][0];
        if old_color == new_color {
            return;
        }

        // BFS from (0,0) to find all connected cells of old_color
        let mut visited = vec![vec![false; self.size]; self.size];
        let mut queue = Vec::new();
        queue.push((0usize, 0usize));
        visited[0][0] = true;

        while let Some((r, c)) = queue.pop() {
            self.grid[r][c] = new_color;
            let neighbors = [
                (r.wrapping_sub(1), c),
                (r + 1, c),
                (r, c.wrapping_sub(1)),
                (r, c + 1),
            ];
            for (nr, nc) in neighbors {
                if nr < self.size && nc < self.size && !visited[nr][nc] && self.grid[nr][nc] == old_color {
                    visited[nr][nc] = true;
                    queue.push((nr, nc));
                }
            }
        }

        self.moves = self.moves.saturating_add(1);
        self.check_end();
    }

    fn check_end(&mut self) {
        // Check if all cells are the same color
        let first = self.grid[0][0];
        let all_same = self.grid.iter().all(|row| row.iter().all(|&c| c == first));
        if all_same {
            self.state = GameState::Won;
        } else if self.moves >= self.max_moves {
            self.state = GameState::Lost;
        }
    }

    fn filled_count(&self) -> usize {
        let target = self.grid[0][0];
        let mut visited = vec![vec![false; self.size]; self.size];
        let mut queue = vec![(0usize, 0usize)];
        visited[0][0] = true;
        let mut count = 0usize;
        while let Some((r, c)) = queue.pop() {
            count += 1;
            let neighbors = [
                (r.wrapping_sub(1), c),
                (r + 1, c),
                (r, c.wrapping_sub(1)),
                (r, c + 1),
            ];
            for (nr, nc) in neighbors {
                if nr < self.size && nc < self.size && !visited[nr][nc] && self.grid[nr][nc] == target {
                    visited[nr][nc] = true;
                    queue.push((nr, nc));
                }
            }
        }
        count
    }

    fn choose_color(&mut self, color_idx: usize) {
        if color_idx < NUM_COLORS {
            self.selected_color = color_idx;
            self.flood_fill(color_idx as u8);
        }
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if *modifiers == Modifiers::NONE {
                    match key {
                        Key::Num1 => self.choose_color(0),
                        Key::Num2 => self.choose_color(1),
                        Key::Num3 => self.choose_color(2),
                        Key::Num4 => self.choose_color(3),
                        Key::Num5 => self.choose_color(4),
                        Key::Num6 => self.choose_color(5),
                        Key::N => self.new_game(),
                        Key::H => self.show_help = !self.show_help,
                        Key::S => self.set_size(8),
                        Key::M => self.set_size(14),
                        Key::L => self.set_size(18),
                        _ => {}
                    }
                }
            }
            Event::Mouse(MouseEvent { x, y, kind }) => {
                if matches!(kind, MouseEventKind::Press(MouseButton::Left)) {
                    self.handle_mouse(*x, *y);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mx: f32, my: f32) {
        // Check color buttons
        let btn_y = 60.0_f32;
        let btn_h = 36.0_f32;
        let btn_w = 50.0_f32;
        let btn_start_x = 50.0_f32;

        if my >= btn_y && my <= btn_y + btn_h {
            for i in 0..NUM_COLORS {
                let bx = btn_start_x + i as f32 * (btn_w + 8.0);
                if mx >= bx && mx <= bx + btn_w {
                    self.choose_color(i);
                    return;
                }
            }
        }
    }

    fn cell_size(&self) -> f32 {
        match self.size {
            8 => 36.0,
            10 => 30.0,
            14 => 22.0,
            18 => 17.0,
            _ => 22.0,
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 50.0, y: 26.0,
            text: "Flood It".into(),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Color buttons
        let btn_y = 56.0_f32;
        let btn_w = 50.0_f32;
        let btn_h = 30.0_f32;
        let btn_start_x = 50.0_f32;

        for i in 0..NUM_COLORS {
            let bx = btn_start_x + i as f32 * (btn_w + 8.0);
            let is_current = self.grid[0][0] == i as u8;
            cmds.push(RenderCommand::FillRect {
                x: bx, y: btn_y,
                width: btn_w, height: btn_h,
                color: PALETTE[i],
                corner_radii: CornerRadii::all(4.0),
            });
            // Button label
            cmds.push(RenderCommand::Text {
                x: bx + 5.0, y: btn_y + 6.0,
                text: format!("{} {}", i + 1, PALETTE_LABELS[i]),
                color: if is_current { CRUST } else { CRUST },
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            // Highlight current color
            if is_current {
                cmds.push(RenderCommand::FillRect {
                    x: bx, y: btn_y + btn_h + 2.0,
                    width: btn_w, height: 3.0,
                    color: TEXT,
                    corner_radii: CornerRadii::ZERO,
                });
            }
        }

        // Info
        let info = format!(
            "Moves: {}/{}   Filled: {}/{}",
            self.moves, self.max_moves,
            self.filled_count(), self.size * self.size
        );
        cmds.push(RenderCommand::Text {
            x: btn_start_x + NUM_COLORS as f32 * (btn_w + 8.0) + 10.0,
            y: btn_y + 8.0,
            text: info,
            color: SUBTEXT0,
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid
        let grid_x = 50.0_f32;
        let grid_y = 100.0_f32;
        let cs = self.cell_size();
        let grid_total = cs * self.size as f32;

        cmds.push(RenderCommand::FillRect {
            x: grid_x - 3.0, y: grid_y - 3.0,
            width: grid_total + 6.0, height: grid_total + 6.0,
            color: CRUST,
            corner_radii: CornerRadii::all(4.0),
        });

        for row in 0..self.size {
            for col in 0..self.size {
                let c = self.grid[row][col] as usize;
                let color = if c < NUM_COLORS { PALETTE[c] } else { SURFACE0 };
                cmds.push(RenderCommand::FillRect {
                    x: grid_x + col as f32 * cs + 1.0,
                    y: grid_y + row as f32 * cs + 1.0,
                    width: cs - 2.0,
                    height: cs - 2.0,
                    color,
                    corner_radii: CornerRadii::all(2.0),
                });
            }
        }

        // Game over messages
        match self.state {
            GameState::Won => {
                cmds.push(RenderCommand::Text {
                    x: grid_x, y: grid_y + grid_total + 16.0,
                    text: format!("Board flooded in {} moves!", self.moves),
                    color: GREEN,
                    font_size: 18.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameState::Lost => {
                cmds.push(RenderCommand::Text {
                    x: grid_x, y: grid_y + grid_total + 16.0,
                    text: format!("Out of moves! ({}/{})", self.moves, self.max_moves),
                    color: RED,
                    font_size: 18.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }
            GameState::Playing => {}
        }
        if self.state != GameState::Playing {
            cmds.push(RenderCommand::Text {
                x: grid_x, y: grid_y + grid_total + 40.0,
                text: "Press N for new game".into(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Help
        if self.show_help {
            let help_x = grid_x + grid_total + 20.0;
            let help_y = grid_y;
            cmds.push(RenderCommand::FillRect {
                x: help_x, y: help_y,
                width: 160.0, height: 160.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: help_x + 10.0, y: help_y + 14.0,
                text: "Controls".into(),
                color: YELLOW,
                font_size: 14.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            let lines = [
                ("1-6", "Choose color"),
                ("Click", "Choose color"),
                ("N", "New game"),
                ("S/M/L", "Size 8/14/18"),
                ("H", "Toggle help"),
            ];
            for (i, (k, v)) in lines.iter().enumerate() {
                let ly = help_y + 38.0 + i as f32 * 22.0;
                cmds.push(RenderCommand::Text {
                    x: help_x + 10.0, y: ly,
                    text: (*k).into(),
                    color: BLUE,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                cmds.push(RenderCommand::Text {
                    x: help_x + 60.0, y: ly,
                    text: (*v).into(),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: grid_x + grid_total + 20.0, y: grid_y,
                text: "H for help".into(),
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
    let _app = FloodIt::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let app = FloodIt::new();
        assert_eq!(app.size, 14);
        assert_eq!(app.moves, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_grid_size() {
        let app = FloodIt::new();
        assert_eq!(app.grid.len(), 14);
        assert_eq!(app.grid[0].len(), 14);
    }

    #[test]
    fn test_grid_values_in_range() {
        let app = FloodIt::new();
        for row in &app.grid {
            for &c in row {
                assert!((c as usize) < NUM_COLORS);
            }
        }
    }

    #[test]
    fn test_set_size_8() {
        let mut app = FloodIt::new();
        app.set_size(8);
        assert_eq!(app.size, 8);
        assert_eq!(app.grid.len(), 8);
        assert_eq!(app.max_moves, 14);
    }

    #[test]
    fn test_set_size_18() {
        let mut app = FloodIt::new();
        app.set_size(18);
        assert_eq!(app.size, 18);
        assert_eq!(app.max_moves, 35);
    }

    #[test]
    fn test_set_size_invalid() {
        let mut app = FloodIt::new();
        app.set_size(5);
        assert_eq!(app.size, 14);
    }

    #[test]
    fn test_new_game() {
        let mut app = FloodIt::new();
        app.moves = 10;
        app.state = GameState::Lost;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_flood_fill_same_color_no_op() {
        let mut app = FloodIt::new();
        let c = app.grid[0][0];
        app.flood_fill(c);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_flood_fill_changes_color() {
        let mut app = FloodIt::new();
        let old = app.grid[0][0];
        let new_c = if old == 0 { 1 } else { 0 };
        app.flood_fill(new_c as u8);
        assert_eq!(app.grid[0][0], new_c as u8);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_flood_fill_spreads() {
        let mut app = FloodIt::new();
        app.size = 3;
        app.grid = vec![vec![0; 3]; 3];
        app.grid[0][0] = 0;
        app.grid[0][1] = 0;
        app.grid[0][2] = 1;
        app.grid[1][0] = 0;
        app.grid[1][1] = 1;
        app.grid[1][2] = 1;
        app.grid[2][0] = 2;
        app.grid[2][1] = 2;
        app.grid[2][2] = 2;
        // Flood with color 1: (0,0), (0,1), (1,0) should become 1
        app.flood_fill(1);
        assert_eq!(app.grid[0][0], 1);
        assert_eq!(app.grid[0][1], 1);
        assert_eq!(app.grid[1][0], 1);
        assert_eq!(app.grid[2][0], 2); // not connected
    }

    #[test]
    fn test_flood_when_won() {
        let mut app = FloodIt::new();
        app.state = GameState::Won;
        app.flood_fill(1);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_flood_when_lost() {
        let mut app = FloodIt::new();
        app.state = GameState::Lost;
        app.flood_fill(1);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_win_detection() {
        let mut app = FloodIt::new();
        app.size = 2;
        app.grid = vec![vec![0, 1], vec![0, 0]];
        app.max_moves = 10;
        app.flood_fill(1);
        assert_eq!(app.state, GameState::Won);
    }

    #[test]
    fn test_loss_detection() {
        let mut app = FloodIt::new();
        app.size = 3;
        app.grid = vec![vec![0, 1, 2], vec![3, 4, 5], vec![0, 1, 2]];
        app.max_moves = 1;
        app.moves = 0;
        // Only 1 move allowed
        app.flood_fill(1);
        assert_eq!(app.state, GameState::Lost);
    }

    #[test]
    fn test_filled_count_initial() {
        let mut app = FloodIt::new();
        app.size = 3;
        app.grid = vec![vec![0, 0, 1], vec![0, 1, 1], vec![2, 2, 2]];
        assert_eq!(app.filled_count(), 3); // (0,0), (0,1), (1,0) connected
    }

    #[test]
    fn test_filled_count_all() {
        let mut app = FloodIt::new();
        app.size = 3;
        app.grid = vec![vec![0; 3]; 3];
        assert_eq!(app.filled_count(), 9);
    }

    #[test]
    fn test_choose_color() {
        let mut app = FloodIt::new();
        let old = app.grid[0][0] as usize;
        let new_c = if old == 0 { 1 } else { 0 };
        app.choose_color(new_c);
        assert_eq!(app.grid[0][0], new_c as u8);
    }

    #[test]
    fn test_choose_color_out_of_range() {
        let mut app = FloodIt::new();
        app.choose_color(10);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_cell_size_8() {
        let mut app = FloodIt::new();
        app.size = 8;
        assert!((app.cell_size() - 36.0).abs() < 0.01);
    }

    #[test]
    fn test_cell_size_14() {
        let app = FloodIt::new();
        assert!((app.cell_size() - 22.0).abs() < 0.01);
    }

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..50 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_generate_grid_deterministic() {
        let mut r1 = Rng::new(42);
        let g1 = FloodIt::generate_grid(5, &mut r1);
        let mut r2 = Rng::new(42);
        let g2 = FloodIt::generate_grid(5, &mut r2);
        assert_eq!(g1, g2);
    }

    #[test]
    fn test_key_1_chooses_color() {
        let mut app = FloodIt::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Num1, modifiers: Modifiers::NONE,
            pressed: true, text: None,
        });
        app.event(&evt);
        // May or may not change based on current grid, but should not panic
    }

    #[test]
    fn test_key_n_new_game() {
        let mut app = FloodIt::new();
        app.moves = 10;
        let evt = Event::Key(KeyEvent {
            key: Key::N, modifiers: Modifiers::NONE,
            pressed: true, text: None,
        });
        app.event(&evt);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_key_h_toggle_help() {
        let mut app = FloodIt::new();
        assert!(!app.show_help);
        let evt = Event::Key(KeyEvent {
            key: Key::H, modifiers: Modifiers::NONE,
            pressed: true, text: None,
        });
        app.event(&evt);
        assert!(app.show_help);
    }

    #[test]
    fn test_render() {
        let app = FloodIt::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won() {
        let mut app = FloodIt::new();
        app.state = GameState::Won;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_lost() {
        let mut app = FloodIt::new();
        app.state = GameState::Lost;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 5);
    }

    #[test]
    fn test_render_with_help() {
        let mut app = FloodIt::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_max_moves_sizes() {
        assert_eq!(FloodIt::max_moves_for_size(8), 14);
        assert_eq!(FloodIt::max_moves_for_size(10), 20);
        assert_eq!(FloodIt::max_moves_for_size(14), 25);
        assert_eq!(FloodIt::max_moves_for_size(18), 35);
    }

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_ne!(GameState::Won, GameState::Lost);
    }

    #[test]
    fn test_complete_small_game() {
        let mut app = FloodIt::new();
        app.size = 2;
        app.grid = vec![vec![0, 1], vec![2, 3]];
        app.max_moves = 10;
        app.moves = 0;
        app.state = GameState::Playing;
        // Fill with 1: (0,0)->1, merges with (0,1)
        app.flood_fill(1);
        // Fill with 2: region=(0,0),(0,1) -> 2, merges with (1,0)
        app.flood_fill(2);
        // Fill with 3: region=(0,0),(0,1),(1,0) -> 3, merges with (1,1)
        app.flood_fill(3);
        assert_eq!(app.state, GameState::Won);
        assert_eq!(app.moves, 3);
    }
}
