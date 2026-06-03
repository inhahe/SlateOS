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

//! OurOS Nonogram (Picross / picture-logic puzzle) game.
//!
//! The player deduces which cells to fill based on numeric clues given for
//! each row and column. Clues describe the lengths of consecutive filled
//! runs in order. The puzzle is solved when the player's grid matches the
//! hidden solution exactly.
//!
//! Features:
//! - 5x5, 10x10, and 15x15 grid sizes with 8+ built-in picture puzzles
//! - Row and column clue numbers computed automatically from the solution
//! - Arrow-key cursor movement, Enter/Space to fill, X to mark empty
//! - Current row/column clue highlighting
//! - Win detection when grid matches solution
//! - Puzzle select screen with thumbnails
//! - Elapsed-time timer
//! - Check mode to highlight errors (C key)
//! - Catppuccin Mocha dark theme

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
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
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);

// ── Layout constants ────────────────────────────────────────────────
const CELL_SIZE: f32 = 28.0;
const CELL_GAP: f32 = 2.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 50.0;
const CLUE_FONT_SIZE: f32 = 13.0;
const CELL_FONT_SIZE: f32 = 16.0;
const HEADER_FONT_SIZE: f32 = 20.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const SELECT_FONT_SIZE: f32 = 15.0;
const THUMB_CELL: f32 = 6.0;
const CELL_CORNER_RADIUS: f32 = 3.0;
/// Maximum number of clue values per row/column (determines clue area width/height).
const MAX_CLUE_SLOTS: usize = 8;
/// Pixel width reserved for each clue number in the row clue area.
const CLUE_SLOT_W: f32 = 18.0;
/// Pixel height reserved for each clue number in the column clue area.
const CLUE_SLOT_H: f32 = 16.0;

// ── Grid sizes ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridSize {
    Small,  // 5x5
    Medium, // 10x10
    Large,  // 15x15
}

impl GridSize {
    fn side(self) -> usize {
        match self {
            Self::Small => 5,
            Self::Medium => 10,
            Self::Large => 15,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Small => "5x5",
            Self::Medium => "10x10",
            Self::Large => "15x15",
        }
    }
}

// ── Cell state ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellMark {
    /// Player has not interacted with this cell.
    Empty,
    /// Player filled this cell (believes it is part of the picture).
    Filled,
    /// Player marked this cell as definitely empty.
    MarkedEmpty,
}

// ── Clue computation ───────────────────────────────────────────────

/// Compute the clue (run-length encoding of filled segments) for a single
/// row or column, given as a slice of booleans (`true` = filled).
fn compute_clue(line: &[bool]) -> Vec<u8> {
    let mut clues = Vec::new();
    let mut run: u8 = 0;
    for &filled in line {
        if filled {
            run = run.saturating_add(1);
        } else {
            if run > 0 {
                clues.push(run);
            }
            run = 0;
        }
    }
    if run > 0 {
        clues.push(run);
    }
    if clues.is_empty() {
        clues.push(0);
    }
    clues
}

/// Compute row clues from a solution grid stored in row-major order.
fn compute_row_clues(solution: &[bool], cols: usize) -> Vec<Vec<u8>> {
    let rows = solution.len() / cols;
    (0..rows)
        .map(|r| {
            let start = r * cols;
            let end = start + cols;
            compute_clue(&solution[start..end])
        })
        .collect()
}

/// Compute column clues from a solution grid stored in row-major order.
fn compute_col_clues(solution: &[bool], cols: usize) -> Vec<Vec<u8>> {
    let rows = solution.len() / cols;
    (0..cols)
        .map(|c| {
            let col_vals: Vec<bool> = (0..rows).map(|r| solution[r * cols + c]).collect();
            compute_clue(&col_vals)
        })
        .collect()
}

// ── Built-in puzzles ───────────────────────────────────────────────

/// A puzzle definition: a name, grid size, and solution bitmap.
#[derive(Clone, Debug)]
struct PuzzleDef {
    name: &'static str,
    size: GridSize,
    /// Row-major solution: `true` means the cell should be filled.
    solution: Vec<bool>,
}

/// Parse a multi-line string picture into a boolean grid.
/// `#` = filled, anything else = empty. Each line is one row.
fn parse_picture(s: &str, side: usize) -> Vec<bool> {
    let mut grid = vec![false; side * side];
    for (r, line) in s.lines().enumerate() {
        if r >= side {
            break;
        }
        for (c, ch) in line.chars().enumerate() {
            if c >= side {
                break;
            }
            if ch == '#' {
                grid[r * side + c] = true;
            }
        }
    }
    grid
}

fn builtin_puzzles() -> Vec<PuzzleDef> {
    vec![
        // ── 5x5 puzzles ───────────────────────────────────────
        PuzzleDef {
            name: "Heart",
            size: GridSize::Small,
            solution: parse_picture(
                "\
.#.#.
#####
#####
.###.
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Star",
            size: GridSize::Small,
            solution: parse_picture(
                "\
..#..
.###.
#####
.###.
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Arrow",
            size: GridSize::Small,
            solution: parse_picture(
                "\
..#..
.##..
#####
.##..
..#..",
                5,
            ),
        },
        PuzzleDef {
            name: "Cross",
            size: GridSize::Small,
            solution: parse_picture(
                "\
.###.
..#..
..#..
..#..
.###.",
                5,
            ),
        },
        // ── 10x10 puzzles ──────────────────────────────────────
        PuzzleDef {
            name: "House",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
....##....
...####...
..######..
.########.
##########
##......##
##.#..#.##
##.#..#.##
##......##
##########",
                10,
            ),
        },
        PuzzleDef {
            name: "Smiley",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
..######..
.########.
#..#..#..#
##.#..#.##
##########
##########
#.######.#
#..####..#
.#..##..#.
..######..",
                10,
            ),
        },
        PuzzleDef {
            name: "Tree",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
....##....
...####...
..######..
.########.
....##....
...####...
..######..
.########.
....##....
....##....",
                10,
            ),
        },
        PuzzleDef {
            name: "Boat",
            size: GridSize::Medium,
            solution: parse_picture(
                "\
.....#....
.....##...
.#...###..
.##..####.
.###.#####
..########
...######.
....####..
..........
##########",
                10,
            ),
        },
        // ── 15x15 puzzles ─────────────────────────────────────
        PuzzleDef {
            name: "Cat",
            size: GridSize::Large,
            solution: parse_picture(
                "\
.#...........#.
##...........##
###.........###
####.......####
#####.....#####
###############
###.##...##.###
###.##...##.###
###############
####.......####
#####.#.#.#####
.#####.#.#####.
..####...####..
...###...###...
....#######....",
                15,
            ),
        },
        PuzzleDef {
            name: "Mushroom",
            size: GridSize::Large,
            solution: parse_picture(
                "\
.....#####.....
...#########...
..###########..
.###..###..###.
###...###...###
###...###...###
.###..###..###.
..###########..
...#########...
.....#####.....
......###......
......###......
.....#####.....
....#######....
....#######....",
                15,
            ),
        },
    ]
}

// ── Game status ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    /// Puzzle selection screen.
    Select,
    /// Playing a puzzle.
    Playing,
    /// Puzzle solved — show victory.
    Won,
}

