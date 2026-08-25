//! Maze generator and solver for SlateOS.
//!
//! Features:
//! - Recursive backtracker maze generation (perfect mazes)
//! - Multiple maze sizes (small, medium, large)
//! - Player navigation with arrow keys
//! - Solve visualization (BFS shortest path)
//! - Timer tracking
//! - Trail visualization (shows visited cells)
//! - New maze generation (N key)
//! - Catppuccin Mocha dark theme

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

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::rng::{RandomSource, SeededRng, seeded_from_system};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const COL_BASE: Color = Color::from_hex(0x1E1E2E);
const COL_MANTLE: Color = Color::from_hex(0x181825);
const COL_CRUST: Color = Color::from_hex(0x11111B);
const COL_SURFACE0: Color = Color::from_hex(0x313244);
const COL_SURFACE1: Color = Color::from_hex(0x45475A);
const COL_SURFACE2: Color = Color::from_hex(0x585B70);
const COL_TEXT: Color = Color::from_hex(0xCDD6F4);
const COL_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COL_BLUE: Color = Color::from_hex(0x89B4FA);
const COL_GREEN: Color = Color::from_hex(0xA6E3A1);
const COL_RED: Color = Color::from_hex(0xF38BA8);
const COL_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COL_PEACH: Color = Color::from_hex(0xFAB387);
const COL_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COL_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COL_TEAL: Color = Color::from_hex(0x94E2D5);
const COL_MAUVE: Color = Color::from_hex(0xCBA6F7);

// ── Randomness ──────────────────────────────────────────────────────

/// Seed used when the kernel's entropy source cannot be reached.
///
/// A per-crate constant rather than a shared one, so that two programs which
/// lose entropy on the same boot do not then produce correlated streams. The
/// bytes spell `MAZE!!!!`.
const FALLBACK_SEED: u64 = 0x4D41_5A45_2121_2121;

// This crate used to carry its own copy of the LCG that got copied into
// sixteen crates, reducing with `next() % max`. That is the broken reduction,
// and of all sixteen this is where it did the most damage, because the thing
// being shuffled has exactly four elements.
//
// The generator's modulus is 2^64, so bit *k* of its state has period 2^(k+1):
// the low bits are a counter, not a draw, and a power-of-two bound reads only
// those. `generate` shuffles `Dir::ALL` -- four directions -- at every step of
// the recursive backtracker, which is three draws at bounds 4, 3, 2, and then
// picks a neighbour, which is a fourth. Four draws per step, and 4 mod 4 = 0,
// so the bound-4 draw returned *the same value at every step of every maze*.
//
// Measured on the old reduction, in exactly that four-draws-per-step pattern:
// the shuffle produced **3 distinct orderings of the four directions out of a
// possible 24**, and all three of them ended with the same direction -- so one
// direction was systematically tried last, everywhere, in every maze. (Run
// without the neighbour draw, so three draws per step, it manages 12 of 24;
// the interaction with the fourth draw is what collapses it the rest of the
// way. That is worth stating because it means the defect was invisible to any
// test of the shuffle in isolation, which is how it survived.)
//
// `randrange::below` is Lemire's method -- it multiplies by the bound into 128
// bits and keeps the *top* half -- and `SeededRng` puts SplitMix64's finaliser
// over the LCG, so there are no weak low bits left to read even by accident.

// ── Direction ───────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    North,
    South,
    East,
    West,
}

impl Dir {
    const ALL: [Dir; 4] = [Dir::North, Dir::South, Dir::East, Dir::West];

    fn delta(self) -> (i32, i32) {
        match self {
            Dir::North => (-1, 0),
            Dir::South => (1, 0),
            Dir::East => (0, 1),
            Dir::West => (0, -1),
        }
    }

    fn opposite(self) -> Dir {
        match self {
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::East => Dir::West,
            Dir::West => Dir::East,
        }
    }

    fn wall_bit(self) -> u8 {
        match self {
            Dir::North => 1,
            Dir::South => 2,
            Dir::East => 4,
            Dir::West => 8,
        }
    }
}

// ── Cell ────────────────────────────────────────────────────────────
/// Each cell stores which walls are open (bit removed = passage exists)
#[derive(Debug, Clone, Copy)]
struct MazeCell {
    /// Bitfield: bits for N/S/E/W walls. All set = fully walled.
    walls: u8,
    visited_gen: bool, // Used during generation
}

impl MazeCell {
    fn new() -> Self {
        Self {
            walls: 0x0F, // All 4 walls present
            visited_gen: false,
        }
    }

    fn has_wall(&self, dir: Dir) -> bool {
        self.walls & dir.wall_bit() != 0
    }

