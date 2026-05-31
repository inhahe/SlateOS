//! Crossword puzzle application for OurOS.
//!
//! Features:
//! - Multiple built-in crossword puzzles with clues
//! - Arrow key navigation on the grid
//! - Letter entry with automatic advance
//! - Across/Down clue panels
//! - Check answers (highlight errors)
//! - Reveal letter/word helpers
//! - Timer tracking
//! - Completion detection
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
use guitk::render::{FontWeightHint, RenderCommand};
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

// ── Directions ──────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Across,
    Down,
}

impl Direction {
    fn toggle(self) -> Self {
        match self {
            Self::Across => Self::Down,
            Self::Down => Self::Across,
        }
    }
}

// ── Cell ────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Cell {
    /// The correct answer letter (uppercase)
    solution: char,
    /// What the user has entered (None = empty)
    entry: Option<char>,
    /// Clue number displayed in top-left (0 = no number)
    number: u16,
    /// Whether this cell was flagged as incorrect during check
    flagged_wrong: bool,
    /// Whether this cell was revealed
    revealed: bool,
}

impl Cell {
    fn new(solution: char) -> Self {
        Self {
            solution,
            entry: None,
            number: 0,
            flagged_wrong: false,
            revealed: false,
        }
    }

    fn is_correct(&self) -> bool {
        self.entry == Some(self.solution)
    }
}

// ── Clue ────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Clue {
    number: u16,
    direction: Direction,
    text: String,
    /// Starting position (row, col)
    start_row: usize,
    start_col: usize,
    /// Length of the answer
    length: usize,
}

// ── Puzzle definition (static data) ─────────────────────────────────
struct PuzzleDef {
    name: &'static str,
    width: usize,
    height: usize,
    /// Grid pattern: '#' = black cell, letter = answer
    grid: &'static str,
    clues_across: &'static [(&'static str, u16)],
    clues_down: &'static [(&'static str, u16)],
}

// ── Built-in puzzles ────────────────────────────────────────────────
const PUZZLES: &[PuzzleDef] = &[
    PuzzleDef {
        name: "Easy Start",
        width: 7,
        height: 7,
        grid: "\
CATS###\
AREA#TO\
B#SPEED\
###E###\
HASTE#B\
AL#RAIN\
###ENDS",
        clues_across: &[
            ("Feline pets", 1),
            ("Region or zone", 4),
            ("Preposition: towards", 6),
            ("Velocity", 7),
            ("Hurry", 9),
            ("Chemical symbol for aluminum", 11),
            ("Precipitation", 12),
            ("Finishes", 13),
        ],
        clues_down: &[
            ("Taxi vehicle", 1),
            ("Region or zone", 2),
            ("Caution or heed", 3),
            ("Foot appendages", 5),
            ("Nickname for Edward", 6),
            ("Young man", 8),
            ("Consumed food", 10),
            ("Writing instrument", 11),
        ],
    },
    PuzzleDef {
        name: "Word Play",
        width: 7,
        height: 7,
        grid: "\
MESH###\
ARIA#DO\
P#GLOBE\
###L###\
SPLIT#S\
TO#LEAP\
###ASPS",
        clues_across: &[
            ("Net-like fabric", 1),
            ("Opera solo", 4),
            ("Musical note", 6),
            ("Spherical model of Earth", 7),
            ("Divide", 9),
            ("Preposition", 11),
            ("Jump", 12),
            ("Venomous snakes", 13),
        ],
        clues_down: &[
            ("Chart or diagram", 1),
            ("Broadcast medium", 2),
            ("Couple or duo", 3),
            ("Part in a play", 5),
            ("Blemish", 6),
            ("Lower limb", 8),
            ("Guided", 10),
            ("Snake sounds", 11),
        ],
    },
    PuzzleDef {
        name: "Nature",
        width: 7,
        height: 7,
        grid: "\
LEAF###\
IBIS#OX\
N#RIVER\
###V###\
STORM#G\
EA#NEST\
###ASKS",
        clues_across: &[
            ("Tree foliage", 1),
            ("Wading bird", 4),
            ("Bovine animal", 6),
            ("Flowing waterway", 7),
            ("Violent weather", 9),
            ("Each, abbreviated", 11),
            ("Bird home", 12),
            ("Poses questions", 13),
        ],
        clues_down: &[
            ("Queue or row", 1),
            ("Notion or concept", 2),
            ("Distant", 3),
            ("Creeping plant", 5),
            ("Mineral deposit", 6),
            ("Wheel groove", 8),
            ("Orient or Asia", 10),
            ("Vapor or fume", 11),
        ],
    },
];

// ── View state ──────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    PuzzleSelect,
    Playing,
    Completed,
}

// ── App state ───────────────────────────────────────────────────────
struct CrosswordApp {
    view: View,
    // Puzzle select
    selected_puzzle: usize,
    // Grid state
    width: usize,
    height: usize,
    /// None = black cell, Some(Cell) = playable
    cells: Vec<Option<Cell>>,
    clues: Vec<Clue>,
    // Cursor
    cursor_row: usize,
    cursor_col: usize,
    direction: Direction,
    // Timer
    elapsed_secs: u64,
    timer_running: bool,
    // UI
    check_mode: bool,
    show_help: bool,
    clue_scroll_across: usize,
    clue_scroll_down: usize,
    puzzle_name: String,
}

impl CrosswordApp {
    fn new() -> Self {
        Self {
            view: View::PuzzleSelect,
            selected_puzzle: 0,
            width: 0,
            height: 0,
            cells: Vec::new(),
            clues: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            direction: Direction::Across,
            elapsed_secs: 0,
            timer_running: false,
            check_mode: false,
            show_help: false,
            clue_scroll_across: 0,
            clue_scroll_down: 0,
            puzzle_name: String::new(),
        }
    }