// ── Main application struct ────────────────────────────────────────
struct NonogramApp {
    screen: Screen,
    /// Index into the puzzle catalogue, or which puzzle is being played.
    selected_puzzle: usize,
    /// All available puzzles.
    puzzles: Vec<PuzzleDef>,
    /// Grid side length (5, 10, or 15).
    grid_side: usize,
    /// The solution grid (row-major booleans).
    solution: Vec<bool>,
    /// The player's current cell marks.
    cells: Vec<CellMark>,
    /// Precomputed row clues.
    row_clues: Vec<Vec<u8>>,
    /// Precomputed column clues.
    col_clues: Vec<Vec<u8>>,
    /// Cursor row.
    cursor_row: usize,
    /// Cursor column.
    cursor_col: usize,
    /// Elapsed milliseconds for the timer.
    elapsed_ms: u64,
    /// Whether check mode is active (highlight errors).
    check_mode: bool,
    /// The index on the select screen that is highlighted.
    select_cursor: usize,
}

impl NonogramApp {
    fn new() -> Self {
        let puzzles = builtin_puzzles();
        Self {
            screen: Screen::Select,
            selected_puzzle: 0,
            puzzles,
            grid_side: 5,
            solution: vec![false; 25],
            cells: vec![CellMark::Empty; 25],
            row_clues: vec![vec![0]; 5],
            col_clues: vec![vec![0]; 5],
            cursor_row: 0,
            cursor_col: 0,
            elapsed_ms: 0,
            check_mode: false,
            select_cursor: 0,
        }
    }

    /// Start playing a specific puzzle by index.
    fn start_puzzle(&mut self, index: usize) {
        if index >= self.puzzles.len() {
            return;
        }
        let def = self.puzzles[index].clone();
        self.selected_puzzle = index;
        self.grid_side = def.size.side();
        let total = self.grid_side * self.grid_side;
        self.row_clues = compute_row_clues(&def.solution, self.grid_side);
        self.col_clues = compute_col_clues(&def.solution, self.grid_side);
        self.solution = def.solution;
        self.cells = vec![CellMark::Empty; total];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.elapsed_ms = 0;
        self.check_mode = false;
        self.screen = Screen::Playing;
    }

    /// Return the cell mark at (row, col), or `Empty` if out of bounds.
    fn cell_at(&self, row: usize, col: usize) -> CellMark {
        if row < self.grid_side && col < self.grid_side {
            self.cells[row * self.grid_side + col]
        } else {
            CellMark::Empty
        }
    }

    /// Set the cell mark at (row, col).
    fn set_cell(&mut self, row: usize, col: usize, mark: CellMark) {
        if row < self.grid_side && col < self.grid_side {
            self.cells[row * self.grid_side + col] = mark;
        }
    }

    /// Toggle a cell between Empty and Filled.
    fn toggle_fill(&mut self, row: usize, col: usize) {
        let current = self.cell_at(row, col);
        let next = match current {
            CellMark::Empty | CellMark::MarkedEmpty => CellMark::Filled,
            CellMark::Filled => CellMark::Empty,
        };
        self.set_cell(row, col, next);
    }

    /// Toggle a cell between Empty and MarkedEmpty (the X mark).
    fn toggle_mark_empty(&mut self, row: usize, col: usize) {
        let current = self.cell_at(row, col);
        let next = match current {
            CellMark::Empty | CellMark::Filled => CellMark::MarkedEmpty,
            CellMark::MarkedEmpty => CellMark::Empty,
        };
        self.set_cell(row, col, next);
    }

    /// Check whether the player's filled cells match the solution exactly.
    fn check_win(&self) -> bool {
        for i in 0..self.solution.len() {
            let player_filled = self.cells[i] == CellMark::Filled;
            if player_filled != self.solution[i] {
                return false;
            }
        }
        true
    }

    /// Return whether a cell is an error (filled but should not be, or
    /// not filled but should be). Used in check mode.
    fn is_error(&self, row: usize, col: usize) -> bool {
        if row >= self.grid_side || col >= self.grid_side {
            return false;
        }
        let idx = row * self.grid_side + col;
        let should_fill = self.solution[idx];
        // Only flag filled-but-wrong or marked-empty-but-should-be-filled.
        match self.cells[idx] {
            CellMark::Filled => !should_fill,
            CellMark::MarkedEmpty => should_fill,
            CellMark::Empty => false,
        }
    }

    /// Count how many cells the player has correctly filled.
    fn filled_correct_count(&self) -> usize {
        (0..self.solution.len())
            .filter(|&i| self.cells[i] == CellMark::Filled && self.solution[i])
            .count()
    }

    /// Count how many cells should be filled in the solution.
    fn total_filled_in_solution(&self) -> usize {
        self.solution.iter().filter(|&&v| v).count()
    }

    /// Number of cells the player has filled (regardless of correctness).
    fn player_filled_count(&self) -> usize {
        self.cells.iter().filter(|&&c| c == CellMark::Filled).count()
    }