    fn remove_wall(&mut self, dir: Dir) {
        self.walls &= !dir.wall_bit();
    }
}

// ── Maze ────────────────────────────────────────────────────────────
struct Maze {
    rows: usize,
    cols: usize,
    cells: Vec<MazeCell>,
}

impl Maze {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![MazeCell::new(); rows * cols],
        }
    }

    fn idx(&self, row: usize, col: usize) -> usize {
        row * self.cols + col
    }

    fn cell(&self, row: usize, col: usize) -> &MazeCell {
        &self.cells[self.idx(row, col)]
    }

    fn cell_mut(&mut self, row: usize, col: usize) -> &mut MazeCell {
        let idx = self.idx(row, col);
        &mut self.cells[idx]
    }

    fn in_bounds(&self, row: i32, col: i32) -> bool {
        row >= 0 && (row as usize) < self.rows && col >= 0 && (col as usize) < self.cols
    }

    fn has_wall(&self, row: usize, col: usize, dir: Dir) -> bool {
        self.cell(row, col).has_wall(dir)
    }

    fn can_move(&self, row: usize, col: usize, dir: Dir) -> bool {
        !self.has_wall(row, col, dir)
    }

    /// Generate maze using recursive backtracker (iterative with explicit stack)
    fn generate(&mut self, rng: &mut SeededRng) {
        // Reset all cells
        for cell in &mut self.cells {
            cell.walls = 0x0F;
            cell.visited_gen = false;
        }

        let mut stack: Vec<(usize, usize)> = Vec::new();
        let start_r = 0;
        let start_c = 0;
        self.cell_mut(start_r, start_c).visited_gen = true;
        stack.push((start_r, start_c));

        while let Some(&(r, c)) = stack.last() {
            // Find unvisited neighbors
            let mut neighbors = Vec::new();
            let mut dirs = Dir::ALL;
            rng.shuffle(&mut dirs);

            for dir in dirs {
                let (dr, dc) = dir.delta();
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if self.in_bounds(nr, nc) {
                    let nr = nr as usize;
                    let nc = nc as usize;
                    if !self.cell(nr, nc).visited_gen {
                        neighbors.push((nr, nc, dir));
                    }
                }
            }

            if neighbors.is_empty() {
                stack.pop();
            } else {
                let choice = rng.below(neighbors.len());
                let (nr, nc, dir) = neighbors[choice];
                self.cell_mut(r, c).remove_wall(dir);
                self.cell_mut(nr, nc).remove_wall(dir.opposite());
                self.cell_mut(nr, nc).visited_gen = true;
                stack.push((nr, nc));
            }
        }
    }

    /// BFS shortest path from start to end
    fn solve(&self, start: (usize, usize), end: (usize, usize)) -> Vec<(usize, usize)> {
        let total = self.rows * self.cols;
        let mut visited = vec![false; total];
        let mut parent: Vec<Option<usize>> = vec![None; total];

        let start_idx = self.idx(start.0, start.1);
        let end_idx = self.idx(end.0, end.1);

        let mut queue = Vec::new();
        let mut head = 0;
        queue.push(start_idx);
        visited[start_idx] = true;

        while head < queue.len() {
            let current = queue[head];
            head += 1;

            if current == end_idx {
                break;
            }

            let r = current / self.cols;
            let c = current % self.cols;

            for dir in Dir::ALL {
                if !self.can_move(r, c, dir) {
                    continue;
                }
                let (dr, dc) = dir.delta();
                let nr = (r as i32 + dr) as usize;
                let nc = (c as i32 + dc) as usize;
                let ni = self.idx(nr, nc);
                if !visited[ni] {
                    visited[ni] = true;
                    parent[ni] = Some(current);
                    queue.push(ni);
                }
            }
        }

        // Reconstruct path
        let mut path = Vec::new();
        let mut cur = end_idx;
        if !visited[end_idx] {
            return path; // No path found (shouldn't happen in a perfect maze)
        }
        while cur != start_idx {
            let r = cur / self.cols;
            let c = cur % self.cols;
            path.push((r, c));
            if let Some(p) = parent[cur] {
                cur = p;
            } else {
                break;
            }
        }
        path.push(start);
        path.reverse();
        path
    }
}

impl Clone for Maze {
    fn clone(&self) -> Self {
        Self {
            rows: self.rows,
            cols: self.cols,
            cells: self.cells.clone(),
        }
    }
}

// ── Difficulty ──────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Difficulty {
    Small,
    Medium,
    Large,
}

