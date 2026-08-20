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

//! Slate OS Nonogram (Picross / picture-logic puzzle) game.
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
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
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

/// Half a clue number's drawn width and height at `CLUE_FONT_SIZE`.
///
/// `RenderCommand::Text` is positioned by its top-left corner, so a number is
/// centred on a row or column by starting it this far up and to the left of
/// that row's or column's middle. The figures are eyeballed -- the renderer has
/// no text-metrics call to ask -- which is why they are named rather than left
/// as bare literals subtracted from an inline half-cell.
const CLUE_HALF_WIDTH: f32 = 4.0;
/// Half a clue number's drawn height at `CLUE_FONT_SIZE`. See
/// `CLUE_HALF_WIDTH`.
const CLUE_HALF_HEIGHT: f32 = 7.0;

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
        self.cells
            .iter()
            .filter(|&&c| c == CellMark::Filled)
            .count()
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

    // ── Grid geometry ──────────────────────────────────────────────
    //
    // Where a cell is painted and which cell a click lands in used to be
    // worked out separately: `CELL_SIZE + CELL_GAP` was spelled out at five
    // sites and the grid's origin at two, once in `handle_mouse_playing` and
    // once in `render_playing`. That origin is not a constant -- it is pushed
    // right and down by however much room this puzzle's clues need -- so the
    // two copies had to agree about the clue slot sizes as well as about the
    // cells, and a change to either would have moved the picture without
    // moving the hit test. A third copy lived in the tests, which meant the
    // suite was measuring its own arithmetic rather than the app's; see
    // design-decisions.md §486. These are now the one place it is written.

    /// Distance from one cell's near edge to the next one's.
    const CELL_STEP: f32 = CELL_SIZE + CELL_GAP;

    /// Width of the band of row clues down the left-hand side.
    ///
    /// Every row is given the same width -- that of the wordiest row in this
    /// puzzle -- so that the clue columns line up with each other.
    fn row_clue_area_w(&self) -> f32 {
        self.max_row_clue_len() as f32 * CLUE_SLOT_W
    }

    /// Height of the band of column clues across the top. See
    /// `row_clue_area_w`.
    fn col_clue_area_h(&self) -> f32 {
        self.max_col_clue_len() as f32 * CLUE_SLOT_H
    }

    /// Screen coordinates of the grid's top-left corner.
    fn grid_origin(&self) -> (f32, f32) {
        (
            PADDING + self.row_clue_area_w(),
            HEADER_HEIGHT + PADDING + self.col_clue_area_h(),
        )
    }

    /// Distance across the whole grid, in pixels.
    ///
    /// The last cell has no gap after it, so this is one gap short of
    /// `side * CELL_STEP` -- which is the sort of detail that goes wrong when
    /// the window size and the cells are measured by different code.
    fn grid_pixel_span(&self) -> f32 {
        self.grid_side as f32 * Self::CELL_STEP - CELL_GAP
    }

    /// Screen coordinates of the top-left corner of a cell.
    fn cell_origin(&self, row: usize, col: usize) -> (f32, f32) {
        let (ox, oy) = self.grid_origin();
        (
            ox + col as f32 * Self::CELL_STEP,
            oy + row as f32 * Self::CELL_STEP,
        )
    }

    /// Screen coordinates of the middle of a cell.
    fn cell_center(&self, row: usize, col: usize) -> (f32, f32) {
        let (x, y) = self.cell_origin(row, col);
        (x + CELL_SIZE / 2.0, y + CELL_SIZE / 2.0)
    }

    /// The row or column an offset from the grid's near edge falls in, or
    /// `None` if it falls outside the grid.
    ///
    /// Each cell owns half of the gap on either side of it, which makes the
    /// mapping forgiving in the middle of the grid *and* symmetric at its
    /// edges: there is exactly `CELL_GAP / 2` of slop past the first cell's
    /// near edge and past the last cell's far edge, the same as between any
    /// two neighbours. The previous version handed each cell the whole gap
    /// *after* it, which gave the right-hand and bottom edges a gap's worth of
    /// slop and the left and top edges none -- an asymmetry small enough never
    /// to be noticed and still worth not having. Nonogram is a game of many
    /// rapid clicks, so slop is wanted here; sudoku made the opposite call for
    /// the opposite reason (design-decisions.md §486).
    ///
    /// The bounds are checked before any cast, because a float-to-integer cast
    /// in Rust truncates toward zero and would turn a point just left of the
    /// grid into column 0. See known-issues.md
    /// `C-GOMOKU-THE-CLICK-SLOP-ONLY-WORKED-ON-TWO-EDGES`.
    fn axis_index(&self, offset: f32) -> Option<usize> {
        let shifted = offset + CELL_GAP / 2.0;
        if shifted < 0.0 || shifted >= self.grid_side as f32 * Self::CELL_STEP {
            return None;
        }
        // Truncation is the intent -- the fraction is the position within the
        // cell -- and the guard above is what makes the cast safe.
        Some((shifted / Self::CELL_STEP) as usize)
    }

    /// The cell a click at `(x, y)` lands on, or `None` if it misses the grid.
    fn cell_at_point(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let (ox, oy) = self.grid_origin();
        let row = self.axis_index(y - oy)?;
        let col = self.axis_index(x - ox)?;
        Some((row, col))
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
            Key::Up if self.select_cursor > 0 => {
                self.select_cursor -= 1;
            }
            Key::Down if self.select_cursor + 1 < self.puzzles.len() => {
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
            Key::Up if self.cursor_row > 0 => {
                self.cursor_row -= 1;
            }
            Key::Down if self.cursor_row + 1 < self.grid_side => {
                self.cursor_row += 1;
            }
            Key::Left if self.cursor_col > 0 => {
                self.cursor_col -= 1;
            }
            Key::Right if self.cursor_col + 1 < self.grid_side => {
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

        if let Some((row, col)) = self.cell_at_point(mx, my) {
            self.cursor_row = row;
            self.cursor_col = col;
            self.toggle_fill(row, col);
            if self.check_win() {
                self.screen = Screen::Won;
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: PADDING + 160.0,
                y: entry_y + 10.0,
                text: format!("({})", puzzle.size.label()),
                color: SUBTEXT0,
                font_size: STATUS_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
        });

        cmds
    }

    fn render_playing(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let grid_span = self.grid_pixel_span();
        let (grid_origin_x, grid_origin_y) = self.grid_origin();

        let footer_height = 40.0;
        // The window is measured from the grid's far edge rather than restating
        // the clue widths, so it cannot disagree with where the grid is drawn.
        let total_width = grid_origin_x + grid_span + PADDING;
        let total_height = grid_origin_y + grid_span + footer_height + PADDING;

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
        self.render_col_clues(&mut cmds);

        // Row clues
        self.render_row_clues(&mut cmds);

        // Grid cells
        self.render_grid(&mut cmds);

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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_col_clues(&self, cmds: &mut Vec<RenderCommand>) {
        let max_len = self.max_col_clue_len();
        // The clue band sits directly on top of the grid, so its top edge is
        // the grid's own origin less the band's height -- derived rather than
        // restated, so the clues cannot drift away from the columns they
        // describe.
        let clue_area_y = self.grid_origin().1 - self.col_clue_area_h();

        for (c, clue) in self.col_clues.iter().enumerate() {
            let is_current = self.screen == Screen::Playing && c == self.cursor_col;
            // Centred on the column it belongs to, so a clue is over its own
            // column whatever the spacing is.
            let center_x = self.cell_center(0, c).0;
            // Bottom-align clue numbers against the grid: the last value of
            // every clue sits in the slot immediately above the first row.
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
                    x: center_x - CLUE_HALF_WIDTH,
                    y: cy,
                    text: val.to_string(),
                    color,
                    font_size: CLUE_FONT_SIZE,
                    font_weight: weight,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }
    }

    fn render_row_clues(&self, cmds: &mut Vec<RenderCommand>) {
        let max_len = self.max_row_clue_len();
        // The clue band butts up against the grid's left edge; see
        // `render_col_clues` for why this is derived and not restated.
        let clue_area_x = self.grid_origin().0 - self.row_clue_area_w();

        for (r, clue) in self.row_clues.iter().enumerate() {
            let is_current = self.screen == Screen::Playing && r == self.cursor_row;
            // Centred on the row it belongs to.
            let base_y = self.cell_center(r, 0).1 - CLUE_HALF_HEIGHT;
            // Right-align clue numbers against the grid: the last value of
            // every clue sits in the slot immediately left of the first column.
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
                    overflow: TextOverflow::Clip,
                });
            }
        }
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let (grid_origin_x, grid_origin_y) = self.grid_origin();

        for r in 0..self.grid_side {
            for c in 0..self.grid_side {
                let (cx, cy) = self.cell_origin(r, c);

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
                        if error {
                            Color::rgba(243, 139, 168, 60)
                        } else {
                            SURFACE0
                        }
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
            let span = self.grid_pixel_span();
            for g in 1..(self.grid_side / 5) {
                // Down the middle of the gap before the group's first cell,
                // taken from that cell's own origin so the rule always lands
                // between the two cells it separates.
                let cell = g * 5;
                let (bx, by) = self.cell_origin(cell, cell);
                // Vertical line
                cmds.push(RenderCommand::Line {
                    x1: bx - CELL_GAP / 2.0,
                    y1: grid_origin_y,
                    x2: bx - CELL_GAP / 2.0,
                    y2: grid_origin_y + span,
                    color: line_color,
                    width: 1.5,
                });
                // Horizontal line
                cmds.push(RenderCommand::Line {
                    x1: grid_origin_x,
                    y1: by - CELL_GAP / 2.0,
                    x2: grid_origin_x + span,
                    y2: by - CELL_GAP / 2.0,
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
            overflow: TextOverflow::Clip,
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
            overflow: TextOverflow::Clip,
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
    // A test that indexes past the end, or unwraps a `None`, is a test that
    // has already failed; panicking is the reporting mechanism, not a fault.
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used
    )]

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
        let heart = parse_picture(".#.#.\n#####\n#####\n.###.\n..#..", 5);
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
        let heart = parse_picture(".#.#.\n#####\n#####\n.###.\n..#..", 5);
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
        let arrow = parse_picture("..#..\n.##..\n#####\n.##..\n..#..", 5);
        let row_clues = compute_row_clues(&arrow, 5);
        assert_eq!(row_clues[0], vec![1]);
        assert_eq!(row_clues[1], vec![2]);
        assert_eq!(row_clues[2], vec![5]);
        assert_eq!(row_clues[3], vec![2]);
        assert_eq!(row_clues[4], vec![1]);
    }

    #[test]
    fn test_compute_col_clues_arrow() {
        let arrow = parse_picture("..#..\n.##..\n#####\n.##..\n..#..", 5);
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
        let grid = parse_picture(".#.#.\n#####\n#####\n.###.\n..#..", 5);
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
        assert!(
            puzzles.len() >= 8,
            "Should have at least 8 built-in puzzles"
        );
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
        // This test used to work the grid's origin out for itself from
        // PADDING, HEADER_HEIGHT and the clue slot sizes -- a second copy of
        // the app's own arithmetic, which meant it would have gone on passing
        // if the grid had moved. It now asks the app where a cell is; that the
        // answer matches the picture is what the geometry tests below check.
        let mut app = app_playing_heart();
        let (cx, cy) = app.cell_center(2, 3);
        app.handle_event(&left_click(cx, cy));
        assert_eq!(app.cell_at(2, 3), CellMark::Filled);
        assert_eq!((app.cursor_row, app.cursor_col), (2, 3));
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
        assert!(
            text_count > 0,
            "Playing screen should contain text commands"
        );
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
        let row_sum: u32 = app
            .row_clues
            .iter()
            .flat_map(|c| c.iter())
            .map(|&v| v as u32)
            .sum();
        let filled = app.total_filled_in_solution() as u32;
        assert_eq!(row_sum, filled);
    }

    #[test]
    fn test_col_clue_sum_equals_filled() {
        let mut app = NonogramApp::new();
        app.start_puzzle(0);
        let col_sum: u32 = app
            .col_clues
            .iter()
            .flat_map(|c| c.iter())
            .map(|&v| v as u32)
            .sum();
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
        assert!(
            line_count >= 2,
            "Medium grid should have group divider lines"
        );
    }

    // ── Grid geometry, measured against the picture ────────────────
    //
    // Written to the checklist in design-decisions.md §485: the lattice
    // against the window, every point of a cell rather than its middle, all
    // four edges inward and outward, a far sweep well past the grid, and each
    // element's place *within* the cell that holds it. Expected values come
    // from the render list wherever they can, never from the function under
    // test -- the previous mouse test worked the grid's origin out for itself
    // and would have passed with the grid drawn anywhere at all.

    /// Top-left corners of the painted cells, in render order.
    ///
    /// A cell is the only thing drawn exactly `CELL_SIZE` square, which picks
    /// them out of the render list without consulting `cell_origin`.
    fn painted_cells(cmds: &[RenderCommand]) -> Vec<(f32, f32)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } if (*width - CELL_SIZE).abs() < 0.01 && (*height - CELL_SIZE).abs() < 0.01 => {
                    Some((*x, *y))
                }
                _ => None,
            })
            .collect()
    }

    /// The painted cell containing `(x, y)`, if any.
    fn painted_cell_containing(cmds: &[RenderCommand], x: f32, y: f32) -> Option<(f32, f32)> {
        painted_cells(cmds)
            .into_iter()
            .find(|&(cx, cy)| x >= cx && x < cx + CELL_SIZE && y >= cy && y < cy + CELL_SIZE)
    }

    /// The window the app asks for, as the background rectangle states it.
    fn window_size(cmds: &[RenderCommand]) -> (f32, f32) {
        match cmds.first() {
            Some(RenderCommand::FillRect { width, height, .. }) => (*width, *height),
            other => panic!("expected a background rect first, got {other:?}"),
        }
    }

    // ── 1. The lattice, measured against the window ────────────

    #[test]
    fn the_painted_grid_is_square_evenly_spaced_and_fits_the_window() {
        // Every puzzle, because the grid's origin depends on how much room
        // this puzzle's clues need -- a 5x5 with one-figure clues and a 15x15
        // with four-figure ones do not start in the same place.
        for idx in 0..NonogramApp::new().puzzles.len() {
            let mut app = NonogramApp::new();
            app.start_puzzle(idx);
            let side = app.grid_side;
            let cmds = app.render();
            let cells = painted_cells(&cmds);
            assert_eq!(cells.len(), side * side, "puzzle {idx}: one rect per cell");

            let mut xs: Vec<f32> = cells.iter().map(|c| c.0).collect();
            let mut ys: Vec<f32> = cells.iter().map(|c| c.1).collect();
            xs.sort_by(f32::total_cmp);
            xs.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            ys.sort_by(f32::total_cmp);
            ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
            assert_eq!(xs.len(), side, "puzzle {idx}: {side} distinct columns");
            assert_eq!(ys.len(), side, "puzzle {idx}: {side} distinct rows");

            // Stated against the constants rather than against `CELL_STEP`, so
            // a change to the spacing has to be made here too.
            let step = CELL_SIZE + CELL_GAP;
            for pair in xs.windows(2) {
                assert!(
                    (pair[1] - pair[0] - step).abs() < 0.01,
                    "puzzle {idx}: columns are {} apart, expected {step}",
                    pair[1] - pair[0]
                );
            }
            for pair in ys.windows(2) {
                assert!(
                    (pair[1] - pair[0] - step).abs() < 0.01,
                    "puzzle {idx}: rows are {} apart, expected {step}",
                    pair[1] - pair[0]
                );
            }

            let (win_w, win_h) = window_size(&cmds);
            let right = xs[side - 1] + CELL_SIZE;
            let bottom = ys[side - 1] + CELL_SIZE;
            assert!(
                xs[0] > 0.0 && ys[0] > 0.0,
                "puzzle {idx}: the grid starts inside the window"
            );
            assert!(
                right < win_w,
                "puzzle {idx}: the grid ends inside the window, {right} vs {win_w}"
            );
            assert!(
                bottom < win_h,
                "puzzle {idx}: the grid ends above the footer, {bottom} vs {win_h}"
            );
            assert!(
                ys[0] >= HEADER_HEIGHT,
                "puzzle {idx}: the grid starts below the header, {} vs {HEADER_HEIGHT}",
                ys[0]
            );

            // The frame is even: the margin to the right of the last column is
            // the same as the margin to the left of the clue band. "Inside the
            // window" alone would let the window be any amount too wide, which
            // is exactly what happens if the grid's width is measured with a
            // trailing gap the last cell does not have.
            let band_w = app.max_row_clue_len() as f32 * CLUE_SLOT_W;
            let left_margin = xs[0] - band_w;
            let right_margin = win_w - right;
            assert!(
                (left_margin - right_margin).abs() < 0.01,
                "puzzle {idx}: {left_margin} of margin on the left but {right_margin} on the right"
            );
        }
    }

    #[test]
    fn the_clue_bands_leave_room_for_the_wordiest_clue_and_no_more() {
        // The two bands are what push the grid right and down, so their width
        // is measured as the gap between the window's padding and the grid the
        // renderer actually painted -- not as the app's own formula.
        for idx in 0..NonogramApp::new().puzzles.len() {
            let mut app = NonogramApp::new();
            app.start_puzzle(idx);
            let cmds = app.render();
            let cells = painted_cells(&cmds);
            let left = cells
                .iter()
                .map(|c| c.0)
                .fold(f32::INFINITY, |a, b| if b < a { b } else { a });
            let top = cells
                .iter()
                .map(|c| c.1)
                .fold(f32::INFINITY, |a, b| if b < a { b } else { a });
            let expected_w = app.max_row_clue_len() as f32 * CLUE_SLOT_W;
            let expected_h = app.max_col_clue_len() as f32 * CLUE_SLOT_H;
            assert!(
                (left - PADDING - expected_w).abs() < 0.01,
                "puzzle {idx}: row clue band is {} wide, expected {expected_w}",
                left - PADDING
            );
            assert!(
                (top - HEADER_HEIGHT - PADDING - expected_h).abs() < 0.01,
                "puzzle {idx}: column clue band is {} tall, expected {expected_h}",
                top - HEADER_HEIGHT - PADDING
            );
        }
    }

    // ── 2. Every point of a cell, not just its middle ──────────

    #[test]
    fn every_point_in_a_painted_cell_resolves_to_that_cell() {
        // A mapping shifted by less than half a cell still answers correctly
        // dead centre, which is all the old round-trip test ever asked.
        let app = app_playing_heart();
        let cmds = app.render();
        let inset = 0.5;
        let far = CELL_SIZE - 0.5;
        for row in 0..app.grid_side {
            for col in 0..app.grid_side {
                let (ox, oy) = app.cell_origin(row, col);
                for dx in [inset, CELL_SIZE / 2.0, far] {
                    for dy in [inset, CELL_SIZE / 2.0, far] {
                        assert_eq!(
                            app.cell_at_point(ox + dx, oy + dy),
                            Some((row, col)),
                            "({dx}, {dy}) into cell ({row}, {col}) missed it"
                        );
                    }
                }
                // And that cell is the one actually painted there.
                let (mx, my) = app.cell_center(row, col);
                assert_eq!(
                    painted_cell_containing(&cmds, mx, my),
                    Some((ox, oy)),
                    "cell ({row}, {col}) is not painted where cell_origin says"
                );
            }
        }
    }

    // ── 3. All four edges, inward and outward ───────────────

    #[test]
    fn the_grid_edges_take_the_same_slop_on_all_four_sides() {
        // Each cell owns half the gap on either side of it, so the outermost
        // cells reach exactly CELL_GAP / 2 beyond the painted grid -- the same
        // on every side. The previous mapping gave the right and bottom edges
        // a whole gap and the left and top none. Note the clickable region is
        // the half-open box [near, far), so the two ends are not mirror images
        // and no offset makes them so; state the interval (§485).
        let app = app_playing_heart();
        let side = app.grid_side;
        let (left, top) = app.cell_origin(0, 0);
        let (last_x, last_y) = app.cell_origin(side - 1, side - 1);
        let slop = CELL_GAP / 2.0;
        let near_x = left - slop;
        let near_y = top - slop;
        let far_x = last_x + CELL_SIZE + slop;
        let far_y = last_y + CELL_SIZE + slop;
        let mid_x = app.cell_center(0, side / 2).0;
        let mid_y = app.cell_center(side / 2, 0).1;

        assert_eq!(
            app.cell_at_point(near_x, mid_y),
            Some((side / 2, 0)),
            "the left edge takes its half-gap of slop"
        );
        assert_eq!(
            app.cell_at_point(mid_x, near_y),
            Some((0, side / 2)),
            "the top edge takes its half-gap of slop"
        );
        assert_eq!(
            app.cell_at_point(far_x - 0.01, mid_y),
            Some((side / 2, side - 1)),
            "the right edge takes its half-gap of slop"
        );
        assert_eq!(
            app.cell_at_point(mid_x, far_y - 0.01),
            Some((side - 1, side / 2)),
            "the bottom edge takes its half-gap of slop"
        );

        assert_eq!(
            app.cell_at_point(near_x - 0.01, mid_y),
            None,
            "a hair further left than the slop is off the grid"
        );
        assert_eq!(
            app.cell_at_point(mid_x, near_y - 0.01),
            None,
            "a hair above the slop is off the grid"
        );
        assert_eq!(
            app.cell_at_point(far_x, mid_y),
            None,
            "a hair past the right-hand slop is off the grid"
        );
        assert_eq!(
            app.cell_at_point(mid_x, far_y),
            None,
            "a hair below the bottom slop is off the grid"
        );
    }

    // ── 4. A far sweep, well past the grid ──────────────────

    #[test]
    fn nothing_far_outside_the_painted_grid_lands_on_a_cell() {
        // A hit test with no far edge answers the last cell for every point
        // past the grid, and a single probe just outside it would not tell.
        let app = app_playing_heart();
        let cmds = app.render();
        let (win_w, win_h) = window_size(&cmds);
        let slop = CELL_GAP / 2.0;
        let mut y = -40.0;
        while y < win_h + 40.0 {
            let mut x = -40.0;
            while x < win_w + 40.0 {
                let hit = app.cell_at_point(x, y);
                match hit {
                    Some((r, c)) => {
                        let (ox, oy) = app.cell_origin(r, c);
                        assert!(
                            x >= ox - slop
                                && x < ox + CELL_SIZE + slop
                                && y >= oy - slop
                                && y < oy + CELL_SIZE + slop,
                            "({x}, {y}) was answered as cell ({r}, {c}) painted at ({ox}, {oy})"
                        );
                    }
                    None => {
                        assert!(
                            painted_cell_containing(&cmds, x, y).is_none(),
                            "({x}, {y}) is inside a painted cell but cell_at_point says otherwise"
                        );
                    }
                }
                x += 6.0;
            }
            y += 6.0;
        }
    }

    // ── 5. Each element's place *within* its cell ────────────

    #[test]
    fn a_row_clue_is_painted_level_with_the_row_it_describes() {
        // Measured against the painted cells of that row, never against
        // `cell_origin` -- the clue has to be over its own row on screen.
        let app = app_playing_heart();
        let cmds = app.render();
        let (left, _) = app.cell_origin(0, 0);
        for (r, clue) in app.row_clues.iter().enumerate() {
            let (_, oy) = app.cell_origin(r, 0);
            let last = *clue.last().expect("every heart row has a clue");
            let found = cmds.iter().any(|c| match c {
                RenderCommand::Text { x, y, text, .. } => {
                    *text == last.to_string() && *x < left && *y >= oy && *y < oy + CELL_SIZE
                }
                _ => false,
            });
            assert!(
                found,
                "row {r}'s clue {last} is not drawn left of the grid and level with the row painted at y={oy}"
            );
        }
    }

    #[test]
    fn a_column_clue_is_painted_above_the_column_it_describes() {
        let app = app_playing_heart();
        let cmds = app.render();
        let (_, top) = app.cell_origin(0, 0);
        for (c, clue) in app.col_clues.iter().enumerate() {
            let (ox, _) = app.cell_origin(0, c);
            let last = *clue.last().expect("every heart column has a clue");
            let found = cmds.iter().any(|cmd| match cmd {
                RenderCommand::Text { x, y, text, .. } => {
                    *text == last.to_string() && *y < top && *x >= ox && *x < ox + CELL_SIZE
                }
                _ => false,
            });
            assert!(
                found,
                "column {c}'s clue {last} is not drawn above the grid and over the column painted at x={ox}"
            );
        }
    }

    #[test]
    fn the_cursor_is_outlined_where_that_cell_is_painted() {
        let mut app = app_playing_heart();
        app.cursor_row = 3;
        app.cursor_col = 1;
        let cmds = app.render();
        let (ox, oy) = app.cell_origin(3, 1);
        let outline = cmds.iter().any(|c| match c {
            RenderCommand::StrokeRect {
                x, y, width, color, ..
            } => {
                *color == YELLOW
                    && *width > CELL_SIZE
                    && (*x - (ox - 1.0)).abs() < 0.01
                    && (*y - (oy - 1.0)).abs() < 0.01
            }
            _ => false,
        });
        assert!(
            outline,
            "the cursor outline is not drawn around the cell painted at ({ox}, {oy})"
        );
    }

    // ── 6. The group rules fall between cells, not across them ──

    #[test]
    fn the_group_rules_fall_in_the_gaps_between_cells() {
        // A rule every five cells marks a boundary; if it is drawn a whole
        // cell out it crosses the cells it is meant to separate. Checked
        // against the painted cells: no rule may pass through one.
        let mut app = NonogramApp::new();
        let idx = app
            .puzzles
            .iter()
            .position(|p| p.size == GridSize::Medium)
            .expect("a medium puzzle");
        app.start_puzzle(idx);
        let cmds = app.render();
        let cells = painted_cells(&cmds);
        let mut verticals = 0;
        let mut horizontals = 0;
        for cmd in &cmds {
            let RenderCommand::Line { x1, y1, x2, y2, .. } = cmd else {
                continue;
            };
            if (x1 - x2).abs() < 0.01 {
                verticals += 1;
                for &(cx, _) in &cells {
                    assert!(
                        *x1 <= cx || *x1 >= cx + CELL_SIZE,
                        "a vertical rule at x={x1} crosses the cell painted at x={cx}"
                    );
                }
            } else if (y1 - y2).abs() < 0.01 {
                horizontals += 1;
                for &(_, cy) in &cells {
                    assert!(
                        *y1 <= cy || *y1 >= cy + CELL_SIZE,
                        "a horizontal rule at y={y1} crosses the cell painted at y={cy}"
                    );
                }
            }
        }
        assert_eq!(verticals, 1, "a 10-wide grid has one vertical group rule");
        assert_eq!(horizontals, 1, "and one horizontal");
    }
}
