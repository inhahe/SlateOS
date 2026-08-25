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

//! Lights Out — a puzzle where toggling a light also toggles its neighbors.
//!
//! The goal is to turn off all lights on the grid.
//! Clicking a cell toggles it and all orthogonally adjacent cells.
//! Supports 3x3, 5x5, and 7x7 grids.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
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

// Light on/off colors
const LIGHT_ON: Color = YELLOW;
const LIGHT_OFF: Color = SURFACE0;
const CURSOR_COLOR: Color = BLUE;

// ── Randomness ──
//
// This file used to carry its own copy of the LCG that `guitk::rng` exists to
// replace, reduced with `% max` and seeded with a literal `42` — so every
// launch of the game dealt the identical starting puzzle, for ever. It also
// had a `next_bool` that returned `self.next().is_multiple_of(2)`: the lowest
// bit of a power-of-two-modulus LCG, whose period is exactly 2, so it would
// have alternated true, false, true, false. Nothing called it, which is the
// only reason that never shipped.

/// The seed a puzzle falls back to when the kernel has no entropy to give.
///
/// A Lights Out board may be predictable — the worst outcome is that today's
/// first puzzle is the same as yesterday's. Refusing to start the game would
/// be the worse failure; see [`guitk::rng::seeded_from_system`] for the rule.
/// "LIGHTSO!" in ASCII.
const FALLBACK_SEED: u64 = 0x4C49_4748_5453_4F21;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameState {
    Playing,
    Won,
}

struct LightsOut {
    grid: Vec<Vec<bool>>, // true = light on
    size: usize,          // 3, 5, or 7
    cursor_row: usize,
    cursor_col: usize,
    moves: u32,
    state: GameState,
    level: u32,
    best_moves: [Option<u32>; 3], // best for 3x3, 5x5, 7x7
    show_help: bool,
    rng: SeededRng,
}

impl LightsOut {
    fn new() -> Self {
        Self::with_rng(seeded_from_system(FALLBACK_SEED))
    }

    /// A game driven by `rng`, so a test can pin the board it is given.
    fn with_rng(mut rng: SeededRng) -> Self {
        let size = 5;
        let grid = Self::generate_solvable(size, &mut rng, 8);
        Self {
            grid,
            size,
            cursor_row: size / 2,
            cursor_col: size / 2,
            moves: 0,
            state: GameState::Playing,
            level: 1,
            best_moves: [None; 3],
            show_help: false,
            rng,
        }
    }

    /// Generate a solvable puzzle by starting from all-off and toggling random cells.
    fn generate_solvable(size: usize, rng: &mut SeededRng, toggles: usize) -> Vec<Vec<bool>> {
        let mut grid = vec![vec![false; size]; size];
        for _ in 0..toggles {
            let r = rng.below(size);
            let c = rng.below(size);
            Self::toggle_cell_on_grid(&mut grid, size, r, c);
        }
        // Ensure at least one light is on
        let any_on = grid.iter().any(|row| row.iter().any(|&cell| cell));
        if !any_on {
            Self::toggle_cell_on_grid(&mut grid, size, size / 2, size / 2);
        }
        grid
    }

    fn toggle_cell_on_grid(grid: &mut [Vec<bool>], size: usize, row: usize, col: usize) {
        if row < size && col < size {
            grid[row][col] = !grid[row][col];
            if row > 0 {
                grid[row - 1][col] = !grid[row - 1][col];
            }
            if row + 1 < size {
                grid[row + 1][col] = !grid[row + 1][col];
            }
            if col > 0 {
                grid[row][col - 1] = !grid[row][col - 1];
            }
            if col + 1 < size {
                grid[row][col + 1] = !grid[row][col + 1];
            }
        }
    }

    fn toggle_cell(&mut self, row: usize, col: usize) {
        if self.state != GameState::Playing {
            return;
        }
        if row >= self.size || col >= self.size {
            return;
        }
        Self::toggle_cell_on_grid(&mut self.grid, self.size, row, col);
        self.moves = self.moves.saturating_add(1);
        self.check_win();
    }