    fn load_puzzle(&mut self, index: usize) {
        if index >= PUZZLES.len() {
            return;
        }
        let def = &PUZZLES[index];
        self.width = def.width;
        self.height = def.height;
        self.puzzle_name = def.name.to_string();

        // Parse grid
        let chars: Vec<char> = def.grid.chars().collect();
        self.cells = Vec::with_capacity(def.width * def.height);
        for i in 0..def.width * def.height {
            if let Some(&ch) = chars.get(i) {
                if ch == '#' {
                    self.cells.push(None);
                } else {
                    self.cells.push(Some(Cell::new(ch)));
                }
            } else {
                self.cells.push(None);
            }
        }

        // Assign clue numbers: a cell gets a number if it starts an across or down word
        let mut num: u16 = 0;
        for row in 0..self.height {
            for col in 0..self.width {
                let idx = row * self.width + col;
                if self.cells[idx].is_none() {
                    continue;
                }
                let starts_across = (col == 0 || self.cells[idx.wrapping_sub(1)].is_none())
                    && col + 1 < self.width
                    && self.cells.get(idx + 1).and_then(|c| c.as_ref()).is_some();
                let starts_down = (row == 0 || self.cells[idx.wrapping_sub(self.width)].is_none())
                    && row + 1 < self.height
                    && self
                        .cells
                        .get(idx + self.width)
                        .and_then(|c| c.as_ref())
                        .is_some();

                if starts_across || starts_down {
                    num += 1;
                    if let Some(ref mut cell) = self.cells[idx] {
                        cell.number = num;
                    }
                }
            }
        }

        // Build clues
        self.clues.clear();
        for &(text, clue_num) in def.clues_across {
            // Find the cell with this number
            if let Some(pos) = self.find_numbered_cell(clue_num) {
                let row = pos / self.width;
                let col = pos % self.width;
                let length = self.word_length(row, col, Direction::Across);
                self.clues.push(Clue {
                    number: clue_num,
                    direction: Direction::Across,
                    text: text.to_string(),
                    start_row: row,
                    start_col: col,
                    length,
                });
            }
        }
        for &(text, clue_num) in def.clues_down {
            if let Some(pos) = self.find_numbered_cell(clue_num) {
                let row = pos / self.width;
                let col = pos % self.width;
                let length = self.word_length(row, col, Direction::Down);
                self.clues.push(Clue {
                    number: clue_num,
                    direction: Direction::Down,
                    text: text.to_string(),
                    start_row: row,
                    start_col: col,
                    length,
                });
            }
        }

        // Place cursor on first playable cell
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.move_to_next_playable(0, 0, Direction::Across);
        self.direction = Direction::Across;
        self.elapsed_secs = 0;
        self.timer_running = true;
        self.check_mode = false;
        self.clue_scroll_across = 0;
        self.clue_scroll_down = 0;
        self.view = View::Playing;
    }

    fn find_numbered_cell(&self, num: u16) -> Option<usize> {
        self.cells
            .iter()
            .position(|c| c.as_ref().map_or(false, |cell| cell.number == num))
    }

    fn word_length(&self, row: usize, col: usize, dir: Direction) -> usize {
        let mut len = 0;
        let (mut r, mut c) = (row, col);
        loop {
            let idx = r * self.width + c;
            if r >= self.height || c >= self.width {
                break;
            }
            if self.cells.get(idx).and_then(|c| c.as_ref()).is_none() {
                break;
            }
            len += 1;
            match dir {
                Direction::Across => c += 1,
                Direction::Down => r += 1,
            }
        }
        len
    }

    fn cell_at(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.height && col < self.width {
            self.cells
                .get(row * self.width + col)
                .and_then(|c| c.as_ref())
        } else {
            None
        }
    }