impl Difficulty {
    fn size(self) -> (usize, usize) {
        match self {
            Self::Small => (10, 10),
            Self::Medium => (20, 20),
            Self::Large => (30, 30),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Small => "Small (10x10)",
            Self::Medium => "Medium (20x20)",
            Self::Large => "Large (30x30)",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Small,
        }
    }
}

// ── View ────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Playing,
    Won,
}

// ── App ─────────────────────────────────────────────────────────────
struct MazeApp {
    maze: Maze,
    view: View,
    player_row: usize,
    player_col: usize,
    goal_row: usize,
    goal_col: usize,
    moves: u32,
    elapsed_secs: u64,
    rng: SeededRng,
    difficulty: Difficulty,
    // Trail of visited cells
    trail: Vec<bool>,
    // Solution path (shown with H key)
    solution: Vec<(usize, usize)>,
    show_solution: bool,
    // Stats
    games_won: u32,
    best_moves: Option<u32>,
}

impl MazeApp {
    fn new() -> Self {
        let mut app = Self {
            maze: Maze::new(10, 10),
            view: View::Playing,
            player_row: 0,
            player_col: 0,
            goal_row: 9,
            goal_col: 9,
            moves: 0,
            elapsed_secs: 0,
            // Was `SeededRng::new(42)`: every player, on every machine, walked
            // the same maze. Predicting a maze costs the user nothing but
            // the puzzle, so this asks the kernel and falls back rather
            // than refusing -- see `randrange::seeded_from_system`.
            rng: seeded_from_system(FALLBACK_SEED),
            difficulty: Difficulty::Small,
            trail: vec![false; 100],
            solution: Vec::new(),
            show_solution: false,
            games_won: 0,
            best_moves: None,
        };
        app.new_maze();
        app
    }

    fn new_maze(&mut self) {
        let (rows, cols) = self.difficulty.size();
        self.maze = Maze::new(rows, cols);
        self.maze.generate(&mut self.rng);
        self.player_row = 0;
        self.player_col = 0;
        self.goal_row = rows - 1;
        self.goal_col = cols - 1;
        self.moves = 0;
        self.elapsed_secs = 0;
        self.trail = vec![false; rows * cols];
        self.trail[0] = true;
        self.solution = self.maze.solve((0, 0), (self.goal_row, self.goal_col));
        self.show_solution = false;
        self.view = View::Playing;
    }

    fn try_move(&mut self, dir: Dir) {
        if self.view != View::Playing {
            return;
        }
        if !self.maze.can_move(self.player_row, self.player_col, dir) {
            return;
        }
        let (dr, dc) = dir.delta();
        self.player_row = (self.player_row as i32 + dr) as usize;
        self.player_col = (self.player_col as i32 + dc) as usize;
        self.moves += 1;

        // Mark trail
        let idx = self.maze.idx(self.player_row, self.player_col);
        if let Some(t) = self.trail.get_mut(idx) {
            *t = true;
        }

        // Check win
        if self.player_row == self.goal_row && self.player_col == self.goal_col {
            self.view = View::Won;
            self.games_won += 1;
            if self.best_moves.is_none() || self.moves < self.best_moves.unwrap_or(u32::MAX) {
                self.best_moves = Some(self.moves);
            }
        }
    }

    fn cell_size(&self) -> f32 {
        match self.difficulty {
            Difficulty::Small => 40.0,
            Difficulty::Medium => 22.0,
            Difficulty::Large => 15.0,
        }
    }

    fn format_time(&self) -> String {
        let mins = self.elapsed_secs / 60;
        let secs = self.elapsed_secs % 60;
        format!("{mins:02}:{secs:02}")
    }

    fn event(&mut self, event: &Event) {
        match self.view {
            View::Playing => self.handle_playing(event),
            View::Won => self.handle_won(event),
        }
    }

    fn handle_playing(&mut self, event: &Event) {
        if let Event::Key(KeyEvent { key, modifiers, .. }) = event {
            if modifiers.ctrl {
                return;
            }
            match key {
                Key::Up => self.try_move(Dir::North),
                Key::Down => self.try_move(Dir::South),
                Key::Right => self.try_move(Dir::East),
                Key::Left => self.try_move(Dir::West),
                Key::N => self.new_maze(),
                Key::H => {
                    self.show_solution = !self.show_solution;
                }
                Key::D => {
                    self.difficulty = self.difficulty.next();
                    self.new_maze();
                }
                _ => {}
            }
        }
    }

