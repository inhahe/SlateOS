//! OurOS Pipes Puzzle
//!
//! A pipe-connection puzzle game where the player rotates pipe segments
//! to connect a water source to a drain, forming a continuous path.

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

const COL_BASE: u32 = 0x1E1E2E;
const COL_MANTLE: u32 = 0x181825;
const COL_SURFACE0: u32 = 0x313244;
const COL_SURFACE1: u32 = 0x45475A;
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
// Direction — the four cardinal openings a pipe can have
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Right => Self::Left,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
        }
    }

    fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (-1, 0),
            Self::Right => (0, 1),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
        }
    }

    fn rotate_cw(self) -> Self {
        match self {
            Self::Up => Self::Right,
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
        }
    }
}

// ---------------------------------------------------------------------------
// Pipe types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipeKind {
    Straight, // two opposite openings
    Corner,   // two adjacent openings (L-shaped)
    Tee,      // three openings (T-shaped)
    Cross,    // four openings (+ shaped)
    End,      // one opening (dead end / source/drain)
    Empty,    // no pipe
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pipe {
    kind: PipeKind,
    rotation: u8, // 0-3 (× 90°)
}

impl Pipe {
    fn new(kind: PipeKind, rotation: u8) -> Self {
        Self {
            kind,
            rotation: rotation % 4,
        }
    }

    fn openings(self) -> Vec<Dir> {
        let base: Vec<Dir> = match self.kind {
            PipeKind::Straight => vec![Dir::Up, Dir::Down],
            PipeKind::Corner => vec![Dir::Up, Dir::Right],
            PipeKind::Tee => vec![Dir::Up, Dir::Right, Dir::Down],
            PipeKind::Cross => vec![Dir::Up, Dir::Right, Dir::Down, Dir::Left],
            PipeKind::End => vec![Dir::Up],
            PipeKind::Empty => vec![],
        };
        base.into_iter()
            .map(|d| {
                let mut dir = d;
                for _ in 0..self.rotation {
                    dir = dir.rotate_cw();
                }
                dir
            })
            .collect()
    }

    fn has_opening(self, dir: Dir) -> bool {
        self.openings().contains(&dir)
    }

    fn rotate_cw(&mut self) {
        self.rotation = (self.rotation + 1) % 4;
    }

    fn rotate_ccw(&mut self) {
        self.rotation = (self.rotation + 3) % 4;
    }
}

// ---------------------------------------------------------------------------
// LCG
// ---------------------------------------------------------------------------

struct Lcg {
    state: u64,
}

impl Lcg {
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
        (self.next() >> 33) as usize % max
    }
}

// ---------------------------------------------------------------------------
// Difficulty
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    fn grid_size(self) -> (usize, usize) {
        match self {
            Self::Easy => (5, 5),
            Self::Medium => (7, 7),
            Self::Hard => (9, 9),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Easy => "Easy",
            Self::Medium => "Medium",
            Self::Hard => "Hard",
        }
    }
}

// ---------------------------------------------------------------------------
// Game board
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Board {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<Pipe>>,
    source: (usize, usize),
    drain: (usize, usize),
}