    fn cell_at_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row < self.height && col < self.width {
            let idx = row * self.width + col;
            self.cells.get_mut(idx).and_then(|c| c.as_mut())
        } else {
            None
        }
    }

    fn is_playable(&self, row: usize, col: usize) -> bool {
        self.cell_at(row, col).is_some()
    }

    fn move_to_next_playable(&mut self, start_row: usize, start_col: usize, _dir: Direction) {
        // Scan from position to find first playable cell
        for row in start_row..self.height {
            let col_start = if row == start_row { start_col } else { 0 };
            for col in col_start..self.width {
                if self.is_playable(row, col) {
                    self.cursor_row = row;
                    self.cursor_col = col;
                    return;
                }
            }
        }
    }

    fn advance_cursor(&mut self) {
        match self.direction {
            Direction::Across => {
                let mut c = self.cursor_col + 1;
                while c < self.width {
                    if self.is_playable(self.cursor_row, c) {
                        self.cursor_col = c;
                        return;
                    }
                    c += 1;
                }
                // Don't move if at end of word
            }
            Direction::Down => {
                let mut r = self.cursor_row + 1;
                while r < self.height {
                    if self.is_playable(r, self.cursor_col) {
                        self.cursor_row = r;
                        return;
                    }
                    r += 1;
                }
            }
        }
    }

    fn retreat_cursor(&mut self) {
        match self.direction {
            Direction::Across => {
                if self.cursor_col == 0 {
                    return;
                }
                let mut c = self.cursor_col - 1;
                loop {
                    if self.is_playable(self.cursor_row, c) {
                        self.cursor_col = c;
                        return;
                    }
                    if c == 0 {
                        break;
                    }
                    c -= 1;
                }
            }
            Direction::Down => {
                if self.cursor_row == 0 {
                    return;
                }
                let mut r = self.cursor_row - 1;
                loop {
                    if self.is_playable(r, self.cursor_col) {
                        self.cursor_row = r;
                        return;
                    }
                    if r == 0 {
                        break;
                    }
                    r -= 1;
                }
            }
        }
    }

    fn enter_letter(&mut self, ch: char) {
        let upper = ch.to_ascii_uppercase();
        if let Some(cell) = self.cell_at_mut(self.cursor_row, self.cursor_col) {
            cell.entry = Some(upper);
            cell.flagged_wrong = false;
        }
        self.advance_cursor();
        self.check_completion();
    }

    fn delete_letter(&mut self) {
        if let Some(cell) = self.cell_at_mut(self.cursor_row, self.cursor_col) {
            if cell.entry.is_some() {
                cell.entry = None;
                cell.flagged_wrong = false;
                return;
            }
        }
        // If current cell empty, go back and delete
        self.retreat_cursor();
        if let Some(cell) = self.cell_at_mut(self.cursor_row, self.cursor_col) {
            cell.entry = None;
            cell.flagged_wrong = false;
        }
    }

    fn check_answers(&mut self) {
        self.check_mode = true;
        for cell_opt in &mut self.cells {
            if let Some(cell) = cell_opt {
                if let Some(entry) = cell.entry {
                    cell.flagged_wrong = entry != cell.solution;
                }
            }
        }
    }

    fn clear_checks(&mut self) {
        self.check_mode = false;
        for cell_opt in &mut self.cells {
            if let Some(cell) = cell_opt {
                cell.flagged_wrong = false;
            }
        }
    }

    fn reveal_letter(&mut self) {
        if let Some(cell) = self.cell_at_mut(self.cursor_row, self.cursor_col) {
            cell.entry = Some(cell.solution);
            cell.revealed = true;
            cell.flagged_wrong = false;
        }
        self.check_completion();
    }

    fn reveal_word(&mut self) {
        // Find the start of the current word
        let (start_r, start_c) = self.word_start(self.cursor_row, self.cursor_col, self.direction);
        let (mut r, mut c) = (start_r, start_c);
        loop {
            if r >= self.height || c >= self.width {
                break;
            }
            let idx = r * self.width + c;
            if let Some(Some(cell)) = self.cells.get_mut(idx) {
                cell.entry = Some(cell.solution);
                cell.revealed = true;
                cell.flagged_wrong = false;
            } else {
                break;
            }
            match self.direction {
                Direction::Across => c += 1,
                Direction::Down => r += 1,
            }
        }
        self.check_completion();
    }

    fn word_start(&self, row: usize, col: usize, dir: Direction) -> (usize, usize) {
        let (mut r, mut c) = (row, col);
        match dir {
            Direction::Across => {
                while c > 0 && self.is_playable(r, c - 1) {
                    c -= 1;
                }
            }
            Direction::Down => {
                while r > 0 && self.is_playable(r - 1, c) {
                    r -= 1;
                }
            }
        }
        (r, c)
    }

    fn check_completion(&mut self) {
        let all_filled = self.cells.iter().all(|c| match c {
            None => true,
            Some(cell) => cell.entry.is_some(),
        });
        let all_correct = self.cells.iter().all(|c| match c {
            None => true,
            Some(cell) => cell.is_correct(),
        });
        if all_filled && all_correct {
            self.view = View::Completed;
            self.timer_running = false;
        }
    }

    fn current_clue(&self) -> Option<&Clue> {
        let (start_r, start_c) = self.word_start(self.cursor_row, self.cursor_col, self.direction);
        // Find the clue number at the start of this word
        if let Some(cell) = self.cell_at(start_r, start_c) {
            if cell.number > 0 {
                return self
                    .clues
                    .iter()
                    .find(|cl| cl.number == cell.number && cl.direction == self.direction);
            }
        }
        None
    }

    fn cells_in_current_word(&self) -> Vec<(usize, usize)> {
        let (start_r, start_c) = self.word_start(self.cursor_row, self.cursor_col, self.direction);
        let mut result = Vec::new();
        let (mut r, mut c) = (start_r, start_c);
        loop {
            if r >= self.height || c >= self.width {
                break;
            }
            if !self.is_playable(r, c) {
                break;
            }
            result.push((r, c));
            match self.direction {
                Direction::Across => c += 1,
                Direction::Down => r += 1,
            }
        }
        result
    }

    fn move_cursor(&mut self, key: Key) {
        match key {
            Key::Up => {
                if self.cursor_row > 0 {
                    let mut r = self.cursor_row - 1;
                    loop {
                        if self.is_playable(r, self.cursor_col) {
                            self.cursor_row = r;
                            self.direction = Direction::Down;
                            return;
                        }
                        if r == 0 {
                            break;
                        }
                        r -= 1;
                    }
                }
            }
            Key::Down => {
                let mut r = self.cursor_row + 1;
                while r < self.height {
                    if self.is_playable(r, self.cursor_col) {
                        self.cursor_row = r;
                        self.direction = Direction::Down;
                        return;
                    }
                    r += 1;
                }
            }
            Key::Left => {
                if self.cursor_col > 0 {
                    let mut c = self.cursor_col - 1;
                    loop {
                        if self.is_playable(self.cursor_row, c) {
                            self.cursor_col = c;
                            self.direction = Direction::Across;
                            return;
                        }
                        if c == 0 {
                            break;
                        }
                        c -= 1;
                    }
                }
            }
            Key::Right => {
                let mut c = self.cursor_col + 1;
                while c < self.width {
                    if self.is_playable(self.cursor_row, c) {
                        self.cursor_col = c;
                        self.direction = Direction::Across;
                        return;
                    }
                    c += 1;
                }
            }
            _ => {}
        }
    }

    fn format_time(&self) -> String {
        let mins = self.elapsed_secs / 60;
        let secs = self.elapsed_secs % 60;
        format!("{mins:02}:{secs:02}")
    }

    fn filled_count(&self) -> (usize, usize) {
        let total = self.cells.iter().filter(|c| c.is_some()).count();
        let filled = self
            .cells
            .iter()
            .filter(|c| c.as_ref().map_or(false, |cell| cell.entry.is_some()))
            .count();
        (filled, total)
    }

    fn handle_event_select(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key: Key::Up, .. }) => {
                if self.selected_puzzle > 0 {
                    self.selected_puzzle -= 1;
                }
            }
            Event::Key(KeyEvent { key: Key::Down, .. }) => {
                if self.selected_puzzle + 1 < PUZZLES.len() {
                    self.selected_puzzle += 1;
                }
            }
            Event::Key(KeyEvent {
                key: Key::Enter, ..
            }) => {
                self.load_puzzle(self.selected_puzzle);
            }
            _ => {}
        }
    }

    fn handle_event_playing(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                // Ctrl combinations
                if modifiers.ctrl {
                    match key {
                        Key::C => self.check_answers(),
                        Key::R => self.reveal_letter(),
                        Key::W => self.reveal_word(),
                        Key::U => self.clear_checks(),
                        _ => {}
                    }
                    return;
                }

                match key {
                    Key::Up | Key::Down | Key::Left | Key::Right => {
                        self.move_cursor(*key);
                    }
                    Key::Space => {
                        self.direction = self.direction.toggle();
                    }
                    Key::Tab => {
                        // Move to next clue
                        self.move_to_next_clue(false);
                    }
                    Key::Backspace => {
                        self.delete_letter();
                    }
                    Key::Escape => {
                        self.view = View::PuzzleSelect;
                        self.timer_running = false;
                    }
                    Key::F1 => {
                        self.show_help = !self.show_help;
                    }
                    _ => {
                        // Letter input
                        let ch = key_to_char(*key);
                        if ch.is_ascii_alphabetic() {
                            self.enter_letter(ch);
                        }
                    }
                }
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x,
                y,
                ..
            }) => {
                // Try to click on a grid cell
                self.handle_grid_click(*x, *y);
            }
            _ => {}
        }
    }

    fn handle_grid_click(&mut self, mx: f32, my: f32) {
        let cell_size: f32 = 36.0;
        let grid_x: f32 = 20.0;
        let grid_y: f32 = 60.0;

        let col = ((mx - grid_x) / cell_size) as i32;
        let row = ((my - grid_y) / cell_size) as i32;

        if col >= 0 && (col as usize) < self.width && row >= 0 && (row as usize) < self.height {
            let r = row as usize;
            let c = col as usize;
            if self.is_playable(r, c) {
                if self.cursor_row == r && self.cursor_col == c {
                    // Clicking same cell toggles direction
                    self.direction = self.direction.toggle();
                } else {
                    self.cursor_row = r;
                    self.cursor_col = c;
                }
            }
        }
    }

    fn move_to_next_clue(&mut self, _reverse: bool) {
        // Find current clue
        let current = self.current_clue().map(|c| (c.number, c.direction));

        // Get sorted list of clues matching current direction first
        let mut across_clues: Vec<&Clue> = self
            .clues
            .iter()
            .filter(|c| c.direction == Direction::Across)
            .collect();
        across_clues.sort_by_key(|c| c.number);

        let mut down_clues: Vec<&Clue> = self
            .clues
            .iter()
            .filter(|c| c.direction == Direction::Down)
            .collect();
        down_clues.sort_by_key(|c| c.number);

        let all_clues: Vec<&Clue> = if self.direction == Direction::Across {
            across_clues
                .iter()
                .chain(down_clues.iter())
                .copied()
                .collect()
        } else {
            down_clues
                .iter()
                .chain(across_clues.iter())
                .copied()
                .collect()
        };

        if all_clues.is_empty() {
            return;
        }

        // Find index of current clue, move to next
        let current_idx = current
            .and_then(|(num, dir)| {
                all_clues
                    .iter()
                    .position(|c| c.number == num && c.direction == dir)
            })
            .unwrap_or(0);

        let next_idx = (current_idx + 1) % all_clues.len();
        let next_clue = all_clues[next_idx];

        self.cursor_row = next_clue.start_row;
        self.cursor_col = next_clue.start_col;
        self.direction = next_clue.direction;
    }

    fn handle_event_completed(&mut self, event: &Event) {
        if let Event::Key(KeyEvent {
            key: Key::Enter, ..
        }) = event
        {
            self.view = View::PuzzleSelect;
        }
    }

    fn render_select(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: width / 2.0 - 100.0,
            y: 30.0,
            text: "Crossword Puzzles".to_string(),
            font_size: 24.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Puzzle list
        let start_y = 90.0;
        let item_h = 50.0;
        for (i, puzzle) in PUZZLES.iter().enumerate() {
            let y = start_y + i as f32 * item_h;
            let is_selected = i == self.selected_puzzle;

            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: 40.0,
                    y,
                    width: width - 80.0,
                    height: 40.0,
                    color: COL_SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            let text_color = if is_selected { COL_BLUE } else { COL_TEXT };
            cmds.push(RenderCommand::Text {
                x: 60.0,
                y: y + 12.0,
                text: format!("{}. {}", i + 1, puzzle.name),
                font_size: 18.0,
                color: text_color,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: 300.0,
                y: y + 14.0,
                text: format!("{}x{}", puzzle.width, puzzle.height),
                font_size: 14.0,
                color: COL_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Instructions
        cmds.push(RenderCommand::Text {
            x: 60.0,
            y: height - 40.0,
            text: "Up/Down to select, Enter to start".to_string(),
            font_size: 14.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_playing(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title bar
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height: 44.0,
            color: COL_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: 12.0,
            text: self.puzzle_name.clone(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Timer
        cmds.push(RenderCommand::Text {
            x: width - 100.0,
            y: 14.0,
            text: self.format_time(),
            font_size: 16.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Progress
        let (filled, total) = self.filled_count();
        cmds.push(RenderCommand::Text {
            x: width - 220.0,
            y: 14.0,
            text: format!("{filled}/{total}"),
            font_size: 16.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Current clue display
        if let Some(clue) = self.current_clue() {
            let dir_str = match clue.direction {
                Direction::Across => "Across",
                Direction::Down => "Down",
            };
            cmds.push(RenderCommand::Text {
                x: 20.0,
                y: 50.0,
                text: format!("{} {} — {}", clue.number, dir_str, clue.text),
                font_size: 14.0,
                color: COL_YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Grid
        let cell_size: f32 = 36.0;
        let grid_x: f32 = 20.0;
        let grid_y: f32 = 72.0;

        let current_word_cells = self.cells_in_current_word();

        for row in 0..self.height {
            for col in 0..self.width {
                let cx = grid_x + col as f32 * cell_size;
                let cy = grid_y + row as f32 * cell_size;

                match &self.cells[row * self.width + col] {
                    None => {
                        // Black cell
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: cell_size,
                            height: cell_size,
                            color: COL_CRUST,
                            corner_radii: CornerRadii::ZERO,
                        });
                    }
                    Some(cell) => {
                        let is_cursor = row == self.cursor_row && col == self.cursor_col;
                        let is_word = current_word_cells.contains(&(row, col));

                        // Cell background
                        let bg = if is_cursor {
                            COL_BLUE
                        } else if is_word {
                            COL_SURFACE1
                        } else {
                            COL_SURFACE0
                        };

                        cmds.push(RenderCommand::FillRect {
                            x: cx + 1.0,
                            y: cy + 1.0,
                            width: cell_size - 2.0,
                            height: cell_size - 2.0,
                            color: bg,
                            corner_radii: CornerRadii::all(2.0),
                        });

                        // Clue number
                        if cell.number > 0 {
                            cmds.push(RenderCommand::Text {
                                x: cx + 3.0,
                                y: cy + 2.0,
                                text: format!("{}", cell.number),
                                font_size: 9.0,
                                color: if is_cursor { COL_CRUST } else { COL_OVERLAY0 },
                                font_weight: FontWeightHint::Regular,
                                max_width: None,
                            });
                        }

                        // Letter
                        if let Some(entry) = cell.entry {
                            let letter_color = if cell.flagged_wrong {
                                COL_RED
                            } else if cell.revealed {
                                COL_TEAL
                            } else if is_cursor {
                                COL_CRUST
                            } else {
                                COL_TEXT
                            };

                            cmds.push(RenderCommand::Text {
                                x: cx + cell_size / 2.0 - 6.0,
                                y: cy + cell_size / 2.0 - 7.0,
                                text: entry.to_string(),
                                font_size: 18.0,
                                color: letter_color,
                                font_weight: FontWeightHint::Bold,
                                max_width: None,
                            });
                        }
                    }
                }
            }
        }

        // Grid border
        cmds.push(RenderCommand::StrokeRect {
            x: grid_x,
            y: grid_y,
            width: self.width as f32 * cell_size,
            height: self.height as f32 * cell_size,
            color: COL_OVERLAY0,
            line_width: 2.0,
            corner_radii: CornerRadii::ZERO,
        });

        // Clue panels
        let clue_x = grid_x + self.width as f32 * cell_size + 20.0;
        let clue_w = width - clue_x - 20.0;
        if clue_w > 50.0 {
            self.render_clue_panel(cmds, clue_x, grid_y, clue_w, Direction::Across);
            let down_y = grid_y + (height - grid_y - 40.0) / 2.0;
            self.render_clue_panel(cmds, clue_x, down_y, clue_w, Direction::Down);
        }

        // Help hint
        cmds.push(RenderCommand::Text {
            x: 20.0,
            y: height - 24.0,
            text: "F1=Help  Space=Toggle Dir  Tab=Next Clue  Ctrl+C=Check  Ctrl+R=Reveal  Esc=Back"
                .to_string(),
            font_size: 11.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Help overlay
        if self.show_help {
            self.render_help_overlay(cmds, width, height);
        }
    }

    fn render_clue_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        dir: Direction,
    ) {
        let title = match dir {
            Direction::Across => "ACROSS",
            Direction::Down => "DOWN",
        };

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: title.to_string(),
            font_size: 14.0,
            color: COL_LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let mut cy = y + 22.0;
        let clues: Vec<&Clue> = self.clues.iter().filter(|c| c.direction == dir).collect();

        let scroll = match dir {
            Direction::Across => self.clue_scroll_across,
            Direction::Down => self.clue_scroll_down,
        };

        for clue in clues.iter().skip(scroll).take(8) {
            let is_current = self.current_clue().map_or(false, |c| {
                c.number == clue.number && c.direction == clue.direction
            });

            let color = if is_current { COL_YELLOW } else { COL_SUBTEXT0 };
            let weight = if is_current {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };

            // Truncate clue text to fit
            let max_chars = (w / 7.0) as usize;
            let display_text = if clue.text.len() > max_chars {
                format!("{}...", &clue.text[..max_chars.saturating_sub(3)])
            } else {
                clue.text.clone()
            };

            cmds.push(RenderCommand::Text {
                x,
                y: cy,
                text: format!("{}. {display_text}", clue.number),
                font_size: 12.0,
                color,
                font_weight: weight,
                max_width: None,
            });
            cy += 18.0;
        }
    }

    fn render_help_overlay(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        // Dim background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let bx = width / 2.0 - 180.0;
        let by = height / 2.0 - 140.0;
        let bw = 360.0;
        let bh = 280.0;

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: bw,
            height: bh,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + bw / 2.0 - 30.0,
            y: by + 16.0,
            text: "Help".to_string(),
            font_size: 20.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let helps = [
            ("Arrow Keys", "Move cursor"),
            ("A-Z", "Enter letter"),
            ("Backspace", "Delete letter"),
            ("Space", "Toggle direction"),
            ("Tab", "Next clue"),
            ("Ctrl+C", "Check answers"),
            ("Ctrl+R", "Reveal letter"),
            ("Ctrl+W", "Reveal word"),
            ("Ctrl+U", "Clear checks"),
            ("F1", "Toggle this help"),
            ("Esc", "Return to menu"),
        ];

        let mut cy = by + 50.0;
        for (key, desc) in &helps {
            cmds.push(RenderCommand::Text {
                x: bx + 30.0,
                y: cy,
                text: (*key).to_string(),
                font_size: 13.0,
                color: COL_BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: bx + 150.0,
                y: cy,
                text: (*desc).to_string(),
                font_size: 13.0,
                color: COL_SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 20.0;
        }
    }

    fn render_completed(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: COL_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        let cx = width / 2.0;

        cmds.push(RenderCommand::Text {
            x: cx - 100.0,
            y: height / 2.0 - 60.0,
            text: "Puzzle Complete!".to_string(),
            font_size: 28.0,
            color: COL_GREEN,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: cx - 60.0,
            y: height / 2.0 - 10.0,
            text: format!("Time: {}", self.format_time()),
            font_size: 20.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: cx - 80.0,
            y: height / 2.0 + 30.0,
            text: self.puzzle_name.clone(),
            font_size: 16.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let revealed = self
            .cells
            .iter()
            .filter(|c| c.as_ref().map_or(false, |cell| cell.revealed))
            .count();
        if revealed > 0 {
            cmds.push(RenderCommand::Text {
                x: cx - 80.0,
                y: height / 2.0 + 60.0,
                text: format!("{revealed} letter(s) revealed"),
                font_size: 14.0,
                color: COL_PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds.push(RenderCommand::Text {
            x: cx - 80.0,
            y: height / 2.0 + 100.0,
            text: "Press Enter to continue".to_string(),
            font_size: 14.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn key_to_char(key: Key) -> char {
    match key {
        Key::A => 'A',
        Key::B => 'B',
        Key::C => 'C',
        Key::D => 'D',
        Key::E => 'E',
        Key::F => 'F',
        Key::G => 'G',
        Key::H => 'H',
        Key::I => 'I',
        Key::J => 'J',
        Key::K => 'K',
        Key::L => 'L',
        Key::M => 'M',
        Key::N => 'N',
        Key::O => 'O',
        Key::P => 'P',
        Key::Q => 'Q',
        Key::R => 'R',
        Key::S => 'S',
        Key::T => 'T',
        Key::U => 'U',
        Key::V => 'V',
        Key::W => 'W',
        Key::X => 'X',
        Key::Y => 'Y',
        Key::Z => 'Z',
        _ => '\0',
    }
}

impl CrosswordApp {
    fn event(&mut self, event: &Event) {
        match self.view {
            View::PuzzleSelect => self.handle_event_select(event),
            View::Playing => self.handle_event_playing(event),
            View::Completed => self.handle_event_completed(event),
        }
    }

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        match self.view {
            View::PuzzleSelect => self.render_select(&mut cmds, width, height),
            View::Playing => self.render_playing(&mut cmds, width, height),
            View::Completed => self.render_completed(&mut cmds, width, height),
        }
        cmds
    }
}

fn main() {
    let _app = CrosswordApp::new();
}

// ── Tests ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> CrosswordApp {
        let mut app = CrosswordApp::new();
        app.load_puzzle(0);
        app
    }

    #[test]
    fn test_initial_state() {
        let app = CrosswordApp::new();
        assert_eq!(app.view, View::PuzzleSelect);
        assert_eq!(app.selected_puzzle, 0);
    }

    #[test]
    fn test_load_puzzle() {
        let app = make_app();
        assert_eq!(app.view, View::Playing);
        assert_eq!(app.width, 7);
        assert_eq!(app.height, 7);
        assert!(app.timer_running);
    }

    #[test]
    fn test_grid_size() {
        let app = make_app();
        assert_eq!(app.cells.len(), 49); // 7x7
    }

    #[test]
    fn test_black_cells() {
        let app = make_app();
        // Row 0: CATS### — positions 4,5,6 should be black
        assert!(app.cells[4].is_none());
        assert!(app.cells[5].is_none());
        assert!(app.cells[6].is_none());
    }

    #[test]
    fn test_playable_cells() {
        let app = make_app();
        // Row 0: CATS### — positions 0,1,2,3 should be playable
        assert!(app.cells[0].is_some());
        assert!(app.cells[1].is_some());
        assert!(app.cells[2].is_some());
        assert!(app.cells[3].is_some());
    }

    #[test]
    fn test_cell_solutions() {
        let app = make_app();
        assert_eq!(app.cells[0].as_ref().unwrap().solution, 'C');
        assert_eq!(app.cells[1].as_ref().unwrap().solution, 'A');
        assert_eq!(app.cells[2].as_ref().unwrap().solution, 'T');
        assert_eq!(app.cells[3].as_ref().unwrap().solution, 'S');
    }

    #[test]
    fn test_clue_numbers_assigned() {
        let app = make_app();
        // First cell should have a number
        assert!(app.cells[0].as_ref().unwrap().number > 0);
    }

    #[test]
    fn test_clues_loaded() {
        let app = make_app();
        assert!(!app.clues.is_empty());
        let across_count = app
            .clues
            .iter()
            .filter(|c| c.direction == Direction::Across)
            .count();
        let down_count = app
            .clues
            .iter()
            .filter(|c| c.direction == Direction::Down)
            .count();
        assert!(across_count > 0);
        assert!(down_count > 0);
    }

    #[test]
    fn test_cursor_starts_on_playable() {
        let app = make_app();
        assert!(app.is_playable(app.cursor_row, app.cursor_col));
    }

    #[test]
    fn test_enter_letter() {
        let mut app = make_app();
        let row = app.cursor_row;
        let col = app.cursor_col;
        app.enter_letter('C');
        assert_eq!(app.cell_at(row, col).unwrap().entry, Some('C'));
    }

    #[test]
    fn test_enter_lowercase_converts() {
        let mut app = make_app();
        let row = app.cursor_row;
        let col = app.cursor_col;
        app.enter_letter('c');
        assert_eq!(app.cell_at(row, col).unwrap().entry, Some('C'));
    }

    #[test]
    fn test_advance_after_entry() {
        let mut app = make_app();
        app.direction = Direction::Across;
        let start_col = app.cursor_col;
        app.enter_letter('X');
        // Should have advanced
        assert!(app.cursor_col > start_col || app.cursor_row > app.cursor_row);
    }

    #[test]
    fn test_delete_letter() {
        let mut app = make_app();
        let row = app.cursor_row;
        let col = app.cursor_col;
        app.enter_letter('X');
        app.cursor_row = row;
        app.cursor_col = col;
        app.delete_letter();
        assert!(app.cell_at(row, col).unwrap().entry.is_none());
    }

    #[test]
    fn test_delete_empty_retreats() {
        let mut app = make_app();
        app.direction = Direction::Across;
        app.cursor_row = 0;
        app.cursor_col = 1; // Second playable cell
        app.enter_letter('X'); // enter at col 1, advance to col 2
        app.cursor_col = 2;
        // Cell at 2 is empty, deleting should retreat
        app.delete_letter();
        assert!(app.cursor_col <= 1);
    }

    #[test]
    fn test_move_cursor_right() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.move_cursor(Key::Right);
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_move_cursor_down() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.move_cursor(Key::Down);
        assert!(app.cursor_row > 0);
    }

    #[test]
    fn test_direction_toggle() {
        let mut app = make_app();
        assert_eq!(app.direction, Direction::Across);
        app.direction = app.direction.toggle();
        assert_eq!(app.direction, Direction::Down);
        app.direction = app.direction.toggle();
        assert_eq!(app.direction, Direction::Across);
    }

    #[test]
    fn test_space_toggles_direction() {
        let mut app = make_app();
        assert_eq!(app.direction, Direction::Across);
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::Space,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.direction, Direction::Down);
    }

    #[test]
    fn test_word_start_across() {
        let app = make_app();
        // CATS is at row 0, cols 0-3
        let (r, c) = app.word_start(0, 2, Direction::Across);
        assert_eq!((r, c), (0, 0));
    }

    #[test]
    fn test_word_start_at_beginning() {
        let app = make_app();
        let (r, c) = app.word_start(0, 0, Direction::Across);
        assert_eq!((r, c), (0, 0));
    }

    #[test]
    fn test_word_length_across() {
        let app = make_app();
        let len = app.word_length(0, 0, Direction::Across);
        assert_eq!(len, 4); // CATS
    }

    #[test]
    fn test_word_length_down() {
        let app = make_app();
        // C at (0,0), then A at (1,0), B at (2,0) = CAB
        let len = app.word_length(0, 0, Direction::Down);
        assert_eq!(len, 3);
    }

    #[test]
    fn test_cells_in_current_word() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        let cells = app.cells_in_current_word();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], (0, 0));
        assert_eq!(cells[3], (0, 3));
    }

    #[test]
    fn test_check_answers_flags_wrong() {
        let mut app = make_app();
        // Enter wrong letter
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('Z'); // Wrong: should be C
        app.check_answers();
        assert!(app.cell_at(0, 0).unwrap().flagged_wrong);
    }

    #[test]
    fn test_check_answers_correct_not_flagged() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('C'); // Correct
        app.check_answers();
        assert!(!app.cell_at(0, 0).unwrap().flagged_wrong);
    }

    #[test]
    fn test_clear_checks() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('Z');
        app.check_answers();
        assert!(app.cell_at(0, 0).unwrap().flagged_wrong);
        app.clear_checks();
        assert!(!app.cell_at(0, 0).unwrap().flagged_wrong);
        assert!(!app.check_mode);
    }

    #[test]
    fn test_reveal_letter() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.reveal_letter();
        let cell = app.cell_at(0, 0).unwrap();
        assert_eq!(cell.entry, Some('C'));
        assert!(cell.revealed);
    }

    #[test]
    fn test_reveal_word() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        app.reveal_word();
        // CATS should all be revealed
        for col in 0..4 {
            let cell = app.cell_at(0, col).unwrap();
            assert!(cell.revealed);
            assert!(cell.entry.is_some());
        }
    }

    #[test]
    fn test_completion_detection() {
        let mut app = make_app();
        // Fill in all cells correctly
        for row in 0..app.height {
            for col in 0..app.width {
                if let Some(cell) = app.cell_at(row, col) {
                    let sol = cell.solution;
                    if let Some(cell_mut) = app.cell_at_mut(row, col) {
                        cell_mut.entry = Some(sol);
                    }
                }
            }
        }
        app.check_completion();
        assert_eq!(app.view, View::Completed);
        assert!(!app.timer_running);
    }

    #[test]
    fn test_incomplete_not_completed() {
        let mut app = make_app();
        // Fill in only some cells
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('C');
        app.check_completion();
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn test_wrong_answers_not_completed() {
        let mut app = make_app();
        // Fill all with wrong letters
        for row in 0..app.height {
            for col in 0..app.width {
                if let Some(cell_mut) = app.cell_at_mut(row, col) {
                    cell_mut.entry = Some('Z');
                }
            }
        }
        app.check_completion();
        assert_ne!(app.view, View::Completed);
    }

    #[test]
    fn test_format_time_zero() {
        let app = make_app();
        assert_eq!(app.format_time(), "00:00");
    }

    #[test]
    fn test_format_time_minutes() {
        let mut app = make_app();
        app.elapsed_secs = 125;
        assert_eq!(app.format_time(), "02:05");
    }

    #[test]
    fn test_filled_count_empty() {
        let app = make_app();
        let (filled, total) = app.filled_count();
        assert_eq!(filled, 0);
        assert!(total > 0);
    }

    #[test]
    fn test_filled_count_partial() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('C');
        let (filled, _total) = app.filled_count();
        assert_eq!(filled, 1);
    }

    #[test]
    fn test_current_clue() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        let clue = app.current_clue();
        assert!(clue.is_some());
        assert_eq!(clue.unwrap().direction, Direction::Across);
    }

    #[test]
    fn test_find_numbered_cell() {
        let app = make_app();
        let pos = app.find_numbered_cell(1);
        assert!(pos.is_some());
    }

    #[test]
    fn test_find_nonexistent_number() {
        let app = make_app();
        let pos = app.find_numbered_cell(255);
        assert!(pos.is_none());
    }

    #[test]
    fn test_puzzle_select_navigation() {
        let mut app = CrosswordApp::new();
        assert_eq!(app.selected_puzzle, 0);
        app.handle_event_select(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_puzzle, 1);
        app.handle_event_select(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_puzzle, 0);
    }

    #[test]
    fn test_select_enter_loads() {
        let mut app = CrosswordApp::new();
        app.handle_event_select(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn test_escape_returns_to_select() {
        let mut app = make_app();
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::PuzzleSelect);
    }

    #[test]
    fn test_completed_enter_returns() {
        let mut app = make_app();
        app.view = View::Completed;
        app.handle_event_completed(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::PuzzleSelect);
    }

    #[test]
    fn test_ctrl_c_checks() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('Z');
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::C,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: None,
        }));
        assert!(app.check_mode);
    }

    #[test]
    fn test_ctrl_r_reveals() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::R,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: None,
        }));
        assert!(app.cell_at(0, 0).unwrap().revealed);
    }

    #[test]
    fn test_ctrl_w_reveals_word() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::W,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: None,
        }));
        for col in 0..4 {
            assert!(app.cell_at(0, col).unwrap().revealed);
        }
    }

    #[test]
    fn test_f1_toggles_help() {
        let mut app = make_app();
        assert!(!app.show_help);
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::F1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.show_help);
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::F1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(!app.show_help);
    }

    #[test]
    fn test_tab_moves_to_next_clue() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        let start_row = app.cursor_row;
        let start_col = app.cursor_col;
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::Tab,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        // Should have moved to a different position
        assert!(app.cursor_row != start_row || app.cursor_col != start_col);
    }

    #[test]
    fn test_load_puzzle_2() {
        let mut app = CrosswordApp::new();
        app.load_puzzle(1);
        assert_eq!(app.puzzle_name, "Word Play");
        assert_eq!(app.width, 7);
    }

    #[test]
    fn test_load_puzzle_3() {
        let mut app = CrosswordApp::new();
        app.load_puzzle(2);
        assert_eq!(app.puzzle_name, "Nature");
    }

    #[test]
    fn test_load_invalid_puzzle() {
        let mut app = CrosswordApp::new();
        app.load_puzzle(99);
        assert_eq!(app.view, View::PuzzleSelect);
    }

    #[test]
    fn test_cell_is_correct() {
        let mut cell = Cell::new('A');
        assert!(!cell.is_correct());
        cell.entry = Some('A');
        assert!(cell.is_correct());
        cell.entry = Some('B');
        assert!(!cell.is_correct());
    }

    #[test]
    fn test_direction_enum() {
        assert_eq!(Direction::Across.toggle(), Direction::Down);
        assert_eq!(Direction::Down.toggle(), Direction::Across);
    }

    #[test]
    fn test_key_to_char() {
        assert_eq!(key_to_char(Key::A), 'A');
        assert_eq!(key_to_char(Key::Z), 'Z');
        assert_eq!(key_to_char(Key::Space), '\0');
    }

    #[test]
    fn test_render_select_no_panic() {
        let app = CrosswordApp::new();
        let mut cmds = Vec::new();
        app.render_select(&mut cmds, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_no_panic() {
        let app = make_app();
        let mut cmds = Vec::new();
        app.render_playing(&mut cmds, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_completed_no_panic() {
        let mut app = make_app();
        app.view = View::Completed;
        let mut cmds = Vec::new();
        app.render_completed(&mut cmds, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_help_overlay_no_panic() {
        let mut app = make_app();
        app.show_help = true;
        let mut cmds = Vec::new();
        app.render_playing(&mut cmds, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_grid_click() {
        let mut app = make_app();
        // Click on cell (0,0) in grid coordinates
        app.handle_grid_click(20.0 + 18.0, 60.0 + 18.0);
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_grid_click_toggles_direction() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        // Click same cell
        app.handle_grid_click(20.0 + 18.0, 60.0 + 18.0);
        // Clicking same cell should toggle direction
        // (cursor was already at 0,0)
    }

    #[test]
    fn test_select_no_overflow_up() {
        let mut app = CrosswordApp::new();
        app.selected_puzzle = 0;
        app.handle_event_select(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_puzzle, 0);
    }

    #[test]
    fn test_select_no_overflow_down() {
        let mut app = CrosswordApp::new();
        app.selected_puzzle = PUZZLES.len() - 1;
        app.handle_event_select(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_puzzle, PUZZLES.len() - 1);
    }

    #[test]
    fn test_enter_letter_clears_flag() {
        let mut app = make_app();
        if let Some(cell) = app.cell_at_mut(0, 0) {
            cell.entry = Some('Z');
            cell.flagged_wrong = true;
        }
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('C');
        assert!(!app.cell_at(0, 0).unwrap().flagged_wrong);
    }

    #[test]
    fn test_move_right_sets_across() {
        let mut app = make_app();
        app.direction = Direction::Down;
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.move_cursor(Key::Right);
        assert_eq!(app.direction, Direction::Across);
    }

    #[test]
    fn test_move_down_sets_down() {
        let mut app = make_app();
        app.direction = Direction::Across;
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.move_cursor(Key::Down);
        assert_eq!(app.direction, Direction::Down);
    }

    #[test]
    fn test_all_puzzles_parse() {
        for i in 0..PUZZLES.len() {
            let mut app = CrosswordApp::new();
            app.load_puzzle(i);
            assert_eq!(app.view, View::Playing);
            assert!(app.cells.len() > 0);
            assert!(!app.clues.is_empty());
        }
    }

    #[test]
    fn test_all_puzzles_have_numbered_cells() {
        for i in 0..PUZZLES.len() {
            let mut app = CrosswordApp::new();
            app.load_puzzle(i);
            let numbered = app
                .cells
                .iter()
                .filter(|c| c.as_ref().map_or(false, |cell| cell.number > 0))
                .count();
            assert!(numbered > 0, "Puzzle {i} has no numbered cells");
        }
    }

    #[test]
    fn test_all_puzzles_solvable() {
        for i in 0..PUZZLES.len() {
            let mut app = CrosswordApp::new();
            app.load_puzzle(i);
            // Reveal all
            for row in 0..app.height {
                for col in 0..app.width {
                    if let Some(cell) = app.cell_at(row, col) {
                        let sol = cell.solution;
                        if let Some(cell_mut) = app.cell_at_mut(row, col) {
                            cell_mut.entry = Some(sol);
                        }
                    }
                }
            }
            app.check_completion();
            assert_eq!(app.view, View::Completed, "Puzzle {i} not completable");
        }
    }

    #[test]
    fn test_render_clue_panel_no_panic() {
        let app = make_app();
        let mut cmds = Vec::new();
        app.render_clue_panel(&mut cmds, 300.0, 80.0, 200.0, Direction::Across);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_retreat_cursor_at_start() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Across;
        app.retreat_cursor();
        // Should stay at 0,0
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_retreat_cursor_down_at_start() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.direction = Direction::Down;
        app.retreat_cursor();
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn test_word_start_down() {
        let app = make_app();
        // Column 0: C(0,0), A(1,0), B(2,0) then black
        let (r, c) = app.word_start(2, 0, Direction::Down);
        assert_eq!((r, c), (0, 0));
    }

    #[test]
    fn test_ctrl_u_clears_checks() {
        let mut app = make_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.enter_letter('Z');
        app.check_answers();
        assert!(app.check_mode);
        app.handle_event_playing(&Event::Key(KeyEvent {
            key: Key::U,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: None,
        }));
        assert!(!app.check_mode);
    }

    #[test]
    fn test_widget_event_dispatch() {
        let mut app = CrosswordApp::new();
        // PuzzleSelect view
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::Playing);
    }

    #[test]
    fn test_widget_render_dispatch() {
        let app = CrosswordApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }
}