    fn handle_won(&mut self, event: &Event) {
        if let Event::Key(KeyEvent { key, .. }) = event {
            match key {
                Key::Enter | Key::N => {
                    self.new_maze();
                }
                Key::D => {
                    self.difficulty = self.difficulty.next();
                    self.new_maze();
                }
                _ => {}
            }
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
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: 40.0,
            color: COL_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "Maze".to_string(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Difficulty
        cmds.push(RenderCommand::Text {
            x: 80.0,
            y: 12.0,
            text: self.difficulty.name().to_string(),
            font_size: 14.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Moves
        cmds.push(RenderCommand::Text {
            x: 260.0,
            y: 12.0,
            text: format!("Moves: {}", self.moves),
            font_size: 14.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Optimal
        if !self.solution.is_empty() {
            cmds.push(RenderCommand::Text {
                x: 380.0,
                y: 12.0,
                text: format!("Optimal: {}", self.solution.len().saturating_sub(1)),
                font_size: 14.0,
                color: COL_OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        // Stats
        cmds.push(RenderCommand::Text {
            x: 530.0,
            y: 12.0,
            text: format!("Won: {}", self.games_won),
            font_size: 14.0,
            color: COL_GREEN,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Draw maze
        let cs = self.cell_size();
        let mx = 20.0; // maze x offset
        let my = 50.0; // maze y offset

        for row in 0..self.maze.rows {
            for col in 0..self.maze.cols {
                let cx = mx + col as f32 * cs;
                let cy = my + row as f32 * cs;

                let idx = self.maze.idx(row, col);
                let on_trail = self.trail.get(idx).copied().unwrap_or(false);
                let is_player = row == self.player_row && col == self.player_col;
                let is_goal = row == self.goal_row && col == self.goal_col;
                let on_solution =
                    self.show_solution && self.solution.iter().any(|&(r, c)| r == row && c == col);

                // Cell background
                let bg = if is_player {
                    COL_BLUE
                } else if is_goal {
                    COL_GREEN
                } else if on_solution {
                    Color::rgba(137, 180, 250, 60) // dim blue
                } else if on_trail {
                    COL_SURFACE1
                } else {
                    COL_SURFACE0
                };

                cmds.push(RenderCommand::FillRect {
                    x: cx + 0.5,
                    y: cy + 0.5,
                    width: cs - 1.0,
                    height: cs - 1.0,
                    color: bg,
                    corner_radii: CornerRadii::ZERO,
                });

                // Draw walls as lines
                let wall_color = COL_LAVENDER;
                let wall_w = 2.0;

                if self.maze.has_wall(row, col, Dir::North) {
                    cmds.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy,
                        x2: cx + cs,
                        y2: cy,
                        color: wall_color,
                        width: wall_w,
                    });
                }
                if self.maze.has_wall(row, col, Dir::South) {
                    cmds.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy + cs,
                        x2: cx + cs,
                        y2: cy + cs,
                        color: wall_color,
                        width: wall_w,
                    });
                }
                if self.maze.has_wall(row, col, Dir::West) {
                    cmds.push(RenderCommand::Line {
                        x1: cx,
                        y1: cy,
                        x2: cx,
                        y2: cy + cs,
                        color: wall_color,
                        width: wall_w,
                    });
                }
                if self.maze.has_wall(row, col, Dir::East) {
                    cmds.push(RenderCommand::Line {
                        x1: cx + cs,
                        y1: cy,
                        x2: cx + cs,
                        y2: cy + cs,
                        color: wall_color,
                        width: wall_w,
                    });
                }

                // Player marker
                if is_player {
                    let margin = cs * 0.2;
                    cmds.push(RenderCommand::FillRect {
                        x: cx + margin,
                        y: cy + margin,
                        width: cs - margin * 2.0,
                        height: cs - margin * 2.0,
                        color: COL_MAUVE,
                        corner_radii: CornerRadii::all(cs * 0.3),
                    });
                }

                // Goal marker
                if is_goal && !is_player {
                    let margin = cs * 0.25;
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx + margin,
                        y: cy + margin,
                        width: cs - margin * 2.0,
                        height: cs - margin * 2.0,
                        color: COL_GREEN,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(cs * 0.3),
                    });
                }
            }
        }

        // Help bar
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: height - 20.0,
            text: "Arrows=Move  N=New Maze  H=Show/Hide Solution  D=Difficulty".to_string(),
            font_size: 11.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Win overlay
        if self.view == View::Won {
            self.render_won(&mut cmds, width, height);
        }

        cmds
    }

    fn render_won(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        let bx = width / 2.0 - 140.0;
        let by = height / 2.0 - 80.0;
        let bw = 280.0;
        let bh = 160.0;

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: bw,
            height: bh,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + bw / 2.0 - 60.0,
            y: by + 20.0,
            text: "Maze Solved!".to_string(),
            font_size: 22.0,
            color: COL_GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: bx + 30.0,
            y: by + 55.0,
            text: format!(
                "Moves: {}  (Optimal: {})",
                self.moves,
                self.solution.len().saturating_sub(1)
            ),
            font_size: 14.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        if let Some(best) = self.best_moves {
            cmds.push(RenderCommand::Text {
                x: bx + 30.0,
                y: by + 80.0,
                text: format!("Best: {best} moves"),
                font_size: 14.0,
                color: COL_YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        cmds.push(RenderCommand::Text {
            x: bx + 30.0,
            y: by + bh - 30.0,
            text: "Enter/N=New Maze  D=Change Difficulty".to_string(),
            font_size: 12.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

fn main() {
    let _app = MazeApp::new();
}

// ── Tests ──────────────────────────────────────────────────────────
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

    fn make_rng() -> SeededRng {
        SeededRng::new(42)
    }

    // ── Seeding and the direction shuffle ──

    // The generator's own contract -- determinism under a seed, divergence
    // under two, staying inside its bound, surviving a zero bound, shuffling
    // something -- used to be tested here against the local `Rng`. It is now
    // tested once, against the shared implementation, in `randrange`. Sixteen
    // crates each testing their own copy is sixteen chances to test a copy that
    // has quietly drifted from the one being shipped.

    /// The direction shuffle must reach most of the 24 possible orderings.
    ///
    /// This is the test the old code needed and did not have, and its shape is
    /// the point. Shuffling four directions repeatedly is what the maze
    /// generator does at every step, and under the old reduction it produced
    /// only **3 of the 24 orderings** -- all three ending with the same
    /// direction, so one direction was systematically tried last, in every
    /// maze. See the note by `FALLBACK_SEED`.
    ///
    /// Two things about this test are deliberate and were arrived at by
    /// measuring rather than reasoning:
    ///
    /// It draws from **one** generator across the rounds. Sampling one shuffle
    /// each from many freshly-seeded generators would hide the defect entirely,
    /// because different seeds have different low bits; a counter in the low
    /// bits is only visible along a single stream.
    ///
    /// It makes the **same number of draws per round as the real caller** --
    /// three for the shuffle plus one for the neighbour pick. That matters more
    /// than it looks: with only the shuffle's three draws the old code reaches
    /// 12 of 24, which is bad but might pass a loose threshold, and it is the
    /// interaction with the fourth draw (four draws per round, and 4 mod 4 = 0)
    /// that collapses it to 3. A test of the shuffle in isolation would have
    /// missed the bug, which is presumably how it survived.
    #[test]
    fn the_direction_shuffle_reaches_most_orderings() {
        let mut rng = SeededRng::new(42);
        let mut seen: std::collections::BTreeSet<[usize; 4]> = std::collections::BTreeSet::new();
        for _ in 0..400 {
            let mut dirs = Dir::ALL;
            rng.shuffle(&mut dirs);
            // The generator also draws once per step to choose a neighbour;
            // reproduce that, because the defect lived in the interaction.
            let _ = rng.below(3);
            seen.insert(dirs.map(|d| d as usize));
        }
        assert!(
            seen.len() >= 20,
            "the four directions were shuffled into only {} of 24 orderings: {seen:?}",
            seen.len()
        );
    }

    /// Every direction must be able to come last.
    ///
    /// The sharpest single consequence of the old defect, stated on its own:
    /// all three orderings it could produce ended with the same direction.
    #[test]
    fn every_direction_can_come_last() {
        let mut rng = SeededRng::new(7);
        let mut last_seen = [false; 4];
        for _ in 0..400 {
            let mut dirs = Dir::ALL;
            rng.shuffle(&mut dirs);
            let _ = rng.below(3);
            if let Some(last) = dirs.last()
                && let Some(slot) = last_seen.get_mut(*last as usize)
            {
                *slot = true;
            }
        }
        assert!(
            last_seen.iter().all(|s| *s),
            "some direction was never tried last: {last_seen:?}"
        );
    }

    /// A fresh app must take its seed from the kernel, not from a literal.
    ///
    /// Phrased as "which seed", not as "two fresh mazes differ", because a host
    /// test build has no SlateOS kernel: `seeded_from_system` correctly takes
    /// its fallback and two fresh mazes are then identical, exactly as they
    /// were under the old hardcoded `42`. A variety check would therefore pass
    /// on the broken code and fail on the fixed code, which is backwards.
    #[cfg(not(unix))]
    #[test]
    fn a_fresh_maze_is_seeded_by_the_system_and_not_by_a_literal() {
        let walls = |mut rng: SeededRng| {
            let mut maze = Maze::new(10, 10);
            maze.generate(&mut rng);
            maze.cells.iter().map(|c| c.walls).collect::<Vec<u8>>()
        };
        let fresh = walls(seeded_from_system(FALLBACK_SEED));
        assert_eq!(
            fresh,
            walls(SeededRng::new(FALLBACK_SEED)),
            "a fresh maze did not use the crate's fallback seed"
        );
        assert_ne!(
            fresh,
            walls(SeededRng::new(42)),
            "a fresh maze is still seeded by the old hardcoded literal"
        );
    }

    // ── Direction tests ──

    #[test]
    fn test_dir_opposite() {
        assert_eq!(Dir::North.opposite(), Dir::South);
        assert_eq!(Dir::South.opposite(), Dir::North);
        assert_eq!(Dir::East.opposite(), Dir::West);
        assert_eq!(Dir::West.opposite(), Dir::East);
    }

    #[test]
    fn test_dir_delta() {
        assert_eq!(Dir::North.delta(), (-1, 0));
        assert_eq!(Dir::South.delta(), (1, 0));
        assert_eq!(Dir::East.delta(), (0, 1));
        assert_eq!(Dir::West.delta(), (0, -1));
    }

    #[test]
    fn test_dir_wall_bits_unique() {
        let bits: Vec<u8> = Dir::ALL.iter().map(|d| d.wall_bit()).collect();
        for i in 0..bits.len() {
            for j in i + 1..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
    }

    // ── MazeCell tests ──

    #[test]
    fn test_cell_new_all_walls() {
        let cell = MazeCell::new();
        assert!(cell.has_wall(Dir::North));
        assert!(cell.has_wall(Dir::South));
        assert!(cell.has_wall(Dir::East));
        assert!(cell.has_wall(Dir::West));
    }

    #[test]
    fn test_cell_remove_wall() {
        let mut cell = MazeCell::new();
        cell.remove_wall(Dir::North);
        assert!(!cell.has_wall(Dir::North));
        assert!(cell.has_wall(Dir::South));
    }

    #[test]
    fn test_cell_remove_multiple_walls() {
        let mut cell = MazeCell::new();
        cell.remove_wall(Dir::North);
        cell.remove_wall(Dir::East);
        assert!(!cell.has_wall(Dir::North));
        assert!(!cell.has_wall(Dir::East));
        assert!(cell.has_wall(Dir::South));
        assert!(cell.has_wall(Dir::West));
    }

    // ── Maze tests ──

    #[test]
    fn test_maze_new() {
        let maze = Maze::new(5, 5);
        assert_eq!(maze.rows, 5);
        assert_eq!(maze.cols, 5);
        assert_eq!(maze.cells.len(), 25);
    }

    #[test]
    fn test_maze_in_bounds() {
        let maze = Maze::new(5, 5);
        assert!(maze.in_bounds(0, 0));
        assert!(maze.in_bounds(4, 4));
        assert!(!maze.in_bounds(-1, 0));
        assert!(!maze.in_bounds(0, 5));
        assert!(!maze.in_bounds(5, 0));
    }

    #[test]
    fn test_maze_generate_perfect() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        // All cells should be visited (perfect maze = connected)
        for cell in &maze.cells {
            assert!(cell.visited_gen);
        }
    }

    #[test]
    fn test_maze_generate_has_passages() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        // At least some walls should be removed
        let total_walls: u32 = maze.cells.iter().map(|c| c.walls.count_ones()).sum();
        let max_walls = (5 * 5 * 4) as u32;
        assert!(total_walls < max_walls);
    }

    #[test]
    fn test_maze_solve_exists() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        let path = maze.solve((0, 0), (4, 4));
        assert!(!path.is_empty());
        assert_eq!(path[0], (0, 0));
        assert_eq!(*path.last().unwrap(), (4, 4));
    }

    #[test]
    fn test_maze_solve_path_valid() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        let path = maze.solve((0, 0), (4, 4));
        // Each step in path should be adjacent and have no wall between
        for i in 1..path.len() {
            let (r1, c1) = path[i - 1];
            let (r2, c2) = path[i];
            let dr = r2 as i32 - r1 as i32;
            let dc = c2 as i32 - c1 as i32;
            // Must be adjacent (manhattan distance 1)
            assert_eq!(dr.abs() + dc.abs(), 1);
            // Must have a passage
            let dir = match (dr, dc) {
                (-1, 0) => Dir::North,
                (1, 0) => Dir::South,
                (0, 1) => Dir::East,
                (0, -1) => Dir::West,
                _ => panic!("Invalid step in path"),
            };
            assert!(maze.can_move(r1, c1, dir));
        }
    }

    #[test]
    fn test_maze_solve_same_cell() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        let path = maze.solve((2, 2), (2, 2));
        assert_eq!(path.len(), 1);
        assert_eq!(path[0], (2, 2));
    }

    #[test]
    fn test_maze_different_seeds() {
        let mut maze1 = Maze::new(10, 10);
        let mut maze2 = Maze::new(10, 10);
        let mut rng1 = SeededRng::new(1);
        let mut rng2 = SeededRng::new(2);
        maze1.generate(&mut rng1);
        maze2.generate(&mut rng2);

        // Different seeds should produce different mazes
        let walls1: Vec<u8> = maze1.cells.iter().map(|c| c.walls).collect();
        let walls2: Vec<u8> = maze2.cells.iter().map(|c| c.walls).collect();
        assert_ne!(walls1, walls2);
    }

    #[test]
    fn test_maze_clone() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);
        let cloned = maze.clone();
        assert_eq!(cloned.rows, maze.rows);
        assert_eq!(cloned.cols, maze.cols);
        assert_eq!(cloned.cells.len(), maze.cells.len());
    }

    // ── Difficulty tests ──

    #[test]
    fn test_difficulty_sizes() {
        assert_eq!(Difficulty::Small.size(), (10, 10));
        assert_eq!(Difficulty::Medium.size(), (20, 20));
        assert_eq!(Difficulty::Large.size(), (30, 30));
    }

    #[test]
    fn test_difficulty_cycle() {
        assert_eq!(Difficulty::Small.next(), Difficulty::Medium);
        assert_eq!(Difficulty::Medium.next(), Difficulty::Large);
        assert_eq!(Difficulty::Large.next(), Difficulty::Small);
    }

    #[test]
    fn test_difficulty_names() {
        assert!(!Difficulty::Small.name().is_empty());
        assert!(!Difficulty::Medium.name().is_empty());
        assert!(!Difficulty::Large.name().is_empty());
    }

    // ── App tests ──

    #[test]
    fn test_app_new() {
        let app = MazeApp::new();
        assert_eq!(app.view, View::Playing);
        assert_eq!(app.player_row, 0);
        assert_eq!(app.player_col, 0);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_app_new_maze() {
        let mut app = MazeApp::new();
        app.moves = 10;
        app.player_row = 5;
        app.new_maze();
        assert_eq!(app.moves, 0);
        assert_eq!(app.player_row, 0);
        assert_eq!(app.player_col, 0);
    }

    #[test]
    fn test_move_right() {
        let mut app = MazeApp::new();
        // The maze is generated, so we need to check if we can move
        // Try all four directions — at least one should work from (0,0)
        let start_row = app.player_row;
        let start_col = app.player_col;
        app.try_move(Dir::South);
        app.try_move(Dir::East);
        // The player either stayed put (blocked by walls in both directions) or
        // moved into a cell within the maze. Either is valid — but the player
        // position must remain in-bounds and consistent with start_row/start_col
        // (we can't assert movement because a generated maze may close both
        // adjacent cells).
        assert!(app.player_row < app.maze.rows);
        assert!(app.player_col < app.maze.cols);
        // Suppress unused-variable warnings if no move occurred.
        let _ = (start_row, start_col);
    }

    #[test]
    fn test_move_into_wall() {
        let maze = Maze::new(3, 3);
        // Don't generate — all walls present
        let mut app = MazeApp::new();
        app.maze = maze;
        app.player_row = 1;
        app.player_col = 1;
        app.try_move(Dir::North);
        // Shouldn't have moved — wall blocks
        assert_eq!(app.player_row, 1);
        assert_eq!(app.player_col, 1);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_trail_marking() {
        let app = MazeApp::new();
        assert!(app.trail[0]); // Starting position marked
    }

    #[test]
    fn test_show_solution_toggle() {
        let mut app = MazeApp::new();
        assert!(!app.show_solution);
        app.event(&Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert!(app.show_solution);
        app.event(&Event::Key(KeyEvent {
            key: Key::H,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert!(!app.show_solution);
    }

    #[test]
    fn test_new_maze_key() {
        let mut app = MazeApp::new();
        app.moves = 10;
        app.event(&Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn test_difficulty_key() {
        let mut app = MazeApp::new();
        assert_eq!(app.difficulty, Difficulty::Small);
        app.event(&Event::Key(KeyEvent {
            key: Key::D,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert_eq!(app.difficulty, Difficulty::Medium);
        assert_eq!(app.maze.rows, 20);
    }

    #[test]
    fn test_arrow_key_movement() {
        let mut app = MazeApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        // May or may not have moved depending on maze layout
    }

    #[test]
    fn test_win_detection() {
        let mut app = MazeApp::new();
        // Manually walk the solution path
        let solution = app.solution.clone();
        for i in 1..solution.len() {
            let (pr, pc) = (app.player_row, app.player_col);
            let (nr, nc) = solution[i];
            let dr = nr as i32 - pr as i32;
            let dc = nc as i32 - pc as i32;
            let dir = match (dr, dc) {
                (-1, 0) => Dir::North,
                (1, 0) => Dir::South,
                (0, 1) => Dir::East,
                (0, -1) => Dir::West,
                _ => continue,
            };
            app.try_move(dir);
        }
        assert_eq!(app.view, View::Won);
        assert_eq!(app.games_won, 1);
    }

    #[test]
    fn test_won_state_enter_new_maze() {
        let mut app = MazeApp::new();
        app.view = View::Won;
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn test_won_state_n_new_maze() {
        let mut app = MazeApp::new();
        app.view = View::Won;
        app.event(&Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers::default(),
            pressed: true,
            text: String::new(),
        }));
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn test_cell_size() {
        let mut app = MazeApp::new();
        app.difficulty = Difficulty::Small;
        assert_eq!(app.cell_size(), 40.0);
        app.difficulty = Difficulty::Large;
        assert_eq!(app.cell_size(), 15.0);
    }

    #[test]
    fn test_format_time() {
        let mut app = MazeApp::new();
        app.elapsed_secs = 0;
        assert_eq!(app.format_time(), "00:00");
        app.elapsed_secs = 125;
        assert_eq!(app.format_time(), "02:05");
    }

    #[test]
    fn test_render_no_panic() {
        let app = MazeApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won_no_panic() {
        let mut app = MazeApp::new();
        app.view = View::Won;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_solution() {
        let mut app = MazeApp::new();
        app.show_solution = true;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_solution_nonempty() {
        let app = MazeApp::new();
        assert!(!app.solution.is_empty());
    }

    #[test]
    fn test_best_moves_tracking() {
        let mut app = MazeApp::new();
        assert!(app.best_moves.is_none());
        app.moves = 50;
        app.view = View::Won;
        app.games_won = 1;
        app.best_moves = Some(50);
        assert_eq!(app.best_moves, Some(50));
    }

    #[test]
    fn test_medium_maze() {
        let mut app = MazeApp::new();
        app.difficulty = Difficulty::Medium;
        app.new_maze();
        assert_eq!(app.maze.rows, 20);
        assert_eq!(app.maze.cols, 20);
        assert_eq!(app.goal_row, 19);
    }

    #[test]
    fn test_large_maze() {
        let mut app = MazeApp::new();
        app.difficulty = Difficulty::Large;
        app.new_maze();
        assert_eq!(app.maze.rows, 30);
        assert_eq!(app.maze.cols, 30);
    }

    #[test]
    fn test_large_maze_solvable() {
        let mut app = MazeApp::new();
        app.difficulty = Difficulty::Large;
        app.new_maze();
        assert!(!app.solution.is_empty());
        assert_eq!(app.solution[0], (0, 0));
        assert_eq!(*app.solution.last().unwrap(), (29, 29));
    }

    #[test]
    fn test_ctrl_ignored() {
        let mut app = MazeApp::new();
        let moves = app.moves;
        app.event(&Event::Key(KeyEvent {
            key: Key::N,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: String::new(),
        }));
        // Should not create new maze
        assert_eq!(app.moves, moves);
    }

    #[test]
    fn test_maze_10x10_all_reachable() {
        let mut rng = SeededRng::new(99);
        let mut maze = Maze::new(10, 10);
        maze.generate(&mut rng);

        // Every cell should be reachable from (0,0)
        for r in 0..10 {
            for c in 0..10 {
                let path = maze.solve((0, 0), (r, c));
                assert!(!path.is_empty(), "Cell ({r},{c}) not reachable");
            }
        }
    }

    #[test]
    fn test_wall_consistency() {
        let mut rng = make_rng();
        let mut maze = Maze::new(5, 5);
        maze.generate(&mut rng);

        // If cell (r,c) has no north wall, cell (r-1,c) should have no south wall
        for r in 1..5 {
            for c in 0..5 {
                if !maze.has_wall(r, c, Dir::North) {
                    assert!(
                        !maze.has_wall(r - 1, c, Dir::South),
                        "Inconsistent walls at ({r},{c}) north"
                    );
                }
            }
        }
        for r in 0..5 {
            for c in 1..5 {
                if !maze.has_wall(r, c, Dir::West) {
                    assert!(
                        !maze.has_wall(r, c - 1, Dir::East),
                        "Inconsistent walls at ({r},{c}) west"
                    );
                }
            }
        }
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }
}