impl Board {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![vec![Pipe::new(PipeKind::Empty, 0); cols]; rows],
            source: (0, 0),
            drain: (rows.saturating_sub(1), cols.saturating_sub(1)),
        }
    }

    fn get(&self, r: usize, c: usize) -> Option<&Pipe> {
        self.cells.get(r).and_then(|row| row.get(c))
    }

    fn get_mut(&mut self, r: usize, c: usize) -> Option<&mut Pipe> {
        self.cells.get_mut(r).and_then(|row| row.get_mut(c))
    }

    fn in_bounds(&self, r: i32, c: i32) -> bool {
        r >= 0 && c >= 0 && (r as usize) < self.rows && (c as usize) < self.cols
    }

    /// Check if two adjacent cells are connected (both have matching openings).
    fn connected(&self, r1: usize, c1: usize, r2: usize, c2: usize) -> bool {
        let dr = r2 as i32 - r1 as i32;
        let dc = c2 as i32 - c1 as i32;
        let dir = match (dr, dc) {
            (-1, 0) => Dir::Up,
            (1, 0) => Dir::Down,
            (0, -1) => Dir::Left,
            (0, 1) => Dir::Right,
            _ => return false,
        };
        if let (Some(p1), Some(p2)) = (self.get(r1, c1), self.get(r2, c2)) {
            p1.has_opening(dir) && p2.has_opening(dir.opposite())
        } else {
            false
        }
    }

    /// BFS from source, returning all reachable cells via valid connections.
    fn flood_fill(&self) -> Vec<Vec<bool>> {
        let mut visited = vec![vec![false; self.cols]; self.rows];
        let mut queue = std::collections::VecDeque::new();
        let (sr, sc) = self.source;
        visited[sr][sc] = true;
        queue.push_back((sr, sc));

        while let Some((r, c)) = queue.pop_front() {
            if let Some(pipe) = self.get(r, c) {
                for dir in pipe.openings() {
                    let (dr, dc) = dir.delta();
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if self.in_bounds(nr, nc) {
                        let nur = nr as usize;
                        let nuc = nc as usize;
                        if !visited[nur][nuc] && self.connected(r, c, nur, nuc) {
                            visited[nur][nuc] = true;
                            queue.push_back((nur, nuc));
                        }
                    }
                }
            }
        }

        visited
    }

    /// Check if source and drain are connected.
    fn is_solved(&self) -> bool {
        let fill = self.flood_fill();
        let (dr, dc) = self.drain;
        fill[dr][dc]
    }

    /// Generate a solvable puzzle: lay a path from source to drain, place pipes,
    /// then randomize rotations.
    fn generate(difficulty: Difficulty, rng: &mut Lcg) -> Self {
        let (rows, cols) = difficulty.grid_size();
        let mut board = Board::new(rows, cols);

        // Random walk from source to drain to create a path
        let path = Self::random_path(rows, cols, board.source, board.drain, rng);

        // Place pipes along the path
        for (idx, &(r, c)) in path.iter().enumerate() {
            let prev_dir = if idx > 0 {
                let (pr, pc) = path[idx - 1];
                Some(Self::dir_from(pr, pc, r, c))
            } else {
                None
            };
            let next_dir = if idx + 1 < path.len() {
                let (nr, nc) = path[idx + 1];
                Some(Self::dir_from(r, c, nr, nc))
            } else {
                None
            };

            let openings: Vec<Dir> = [prev_dir, next_dir].iter().filter_map(|d| *d).collect();

            let (kind, rotation) = Self::pipe_for_openings(&openings);
            board.cells[r][c] = Pipe::new(kind, rotation);
        }

        // Fill remaining cells with random pipes
        for r in 0..rows {
            for c in 0..cols {
                if board.cells[r][c].kind == PipeKind::Empty {
                    let kind = match rng.next_range(5) {
                        0 => PipeKind::Straight,
                        1 => PipeKind::Corner,
                        2 => PipeKind::Tee,
                        3 => PipeKind::End,
                        _ => PipeKind::Straight,
                    };
                    let rot = rng.next_range(4) as u8;
                    board.cells[r][c] = Pipe::new(kind, rot);
                }
            }
        }

        // Scramble all rotations (except keep a solvable state possible)
        for r in 0..rows {
            for c in 0..cols {
                let rotations = rng.next_range(4) as u8;
                for _ in 0..rotations {
                    board.cells[r][c].rotate_cw();
                }
            }
        }

        board
    }

    fn dir_from(from_r: usize, from_c: usize, to_r: usize, to_c: usize) -> Dir {
        let dr = to_r as i32 - from_r as i32;
        let dc = to_c as i32 - from_c as i32;
        match (dr, dc) {
            (-1, 0) => Dir::Up,
            (1, 0) => Dir::Down,
            (0, -1) => Dir::Left,
            (0, 1) => Dir::Right,
            _ => Dir::Right, // fallback
        }
    }

    fn pipe_for_openings(openings: &[Dir]) -> (PipeKind, u8) {
        match openings.len() {
            0 => (PipeKind::End, 0),
            1 => {
                let rot = match openings[0] {
                    Dir::Up => 0,
                    Dir::Right => 1,
                    Dir::Down => 2,
                    Dir::Left => 3,
                };
                (PipeKind::End, rot)
            }
            2 => {
                let a = openings[0];
                let b = openings[1];
                // Check if opposite (straight) or adjacent (corner)
                if a.opposite() == b {
                    let rot = match a {
                        Dir::Up | Dir::Down => 0,
                        Dir::Left | Dir::Right => 1,
                    };
                    (PipeKind::Straight, rot)
                } else {
                    // Corner: find the rotation that gives these two openings
                    for rot in 0..4 {
                        let p = Pipe::new(PipeKind::Corner, rot);
                        let o = p.openings();
                        if o.contains(&a) && o.contains(&b) {
                            return (PipeKind::Corner, rot);
                        }
                    }
                    (PipeKind::Corner, 0)
                }
            }
            _ => (PipeKind::Tee, 0),
        }
    }

    fn random_path(
        rows: usize,
        cols: usize,
        start: (usize, usize),
        end: (usize, usize),
        rng: &mut Lcg,
    ) -> Vec<(usize, usize)> {
        let mut visited = vec![vec![false; cols]; rows];
        let mut path = vec![start];
        visited[start.0][start.1] = true;

        let dirs = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

        loop {
            let &(r, c) = path.last().unwrap_or(&start);
            if (r, c) == end {
                break;
            }

            // Find unvisited neighbors
            let mut neighbors = Vec::new();
            for &d in &dirs {
                let (dr, dc) = d.delta();
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr >= 0 && nc >= 0 && (nr as usize) < rows && (nc as usize) < cols {
                    let nur = nr as usize;
                    let nuc = nc as usize;
                    if !visited[nur][nuc] {
                        neighbors.push((nur, nuc));
                    }
                }
            }

            if neighbors.is_empty() {
                // Backtrack
                path.pop();
                if path.is_empty() {
                    // Shouldn't happen on a connected grid, but safety
                    path.push(start);
                    break;
                }
                continue;
            }

            // Bias toward the target
            neighbors.sort_by_key(|&(nr, nc)| {
                let dr = (nr as i32 - end.0 as i32).unsigned_abs();
                let dc = (nc as i32 - end.1 as i32).unsigned_abs();
                dr + dc
            });

            // Pick with bias toward closer cells but some randomness
            let idx = if rng.next_range(3) == 0 {
                rng.next_range(neighbors.len())
            } else {
                0 // closest to target
            };

            let next = neighbors[idx];
            visited[next.0][next.1] = true;
            path.push(next);
        }

        path
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct PipesApp {
    board: Board,
    cursor_r: usize,
    cursor_c: usize,
    difficulty: Difficulty,
    moves: u32,
    solved: bool,
    rng: Lcg,
    games_won: u32,
    show_flow: bool,
}

impl PipesApp {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut rng = Lcg::new(seed);
        let difficulty = Difficulty::Easy;
        let board = Board::generate(difficulty, &mut rng);
        Self {
            board,
            cursor_r: 0,
            cursor_c: 0,
            difficulty,
            moves: 0,
            solved: false,
            rng,
            games_won: 0,
            show_flow: true,
        }
    }

    fn new_game(&mut self) {
        self.board = Board::generate(self.difficulty, &mut self.rng);
        self.cursor_r = 0;
        self.cursor_c = 0;
        self.moves = 0;
        self.solved = false;
    }

    fn rotate_current_cw(&mut self) {
        if self.solved {
            return;
        }
        if let Some(pipe) = self.board.get_mut(self.cursor_r, self.cursor_c) {
            pipe.rotate_cw();
            self.moves = self.moves.saturating_add(1);
            self.check_solved();
        }
    }

    fn rotate_current_ccw(&mut self) {
        if self.solved {
            return;
        }
        if let Some(pipe) = self.board.get_mut(self.cursor_r, self.cursor_c) {
            pipe.rotate_ccw();
            self.moves = self.moves.saturating_add(1);
            self.check_solved();
        }
    }

    fn check_solved(&mut self) {
        if self.board.is_solved() {
            self.solved = true;
            self.games_won = self.games_won.saturating_add(1);
        }
    }

    fn set_difficulty(&mut self, diff: Difficulty) {
        self.difficulty = diff;
        self.new_game();
    }

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.key {
            Key::Up
                if self.cursor_r > 0 => {
                    self.cursor_r -= 1;
                }
            Key::Down
                if self.cursor_r + 1 < self.board.rows => {
                    self.cursor_r += 1;
                }
            Key::Left
                if self.cursor_c > 0 => {
                    self.cursor_c -= 1;
                }
            Key::Right
                if self.cursor_c + 1 < self.board.cols => {
                    self.cursor_c += 1;
                }
            Key::Space | Key::Enter => {
                self.rotate_current_cw();
            }
            Key::Z => {
                self.rotate_current_ccw();
            }
            Key::N => {
                self.new_game();
            }
            Key::F => {
                self.show_flow = !self.show_flow;
            }
            Key::Num1 => self.set_difficulty(Difficulty::Easy),
            Key::Num2 => self.set_difficulty(Difficulty::Medium),
            Key::Num3 => self.set_difficulty(Difficulty::Hard),
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event {
            self.handle_key(ke);
        }
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

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
            x: 20.0,
            y: 15.0,
            text: String::from("Pipes"),
            color: Color::from_hex(COL_BLUE),
            font_size: 28.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status
        let status = if self.solved {
            "SOLVED!"
        } else {
            self.difficulty.name()
        };
        let status_color = if self.solved { COL_GREEN } else { COL_SUBTEXT0 };
        cmds.push(RenderCommand::Text {
            x: 130.0,
            y: 22.0,
            text: String::from(status),
            color: Color::from_hex(status_color),
            font_size: 16.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Stats
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: 50.0,
            text: format!(
                "Moves: {}  |  Won: {}  |  Grid: {}x{}",
                self.moves, self.games_won, self.board.rows, self.board.cols
            ),
            color: Color::from_hex(COL_SUBTEXT0),
            font_size: 13.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Controls
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: 70.0,
            text: String::from(
                "Arrows: Move  |  Space: Rotate CW  |  Z: CCW  |  N: New  |  1-3: Difficulty",
            ),
            color: Color::from_hex(COL_OVERLAY0),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Board
        let cell_size = 50.0_f32.min(400.0 / self.board.cols as f32);
        let board_x = 20.0;
        let board_y = 95.0;
        let flow = if self.show_flow {
            self.board.flood_fill()
        } else {
            vec![vec![false; self.board.cols]; self.board.rows]
        };

        // r/c double as positions in (cx, cy) math AND as indices into the
        // flow grid; converting to iter().enumerate() would require nested
        // zips with little clarity gain.
        #[allow(clippy::needless_range_loop)]
        for r in 0..self.board.rows {
            for c in 0..self.board.cols {
                let cx = board_x + c as f32 * cell_size;
                let cy = board_y + r as f32 * cell_size;
                let is_cursor = r == self.cursor_r && c == self.cursor_c;
                let is_source = (r, c) == self.board.source;
                let is_drain = (r, c) == self.board.drain;
                let is_filled = flow[r][c];

                // Cell background
                let bg_color = if self.solved && is_filled {
                    COL_SURFACE1
                } else if is_source {
                    0x2A4A3A
                } else if is_drain {
                    0x4A2A3A
                } else {
                    COL_SURFACE0
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: cell_size - 2.0,
                    height: cell_size - 2.0,
                    color: Color::from_hex(bg_color),
                    corner_radii: CornerRadii::all(4.0),
                });

                // Cursor highlight
                if is_cursor {
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx,
                        y: cy,
                        width: cell_size - 2.0,
                        height: cell_size - 2.0,
                        color: Color::from_hex(COL_YELLOW),
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Draw pipe
                let pipe = self.board.cells[r][c];
                let pipe_color = if self.solved && is_filled {
                    COL_GREEN
                } else if is_filled {
                    COL_TEAL
                } else {
                    COL_OVERLAY0
                };
                let mid_x = cx + (cell_size - 2.0) / 2.0;
                let mid_y = cy + (cell_size - 2.0) / 2.0;
                let _half = (cell_size - 2.0) / 2.0;
                let pipe_w = 4.0;

                for dir in pipe.openings() {
                    let (ex, ey) = match dir {
                        Dir::Up => (mid_x, cy),
                        Dir::Down => (mid_x, cy + cell_size - 2.0),
                        Dir::Left => (cx, mid_y),
                        Dir::Right => (cx + cell_size - 2.0, mid_y),
                    };
                    cmds.push(RenderCommand::Line {
                        x1: mid_x,
                        y1: mid_y,
                        x2: ex,
                        y2: ey,
                        color: Color::from_hex(pipe_color),
                        width: pipe_w,
                    });
                }

                // Center dot for non-empty pipes
                if pipe.kind != PipeKind::Empty {
                    cmds.push(RenderCommand::FillRect {
                        x: mid_x - 4.0,
                        y: mid_y - 4.0,
                        width: 8.0,
                        height: 8.0,
                        color: Color::from_hex(pipe_color),
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Source/drain labels
                if is_source {
                    cmds.push(RenderCommand::Text {
                        x: cx + 2.0,
                        y: cy + 2.0,
                        text: String::from("S"),
                        color: Color::from_hex(COL_GREEN),
                        font_size: 12.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
                if is_drain {
                    cmds.push(RenderCommand::Text {
                        x: cx + 2.0,
                        y: cy + 2.0,
                        text: String::from("D"),
                        color: Color::from_hex(COL_RED),
                        font_size: 12.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }
        }

        // Victory message
        if self.solved {
            let msg_y = board_y + self.board.rows as f32 * cell_size + 15.0;
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: msg_y,
                text: format!("Congratulations! Solved in {} moves!", self.moves),
                color: Color::from_hex(COL_GREEN),
                font_size: 20.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: msg_y + 28.0,
                text: String::from("Press N for a new puzzle"),
                color: Color::from_hex(COL_SUBTEXT0),
                font_size: 14.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds
    }
}

fn main() {
    let _app = PipesApp::new();
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Dir ---

    #[test]
    fn dir_opposite() {
        assert_eq!(Dir::Up.opposite(), Dir::Down);
        assert_eq!(Dir::Down.opposite(), Dir::Up);
        assert_eq!(Dir::Left.opposite(), Dir::Right);
        assert_eq!(Dir::Right.opposite(), Dir::Left);
    }

    #[test]
    fn dir_delta() {
        assert_eq!(Dir::Up.delta(), (-1, 0));
        assert_eq!(Dir::Down.delta(), (1, 0));
        assert_eq!(Dir::Left.delta(), (0, -1));
        assert_eq!(Dir::Right.delta(), (0, 1));
    }

    #[test]
    fn dir_rotate_cw() {
        assert_eq!(Dir::Up.rotate_cw(), Dir::Right);
        assert_eq!(Dir::Right.rotate_cw(), Dir::Down);
        assert_eq!(Dir::Down.rotate_cw(), Dir::Left);
        assert_eq!(Dir::Left.rotate_cw(), Dir::Up);
    }

    // --- Pipe ---

    #[test]
    fn pipe_straight_openings() {
        let p = Pipe::new(PipeKind::Straight, 0);
        let o = p.openings();
        assert!(o.contains(&Dir::Up));
        assert!(o.contains(&Dir::Down));
        assert_eq!(o.len(), 2);
    }

    #[test]
    fn pipe_straight_rotated() {
        let p = Pipe::new(PipeKind::Straight, 1);
        let o = p.openings();
        assert!(o.contains(&Dir::Right));
        assert!(o.contains(&Dir::Left));
    }

    #[test]
    fn pipe_corner_openings() {
        let p = Pipe::new(PipeKind::Corner, 0);
        let o = p.openings();
        assert!(o.contains(&Dir::Up));
        assert!(o.contains(&Dir::Right));
        assert_eq!(o.len(), 2);
    }

    #[test]
    fn pipe_corner_rotated() {
        let p = Pipe::new(PipeKind::Corner, 1);
        let o = p.openings();
        assert!(o.contains(&Dir::Right));
        assert!(o.contains(&Dir::Down));
    }

    #[test]
    fn pipe_tee_openings() {
        let p = Pipe::new(PipeKind::Tee, 0);
        assert_eq!(p.openings().len(), 3);
    }

    #[test]
    fn pipe_cross_openings() {
        let p = Pipe::new(PipeKind::Cross, 0);
        assert_eq!(p.openings().len(), 4);
    }

    #[test]
    fn pipe_end_openings() {
        let p = Pipe::new(PipeKind::End, 0);
        assert_eq!(p.openings().len(), 1);
        assert!(p.has_opening(Dir::Up));
    }

    #[test]
    fn pipe_empty_openings() {
        let p = Pipe::new(PipeKind::Empty, 0);
        assert!(p.openings().is_empty());
    }

    #[test]
    fn pipe_rotate_cw() {
        let mut p = Pipe::new(PipeKind::Straight, 0);
        p.rotate_cw();
        assert_eq!(p.rotation, 1);
        assert!(p.has_opening(Dir::Right));
        assert!(p.has_opening(Dir::Left));
    }

    #[test]
    fn pipe_rotate_ccw() {
        let mut p = Pipe::new(PipeKind::Straight, 0);
        p.rotate_ccw();
        assert_eq!(p.rotation, 3);
    }

    #[test]
    fn pipe_rotation_wraps() {
        let mut p = Pipe::new(PipeKind::Straight, 3);
        p.rotate_cw();
        assert_eq!(p.rotation, 0);
    }

    #[test]
    fn pipe_has_opening() {
        let p = Pipe::new(PipeKind::Straight, 0);
        assert!(p.has_opening(Dir::Up));
        assert!(!p.has_opening(Dir::Right));
    }

    // --- Board ---

    #[test]
    fn board_new() {
        let board = Board::new(5, 5);
        assert_eq!(board.rows, 5);
        assert_eq!(board.cols, 5);
        assert_eq!(board.source, (0, 0));
        assert_eq!(board.drain, (4, 4));
    }

    #[test]
    fn board_get() {
        let board = Board::new(3, 3);
        assert!(board.get(0, 0).is_some());
        assert!(board.get(5, 5).is_none());
    }

    #[test]
    fn board_in_bounds() {
        let board = Board::new(5, 5);
        assert!(board.in_bounds(0, 0));
        assert!(board.in_bounds(4, 4));
        assert!(!board.in_bounds(-1, 0));
        assert!(!board.in_bounds(5, 0));
    }

    #[test]
    fn board_connected_matching() {
        let mut board = Board::new(3, 3);
        board.cells[0][0] = Pipe::new(PipeKind::Straight, 1); // Left-Right
        board.cells[0][1] = Pipe::new(PipeKind::Straight, 1); // Left-Right
        assert!(board.connected(0, 0, 0, 1));
    }

    #[test]
    fn board_not_connected() {
        let mut board = Board::new(3, 3);
        board.cells[0][0] = Pipe::new(PipeKind::Straight, 0); // Up-Down
        board.cells[0][1] = Pipe::new(PipeKind::Straight, 0); // Up-Down
        assert!(!board.connected(0, 0, 0, 1)); // Neither has Left/Right
    }

    #[test]
    fn board_flood_fill_empty() {
        let board = Board::new(3, 3);
        let fill = board.flood_fill();
        // Empty pipes have no openings, so only source is reachable
        assert!(fill[0][0]);
        assert!(!fill[2][2]);
    }

    #[test]
    fn board_is_solved_connected() {
        let mut board = Board::new(2, 2);
        // Source (0,0) -> (0,1) -> (1,1) drain
        board.cells[0][0] = Pipe::new(PipeKind::Corner, 1); // Right, Down
        board.cells[0][1] = Pipe::new(PipeKind::Corner, 2); // Down, Left
        board.cells[1][1] = Pipe::new(PipeKind::End, 0); // Up
        board.drain = (1, 1);
        assert!(board.is_solved());
    }

    #[test]
    fn board_generate() {
        let mut rng = Lcg::new(42);
        let board = Board::generate(Difficulty::Easy, &mut rng);
        assert_eq!(board.rows, 5);
        assert_eq!(board.cols, 5);
    }

    #[test]
    fn board_generate_different_seeds() {
        let mut rng1 = Lcg::new(1);
        let mut rng2 = Lcg::new(2);
        let b1 = Board::generate(Difficulty::Easy, &mut rng1);
        let b2 = Board::generate(Difficulty::Easy, &mut rng2);
        assert_ne!(b1, b2);
    }

    // --- Difficulty ---

    #[test]
    fn difficulty_sizes() {
        assert_eq!(Difficulty::Easy.grid_size(), (5, 5));
        assert_eq!(Difficulty::Medium.grid_size(), (7, 7));
        assert_eq!(Difficulty::Hard.grid_size(), (9, 9));
    }

    #[test]
    fn difficulty_names() {
        assert_eq!(Difficulty::Easy.name(), "Easy");
        assert_eq!(Difficulty::Hard.name(), "Hard");
    }

    // --- LCG ---

    #[test]
    fn lcg_deterministic() {
        let mut r1 = Lcg::new(42);
        let mut r2 = Lcg::new(42);
        for _ in 0..10 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn lcg_range() {
        let mut rng = Lcg::new(99);
        for _ in 0..100 {
            assert!(rng.next_range(5) < 5);
        }
    }

    // --- App ---

    #[test]
    fn new_app() {
        let app = PipesApp::new();
        assert_eq!(app.difficulty, Difficulty::Easy);
        assert!(!app.solved);
        assert_eq!(app.moves, 0);
        assert_eq!(app.cursor_r, 0);
        assert_eq!(app.cursor_c, 0);
    }

    #[test]
    fn app_deterministic() {
        let a1 = PipesApp::with_seed(99);
        let a2 = PipesApp::with_seed(99);
        assert_eq!(a1.board, a2.board);
    }

    #[test]
    fn cursor_move_down() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_r, 1);
    }

    #[test]
    fn cursor_move_right() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Right,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_c, 1);
    }

    #[test]
    fn cursor_clamped_top() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Up,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_r, 0);
    }

    #[test]
    fn cursor_clamped_left() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Left,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.cursor_c, 0);
    }

    #[test]
    fn rotate_increments_moves() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn new_game_resets() {
        let mut app = PipesApp::new();
        app.moves = 10;
        app.solved = true;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert!(!app.solved);
    }

    #[test]
    fn set_difficulty() {
        let mut app = PipesApp::new();
        app.set_difficulty(Difficulty::Hard);
        assert_eq!(app.difficulty, Difficulty::Hard);
        assert_eq!(app.board.rows, 9);
    }

    #[test]
    fn key_n_new_game() {
        let mut app = PipesApp::new();
        app.moves = 5;
        app.handle_key(&KeyEvent {
            key: Key::N,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn key_difficulty() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Num2,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.difficulty, Difficulty::Medium);
    }

    #[test]
    fn key_released_ignored() {
        let mut app = PipesApp::new();
        app.handle_key(&KeyEvent {
            key: Key::Space,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn rotate_blocked_when_solved() {
        let mut app = PipesApp::new();
        app.solved = true;
        app.rotate_current_cw();
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn flow_toggle() {
        let mut app = PipesApp::new();
        assert!(app.show_flow);
        app.handle_key(&KeyEvent {
            key: Key::F,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        });
        assert!(!app.show_flow);
    }

    #[test]
    fn handle_event() {
        let mut app = PipesApp::new();
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Down,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }));
        assert_eq!(app.cursor_r, 1);
    }

    // --- Rendering ---

    #[test]
    fn render_basic() {
        let app = PipesApp::new();
        let cmds = app.render(600.0, 800.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_has_title() {
        let app = PipesApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_title = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "Pipes"));
        assert!(has_title);
    }

    #[test]
    fn render_has_background() {
        let app = PipesApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_bg = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_solved_message() {
        let mut app = PipesApp::new();
        app.solved = true;
        app.moves = 15;
        let cmds = app.render(600.0, 800.0);
        let has_solved = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Congratulations")),
        );
        assert!(has_solved);
    }

    #[test]
    fn render_has_source_label() {
        let app = PipesApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_s = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "S"));
        assert!(has_s);
    }

    #[test]
    fn render_has_drain_label() {
        let app = PipesApp::new();
        let cmds = app.render(600.0, 800.0);
        let has_d = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text == "D"));
        assert!(has_d);
    }

    // --- Board pipe_for_openings ---

    #[test]
    fn pipe_for_single_opening() {
        let (kind, _) = Board::pipe_for_openings(&[Dir::Up]);
        assert_eq!(kind, PipeKind::End);
    }

    #[test]
    fn pipe_for_opposite_openings() {
        let (kind, _) = Board::pipe_for_openings(&[Dir::Up, Dir::Down]);
        assert_eq!(kind, PipeKind::Straight);
    }

    #[test]
    fn pipe_for_adjacent_openings() {
        let (kind, _) = Board::pipe_for_openings(&[Dir::Up, Dir::Right]);
        assert_eq!(kind, PipeKind::Corner);
    }

    #[test]
    fn pipe_for_no_openings() {
        let (kind, _) = Board::pipe_for_openings(&[]);
        assert_eq!(kind, PipeKind::End);
    }

    #[test]
    fn pipe_for_three_openings() {
        let (kind, _) = Board::pipe_for_openings(&[Dir::Up, Dir::Right, Dir::Down]);
        assert_eq!(kind, PipeKind::Tee);
    }

    // --- dir_from ---

    #[test]
    fn dir_from_right() {
        assert_eq!(Board::dir_from(0, 0, 0, 1), Dir::Right);
    }

    #[test]
    fn dir_from_down() {
        assert_eq!(Board::dir_from(0, 0, 1, 0), Dir::Down);
    }

    #[test]
    fn dir_from_up() {
        assert_eq!(Board::dir_from(1, 0, 0, 0), Dir::Up);
    }

    #[test]
    fn dir_from_left() {
        assert_eq!(Board::dir_from(0, 1, 0, 0), Dir::Left);
    }

    // --- Cross pipe rotation invariant ---

    #[test]
    fn cross_rotation_invariant() {
        for rot in 0..4 {
            let p = Pipe::new(PipeKind::Cross, rot);
            assert_eq!(p.openings().len(), 4);
            assert!(p.has_opening(Dir::Up));
            assert!(p.has_opening(Dir::Right));
            assert!(p.has_opening(Dir::Down));
            assert!(p.has_opening(Dir::Left));
        }
    }

    #[test]
    fn games_won_increments() {
        let mut app = PipesApp::new();
        assert_eq!(app.games_won, 0);
        // Manually trigger a win
        app.solved = false;
        // Force solve by making a trivially connected board
        let mut board = Board::new(2, 2);
        board.source = (0, 0);
        board.drain = (0, 1);
        board.cells[0][0] = Pipe::new(PipeKind::End, 1); // Right
        board.cells[0][1] = Pipe::new(PipeKind::End, 3); // Left
        app.board = board;
        app.check_solved();
        assert!(app.solved);
        assert_eq!(app.games_won, 1);
    }
}