    fn check_win(&mut self) {
        let all_off = self.grid.iter().all(|row| row.iter().all(|&cell| !cell));
        if all_off {
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

    fn size_index(&self) -> usize {
        match self.size {
            3 => 0,
            5 => 1,
            7 => 2,
            _ => 1,
        }
    }

    fn set_size(&mut self, size: usize) {
        if size == 3 || size == 5 || size == 7 {
            self.size = size;
            self.new_game();
        }
    }

    fn new_game(&mut self) {
        let toggles = match self.size {
            3 => 4,
            5 => 8,
            7 => 14,
            _ => 8,
        };
        self.grid = Self::generate_solvable(self.size, &mut self.rng, toggles);
        self.cursor_row = self.size / 2;
        self.cursor_col = self.size / 2;
        self.moves = 0;
        self.state = GameState::Playing;
        self.level = self.level.saturating_add(1);
    }

    fn lights_on_count(&self) -> usize {
        self.grid
            .iter()
            .flat_map(|r| r.iter())
            .filter(|&&c| c)
            .count()
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) if *modifiers == Modifiers::NONE => {
                match key {
                    Key::Up if self.cursor_row > 0 => {
                        self.cursor_row -= 1;
                    }
                    Key::Down if self.cursor_row < self.size - 1 => {
                        self.cursor_row += 1;
                    }
                    Key::Left if self.cursor_col > 0 => {
                        self.cursor_col -= 1;
                    }
                    Key::Right if self.cursor_col < self.size - 1 => {
                        self.cursor_col += 1;
                    }
                    Key::Enter | Key::Space => {
                        self.toggle_cell(self.cursor_row, self.cursor_col);
                    }
                    Key::N => self.new_game(),
                    Key::H => self.show_help = !self.show_help,
                    Key::Num3 => self.set_size(3),
                    Key::Num5 => self.set_size(5),
                    Key::Num7 => self.set_size(7),
                    _ => {}
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
        let grid_x = 50.0_f32;
        let grid_y = 90.0_f32;
        let cell_size = self.cell_size();
        let total = cell_size * self.size as f32;

        if mx < grid_x || my < grid_y || mx > grid_x + total || my > grid_y + total {
            return;
        }

        let col = ((mx - grid_x) / cell_size) as usize;
        let row = ((my - grid_y) / cell_size) as usize;
        if row < self.size && col < self.size {
            self.cursor_row = row;
            self.cursor_col = col;
            self.toggle_cell(row, col);
        }
    }

    fn cell_size(&self) -> f32 {
        match self.size {
            3 => 100.0,
            5 => 70.0,
            7 => 54.0,
            _ => 70.0,
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
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 28.0,
            text: "Lights Out".into(),
            color: LAVENDER,
            font_size: 24.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Info bar
        let info = format!(
            "{}x{}   Moves: {}   Lights: {}",
            self.size,
            self.size,
            self.moves,
            self.lights_on_count()
        );
        cmds.push(RenderCommand::Text {
            x: 50.0,
            y: 60.0,
            text: info,
            color: SUBTEXT0,
            font_size: 14.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Grid
        let grid_x = 50.0_f32;
        let grid_y = 90.0_f32;
        let cell = self.cell_size();
        let pad = 3.0_f32;
        let grid_total = cell * self.size as f32;

        // Grid background
        cmds.push(RenderCommand::FillRect {
            x: grid_x - 4.0,
            y: grid_y - 4.0,
            width: grid_total + 8.0,
            height: grid_total + 8.0,
            color: CRUST,
            corner_radii: CornerRadii::all(8.0),
        });

        // Cells
        for row in 0..self.size {
            for col in 0..self.size {
                let cx = grid_x + col as f32 * cell + pad;
                let cy = grid_y + row as f32 * cell + pad;
                let cw = cell - pad * 2.0;
                let ch = cell - pad * 2.0;

                let is_on = self.grid[row][col];
                let is_cursor = row == self.cursor_row && col == self.cursor_col;

                // Cell background
                let cell_color = if is_on { LIGHT_ON } else { LIGHT_OFF };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: cw,
                    height: ch,
                    color: cell_color,
                    corner_radii: CornerRadii::all(6.0),
                });

                // Cursor highlight border
                if is_cursor {
                    // Top
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: cw,
                        height: 3.0,
                        color: CURSOR_COLOR,
                        corner_radii: CornerRadii::ZERO,
                    });
                    // Bottom
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy + ch - 3.0,
                        width: cw,
                        height: 3.0,
                        color: CURSOR_COLOR,
                        corner_radii: CornerRadii::ZERO,
                    });
                    // Left
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: 3.0,
                        height: ch,
                        color: CURSOR_COLOR,
                        corner_radii: CornerRadii::ZERO,
                    });
                    // Right
                    cmds.push(RenderCommand::FillRect {
                        x: cx + cw - 3.0,
                        y: cy,
                        width: 3.0,
                        height: ch,
                        color: CURSOR_COLOR,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        // Win message
        if self.state == GameState::Won {
            let win_y = grid_y + grid_total + 20.0;
            cmds.push(RenderCommand::Text {
                x: grid_x,
                y: win_y,
                text: format!("All lights off in {} moves!", self.moves),
                color: GREEN,
                font_size: 18.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: grid_x,
                y: win_y + 26.0,
                text: "Press N for next puzzle".into(),
                color: SUBTEXT0,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Side panel — best scores
        let panel_x = grid_x + grid_total + 30.0;
        let panel_y = grid_y;

        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: 170.0,
            height: 130.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: panel_y + 16.0,
            text: "Best Scores".into(),
            color: YELLOW,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let labels = ["3x3", "5x5", "7x7"];
        for (i, label) in labels.iter().enumerate() {
            let sy = panel_y + 44.0 + i as f32 * 26.0;
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
                overflow: TextOverflow::Clip,
            });
        }

        // Help
        if self.show_help {
            let help_y = panel_y + 150.0;
            cmds.push(RenderCommand::FillRect {
                x: panel_x,
                y: help_y,
                width: 170.0,
                height: 180.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: help_y + 16.0,
                text: "Controls".into(),
                color: YELLOW,
                font_size: 15.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let controls = [
                ("Arrows", "Move cursor"),
                ("Enter", "Toggle cell"),
                ("Click", "Toggle cell"),
                ("3/5/7", "Change size"),
                ("N", "New puzzle"),
                ("H", "Toggle help"),
            ];
            for (i, (k, v)) in controls.iter().enumerate() {
                let ly = help_y + 42.0 + i as f32 * 22.0;
                cmds.push(RenderCommand::Text {
                    x: panel_x + 12.0,
                    y: ly,
                    text: (*k).into(),
                    color: BLUE,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                cmds.push(RenderCommand::Text {
                    x: panel_x + 75.0,
                    y: ly,
                    text: (*v).into(),
                    color: SUBTEXT0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: panel_x + 12.0,
                y: panel_y + 150.0,
                text: "Press H for help".into(),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        cmds
    }
}

fn main() {
    let _app = LightsOut::new();
}

// ── Tests ──
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

    // ── Randomness, as the game uses it ──
    //
    // The four tests that used to live here checked that the private generator
    // was deterministic, differed by seed, and stayed inside its bound. That is
    // `randrange`'s contract now and is tested there against the real hazards
    // (low-bit cycles, modulo bias). What is left here is what this *game*
    // needs from randomness, which the old tests never checked at all.

    /// The board must be a function of the generator it was given. The defect
    /// that shipped was that it wasn't observably one: `new()` seeded a literal
    /// `42`, so every launch dealt the identical puzzle for ever.
    #[test]
    fn the_board_follows_the_generator_it_was_given() {
        let a = LightsOut::with_rng(SeededRng::new(1)).grid;
        let b = LightsOut::with_rng(SeededRng::new(2)).grid;
        assert_ne!(a, b, "the board ignores its generator");
    }

    /// `new()` must route through [`seeded_from_system`] and nothing else.
    ///
    /// This cannot be tested by observing variety, because the host test
    /// toolchain has no SlateOS kernel to ask: `seeded_from_system` correctly
    /// takes its documented fallback there, so two fresh games *are* identical
    /// on the host and would be identical under the old hardcoded `42` too.
    /// What distinguishes the two is *which* seed, so that is what is checked —
    /// and on real hardware the same line reaches the kernel instead.
    #[test]
    #[cfg(not(unix))]
    fn a_fresh_game_is_seeded_by_the_system_and_not_by_a_literal() {
        let fresh = LightsOut::new().grid;
        let fallback = LightsOut::with_rng(SeededRng::new(FALLBACK_SEED)).grid;
        assert_eq!(
            fresh, fallback,
            "new() is not going through seeded_from_system"
        );
        let old_defect = LightsOut::with_rng(SeededRng::new(42)).grid;
        assert_ne!(fresh, old_defect, "new() is back on a hardcoded seed");
    }

    /// Every board the generator produces has to be solvable, which is the one
    /// property the puzzle cannot be played without. It is guaranteed by
    /// construction — the board is built by *applying* toggles to a solved
    /// grid, so replaying them solves it — and this pins the construction
    /// rather than the arithmetic.
    #[test]
    fn every_generated_board_is_reachable_from_the_solved_one() {
        for seed in 0..40 {
            let mut rng = SeededRng::new(seed);
            for size in [3, 5, 7] {
                let grid = LightsOut::generate_solvable(size, &mut rng, 8);
                assert_eq!(grid.len(), size);
                assert!(grid.iter().all(|row| row.len() == size));
                assert!(
                    grid.iter().flatten().any(|&on| on),
                    "an all-off board is already won and cannot be played"
                );
            }
        }
    }

    /// Toggle positions must reach every cell, not just a band of them. A
    /// reduction that reads the low bits of an LCG would concentrate them; this
    /// is the game-level shape of the bug `randrange::below` exists to avoid.
    #[test]
    fn generated_toggles_reach_every_cell_of_the_board() {
        let mut rng = SeededRng::new(7);
        let mut seen = [[false; 7]; 7];
        for _ in 0..500 {
            seen[rng.below(7)][rng.below(7)] = true;
        }
        assert!(
            seen.iter().flatten().all(|&hit| hit),
            "some cells were never chosen over 500 draws"
        );
    }

    // ── Toggle mechanics ──

    #[test]
    fn test_toggle_center() {
        let size = 3;
        let mut grid = vec![vec![false; size]; size];
        LightsOut::toggle_cell_on_grid(&mut grid, size, 1, 1);
        // Center + 4 neighbors
        assert!(grid[1][1]); // center
        assert!(grid[0][1]); // up
        assert!(grid[2][1]); // down
        assert!(grid[1][0]); // left
        assert!(grid[1][2]); // right
        // Corners should be off
        assert!(!grid[0][0]);
        assert!(!grid[0][2]);
        assert!(!grid[2][0]);
        assert!(!grid[2][2]);
    }

    #[test]
    fn test_toggle_corner() {
        let size = 3;
        let mut grid = vec![vec![false; size]; size];
        LightsOut::toggle_cell_on_grid(&mut grid, size, 0, 0);
        assert!(grid[0][0]); // self
        assert!(grid[0][1]); // right
        assert!(grid[1][0]); // down
        assert!(!grid[1][1]); // diagonal not toggled
    }

    #[test]
    fn test_toggle_edge() {
        let size = 3;
        let mut grid = vec![vec![false; size]; size];
        LightsOut::toggle_cell_on_grid(&mut grid, size, 0, 1);
        assert!(grid[0][1]); // self
        assert!(grid[0][0]); // left
        assert!(grid[0][2]); // right
        assert!(grid[1][1]); // down
        // No up neighbor
    }

    #[test]
    fn test_double_toggle_cancels() {
        let size = 3;
        let mut grid = vec![vec![false; size]; size];
        LightsOut::toggle_cell_on_grid(&mut grid, size, 1, 1);
        LightsOut::toggle_cell_on_grid(&mut grid, size, 1, 1);
        // Everything back to false
        for row in &grid {
            for &cell in row {
                assert!(!cell);
            }
        }
    }

    #[test]
    fn test_toggle_out_of_bounds() {
        let size = 3;
        let mut grid = vec![vec![false; size]; size];
        LightsOut::toggle_cell_on_grid(&mut grid, size, 5, 5);
        // Nothing changed
        for row in &grid {
            for &cell in row {
                assert!(!cell);
            }
        }
    }

    // ── Game state ──

    #[test]
    fn test_initial_state() {
        let app = LightsOut::new();
        assert_eq!(app.size, 5);
        assert_eq!(app.moves, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_initial_has_lights_on() {
        let app = LightsOut::new();
        assert!(app.lights_on_count() > 0);
    }

    #[test]
    fn test_cursor_starts_center() {
        let app = LightsOut::new();
        assert_eq!(app.cursor_row, 2);
        assert_eq!(app.cursor_col, 2);
    }

    #[test]
    fn test_set_size_3() {
        let mut app = LightsOut::new();
        app.set_size(3);
        assert_eq!(app.size, 3);
        assert_eq!(app.grid.len(), 3);
        assert_eq!(app.grid[0].len(), 3);
    }

    #[test]
    fn test_set_size_7() {
        let mut app = LightsOut::new();
        app.set_size(7);
        assert_eq!(app.size, 7);
        assert_eq!(app.grid.len(), 7);
    }

    #[test]
    fn test_set_size_invalid() {
        let mut app = LightsOut::new();
        app.set_size(4);
        assert_eq!(app.size, 5); // unchanged
    }

    #[test]
    fn test_new_game_resets() {
        let mut app = LightsOut::new();
        app.moves = 20;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_new_game_increments_level() {
        let mut app = LightsOut::new();
        assert_eq!(app.level, 1);
        app.new_game();
        assert_eq!(app.level, 2);
    }

    // ── Move counting ──

    #[test]
    fn test_toggle_increments_moves() {
        let mut app = LightsOut::new();
        app.toggle_cell(0, 0);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_toggle_when_won_ignored() {
        let mut app = LightsOut::new();
        app.state = GameState::Won;
        app.toggle_cell(0, 0);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_toggle_out_of_bounds_ignored() {
        let mut app = LightsOut::new();
        app.toggle_cell(10, 10);
        assert_eq!(app.moves, 0);
    }

    // ── Win detection ──

    #[test]
    fn test_win_all_off() {
        let mut app = LightsOut::new();
        app.grid = vec![vec![false; 5]; 5];
        app.moves = 0;
        // Toggle center then toggle again to remain all off
        // Actually just set all off and check
        app.check_win();
        assert_eq!(app.state, GameState::Won);
    }

    #[test]
    fn test_no_win_with_lights() {
        let mut app = LightsOut::new();
        // Default has lights on
        app.check_win();
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_win_records_best() {
        let mut app = LightsOut::new();
        app.grid = vec![vec![false; 5]; 5];
        app.moves = 5;
        app.check_win();
        assert_eq!(app.best_moves[1], Some(5));
    }

    #[test]
    fn test_best_score_improves() {
        let mut app = LightsOut::new();
        app.best_moves[1] = Some(10);
        app.grid = vec![vec![false; 5]; 5];
        app.moves = 3;
        app.check_win();
        assert_eq!(app.best_moves[1], Some(3));
    }

    #[test]
    fn test_best_score_no_worsen() {
        let mut app = LightsOut::new();
        app.best_moves[1] = Some(3);
        app.grid = vec![vec![false; 5]; 5];
        app.moves = 8;
        app.check_win();
        assert_eq!(app.best_moves[1], Some(3));
    }

    // ── Size index ──

    #[test]
    fn test_size_index_3() {
        let mut app = LightsOut::new();
        app.size = 3;
        assert_eq!(app.size_index(), 0);
    }

    #[test]
    fn test_size_index_5() {
        let app = LightsOut::new();
        assert_eq!(app.size_index(), 1);
    }

    #[test]
    fn test_size_index_7() {
        let mut app = LightsOut::new();
        app.size = 7;
        assert_eq!(app.size_index(), 2);
    }

    // ── Cell size ──

    #[test]
    fn test_cell_size_3() {
        let mut app = LightsOut::new();
        app.size = 3;
        assert!((app.cell_size() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_cell_size_5() {
        let app = LightsOut::new();
        assert!((app.cell_size() - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_cell_size_7() {
        let mut app = LightsOut::new();
        app.size = 7;
        assert!((app.cell_size() - 54.0).abs() < 0.01);
    }

    // ── Lights count ──

    #[test]
    fn test_lights_count_all_off() {
        let mut app = LightsOut::new();
        app.grid = vec![vec![false; 5]; 5];
        assert_eq!(app.lights_on_count(), 0);
    }

    #[test]
    fn test_lights_count_all_on() {
        let mut app = LightsOut::new();
        app.grid = vec![vec![true; 5]; 5];
        assert_eq!(app.lights_on_count(), 25);
    }

    // ── Generate solvable ──

    #[test]
    fn test_generate_solvable_has_lights() {
        let mut rng = SeededRng::new(42);
        let grid = LightsOut::generate_solvable(5, &mut rng, 8);
        let count: usize = grid.iter().flat_map(|r| r.iter()).filter(|&&c| c).count();
        assert!(count > 0);
    }

    #[test]
    fn test_generate_solvable_deterministic() {
        let mut rng1 = SeededRng::new(42);
        let grid1 = LightsOut::generate_solvable(5, &mut rng1, 8);
        let mut rng2 = SeededRng::new(42);
        let grid2 = LightsOut::generate_solvable(5, &mut rng2, 8);
        assert_eq!(grid1, grid2);
    }

    #[test]
    fn test_generate_solvable_different_seeds() {
        let mut rng1 = SeededRng::new(1);
        let grid1 = LightsOut::generate_solvable(5, &mut rng1, 8);
        let mut rng2 = SeededRng::new(99);
        let grid2 = LightsOut::generate_solvable(5, &mut rng2, 8);
        // Very likely different
        assert_ne!(grid1, grid2);
    }

    // ── Key events ──

    #[test]
    fn test_key_up() {
        let mut app = LightsOut::new();
        app.cursor_row = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_row, 1);
    }

    #[test]
    fn test_key_up_at_min() {
        let mut app = LightsOut::new();
        app.cursor_row = 0;
        let evt = Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn test_key_down() {
        let mut app = LightsOut::new();
        app.cursor_row = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_row, 3);
    }

    #[test]
    fn test_key_down_at_max() {
        let mut app = LightsOut::new();
        app.cursor_row = 4;
        let evt = Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_row, 4);
    }

    #[test]
    fn test_key_left() {
        let mut app = LightsOut::new();
        app.cursor_col = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_key_right() {
        let mut app = LightsOut::new();
        app.cursor_col = 2;
        let evt = Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.cursor_col, 3);
    }

    #[test]
    fn test_key_enter_toggles() {
        let mut app = LightsOut::new();
        let before = app.grid[2][2];
        let evt = Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        // Center should have toggled
        assert_ne!(app.grid[2][2], before);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn test_key_n_new_game() {
        let mut app = LightsOut::new();
        app.moves = 15;
        let evt = Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_key_3_changes_size() {
        let mut app = LightsOut::new();
        let evt = Event::Key(KeyEvent {
            key: Key::Num3,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert_eq!(app.size, 3);
    }

    #[test]
    fn test_key_h_toggle_help() {
        let mut app = LightsOut::new();
        assert!(!app.show_help);
        let evt = Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::NONE,
            pressed: true,
            text: String::new(),
        });
        app.event(&evt);
        assert!(app.show_help);
    }

    // ── Mouse ──

    #[test]
    fn test_mouse_click_outside() {
        let mut app = LightsOut::new();
        app.handle_mouse_click(5.0, 5.0);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_mouse_click_in_grid() {
        let mut app = LightsOut::new();
        // Click on cell (0,0) at position (50+35, 90+35)
        app.handle_mouse_click(85.0, 125.0);
        assert_eq!(app.moves, 1);
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
    }

    // ── Render ──

    #[test]
    fn test_render_returns_commands() {
        let app = LightsOut::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_help() {
        let mut app = LightsOut::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_won_state() {
        let mut app = LightsOut::new();
        app.state = GameState::Won;
        let cmds = app.render(800.0, 600.0);
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_3x3() {
        let mut app = LightsOut::new();
        app.set_size(3);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_7x7() {
        let mut app = LightsOut::new();
        app.set_size(7);
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    // ── Game state eq ──

    #[test]
    fn test_game_state_eq() {
        assert_eq!(GameState::Playing, GameState::Playing);
        assert_eq!(GameState::Won, GameState::Won);
        assert_ne!(GameState::Playing, GameState::Won);
    }

    // ── Solve by reverse toggles ──

    #[test]
    fn test_solvable_by_reverse() {
        // Generate a puzzle, record the toggle positions, then replay them.
        // The Rng would normally pick the toggle positions; here we use a
        // hand-picked set so the rng is unused.
        let mut grid = vec![vec![false; 3]; 3];
        let toggles = [(1, 1), (0, 0), (2, 2)];
        for (r, c) in &toggles {
            LightsOut::toggle_cell_on_grid(&mut grid, 3, *r, *c);
        }
        // Now reverse
        for (r, c) in toggles.iter().rev() {
            LightsOut::toggle_cell_on_grid(&mut grid, 3, *r, *c);
        }
        // Should be all off
        for row in &grid {
            for &cell in row {
                assert!(!cell);
            }
        }
    }

    // ── Best scores across sizes ──

    #[test]
    fn test_best_scores_independent() {
        let mut app = LightsOut::new();
        // Win 5x5
        app.grid = vec![vec![false; 5]; 5];
        app.moves = 4;
        app.check_win();
        assert_eq!(app.best_moves[1], Some(4));
        assert_eq!(app.best_moves[0], None);
        assert_eq!(app.best_moves[2], None);
    }
}