    /// Format elapsed time as M:SS.
    fn format_time(&self) -> String {
        let total_secs = self.elapsed_ms / 1000;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}:{secs:02}")
    }

    /// Maximum number of clue values among all row clues.
    fn max_row_clue_len(&self) -> usize {
        self.row_clues.iter().map(|c| c.len()).max().unwrap_or(1)
    }

    /// Maximum number of clue values among all column clues.
    fn max_col_clue_len(&self) -> usize {
        self.col_clues.iter().map(|c| c.len()).max().unwrap_or(1)
    }

    // ── Event handling ─────────────────────────────────────────────

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(key_event) if key_event.pressed => {
                self.handle_key(key_event);
            }
            Event::Mouse(mouse_event) => {
                self.handle_mouse(mouse_event);
            }
            Event::Tick { elapsed_ms } => {
                self.handle_tick(*elapsed_ms);
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key_event: &KeyEvent) {
        match self.screen {
            Screen::Select => self.handle_key_select(key_event),
            Screen::Playing => self.handle_key_playing(key_event),
            Screen::Won => self.handle_key_won(key_event),
        }
    }

    fn handle_key_select(&mut self, key_event: &KeyEvent) {
        match key_event.key {
            Key::Up
                if self.select_cursor > 0 => {
                    self.select_cursor -= 1;
                }
            Key::Down
                if self.select_cursor + 1 < self.puzzles.len() => {
                    self.select_cursor += 1;
                }
            Key::Enter | Key::Space => {
                self.start_puzzle(self.select_cursor);
            }
            _ => {}
        }
    }

    fn handle_key_playing(&mut self, key_event: &KeyEvent) {
        match key_event.key {
            Key::Up
                if self.cursor_row > 0 => {
                    self.cursor_row -= 1;
                }
            Key::Down
                if self.cursor_row + 1 < self.grid_side => {
                    self.cursor_row += 1;
                }
            Key::Left
                if self.cursor_col > 0 => {
                    self.cursor_col -= 1;
                }
            Key::Right
                if self.cursor_col + 1 < self.grid_side => {
                    self.cursor_col += 1;
                }
            Key::Enter | Key::Space => {
                self.toggle_fill(self.cursor_row, self.cursor_col);
                if self.check_win() {
                    self.screen = Screen::Won;
                }
            }
            Key::X => {
                self.toggle_mark_empty(self.cursor_row, self.cursor_col);
            }
            Key::C => {
                self.check_mode = !self.check_mode;
            }
            Key::Escape => {
                self.screen = Screen::Select;
            }
            _ => {}
        }
    }

    fn handle_key_won(&mut self, key_event: &KeyEvent) {
        match key_event.key {
            Key::Enter | Key::Space | Key::Escape => {
                self.screen = Screen::Select;
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse_event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = mouse_event.kind {
            match self.screen {
                Screen::Select => self.handle_mouse_select(mouse_event),
                Screen::Playing => self.handle_mouse_playing(mouse_event),
                Screen::Won => {
                    self.screen = Screen::Select;
                }
            }
        }
    }

    fn handle_mouse_select(&mut self, mouse_event: &MouseEvent) {
        let mx = mouse_event.x;
        let my = mouse_event.y;
        // Each puzzle entry is rendered as a row starting at y = HEADER_HEIGHT + i * 40.0
        let list_y_start = HEADER_HEIGHT + PADDING;
        for i in 0..self.puzzles.len() {
            let entry_y = list_y_start + i as f32 * 40.0;
            if my >= entry_y && my < entry_y + 36.0 && (PADDING..500.0).contains(&mx) {
                self.start_puzzle(i);
                return;
            }
        }
    }

    fn handle_mouse_playing(&mut self, mouse_event: &MouseEvent) {
        let mx = mouse_event.x;
        let my = mouse_event.y;

        let row_clue_area_w = self.max_row_clue_len() as f32 * CLUE_SLOT_W;
        let col_clue_area_h = self.max_col_clue_len() as f32 * CLUE_SLOT_H;

        let grid_origin_x = PADDING + row_clue_area_w;
        let grid_origin_y = HEADER_HEIGHT + PADDING + col_clue_area_h;

        let cell_step = CELL_SIZE + CELL_GAP;

        let col_f = (mx - grid_origin_x) / cell_step;
        let row_f = (my - grid_origin_y) / cell_step;

        if col_f >= 0.0 && row_f >= 0.0 {
            let col = col_f as usize;
            let row = row_f as usize;
            if row < self.grid_side && col < self.grid_side {
                self.cursor_row = row;
                self.cursor_col = col;
                self.toggle_fill(row, col);
                if self.check_win() {
                    self.screen = Screen::Won;
                }
            }
        }
    }

    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.screen == Screen::Playing {
            self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        }
    }

    // ── Rendering ──────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        match self.screen {
            Screen::Select => self.render_select(),
            Screen::Playing | Screen::Won => self.render_playing(),
        }
    }

    fn render_select(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let total_width = 520.0_f32;
        let list_height = self.puzzles.len() as f32 * 40.0 + PADDING * 2.0;
        let total_height = HEADER_HEIGHT + list_height + PADDING;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: total_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_width,
            height: HEADER_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 15.0,
            text: "Nonogram - Select Puzzle".into(),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Puzzle list
        let list_y_start = HEADER_HEIGHT + PADDING;
        for (i, puzzle) in self.puzzles.iter().enumerate() {
            let entry_y = list_y_start + i as f32 * 40.0;
            let is_selected = i == self.select_cursor;

            // Highlight background for selected entry
            let bg_color = if is_selected { SURFACE1 } else { SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y: entry_y,
                width: total_width - PADDING * 2.0,
                height: 36.0,
                color: bg_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Puzzle name and size label
            let name_color = if is_selected { BLUE } else { TEXT_COLOR };
            cmds.push(RenderCommand::Text {
                x: PADDING + 12.0,
                y: entry_y + 10.0,
                text: puzzle.name.into(),
                color: name_color,
                font_size: SELECT_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 160.0,
                y: entry_y + 10.0,
                text: format!("({})", puzzle.size.label()),
                color: SUBTEXT0,
                font_size: STATUS_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Mini thumbnail
            let thumb_x = PADDING + 260.0;
            let thumb_y = entry_y + 4.0;
            let side = puzzle.size.side();
            // Draw small cells for thumbnail — only draw filled ones
            for r in 0..side {
                for c in 0..side {
                    if puzzle.solution[r * side + c] {
                        cmds.push(RenderCommand::FillRect {
                            x: thumb_x + c as f32 * THUMB_CELL,
                            y: thumb_y + r as f32 * THUMB_CELL,
                            width: THUMB_CELL - 1.0,
                            height: THUMB_CELL - 1.0,
                            color: if is_selected { BLUE } else { LAVENDER },
                            corner_radii: CornerRadii::ZERO,
                        });
                    }
                }
            }
        }

        // Footer hint
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: total_height - 24.0,
            text: "Up/Down: navigate   Enter: play".into(),
            color: OVERLAY0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }

    fn render_playing(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let row_clue_area_w = self.max_row_clue_len() as f32 * CLUE_SLOT_W;
        let col_clue_area_h = self.max_col_clue_len() as f32 * CLUE_SLOT_H;
        let cell_step = CELL_SIZE + CELL_GAP;
        let grid_pixel_w = self.grid_side as f32 * cell_step - CELL_GAP;
        let grid_pixel_h = grid_pixel_w;

        let total_width = PADDING + row_clue_area_w + grid_pixel_w + PADDING;
        let footer_height = 40.0;
        let total_height =
            HEADER_HEIGHT + PADDING + col_clue_area_h + grid_pixel_h + footer_height + PADDING;

        let grid_origin_x = PADDING + row_clue_area_w;
        let grid_origin_y = HEADER_HEIGHT + PADDING + col_clue_area_h;

        // Background
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

        // Column clues
        self.render_col_clues(&mut cmds, grid_origin_x, HEADER_HEIGHT + PADDING, col_clue_area_h);

        // Row clues
        self.render_row_clues(&mut cmds, PADDING, grid_origin_y, row_clue_area_w);

        // Grid cells
        self.render_grid(&mut cmds, grid_origin_x, grid_origin_y);

        // Footer
        self.render_footer(&mut cmds, total_width, total_height, footer_height);

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

        let title = if self.selected_puzzle < self.puzzles.len() {
            &self.puzzles[self.selected_puzzle].name
        } else {
            &"Nonogram"
        };
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: 15.0,
            text: format!("Nonogram - {title}"),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Timer
        let time_text = self.format_time();
        cmds.push(RenderCommand::Text {
            x: total_width - 80.0,
            y: 15.0,
            text: time_text,
            color: SUBTEXT0,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        if self.screen == Screen::Won {
            cmds.push(RenderCommand::Text {
                x: total_width / 2.0 - 30.0,
                y: 30.0,
                text: "SOLVED!".into(),
                color: GREEN,
                font_size: STATUS_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
    }

    fn render_col_clues(
        &self,
        cmds: &mut Vec<RenderCommand>,
        grid_origin_x: f32,
        clue_area_y: f32,
        _col_clue_area_h: f32,
    ) {
        let cell_step = CELL_SIZE + CELL_GAP;
        let max_len = self.max_col_clue_len();

        for (c, clue) in self.col_clues.iter().enumerate() {
            let is_current = self.screen == Screen::Playing && c == self.cursor_col;
            let base_x = grid_origin_x + c as f32 * cell_step;
            // Right-align clue numbers from bottom of the clue area.
            let start_slot = max_len - clue.len();
            for (j, &val) in clue.iter().enumerate() {
                let slot = start_slot + j;
                let cy = clue_area_y + slot as f32 * CLUE_SLOT_H;
                let color = if is_current { BLUE } else { SUBTEXT0 };
                let weight = if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                cmds.push(RenderCommand::Text {
                    x: base_x + CELL_SIZE / 2.0 - 4.0,
                    y: cy,
                    text: val.to_string(),
                    color,
                    font_size: CLUE_FONT_SIZE,
                    font_weight: weight,
                    max_width: None,
                });
            }
        }
    }

    fn render_row_clues(
        &self,
        cmds: &mut Vec<RenderCommand>,
        clue_area_x: f32,
        grid_origin_y: f32,
        _row_clue_area_w: f32,
    ) {
        let cell_step = CELL_SIZE + CELL_GAP;
        let max_len = self.max_row_clue_len();

        for (r, clue) in self.row_clues.iter().enumerate() {
            let is_current = self.screen == Screen::Playing && r == self.cursor_row;
            let base_y = grid_origin_y + r as f32 * cell_step + CELL_SIZE / 2.0 - 7.0;
            // Right-align clue numbers from the right edge of the clue area.
            let start_slot = max_len - clue.len();
            for (j, &val) in clue.iter().enumerate() {
                let slot = start_slot + j;
                let cx = clue_area_x + slot as f32 * CLUE_SLOT_W;
                let color = if is_current { BLUE } else { SUBTEXT0 };
                let weight = if is_current {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                cmds.push(RenderCommand::Text {
                    x: cx,
                    y: base_y,
                    text: val.to_string(),
                    color,
                    font_size: CLUE_FONT_SIZE,
                    font_weight: weight,
                    max_width: None,
                });
            }
        }
    }

    fn render_grid(
        &self,
        cmds: &mut Vec<RenderCommand>,
        grid_origin_x: f32,
        grid_origin_y: f32,
    ) {
        let cell_step = CELL_SIZE + CELL_GAP;

        for r in 0..self.grid_side {
            for c in 0..self.grid_side {
                let cx = grid_origin_x + c as f32 * cell_step;
                let cy = grid_origin_y + r as f32 * cell_step;

                let is_cursor =
                    self.screen == Screen::Playing && r == self.cursor_row && c == self.cursor_col;
                let mark = self.cell_at(r, c);
                let error = self.check_mode && self.is_error(r, c);

                // Cell background
                let bg = match mark {
                    CellMark::Filled => {
                        if error {
                            RED
                        } else if self.screen == Screen::Won {
                            BLUE
                        } else {
                            LAVENDER
                        }
                    }
                    CellMark::MarkedEmpty => {
                        if error { Color::rgba(243, 139, 168, 60) } else { SURFACE0 }
                    }
                    CellMark::Empty => SURFACE0,
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });

                // MarkedEmpty X
                if mark == CellMark::MarkedEmpty {
                    let inset = 6.0;
                    let x_color = if error { RED } else { OVERLAY0 };
                    cmds.push(RenderCommand::Line {
                        x1: cx + inset,
                        y1: cy + inset,
                        x2: cx + CELL_SIZE - inset,
                        y2: cy + CELL_SIZE - inset,
                        color: x_color,
                        width: 2.0,
                    });
                    cmds.push(RenderCommand::Line {
                        x1: cx + CELL_SIZE - inset,
                        y1: cy + inset,
                        x2: cx + inset,
                        y2: cy + CELL_SIZE - inset,
                        color: x_color,
                        width: 2.0,
                    });
                }

                // Cursor outline
                if is_cursor {
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx - 1.0,
                        y: cy - 1.0,
                        width: CELL_SIZE + 2.0,
                        height: CELL_SIZE + 2.0,
                        color: YELLOW,
                        line_width: 2.0,
                        corner_radii: CornerRadii::all(CELL_CORNER_RADIUS + 1.0),
                    });
                }
            }
        }

        // Draw grid lines for 5-cell groups (thicker lines every 5 cells)
        if self.grid_side >= 10 {
            let line_color = OVERLAY0;
            for g in 1..(self.grid_side / 5) {
                let pos = g as f32 * 5.0 * cell_step - CELL_GAP / 2.0;
                // Vertical line
                cmds.push(RenderCommand::Line {
                    x1: grid_origin_x + pos,
                    y1: grid_origin_y,
                    x2: grid_origin_x + pos,
                    y2: grid_origin_y + self.grid_side as f32 * cell_step - CELL_GAP,
                    color: line_color,
                    width: 1.5,
                });
                // Horizontal line
                cmds.push(RenderCommand::Line {
                    x1: grid_origin_x,
                    y1: grid_origin_y + pos,
                    x2: grid_origin_x + self.grid_side as f32 * cell_step - CELL_GAP,
                    y2: grid_origin_y + pos,
                    color: line_color,
                    width: 1.5,
                });
            }
        }
    }

    fn render_footer(
        &self,
        cmds: &mut Vec<RenderCommand>,
        total_width: f32,
        total_height: f32,
        footer_height: f32,
    ) {
        let footer_y = total_height - footer_height;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: footer_y,
            width: total_width,
            height: footer_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let hint = if self.screen == Screen::Won {
            "Enter/Esc: back to menu"
        } else {
            "Arrows: move  Space/Enter: fill  X: mark  C: check  Esc: menu"
        };
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y + 12.0,
            text: hint.into(),
            color: OVERLAY0,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress indicator
        let filled = self.player_filled_count();
        let target = self.total_filled_in_solution();
        let progress_text = format!("{filled}/{target}");
        cmds.push(RenderCommand::Text {
            x: total_width - 80.0,
            y: footer_y + 12.0,
            text: progress_text,
            color: if self.screen == Screen::Won {
                GREEN
            } else {
                SUBTEXT0
            },
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() {
    let _app = NonogramApp::new();
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper: create a key press event ────────────────────────────
    fn key_press(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    fn key_release(key: Key) -> Event {
        Event::Key(KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        })
    }

    fn left_click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn tick(ms: u64) -> Event {
        Event::Tick { elapsed_ms: ms }
    }

    /// Start playing the first puzzle (Heart 5x5).
    fn app_playing_heart() -> NonogramApp {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        app
    }

    /// Fill the solution for a given app so it wins.
    fn fill_solution(app: &mut NonogramApp) {
        for i in 0..app.solution.len() {
            if app.solution[i] {
                let r = i / app.grid_side;
                let c = i % app.grid_side;
                app.set_cell(r, c, CellMark::Filled);
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // Clue computation
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_compute_clue_all_empty() {
        let line = [false, false, false, false, false];
        assert_eq!(compute_clue(&line), vec![0]);
    }

    #[test]
    fn test_compute_clue_all_filled() {
        let line = [true, true, true, true, true];
        assert_eq!(compute_clue(&line), vec![5]);
    }

    #[test]
    fn test_compute_clue_single() {
        let line = [false, false, true, false, false];
        assert_eq!(compute_clue(&line), vec![1]);
    }

    #[test]
    fn test_compute_clue_two_runs() {
        let line = [true, true, false, true, true];
        assert_eq!(compute_clue(&line), vec![2, 2]);
    }

    #[test]
    fn test_compute_clue_mixed() {
        let line = [true, false, true, true, false];
        assert_eq!(compute_clue(&line), vec![1, 2]);
    }

    #[test]
    fn test_compute_clue_starts_empty() {
        let line = [false, true, true, true, false];
        assert_eq!(compute_clue(&line), vec![3]);
    }

    #[test]
    fn test_compute_clue_ends_filled() {
        let line = [false, false, true, true, true];
        assert_eq!(compute_clue(&line), vec![3]);
    }

    #[test]
    fn test_compute_clue_alternating() {
        let line = [true, false, true, false, true];
        assert_eq!(compute_clue(&line), vec![1, 1, 1]);
    }

    #[test]
    fn test_compute_clue_single_cell_filled() {
        let line = [true];
        assert_eq!(compute_clue(&line), vec![1]);
    }

    #[test]
    fn test_compute_clue_single_cell_empty() {
        let line = [false];
        assert_eq!(compute_clue(&line), vec![0]);
    }

    // ── Row/column clue computation ────────────────────────────────

    #[test]
    fn test_compute_row_clues_heart() {
        // Heart puzzle:
        // .#.#.  -> [1, 1]
        // ##### -> [5]
        // ##### -> [5]
        // .###. -> [3]
        // ..#.. -> [1]
        let heart = parse_picture(
            ".#.#.\n#####\n#####\n.###.\n..#..",
            5,
        );
        let row_clues = compute_row_clues(&heart, 5);
        assert_eq!(row_clues.len(), 5);
        assert_eq!(row_clues[0], vec![1, 1]);
        assert_eq!(row_clues[1], vec![5]);
        assert_eq!(row_clues[2], vec![5]);
        assert_eq!(row_clues[3], vec![3]);
        assert_eq!(row_clues[4], vec![1]);
    }

    #[test]
    fn test_compute_col_clues_heart() {
        let heart = parse_picture(
            ".#.#.\n#####\n#####\n.###.\n..#..",
            5,
        );
        let col_clues = compute_col_clues(&heart, 5);
        assert_eq!(col_clues.len(), 5);
        // col 0: .#.#. transposed columns:
        // col0: .,#,#,.,. -> [2]
        assert_eq!(col_clues[0], vec![2]);
        // col1: #,#,#,#,. -> [4]
        assert_eq!(col_clues[1], vec![4]);
        // col2: .,#,#,#,# -> [4]
        assert_eq!(col_clues[2], vec![4]);
        // col3: #,#,#,#,. -> [4]
        assert_eq!(col_clues[3], vec![4]);
        // col4: .,#,#,.,. -> [2]
        assert_eq!(col_clues[4], vec![2]);
    }

    #[test]
    fn test_compute_row_clues_arrow() {
        let arrow = parse_picture(
            "..#..\n.##..\n#####\n.##..\n..#..",
            5,
        );
        let row_clues = compute_row_clues(&arrow, 5);
        assert_eq!(row_clues[0], vec![1]);
        assert_eq!(row_clues[1], vec![2]);
        assert_eq!(row_clues[2], vec![5]);
        assert_eq!(row_clues[3], vec![2]);
        assert_eq!(row_clues[4], vec![1]);
    }

    #[test]
    fn test_compute_col_clues_arrow() {
        let arrow = parse_picture(
            "..#..\n.##..\n#####\n.##..\n..#..",
            5,
        );
        let col_clues = compute_col_clues(&arrow, 5);
        // col0: .,..,#,..,.. -> [1]
        assert_eq!(col_clues[0], vec![1]);
        // col1: .,#,#,#,. -> [3]
        assert_eq!(col_clues[1], vec![3]);
        // col2: #,#,#,#,# -> [5]
        assert_eq!(col_clues[2], vec![5]);
        // col3: .,.,#,.,. -> [1]
        assert_eq!(col_clues[3], vec![1]);
        // col4: .,.,#,.,. -> [1]
        assert_eq!(col_clues[4], vec![1]);
    }

    // ── parse_picture ──────────────────────────────────────────────

    #[test]
    fn test_parse_picture_all_empty() {
        let grid = parse_picture(".....\n.....\n.....\n.....\n.....", 5);
        assert_eq!(grid.len(), 25);
        assert!(grid.iter().all(|&v| !v));
    }

    #[test]
    fn test_parse_picture_all_filled() {
        let grid = parse_picture("#####\n#####\n#####\n#####\n#####", 5);
        assert_eq!(grid.len(), 25);
        assert!(grid.iter().all(|&v| v));
    }

    #[test]
    fn test_parse_picture_heart_fill_count() {
        let grid = parse_picture(
            ".#.#.\n#####\n#####\n.###.\n..#..",
            5,
        );
        let filled = grid.iter().filter(|&&v| v).count();
        // Row 0: 2, Row 1: 5, Row 2: 5, Row 3: 3, Row 4: 1 = 16
        assert_eq!(filled, 16);
    }

    #[test]
    fn test_parse_picture_respects_side() {
        // Provide a 3x3 picture but parse with side=5
        let grid = parse_picture("###\n###\n###", 5);
        assert_eq!(grid.len(), 25);
        // Only first 3 cols of first 3 rows should be filled
        assert!(grid[0]); // (0,0)
        assert!(grid[2]); // (0,2)
        assert!(!grid[3]); // (0,3) — beyond picture data
    }

    // ── GridSize ───────────────────────────────────────────────────

    #[test]
    fn test_grid_size_small() {
        assert_eq!(GridSize::Small.side(), 5);
        assert_eq!(GridSize::Small.label(), "5x5");
    }

    #[test]
    fn test_grid_size_medium() {
        assert_eq!(GridSize::Medium.side(), 10);
        assert_eq!(GridSize::Medium.label(), "10x10");
    }

    #[test]
    fn test_grid_size_large() {
        assert_eq!(GridSize::Large.side(), 15);
        assert_eq!(GridSize::Large.label(), "15x15");
    }

    // ── Builtin puzzles ────────────────────────────────────────────

    #[test]
    fn test_builtin_puzzles_count() {
        let puzzles = builtin_puzzles();
        assert!(puzzles.len() >= 8, "Should have at least 8 built-in puzzles");
    }

    #[test]
    fn test_builtin_puzzles_solution_sizes() {
        let puzzles = builtin_puzzles();
        for p in &puzzles {
            let side = p.size.side();
            assert_eq!(
                p.solution.len(),
                side * side,
                "Puzzle '{}' solution should have {} cells",
                p.name,
                side * side,
            );
        }
    }

    #[test]
    fn test_builtin_puzzles_have_filled_cells() {
        let puzzles = builtin_puzzles();
        for p in &puzzles {
            let filled = p.solution.iter().filter(|&&v| v).count();
            assert!(
                filled > 0,
                "Puzzle '{}' should have at least one filled cell",
                p.name,
            );
        }
    }

    #[test]
    fn test_builtin_puzzles_unique_names() {
        let puzzles = builtin_puzzles();
        for i in 0..puzzles.len() {
            for j in (i + 1)..puzzles.len() {
                assert_ne!(
                    puzzles[i].name, puzzles[j].name,
                    "Puzzle names should be unique",
                );
            }
        }
    }

    #[test]
    fn test_builtin_has_all_sizes() {
        let puzzles = builtin_puzzles();
        let has_small = puzzles.iter().any(|p| p.size == GridSize::Small);
        let has_medium = puzzles.iter().any(|p| p.size == GridSize::Medium);
        let has_large = puzzles.iter().any(|p| p.size == GridSize::Large);
        assert!(has_small, "Should have at least one 5x5 puzzle");
        assert!(has_medium, "Should have at least one 10x10 puzzle");
        assert!(has_large, "Should have at least one 15x15 puzzle");
    }

    // ── NonogramApp creation ───────────────────────────────────────

    #[test]
    fn test_new_app_starts_on_select_screen() {
        let app = NonogramApp::new();
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn test_new_app_has_puzzles() {
        let app = NonogramApp::new();
        assert!(app.puzzles.len() >= 8);
    }

    #[test]
    fn test_new_app_select_cursor_at_zero() {
        let app = NonogramApp::new();
        assert_eq!(app.select_cursor, 0);
    }

    // ── Start puzzle ───────────────────────────────────────────────

    #[test]
    fn test_start_puzzle_transitions_to_playing() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        assert_eq!(app.screen, Screen::Playing);
    }

    #[test]
    fn test_start_puzzle_sets_grid_side() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0); // Heart is 5x5
        assert_eq!(app.grid_side, 5);
    }

    #[test]
    fn test_start_puzzle_resets_cells() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        assert!(app.cells.iter().all(|&c| c == CellMark::Empty));
    }

    #[test]
    fn test_start_puzzle_resets_timer() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        app.elapsed_ms = 5000;
        app.start_puzzle(1);
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn test_start_puzzle_resets_check_mode() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        app.check_mode = true;
        app.start_puzzle(1);
        assert!(!app.check_mode);
    }

    #[test]
    fn test_start_puzzle_out_of_bounds_does_nothing() {
        let mut app = NonogramApp::new();
        app.start_puzzle(9999);
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn test_start_puzzle_computes_row_clues() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0); // Heart
        assert_eq!(app.row_clues.len(), 5);
        assert_eq!(app.row_clues[0], vec![1, 1]);
        assert_eq!(app.row_clues[1], vec![5]);
    }

    #[test]
    fn test_start_puzzle_computes_col_clues() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0); // Heart
        assert_eq!(app.col_clues.len(), 5);
    }

    #[test]
    fn test_start_medium_puzzle() {
        let mut app = NonogramApp::new();
        // Find a medium puzzle
        let idx = app
            .puzzles
            .iter()
            .position(|p| p.size == GridSize::Medium)
            .expect("Should have a medium puzzle");
        app.start_puzzle(idx);
        assert_eq!(app.grid_side, 10);
        assert_eq!(app.cells.len(), 100);
    }

    #[test]
    fn test_start_large_puzzle() {
        let mut app = NonogramApp::new();
        let idx = app
            .puzzles
            .iter()
            .position(|p| p.size == GridSize::Large)
            .expect("Should have a large puzzle");
        app.start_puzzle(idx);
        assert_eq!(app.grid_side, 15);
        assert_eq!(app.cells.len(), 225);
    }

    // ── Cell operations ────────────────────────────────────────────

    #[test]
    fn test_cell_at_empty_initially() {
        let app = app_playing_heart();
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn test_set_cell_and_read_back() {
        let mut app = app_playing_heart();
        app.set_cell(1, 1, CellMark::Filled);
        assert_eq!(app.cell_at(1, 1), CellMark::Filled);
    }

    #[test]
    fn test_cell_at_out_of_bounds() {
        let app = app_playing_heart();
        assert_eq!(app.cell_at(99, 99), CellMark::Empty);
    }

    #[test]
    fn test_set_cell_out_of_bounds_no_panic() {
        let mut app = app_playing_heart();
        app.set_cell(99, 99, CellMark::Filled); // should not panic
    }

    #[test]
    fn test_toggle_fill_empty_to_filled() {
        let mut app = app_playing_heart();
        app.toggle_fill(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
    }

    #[test]
    fn test_toggle_fill_filled_to_empty() {
        let mut app = app_playing_heart();
        app.toggle_fill(0, 0);
        app.toggle_fill(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn test_toggle_fill_marked_to_filled() {
        let mut app = app_playing_heart();
        app.set_cell(0, 0, CellMark::MarkedEmpty);
        app.toggle_fill(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
    }

    #[test]
    fn test_toggle_mark_empty_from_empty() {
        let mut app = app_playing_heart();
        app.toggle_mark_empty(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::MarkedEmpty);
    }

    #[test]
    fn test_toggle_mark_empty_from_marked() {
        let mut app = app_playing_heart();
        app.toggle_mark_empty(0, 0);
        app.toggle_mark_empty(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn test_toggle_mark_empty_from_filled() {
        let mut app = app_playing_heart();
        app.set_cell(0, 0, CellMark::Filled);
        app.toggle_mark_empty(0, 0);
        assert_eq!(app.cell_at(0, 0), CellMark::MarkedEmpty);
    }

    // ── Win detection ──────────────────────────────────────────────

    #[test]
    fn test_check_win_empty_is_false() {
        let app = app_playing_heart();
        assert!(!app.check_win());
    }

    #[test]
    fn test_check_win_correct_solution() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        assert!(app.check_win());
    }

    #[test]
    fn test_check_win_extra_fill_is_false() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        // Fill an extra cell that should NOT be filled
        // (0,0) in heart is empty
        app.set_cell(0, 0, CellMark::Filled);
        assert!(!app.check_win());
    }

    #[test]
    fn test_check_win_missing_fill_is_false() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        // Remove one correct fill
        app.set_cell(0, 1, CellMark::Empty);
        assert!(!app.check_win());
    }

    #[test]
    fn test_check_win_marked_empty_not_counted() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        // Mark a filled cell as MarkedEmpty instead
        app.set_cell(0, 1, CellMark::MarkedEmpty);
        assert!(!app.check_win());
    }

    // ── Error detection (check mode) ───────────────────────────────

    #[test]
    fn test_is_error_filled_wrong() {
        let mut app = app_playing_heart();
        // (0,0) in heart is NOT filled in solution
        app.set_cell(0, 0, CellMark::Filled);
        assert!(app.is_error(0, 0));
    }

    #[test]
    fn test_is_error_filled_correct() {
        let mut app = app_playing_heart();
        // (0,1) in heart IS filled in solution
        app.set_cell(0, 1, CellMark::Filled);
        assert!(!app.is_error(0, 1));
    }

    #[test]
    fn test_is_error_marked_empty_wrong() {
        let mut app = app_playing_heart();
        // (0,1) should be filled, marking it empty is an error
        app.set_cell(0, 1, CellMark::MarkedEmpty);
        assert!(app.is_error(0, 1));
    }

    #[test]
    fn test_is_error_marked_empty_correct() {
        let mut app = app_playing_heart();
        // (0,0) should be empty, marking it empty is correct
        app.set_cell(0, 0, CellMark::MarkedEmpty);
        assert!(!app.is_error(0, 0));
    }

    #[test]
    fn test_is_error_empty_cell_never_error() {
        let app = app_playing_heart();
        // Empty cells are never flagged as errors
        assert!(!app.is_error(0, 0));
        assert!(!app.is_error(0, 1));
    }

    #[test]
    fn test_is_error_out_of_bounds() {
        let app = app_playing_heart();
        assert!(!app.is_error(99, 99));
    }

    // ── Counting ───────────────────────────────────────────────────

    #[test]
    fn test_filled_correct_count_none() {
        let app = app_playing_heart();
        assert_eq!(app.filled_correct_count(), 0);
    }

    #[test]
    fn test_filled_correct_count_partial() {
        let mut app = app_playing_heart();
        // Fill (0,1) which is correct
        app.set_cell(0, 1, CellMark::Filled);
        assert_eq!(app.filled_correct_count(), 1);
    }

    #[test]
    fn test_filled_correct_count_full() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        assert_eq!(app.filled_correct_count(), app.total_filled_in_solution());
    }

    #[test]
    fn test_total_filled_in_solution_heart() {
        let app = app_playing_heart();
        // Heart: 2 + 5 + 5 + 3 + 1 = 16
        assert_eq!(app.total_filled_in_solution(), 16);
    }

    #[test]
    fn test_player_filled_count() {
        let mut app = app_playing_heart();
        assert_eq!(app.player_filled_count(), 0);
        app.set_cell(0, 0, CellMark::Filled);
        app.set_cell(0, 1, CellMark::Filled);
        assert_eq!(app.player_filled_count(), 2);
    }

    // ── Timer ──────────────────────────────────────────────────────

    #[test]
    fn test_timer_format_zero() {
        let app = app_playing_heart();
        assert_eq!(app.format_time(), "0:00");
    }

    #[test]
    fn test_timer_format_seconds() {
        let mut app = app_playing_heart();
        app.elapsed_ms = 45_000;
        assert_eq!(app.format_time(), "0:45");
    }

    #[test]
    fn test_timer_format_minutes() {
        let mut app = app_playing_heart();
        app.elapsed_ms = 125_000;
        assert_eq!(app.format_time(), "2:05");
    }

    #[test]
    fn test_timer_advances_while_playing() {
        let mut app = app_playing_heart();
        app.handle_event(&tick(1000));
        assert_eq!(app.elapsed_ms, 1000);
        app.handle_event(&tick(500));
        assert_eq!(app.elapsed_ms, 1500);
    }

    #[test]
    fn test_timer_does_not_advance_on_select() {
        let mut app = NonogramApp::new();
        app.handle_event(&tick(1000));
        assert_eq!(app.elapsed_ms, 0);
    }

    #[test]
    fn test_timer_does_not_advance_after_win() {
        let mut app = app_playing_heart();
        app.handle_event(&tick(2000));
        fill_solution(&mut app);
        app.screen = Screen::Won;
        app.handle_event(&tick(1000));
        assert_eq!(app.elapsed_ms, 2000);
    }

    // ── Keyboard navigation ────────────────────────────────────────

    #[test]
    fn test_cursor_starts_at_origin() {
        let app = app_playing_heart();
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_move_down() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Down));
        assert_eq!(app.cursor_row, 1);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Right));
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_cursor_move_up_clamped() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Up));
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn test_cursor_move_left_clamped() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Left));
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_cursor_move_down_clamped_at_bottom() {
        let mut app = app_playing_heart();
        app.cursor_row = 4; // last row of 5x5
        app.handle_event(&key_press(Key::Down));
        assert_eq!(app.cursor_row, 4);
    }

    #[test]
    fn test_cursor_move_right_clamped_at_edge() {
        let mut app = app_playing_heart();
        app.cursor_col = 4;
        app.handle_event(&key_press(Key::Right));
        assert_eq!(app.cursor_col, 4);
    }

    #[test]
    fn test_cursor_traverse_entire_grid() {
        let mut app = app_playing_heart();
        // Move to bottom-right
        for _ in 0..4 {
            app.handle_event(&key_press(Key::Down));
        }
        for _ in 0..4 {
            app.handle_event(&key_press(Key::Right));
        }
        assert_eq!(app.cursor_row, 4);
        assert_eq!(app.cursor_col, 4);
        // Move back to top-left
        for _ in 0..4 {
            app.handle_event(&key_press(Key::Up));
        }
        for _ in 0..4 {
            app.handle_event(&key_press(Key::Left));
        }
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
    }

    // ── Fill/mark via keyboard ─────────────────────────────────────

    #[test]
    fn test_space_fills_cell() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
    }

    #[test]
    fn test_enter_fills_cell() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
    }

    #[test]
    fn test_space_toggles_fill() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn test_x_marks_empty() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::X));
        assert_eq!(app.cell_at(0, 0), CellMark::MarkedEmpty);
    }

    #[test]
    fn test_x_toggles_mark() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::X));
        assert_eq!(app.cell_at(0, 0), CellMark::MarkedEmpty);
        app.handle_event(&key_press(Key::X));
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = app_playing_heart();
        app.handle_event(&key_release(Key::Space));
        assert_eq!(app.cell_at(0, 0), CellMark::Empty);
    }

    // ── Check mode ─────────────────────────────────────────────────

    #[test]
    fn test_c_toggles_check_mode() {
        let mut app = app_playing_heart();
        assert!(!app.check_mode);
        app.handle_event(&key_press(Key::C));
        assert!(app.check_mode);
        app.handle_event(&key_press(Key::C));
        assert!(!app.check_mode);
    }

    // ── Escape returns to select ───────────────────────────────────

    #[test]
    fn test_escape_returns_to_select() {
        let mut app = app_playing_heart();
        app.handle_event(&key_press(Key::Escape));
        assert_eq!(app.screen, Screen::Select);
    }

    // ── Select screen navigation ───────────────────────────────────

    #[test]
    fn test_select_cursor_moves_down() {
        let mut app = NonogramApp::new();
        app.handle_event(&key_press(Key::Down));
        assert_eq!(app.select_cursor, 1);
    }

    #[test]
    fn test_select_cursor_moves_up() {
        let mut app = NonogramApp::new();
        app.select_cursor = 2;
        app.handle_event(&key_press(Key::Up));
        assert_eq!(app.select_cursor, 1);
    }

    #[test]
    fn test_select_cursor_clamped_at_top() {
        let mut app = NonogramApp::new();
        app.handle_event(&key_press(Key::Up));
        assert_eq!(app.select_cursor, 0);
    }

    #[test]
    fn test_select_cursor_clamped_at_bottom() {
        let mut app = NonogramApp::new();
        let last = app.puzzles.len() - 1;
        app.select_cursor = last;
        app.handle_event(&key_press(Key::Down));
        assert_eq!(app.select_cursor, last);
    }

    #[test]
    fn test_select_enter_starts_puzzle() {
        let mut app = NonogramApp::new();
        app.select_cursor = 2;
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.selected_puzzle, 2);
    }

    #[test]
    fn test_select_space_starts_puzzle() {
        let mut app = NonogramApp::new();
        app.select_cursor = 1;
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.selected_puzzle, 1);
    }

    // ── Win flow ───────────────────────────────────────────────────

    #[test]
    fn test_filling_solution_triggers_win() {
        let mut app = app_playing_heart();
        // Fill all solution cells except the last one
        let total = app.solution.len();
        for i in 0..total {
            if app.solution[i] {
                let r = i / app.grid_side;
                let c = i % app.grid_side;
                app.set_cell(r, c, CellMark::Filled);
            }
        }
        // Find the last filled solution cell to trigger via key
        // We already filled everything, so check_win should be true
        // But screen is still Playing because we used set_cell directly
        assert!(app.check_win());
    }

    #[test]
    fn test_win_via_keyboard() {
        let mut app = app_playing_heart();
        // Fill all solution cells, then unfill one and refill via keyboard
        fill_solution(&mut app);
        // Unfill the first filled cell
        let first_filled = app.solution.iter().position(|&v| v).unwrap();
        let fr = first_filled / app.grid_side;
        let fc = first_filled % app.grid_side;
        app.set_cell(fr, fc, CellMark::Empty);
        assert!(!app.check_win());

        // Move cursor to that cell and fill it via Space
        app.cursor_row = fr;
        app.cursor_col = fc;
        app.handle_event(&key_press(Key::Space));
        assert_eq!(app.screen, Screen::Won);
    }

    #[test]
    fn test_won_screen_enter_returns_to_select() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        app.screen = Screen::Won;
        app.handle_event(&key_press(Key::Enter));
        assert_eq!(app.screen, Screen::Select);
    }

    #[test]
    fn test_won_screen_escape_returns_to_select() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        app.screen = Screen::Won;
        app.handle_event(&key_press(Key::Escape));
        assert_eq!(app.screen, Screen::Select);
    }

    // ── Mouse click on select screen ───────────────────────────────

    #[test]
    fn test_mouse_click_select_starts_puzzle() {
        let mut app = NonogramApp::new();
        let entry_y = HEADER_HEIGHT + PADDING + 0.0 * 40.0 + 10.0;
        app.handle_event(&left_click(30.0, entry_y));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.selected_puzzle, 0);
    }

    // ── Mouse click on grid ────────────────────────────────────────

    #[test]
    fn test_mouse_click_grid_fills_cell() {
        let mut app = app_playing_heart();
        let row_clue_w = app.max_row_clue_len() as f32 * CLUE_SLOT_W;
        let col_clue_h = app.max_col_clue_len() as f32 * CLUE_SLOT_H;
        let gx = PADDING + row_clue_w;
        let gy = HEADER_HEIGHT + PADDING + col_clue_h;

        // Click in cell (0,0)
        let click_x = gx + CELL_SIZE / 2.0;
        let click_y = gy + CELL_SIZE / 2.0;
        app.handle_event(&left_click(click_x, click_y));
        assert_eq!(app.cell_at(0, 0), CellMark::Filled);
    }

    // ── Max clue lengths ───────────────────────────────────────────

    #[test]
    fn test_max_row_clue_len_heart() {
        let app = app_playing_heart();
        // Heart row clues: [1,1], [5], [5], [3], [1] — max = 2
        assert_eq!(app.max_row_clue_len(), 2);
    }

    #[test]
    fn test_max_col_clue_len_heart() {
        let app = app_playing_heart();
        // Heart col clues: [2], [4], [4], [4], [2] — all length 1
        assert_eq!(app.max_col_clue_len(), 1);
    }

    // ── Rendering produces commands ────────────────────────────────

    #[test]
    fn test_render_select_produces_commands() {
        let app = NonogramApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_produces_commands() {
        let app = app_playing_heart();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_won_produces_commands() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        app.screen = Screen::Won;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_select_has_fill_rects() {
        let app = NonogramApp::new();
        let cmds = app.render();
        let fill_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRect { .. }))
            .count();
        assert!(fill_count > 0);
    }

    #[test]
    fn test_render_playing_has_text() {
        let app = app_playing_heart();
        let cmds = app.render();
        let text_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Text { .. }))
            .count();
        assert!(text_count > 0, "Playing screen should contain text commands");
    }

    #[test]
    fn test_render_playing_has_stroke_rect_for_cursor() {
        let app = app_playing_heart();
        let cmds = app.render();
        let stroke_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert!(
            stroke_count > 0,
            "Playing screen should have a cursor stroke rect",
        );
    }

    #[test]
    fn test_render_marked_empty_has_lines() {
        let mut app = app_playing_heart();
        app.set_cell(0, 0, CellMark::MarkedEmpty);
        let cmds = app.render();
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        assert!(line_count >= 2, "MarkedEmpty cell should render X lines");
    }

    // ── Won screen mouse click returns to select ───────────────────

    #[test]
    fn test_won_mouse_click_returns_to_select() {
        let mut app = app_playing_heart();
        fill_solution(&mut app);
        app.screen = Screen::Won;
        app.handle_event(&left_click(100.0, 100.0));
        assert_eq!(app.screen, Screen::Select);
    }

    // ── Medium grid clues ──────────────────────────────────────────

    #[test]
    fn test_medium_puzzle_clue_consistency() {
        let mut app = NonogramApp::new();
        let idx = app
            .puzzles
            .iter()
            .position(|p| p.size == GridSize::Medium)
            .unwrap();
        app.start_puzzle(idx);
        assert_eq!(app.row_clues.len(), 10);
        assert_eq!(app.col_clues.len(), 10);
        // Sum of all row clue values should equal total filled cells
        let row_sum: u32 = app.row_clues.iter().flat_map(|c| c.iter()).map(|&v| v as u32).sum();
        let filled = app.total_filled_in_solution() as u32;
        assert_eq!(row_sum, filled);
    }

    #[test]
    fn test_col_clue_sum_equals_filled() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        let col_sum: u32 = app.col_clues.iter().flat_map(|c| c.iter()).map(|&v| v as u32).sum();
        let filled = app.total_filled_in_solution() as u32;
        assert_eq!(col_sum, filled);
    }

    // ── Row and column clue sums match for all puzzles ─────────────

    #[test]
    fn test_all_puzzles_row_col_sums_match() {
        let mut app = NonogramApp::new();
        for i in 0..app.puzzles.len() {
            app.start_puzzle(i);
            let row_sum: u32 = app
                .row_clues
                .iter()
                .flat_map(|c| c.iter())
                .map(|&v| v as u32)
                .sum();
            let col_sum: u32 = app
                .col_clues
                .iter()
                .flat_map(|c| c.iter())
                .map(|&v| v as u32)
                .sum();
            assert_eq!(
                row_sum, col_sum,
                "Row/col clue sums should match for puzzle '{}'",
                app.puzzles[i].name,
            );
        }
    }

    // ── Render medium grid with group lines ─────────────────────────

    #[test]
    fn test_render_medium_grid_has_group_lines() {
        let mut app = NonogramApp::new();
        let idx = app
            .puzzles
            .iter()
            .position(|p| p.size == GridSize::Medium)
            .unwrap();
        app.start_puzzle(idx);
        let cmds = app.render();
        let line_count = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        // 10x10 grid should have group lines at position 5 (1 vertical + 1 horizontal)
        assert!(line_count >= 2, "Medium grid should have group divider lines");
    }
}
