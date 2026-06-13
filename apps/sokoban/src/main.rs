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

//! SlateOS Sokoban -- classic box-pushing puzzle game.
//!
//! The player pushes crates onto target positions in a warehouse. Features
//! 15 built-in levels of increasing difficulty, a level select screen,
//! undo/redo with full move history, move counter, win detection, level
//! completion celebration, and auto-advance to the next level. Uses the
//! Catppuccin Mocha dark color palette.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// -- Catppuccin Mocha palette ---------------------------------------------------
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
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);

// -- Layout constants -----------------------------------------------------------
const CELL_SIZE: f32 = 48.0;
const CELL_GAP: f32 = 2.0;
const PADDING: f32 = 16.0;
const HEADER_HEIGHT: f32 = 56.0;
const FOOTER_HEIGHT: f32 = 36.0;
const CELL_CORNER_RADIUS: f32 = 4.0;

const HEADER_FONT_SIZE: f32 = 20.0;
const CELL_FONT_SIZE: f32 = 24.0;
const STATUS_FONT_SIZE: f32 = 14.0;
const LABEL_FONT_SIZE: f32 = 13.0;
const TITLE_FONT_SIZE: f32 = 28.0;
const OVERLAY_FONT_SIZE: f32 = 18.0;
const SELECT_FONT_SIZE: f32 = 16.0;

const MAX_UNDO: usize = 2000;
const MAX_LEVEL_WIDTH: usize = 20;
const MAX_LEVEL_HEIGHT: usize = 20;

// -- Tile types -----------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tile {
    /// Empty floor.
    Floor,
    /// Solid wall.
    Wall,
    /// Target position where a box should go.
    Target,
    /// Nothing (outside the warehouse).
    Empty,
}

// -- Direction ------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
            Direction::Left => (0, -1),
            Direction::Right => (0, 1),
        }
    }
}

// -- Position -------------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    row: i32,
    col: i32,
}

impl Pos {
    const fn new(row: i32, col: i32) -> Self {
        Self { row, col }
    }

    fn moved(self, dir: Direction) -> Self {
        let (dr, dc) = dir.delta();
        Self {
            row: self.row + dr,
            col: self.col + dc,
        }
    }
}

// -- Undo entry -----------------------------------------------------------------
/// Captures one player move so it can be reversed.
#[derive(Clone, Debug)]
struct UndoEntry {
    /// Player position before the move.
    player_pos: Pos,
    /// If a box was pushed, its position before and after the push.
    box_push: Option<(Pos, Pos)>,
}

// -- Screen state ---------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    /// Level select menu.
    LevelSelect,
    /// Playing a level.
    Playing,
    /// Level completed celebration.
    Won,
}

// -- Level data -----------------------------------------------------------------
/// A parsed level ready for play.
#[derive(Clone, Debug)]
struct Level {
    /// Width in cells.
    width: usize,
    /// Height in cells.
    height: usize,
    /// The static tile grid (floor, wall, target, empty).
    tiles: Vec<Vec<Tile>>,
    /// Initial player position.
    start_player: Pos,
    /// Initial box positions.
    start_boxes: Vec<Pos>,
}

/// Parse a level from a string using standard Sokoban notation:
/// `#` = wall, ` ` = floor, `.` = target, `$` = box,
/// `@` = player, `+` = player on target, `*` = box on target,
/// `-` = floor (alternative).
fn parse_level(source: &str) -> Level {
    let mut tiles: Vec<Vec<Tile>> = Vec::new();
    let mut player = Pos::new(0, 0);
    let mut boxes: Vec<Pos> = Vec::new();

    for (row_idx, line) in source.lines().enumerate() {
        let mut row_tiles = Vec::new();
        for (col_idx, ch) in line.chars().enumerate() {
            let r = row_idx as i32;
            let c = col_idx as i32;
            match ch {
                '#' => row_tiles.push(Tile::Wall),
                ' ' | '-' => row_tiles.push(Tile::Floor),
                '.' => row_tiles.push(Tile::Target),
                '$' => {
                    row_tiles.push(Tile::Floor);
                    boxes.push(Pos::new(r, c));
                }
                '@' => {
                    row_tiles.push(Tile::Floor);
                    player = Pos::new(r, c);
                }
                '+' => {
                    // Player on target.
                    row_tiles.push(Tile::Target);
                    player = Pos::new(r, c);
                }
                '*' => {
                    // Box on target.
                    row_tiles.push(Tile::Target);
                    boxes.push(Pos::new(r, c));
                }
                _ => row_tiles.push(Tile::Empty),
            }
        }
        tiles.push(row_tiles);
    }

    // Normalize row widths to the maximum.
    let max_width = tiles.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut tiles {
        while row.len() < max_width {
            row.push(Tile::Empty);
        }
    }

    let height = tiles.len();

    Level {
        width: max_width,
        height,
        tiles,
        start_player: player,
        start_boxes: boxes,
    }
}

// -- Built-in levels (15 levels of increasing difficulty) -----------------------
fn builtin_levels() -> Vec<&'static str> {
    vec![
        // Level 1: Simplest -- one box, one target
        concat!(
            "  ###\n",
            "  #.#\n",
            "###-###\n",
            "#--$--#\n",
            "#--@--#\n",
            "#-----#\n",
            "#######\n",
        ),
        // Level 2: Two boxes in a line
        concat!(
            "######\n",
            "#----#\n",
            "#-$$-#\n",
            "#-..-#\n",
            "#--@-#\n",
            "######\n",
        ),
        // Level 3: L-shaped puzzle
        concat!(
            "#####\n",
            "#---##\n",
            "#-$--#\n",
            "##-$-#\n",
            "-#-.-#\n",
            "-#-.-#\n",
            "-#-@-#\n",
            "-#####\n",
        ),
        // Level 4: Corridor push
        concat!(
            "########\n",
            "#------#\n",
            "#-#.##-#\n",
            "#--$---#\n",
            "#-#$##-#\n",
            "#--.-@-#\n",
            "########\n",
        ),
        // Level 5: Three boxes
        concat!(
            "-#####\n",
            "##---#\n",
            "#-$--#\n",
            "#-.$-##\n",
            "#-.$.@#\n",
            "#-----#\n",
            "#######\n",
        ),
        // Level 6: Wide room
        concat!(
            "-####\n",
            "##--####\n",
            "#--$---#\n",
            "#-#.#$-#\n",
            "#---#.-#\n",
            "##-@####\n",
            "-####\n",
        ),
        // Level 7: Tight corners
        concat!(
            "########\n",
            "#--#---#\n",
            "#--$-$-#\n",
            "#.#--#.#\n",
            "#------#\n",
            "#--@---#\n",
            "########\n",
        ),
        // Level 8: Four boxes
        concat!(
            "--#####\n",
            "###---#\n",
            "#-$.$-#\n",
            "#-.@.-#\n",
            "#-$.$-#\n",
            "###---#\n",
            "--#####\n",
        ),
        // Level 9: Asymmetric
        concat!(
            "#######\n",
            "#--#--#\n",
            "#-$---#\n",
            "#--$#-#\n",
            "##.-.-#\n",
            "-#.$--#\n",
            "-#--@-#\n",
            "-######\n",
        ),
        // Level 10: Winding path
        concat!(
            "-#######\n",
            "-#-----#\n",
            "##-#-#-#\n",
            "#--$-$-#\n",
            "#-.-.#-#\n",
            "#--$---#\n",
            "#--.-@-#\n",
            "########\n",
        ),
        // Level 11: Five boxes
        concat!(
            "########\n",
            "#------#\n",
            "#-$$$--#\n",
            "#-.-.--#\n",
            "#--$-$-#\n",
            "#-.-..-#\n",
            "#--@---#\n",
            "########\n",
        ),
        // Level 12: Cross pattern
        concat!(
            "--####\n",
            "###--##\n",
            "#-.$--#\n",
            "#-#.#-#\n",
            "#-$.$-#\n",
            "#-#.#-#\n",
            "#--$--#\n",
            "##--@##\n",
            "-#####\n",
        ),
        // Level 13: Multi-room
        concat!(
            "####--####\n",
            "#--####--#\n",
            "#--$--$--#\n",
            "#-.----.-#\n",
            "####--####\n",
            "#-.----.-#\n",
            "#--$--$--#\n",
            "#--@##---#\n",
            "##########\n",
        ),
        // Level 14: Five boxes, complex
        concat!(
            "-########\n",
            "-#------#\n",
            "##-$$---#\n",
            "#--#-$#-#\n",
            "#-.#..--#\n",
            "#---$-$-#\n",
            "#--#..--#\n",
            "#---@---#\n",
            "#########\n",
        ),
        // Level 15: Grand finale
        concat!(
            "##########\n",
            "#--------#\n",
            "#-$$-$$--#\n",
            "#-#....#-#\n",
            "#---##---#\n",
            "#-$$-$$--#\n",
            "#-#....#-#\n",
            "#----@---#\n",
            "##########\n",
        ),
    ]
}

// -- Main app -------------------------------------------------------------------

struct SokobanApp {
    /// All parsed levels.
    levels: Vec<Level>,
    /// Currently selected/active level index (0-based).
    current_level: usize,
    /// Current screen mode.
    screen: Screen,
    /// Which levels have been completed.
    completed: Vec<bool>,
    /// Level select cursor position.
    select_cursor: usize,

    // -- In-game state --
    /// The static tile grid for the active level.
    tiles: Vec<Vec<Tile>>,
    /// Level width in cells.
    level_width: usize,
    /// Level height in cells.
    level_height: usize,
    /// Current player position.
    player: Pos,
    /// Current box positions.
    boxes: Vec<Pos>,
    /// Number of moves made since level start.
    move_count: u32,
    /// Undo history stack.
    undo_stack: Vec<UndoEntry>,
    /// Number of pushes (subset of moves where a box moved).
    push_count: u32,
    /// Celebration timer ticks remaining (for the win screen).
    celebration_ticks: u32,
}

impl SokobanApp {
    fn new() -> Self {
        let raw = builtin_levels();
        let levels: Vec<Level> = raw.iter().map(|s| parse_level(s)).collect();
        let num = levels.len();
        let mut app = Self {
            levels,
            current_level: 0,
            screen: Screen::LevelSelect,
            completed: vec![false; num],
            select_cursor: 0,
            tiles: Vec::new(),
            level_width: 0,
            level_height: 0,
            player: Pos::new(0, 0),
            boxes: Vec::new(),
            move_count: 0,
            undo_stack: Vec::new(),
            push_count: 0,
            celebration_ticks: 0,
        };
        app.load_level(0);
        app
    }

    /// Load (or reload) the given level index into the active game state.
    fn load_level(&mut self, idx: usize) {
        let level = &self.levels[idx];
        self.current_level = idx;
        self.tiles = level.tiles.clone();
        self.level_width = level.width;
        self.level_height = level.height;
        self.player = level.start_player;
        self.boxes = level.start_boxes.clone();
        self.move_count = 0;
        self.push_count = 0;
        self.undo_stack.clear();
        self.celebration_ticks = 0;
    }

    /// Reset the current level to its initial state.
    fn reset_level(&mut self) {
        self.load_level(self.current_level);
    }

    /// Start playing the currently selected level.
    fn start_level(&mut self, idx: usize) {
        self.load_level(idx);
        self.screen = Screen::Playing;
    }

    /// Return the number of available levels.
    fn level_count(&self) -> usize {
        self.levels.len()
    }

    // -- Tile queries -------------------------------------------------------

    /// Get the tile at a position, returning `Tile::Wall` for out-of-bounds.
    fn tile_at(&self, pos: Pos) -> Tile {
        if pos.row < 0 || pos.col < 0 {
            return Tile::Wall;
        }
        let r = pos.row as usize;
        let c = pos.col as usize;
        if r >= self.level_height || c >= self.level_width {
            return Tile::Wall;
        }
        self.tiles[r][c]
    }

    /// Check whether a position contains a wall.
    fn is_wall(&self, pos: Pos) -> bool {
        self.tile_at(pos) == Tile::Wall
    }

    /// Check whether a position has a box.
    fn has_box(&self, pos: Pos) -> bool {
        self.boxes.contains(&pos)
    }

    /// Check whether a position is a target.
    fn is_target(&self, pos: Pos) -> bool {
        self.tile_at(pos) == Tile::Target
    }

    /// Check whether all boxes are on targets.
    fn is_solved(&self) -> bool {
        if self.boxes.is_empty() {
            return false;
        }
        self.boxes.iter().all(|b| self.is_target(*b))
    }

    /// Count how many boxes are currently on targets.
    fn boxes_on_targets(&self) -> usize {
        self.boxes.iter().filter(|b| self.is_target(**b)).count()
    }

    /// Total number of targets in the level.
    fn target_count(&self) -> usize {
        let mut count = 0;
        for row in &self.tiles {
            for tile in row {
                if *tile == Tile::Target {
                    count += 1;
                }
            }
        }
        count
    }

    // -- Movement -----------------------------------------------------------

    /// Attempt to move the player in the given direction.
    /// Returns `true` if the player actually moved.
    fn try_move(&mut self, dir: Direction) -> bool {
        let new_pos = self.player.moved(dir);

        // Cannot walk into walls.
        if self.is_wall(new_pos) {
            return false;
        }

        // Check for a box at the destination.
        if self.has_box(new_pos) {
            let box_dest = new_pos.moved(dir);

            // Cannot push a box into a wall or another box.
            if self.is_wall(box_dest) || self.has_box(box_dest) {
                return false;
            }

            // Push the box.
            let undo = UndoEntry {
                player_pos: self.player,
                box_push: Some((new_pos, box_dest)),
            };

            // Move the box.
            if let Some(b) = self.boxes.iter_mut().find(|b| **b == new_pos) {
                *b = box_dest;
            }

            self.player = new_pos;
            self.move_count += 1;
            self.push_count += 1;
            self.undo_stack.push(undo);
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }

            // Check win condition.
            if self.is_solved() {
                self.screen = Screen::Won;
                self.celebration_ticks = 120;
                self.completed[self.current_level] = true;
            }

            return true;
        }

        // Empty floor or target -- just walk.
        let undo = UndoEntry {
            player_pos: self.player,
            box_push: None,
        };
        self.player = new_pos;
        self.move_count += 1;
        self.undo_stack.push(undo);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }

        true
    }

    /// Undo the last move. Returns `true` if an undo was performed.
    fn undo(&mut self) -> bool {
        if let Some(entry) = self.undo_stack.pop() {
            // Restore player position.
            self.player = entry.player_pos;
            self.move_count = self.move_count.saturating_sub(1);

            // Reverse box push if there was one.
            if let Some((box_from, box_to)) = entry.box_push {
                if let Some(b) = self.boxes.iter_mut().find(|b| **b == box_to) {
                    *b = box_from;
                }
                self.push_count = self.push_count.saturating_sub(1);
            }

            true
        } else {
            false
        }
    }

    // -- Input handling -----------------------------------------------------

    /// Handle key input from the user.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match self.screen {
            Screen::LevelSelect => self.handle_key_select(event),
            Screen::Playing => self.handle_key_playing(event),
            Screen::Won => self.handle_key_won(event),
        }
    }

    fn handle_key_select(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Up
                if self.select_cursor > 0 => {
                    self.select_cursor -= 1;
                }
            Key::Down
                if self.select_cursor + 1 < self.level_count() => {
                    self.select_cursor += 1;
                }
            Key::Enter | Key::Space => {
                self.start_level(self.select_cursor);
            }
            Key::Escape => {
                // No-op on select screen (already at root).
            }
            _ => {}
        }
    }

    fn handle_key_playing(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Up => {
                self.try_move(Direction::Up);
            }
            Key::Down => {
                self.try_move(Direction::Down);
            }
            Key::Left => {
                self.try_move(Direction::Left);
            }
            Key::Right => {
                self.try_move(Direction::Right);
            }
            Key::Z => {
                self.undo();
            }
            Key::R => {
                self.reset_level();
            }
            Key::Escape => {
                self.screen = Screen::LevelSelect;
                self.select_cursor = self.current_level;
            }
            _ => {}
        }
    }

    fn handle_key_won(&mut self, event: &KeyEvent) {
        match event.key {
            Key::Enter | Key::Space => {
                // Advance to the next level, or return to select if all done.
                if self.current_level + 1 < self.level_count() {
                    self.start_level(self.current_level + 1);
                    self.select_cursor = self.current_level;
                } else {
                    self.screen = Screen::LevelSelect;
                    self.select_cursor = self.current_level;
                }
            }
            Key::R => {
                // Replay current level.
                self.reset_level();
                self.screen = Screen::Playing;
            }
            Key::Escape => {
                self.screen = Screen::LevelSelect;
                self.select_cursor = self.current_level;
            }
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(ke) = event { self.handle_key(ke) }
    }

    // -- Layout helpers -----------------------------------------------------

    fn grid_pixel_width(&self) -> f32 {
        self.level_width as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    fn grid_pixel_height(&self) -> f32 {
        self.level_height as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP
    }

    fn grid_origin_x(&self) -> f32 {
        PADDING
    }

    fn grid_origin_y(&self) -> f32 {
        PADDING + HEADER_HEIGHT
    }

    fn window_width(&self) -> f32 {
        PADDING * 2.0 + self.grid_pixel_width()
    }

    fn window_height(&self) -> f32 {
        PADDING * 2.0 + HEADER_HEIGHT + self.grid_pixel_height() + FOOTER_HEIGHT
    }

    fn cell_screen_x(&self, col: usize) -> f32 {
        self.grid_origin_x() + col as f32 * (CELL_SIZE + CELL_GAP)
    }

    fn cell_screen_y(&self, row: usize) -> f32 {
        self.grid_origin_y() + row as f32 * (CELL_SIZE + CELL_GAP)
    }

    // -- Rendering ----------------------------------------------------------

    /// Produce render commands for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        match self.screen {
            Screen::LevelSelect => self.render_level_select(),
            Screen::Playing => self.render_playing(),
            Screen::Won => self.render_won(),
        }
    }

    fn render_level_select(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let panel_width: f32 = 400.0;
        let row_height: f32 = 36.0;
        let total_h = PADDING * 2.0
            + 60.0
            + self.level_count() as f32 * row_height
            + 40.0;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: panel_width,
            height: total_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: PADDING,
            text: String::from("Sokoban"),
            color: PEACH,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Subtitle.
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: PADDING + 34.0,
            text: String::from("Select a level to play"),
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let list_y = PADDING + 60.0;

        for i in 0..self.level_count() {
            let y = list_y + i as f32 * row_height;
            let is_selected = i == self.select_cursor;
            let is_completed = self.completed[i];

            // Row background.
            let bg_color = if is_selected { SURFACE0 } else { MANTLE };
            cmds.push(RenderCommand::FillRect {
                x: PADDING,
                y,
                width: panel_width - PADDING * 2.0,
                height: row_height - 4.0,
                color: bg_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Completion marker.
            if is_completed {
                cmds.push(RenderCommand::Text {
                    x: PADDING + 8.0,
                    y: y + 6.0,
                    text: String::from("[done]"),
                    color: GREEN,
                    font_size: LABEL_FONT_SIZE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            // Level name.
            let label = format!("Level {}", i + 1);
            let label_color = if is_selected { TEXT_COLOR } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: PADDING + 60.0,
                y: y + 6.0,
                text: label,
                color: label_color,
                font_size: SELECT_FONT_SIZE,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            // Selection indicator.
            if is_selected {
                cmds.push(RenderCommand::FillRect {
                    x: PADDING + 2.0,
                    y: y + 4.0,
                    width: 3.0,
                    height: row_height - 12.0,
                    color: PEACH,
                    corner_radii: CornerRadii::all(1.0),
                });
            }
        }

        // Footer instructions.
        let footer_y = list_y + self.level_count() as f32 * row_height + 8.0;
        cmds.push(RenderCommand::Text {
            x: PADDING,
            y: footer_y,
            text: String::from("Up/Down: select  Enter: play"),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }

    fn render_playing(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        let win_w = self.window_width();
        let win_h = self.window_height();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header bar.
        self.render_header(&mut cmds);

        // Grid background.
        cmds.push(RenderCommand::FillRect {
            x: self.grid_origin_x() - 4.0,
            y: self.grid_origin_y() - 4.0,
            width: self.grid_pixel_width() + 8.0,
            height: self.grid_pixel_height() + 8.0,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        // Render tiles, targets, boxes, and player.
        self.render_grid(&mut cmds);

        // Footer.
        self.render_footer(&mut cmds);

        cmds
    }

    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let header_w = self.window_width() - PADDING * 2.0;

        // Header background.
        cmds.push(RenderCommand::FillRect {
            x: PADDING,
            y: PADDING,
            width: header_w,
            height: HEADER_HEIGHT - 8.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(4.0),
        });

        // Level title.
        let title = format!("Level {}", self.current_level + 1);
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 10.0,
            text: title,
            color: PEACH,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Move counter.
        let moves_text = format!("Moves: {}", self.move_count);
        cmds.push(RenderCommand::Text {
            x: PADDING + 130.0,
            y: PADDING + 12.0,
            text: moves_text,
            color: TEXT_COLOR,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Push counter.
        let push_text = format!("Pushes: {}", self.push_count);
        cmds.push(RenderCommand::Text {
            x: PADDING + 240.0,
            y: PADDING + 12.0,
            text: push_text,
            color: TEXT_COLOR,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Boxes on targets indicator.
        let target_text = format!(
            "{}/{}",
            self.boxes_on_targets(),
            self.target_count()
        );
        cmds.push(RenderCommand::Text {
            x: PADDING + 350.0,
            y: PADDING + 12.0,
            text: target_text,
            color: if self.boxes_on_targets() == self.target_count() {
                GREEN
            } else {
                YELLOW
            },
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Header second row: controls hint.
        cmds.push(RenderCommand::Text {
            x: PADDING + 10.0,
            y: PADDING + 30.0,
            text: String::from("Arrows: move  Z: undo  R: reset  Esc: menu"),
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>) {
        // Draw each tile.
        for row in 0..self.level_height {
            for col in 0..self.level_width {
                let tile = self.tiles[row][col];
                let cx = self.cell_screen_x(col);
                let cy = self.cell_screen_y(row);

                let cell_color = match tile {
                    Tile::Wall => SURFACE1,
                    Tile::Floor => SURFACE0,
                    Tile::Target => SURFACE0,
                    Tile::Empty => BASE,
                };

                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: cell_color,
                    corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                });

                // Draw walls with a distinct border.
                if tile == Tile::Wall {
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx + 1.0,
                        y: cy + 1.0,
                        width: CELL_SIZE - 2.0,
                        height: CELL_SIZE - 2.0,
                        color: OVERLAY0,
                        line_width: 1.0,
                        corner_radii: CornerRadii::all(CELL_CORNER_RADIUS),
                    });
                }

                // Draw target markers (diamond shape via a small centered square).
                if tile == Tile::Target {
                    let marker_size: f32 = 12.0;
                    let mx = cx + (CELL_SIZE - marker_size) / 2.0;
                    let my = cy + (CELL_SIZE - marker_size) / 2.0;
                    cmds.push(RenderCommand::FillRect {
                        x: mx,
                        y: my,
                        width: marker_size,
                        height: marker_size,
                        color: TEAL,
                        corner_radii: CornerRadii::all(2.0),
                    });
                }
            }
        }

        // Draw boxes (on top of tiles).
        for box_pos in &self.boxes {
            let cx = self.cell_screen_x(box_pos.col as usize);
            let cy = self.cell_screen_y(box_pos.row as usize);
            let on_target = self.is_target(*box_pos);

            let box_color = if on_target { GREEN } else { YELLOW };
            let inset: f32 = 6.0;
            cmds.push(RenderCommand::FillRect {
                x: cx + inset,
                y: cy + inset,
                width: CELL_SIZE - inset * 2.0,
                height: CELL_SIZE - inset * 2.0,
                color: box_color,
                corner_radii: CornerRadii::all(4.0),
            });

            // Inner square to give the box some depth.
            let inner_inset: f32 = 12.0;
            let inner_color = if on_target { TEAL } else { PEACH };
            cmds.push(RenderCommand::FillRect {
                x: cx + inner_inset,
                y: cy + inner_inset,
                width: CELL_SIZE - inner_inset * 2.0,
                height: CELL_SIZE - inner_inset * 2.0,
                color: inner_color,
                corner_radii: CornerRadii::all(3.0),
            });
        }

        // Draw the player.
        let px = self.cell_screen_x(self.player.col as usize);
        let py = self.cell_screen_y(self.player.row as usize);
        let player_inset: f32 = 8.0;

        cmds.push(RenderCommand::FillRect {
            x: px + player_inset,
            y: py + player_inset,
            width: CELL_SIZE - player_inset * 2.0,
            height: CELL_SIZE - player_inset * 2.0,
            color: BLUE,
            corner_radii: CornerRadii::all(CELL_SIZE / 2.0 - player_inset),
        });

        // Player inner highlight.
        let highlight_inset: f32 = 14.0;
        cmds.push(RenderCommand::FillRect {
            x: px + highlight_inset,
            y: py + highlight_inset,
            width: CELL_SIZE - highlight_inset * 2.0,
            height: CELL_SIZE - highlight_inset * 2.0,
            color: LAVENDER,
            corner_radii: CornerRadii::all(CELL_SIZE / 2.0 - highlight_inset),
        });
    }

    fn render_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let footer_y = self.grid_origin_y() + self.grid_pixel_height() + 8.0;

        // Undo stack depth.
        let undo_text = format!("Undo stack: {}", self.undo_stack.len());
        cmds.push(RenderCommand::Text {
            x: self.grid_origin_x(),
            y: footer_y,
            text: undo_text,
            color: OVERLAY0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_won(&self) -> Vec<RenderCommand> {
        let mut cmds = self.render_playing();

        let win_w = self.window_width();
        let win_h = self.window_height();

        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: win_w,
            height: win_h,
            color: Color::rgba(17, 17, 27, 180),
            corner_radii: CornerRadii::ZERO,
        });

        // Victory box.
        let box_w: f32 = 320.0;
        let box_h: f32 = 150.0;
        let bx = (win_w - box_w) / 2.0;
        let by = (win_h - box_h) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: box_w,
            height: box_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::StrokeRect {
            x: bx,
            y: by,
            width: box_w,
            height: box_h,
            color: GREEN,
            line_width: 2.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: by + 20.0,
            text: String::from("Level Complete!"),
            color: GREEN,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Stats.
        let stats_text = format!(
            "Moves: {}  Pushes: {}",
            self.move_count, self.push_count
        );
        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: by + 58.0,
            text: stats_text,
            color: TEXT_COLOR,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Instructions.
        let next_text = if self.current_level + 1 < self.level_count() {
            String::from("Enter: next level  R: replay  Esc: menu")
        } else {
            String::from("All levels complete! R: replay  Esc: menu")
        };
        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: by + 90.0,
            text: next_text,
            color: SUBTEXT0,
            font_size: LABEL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Celebration stars (decorative dots around the box).
        let star_positions: [(f32, f32); 8] = [
            (bx - 10.0, by - 10.0),
            (bx + box_w + 2.0, by - 10.0),
            (bx - 10.0, by + box_h + 2.0),
            (bx + box_w + 2.0, by + box_h + 2.0),
            (bx + box_w / 2.0, by - 14.0),
            (bx + box_w / 2.0, by + box_h + 6.0),
            (bx - 14.0, by + box_h / 2.0),
            (bx + box_w + 6.0, by + box_h / 2.0),
        ];
        let star_colors = [YELLOW, PEACH, GREEN, TEAL, BLUE, MAUVE, RED, LAVENDER];
        for (i, (sx, sy)) in star_positions.iter().enumerate() {
            cmds.push(RenderCommand::FillRect {
                x: *sx,
                y: *sy,
                width: 8.0,
                height: 8.0,
                color: star_colors[i % star_colors.len()],
                corner_radii: CornerRadii::all(4.0),
            });
        }

        cmds
    }
}

// -- Entry point ----------------------------------------------------------------

fn main() {
    let _app = SokobanApp::new();
}

// ===============================================================================
// Tests
// ===============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Helper: create a key event -----------------------------------------
    fn key_event(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers {
                ctrl: false,
                alt: false,
                shift: false,
                super_key: false,
            },
            text: None,
        }
    }

    fn key_event_release(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers {
                ctrl: false,
                alt: false,
                shift: false,
                super_key: false,
            },
            text: None,
        }
    }

    // -- Helper: minimal 3x3 level string -----------------------------------
    fn tiny_level_str() -> &'static str {
        concat!(
            "####\n",
            "#.@#\n",
            "#$-#\n",
            "####\n",
        )
    }

    fn make_tiny_app() -> SokobanApp {
        let level = parse_level(tiny_level_str());
        let mut app = SokobanApp::new();
        app.levels = vec![level.clone()];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;
        app
    }

    // -----------------------------------------------------------------------
    // Level parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_wall() {
        let level = parse_level("###\n# #\n###\n");
        assert_eq!(level.tiles[0][0], Tile::Wall);
        assert_eq!(level.tiles[0][1], Tile::Wall);
        assert_eq!(level.tiles[1][0], Tile::Wall);
        assert_eq!(level.tiles[1][1], Tile::Floor);
    }

    #[test]
    fn parse_player_position() {
        let level = parse_level("###\n#@#\n###\n");
        assert_eq!(level.start_player, Pos::new(1, 1));
    }

    #[test]
    fn parse_box_position() {
        let level = parse_level("###\n#$#\n###\n");
        assert_eq!(level.start_boxes.len(), 1);
        assert_eq!(level.start_boxes[0], Pos::new(1, 1));
    }

    #[test]
    fn parse_target_tile() {
        let level = parse_level("###\n#.#\n###\n");
        assert_eq!(level.tiles[1][1], Tile::Target);
    }

    #[test]
    fn parse_player_on_target() {
        let level = parse_level("###\n#+#\n###\n");
        assert_eq!(level.start_player, Pos::new(1, 1));
        assert_eq!(level.tiles[1][1], Tile::Target);
    }

    #[test]
    fn parse_box_on_target() {
        let level = parse_level("###\n#*#\n###\n");
        assert_eq!(level.start_boxes.len(), 1);
        assert_eq!(level.start_boxes[0], Pos::new(1, 1));
        assert_eq!(level.tiles[1][1], Tile::Target);
    }

    #[test]
    fn parse_dash_is_floor() {
        let level = parse_level("#-#\n");
        assert_eq!(level.tiles[0][1], Tile::Floor);
    }

    #[test]
    fn parse_dimensions() {
        let level = parse_level("####\n#--#\n####\n");
        assert_eq!(level.width, 4);
        assert_eq!(level.height, 3);
    }

    #[test]
    fn parse_uneven_rows_normalized() {
        let level = parse_level("##\n####\n##\n");
        assert_eq!(level.width, 4);
        // Short rows are padded with Empty.
        assert_eq!(level.tiles[0].len(), 4);
        assert_eq!(level.tiles[0][2], Tile::Empty);
        assert_eq!(level.tiles[0][3], Tile::Empty);
    }

    #[test]
    fn parse_multiple_boxes() {
        let level = parse_level("######\n#$$$.#\n#--@-#\n######\n");
        assert_eq!(level.start_boxes.len(), 3);
    }

    #[test]
    fn parse_empty_string() {
        let level = parse_level("");
        assert_eq!(level.width, 0);
        assert_eq!(level.height, 0);
    }

    // -----------------------------------------------------------------------
    // Tile query tests
    // -----------------------------------------------------------------------

    #[test]
    fn tile_at_wall() {
        let app = make_tiny_app();
        assert_eq!(app.tile_at(Pos::new(0, 0)), Tile::Wall);
    }

    #[test]
    fn tile_at_floor() {
        let app = make_tiny_app();
        assert_eq!(app.tile_at(Pos::new(2, 2)), Tile::Floor);
    }

    #[test]
    fn tile_at_target() {
        let app = make_tiny_app();
        assert_eq!(app.tile_at(Pos::new(1, 1)), Tile::Target);
    }

    #[test]
    fn tile_at_out_of_bounds_negative() {
        let app = make_tiny_app();
        assert_eq!(app.tile_at(Pos::new(-1, 0)), Tile::Wall);
    }

    #[test]
    fn tile_at_out_of_bounds_large() {
        let app = make_tiny_app();
        assert_eq!(app.tile_at(Pos::new(100, 100)), Tile::Wall);
    }

    #[test]
    fn is_wall_true() {
        let app = make_tiny_app();
        assert!(app.is_wall(Pos::new(0, 0)));
    }

    #[test]
    fn is_wall_false() {
        let app = make_tiny_app();
        assert!(!app.is_wall(Pos::new(1, 2)));
    }

    // -----------------------------------------------------------------------
    // Box query tests
    // -----------------------------------------------------------------------

    #[test]
    fn has_box_true() {
        let app = make_tiny_app();
        // Box starts at row=2, col=1 in tiny_level_str.
        assert!(app.has_box(Pos::new(2, 1)));
    }

    #[test]
    fn has_box_false() {
        let app = make_tiny_app();
        assert!(!app.has_box(Pos::new(1, 1)));
    }

    #[test]
    fn is_target_true() {
        let app = make_tiny_app();
        assert!(app.is_target(Pos::new(1, 1)));
    }

    #[test]
    fn is_target_false() {
        let app = make_tiny_app();
        assert!(!app.is_target(Pos::new(2, 2)));
    }

    // -----------------------------------------------------------------------
    // Win detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn not_solved_initially() {
        let app = make_tiny_app();
        assert!(!app.is_solved());
    }

    #[test]
    fn solved_when_box_on_target() {
        let mut app = make_tiny_app();
        // Target is at (1,1), move the box there.
        app.boxes = vec![Pos::new(1, 1)];
        assert!(app.is_solved());
    }

    #[test]
    fn not_solved_partial() {
        // Two targets, one box on target, one not.
        let level = parse_level(concat!(
            "#####\n",
            "#.$.#\n",
            "#.@-#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;
        // Box is at (1,2), targets at (1,1) and (2,1). Not solved.
        assert!(!app.is_solved());
    }

    #[test]
    fn solved_with_empty_boxes_returns_false() {
        let mut app = make_tiny_app();
        app.boxes.clear();
        assert!(!app.is_solved());
    }

    #[test]
    fn boxes_on_targets_count() {
        let mut app = make_tiny_app();
        assert_eq!(app.boxes_on_targets(), 0);
        // Place the box on the target.
        app.boxes = vec![Pos::new(1, 1)];
        assert_eq!(app.boxes_on_targets(), 1);
    }

    #[test]
    fn target_count_tiny() {
        let app = make_tiny_app();
        assert_eq!(app.target_count(), 1);
    }

    // -----------------------------------------------------------------------
    // Movement tests
    // -----------------------------------------------------------------------

    #[test]
    fn move_into_wall_fails() {
        let mut app = make_tiny_app();
        // Player is at (1,2), up is wall at (0,2).
        let moved = app.try_move(Direction::Up);
        assert!(!moved);
        assert_eq!(app.player, Pos::new(1, 2));
    }

    #[test]
    fn move_into_wall_no_move_count() {
        let mut app = make_tiny_app();
        app.try_move(Direction::Up);
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn move_to_empty_floor() {
        // Use a level where the player can move freely.
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let moved = app.try_move(Direction::Up);
        assert!(moved);
        assert_eq!(app.player, Pos::new(1, 2));
        assert_eq!(app.move_count, 1);
    }

    #[test]
    fn move_to_target_square() {
        let mut app = make_tiny_app();
        // Player at (1,2), target at (1,1). Move left.
        // But there is also a box at (2,1), so moving left is fine
        // (no box at (1,1)).
        let moved = app.try_move(Direction::Left);
        assert!(moved);
        assert_eq!(app.player, Pos::new(1, 1));
    }

    #[test]
    fn move_down_increments_count() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Down);
        assert_eq!(app.move_count, 1);
        app.try_move(Direction::Left);
        assert_eq!(app.move_count, 2);
    }

    // -----------------------------------------------------------------------
    // Box pushing tests
    // -----------------------------------------------------------------------

    #[test]
    fn push_box_forward() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#@$-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let moved = app.try_move(Direction::Right);
        assert!(moved);
        assert_eq!(app.player, Pos::new(2, 2));
        assert!(app.has_box(Pos::new(2, 3)));
        assert_eq!(app.push_count, 1);
    }

    #[test]
    fn push_box_into_wall_fails() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#-$-#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        // Push box down; the cell below box (row 3) is a wall.
        let moved = app.try_move(Direction::Down);
        assert!(!moved);
        assert_eq!(app.player, Pos::new(1, 2));
    }

    #[test]
    fn push_box_into_another_box_fails() {
        let level = parse_level(concat!(
            "######\n",
            "#-@--#\n",
            "#-$$-#\n",
            "#----#\n",
            "######\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        // Player at (1,2), boxes at (2,2) and (2,3).
        // Push down into box at (2,2): the cell behind it (2,2+down=3,2) is free,
        // wait, we first need to check: player going down means pushing box at (2,2)
        // which needs (3,2) to be empty. (3,2) is floor. So it will work.
        // Instead, set up a scenario where the box behind is another box.
        // boxes at (2,2) and (3,2).
        app.boxes = vec![Pos::new(2, 2), Pos::new(3, 2)];
        let moved = app.try_move(Direction::Down);
        assert!(!moved);
    }

    #[test]
    fn push_box_onto_target() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$.#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let moved = app.try_move(Direction::Right);
        assert!(moved);
        // Box should now be on the target at (1,3).
        assert!(app.has_box(Pos::new(1, 3)));
        assert!(app.is_target(Pos::new(1, 3)));
        // Level is solved.
        assert_eq!(app.screen, Screen::Won);
    }

    #[test]
    fn push_box_off_target() {
        let level = parse_level(concat!(
            "#####\n",
            "#@*-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        // Box is on target at (1,2). Push it right to (1,3).
        let moved = app.try_move(Direction::Right);
        assert!(moved);
        assert!(app.has_box(Pos::new(1, 3)));
        assert!(!app.is_solved());
    }

    #[test]
    fn push_increments_both_counters() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Right);
        assert_eq!(app.move_count, 1);
        assert_eq!(app.push_count, 1);
    }

    #[test]
    fn walk_does_not_increment_push_count() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Down);
        assert_eq!(app.move_count, 1);
        assert_eq!(app.push_count, 0);
    }

    // -----------------------------------------------------------------------
    // Undo tests
    // -----------------------------------------------------------------------

    #[test]
    fn undo_single_walk() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let original = app.player;
        app.try_move(Direction::Down);
        assert_ne!(app.player, original);
        let undone = app.undo();
        assert!(undone);
        assert_eq!(app.player, original);
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn undo_push() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let original_player = app.player;
        let original_box = app.boxes[0];

        app.try_move(Direction::Right);
        assert_ne!(app.player, original_player);
        assert_ne!(app.boxes[0], original_box);

        let undone = app.undo();
        assert!(undone);
        assert_eq!(app.player, original_player);
        assert_eq!(app.boxes[0], original_box);
        assert_eq!(app.push_count, 0);
    }

    #[test]
    fn undo_empty_stack() {
        let mut app = make_tiny_app();
        let undone = app.undo();
        assert!(!undone);
    }

    #[test]
    fn undo_multiple_steps() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let start = app.player;
        app.try_move(Direction::Up);
        app.try_move(Direction::Right);
        app.try_move(Direction::Down);

        assert_eq!(app.move_count, 3);

        app.undo();
        app.undo();
        app.undo();
        assert_eq!(app.player, start);
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn undo_restores_move_count() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Down);
        assert_eq!(app.move_count, 1);
        app.undo();
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn undo_stack_limited() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        // Fill the undo stack beyond MAX_UNDO.
        for _ in 0..MAX_UNDO + 50 {
            app.try_move(Direction::Up);
            // Reset player manually to keep moving.
            app.player = Pos::new(2, 2);
        }
        assert!(app.undo_stack.len() <= MAX_UNDO);
    }

    // -----------------------------------------------------------------------
    // Reset tests
    // -----------------------------------------------------------------------

    #[test]
    fn reset_restores_player() {
        let mut app = make_tiny_app();
        let original = app.player;
        app.try_move(Direction::Left);
        app.reset_level();
        assert_eq!(app.player, original);
    }

    #[test]
    fn reset_restores_boxes() {
        let mut app = make_tiny_app();
        let original_boxes = app.boxes.clone();
        // Move the player down to push the box (box at (2,1)).
        // Player is at (1,2), let's move down then left to push.
        app.try_move(Direction::Down);
        app.reset_level();
        assert_eq!(app.boxes, original_boxes);
    }

    #[test]
    fn reset_clears_move_count() {
        let mut app = make_tiny_app();
        app.try_move(Direction::Left);
        app.reset_level();
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn reset_clears_undo_stack() {
        let mut app = make_tiny_app();
        app.try_move(Direction::Left);
        app.reset_level();
        assert!(app.undo_stack.is_empty());
    }

    #[test]
    fn reset_clears_push_count() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Right);
        assert_eq!(app.push_count, 1);
        app.reset_level();
        assert_eq!(app.push_count, 0);
    }

    // -----------------------------------------------------------------------
    // Key handling tests (playing)
    // -----------------------------------------------------------------------

    #[test]
    fn key_up_moves_player() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-@-#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.handle_key(&key_event(Key::Up));
        assert_eq!(app.player, Pos::new(1, 2));
    }

    #[test]
    fn key_down_moves_player() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.handle_key(&key_event(Key::Down));
        assert_eq!(app.player, Pos::new(2, 2));
    }

    #[test]
    fn key_left_moves_player() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.handle_key(&key_event(Key::Left));
        assert_eq!(app.player, Pos::new(1, 1));
    }

    #[test]
    fn key_right_moves_player() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.handle_key(&key_event(Key::Right));
        assert_eq!(app.player, Pos::new(1, 3));
    }

    #[test]
    fn key_z_undoes() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let start = app.player;
        app.handle_key(&key_event(Key::Down));
        app.handle_key(&key_event(Key::Z));
        assert_eq!(app.player, start);
    }

    #[test]
    fn key_r_resets() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.handle_key(&key_event(Key::Down));
        app.handle_key(&key_event(Key::Right));
        app.handle_key(&key_event(Key::R));
        assert_eq!(app.move_count, 0);
        assert_eq!(app.player, Pos::new(1, 2));
    }

    #[test]
    fn key_escape_goes_to_select() {
        let mut app = make_tiny_app();
        app.handle_key(&key_event(Key::Escape));
        assert_eq!(app.screen, Screen::LevelSelect);
    }

    #[test]
    fn key_release_ignored() {
        let mut app = make_tiny_app();
        let start = app.player;
        app.handle_key(&key_event_release(Key::Left));
        assert_eq!(app.player, start);
    }

    // -----------------------------------------------------------------------
    // Key handling tests (level select)
    // -----------------------------------------------------------------------

    #[test]
    fn select_cursor_initial() {
        let app = SokobanApp::new();
        assert_eq!(app.select_cursor, 0);
    }

    #[test]
    fn select_cursor_down() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.handle_key(&key_event(Key::Down));
        assert_eq!(app.select_cursor, 1);
    }

    #[test]
    fn select_cursor_up() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.select_cursor = 3;
        app.handle_key(&key_event(Key::Up));
        assert_eq!(app.select_cursor, 2);
    }

    #[test]
    fn select_cursor_no_underflow() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.select_cursor = 0;
        app.handle_key(&key_event(Key::Up));
        assert_eq!(app.select_cursor, 0);
    }

    #[test]
    fn select_cursor_no_overflow() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.select_cursor = app.level_count() - 1;
        app.handle_key(&key_event(Key::Down));
        assert_eq!(app.select_cursor, app.level_count() - 1);
    }

    #[test]
    fn select_enter_starts_level() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.select_cursor = 2;
        app.handle_key(&key_event(Key::Enter));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.current_level, 2);
    }

    #[test]
    fn select_space_starts_level() {
        let mut app = SokobanApp::new();
        app.screen = Screen::LevelSelect;
        app.select_cursor = 0;
        app.handle_key(&key_event(Key::Space));
        assert_eq!(app.screen, Screen::Playing);
    }

    // -----------------------------------------------------------------------
    // Key handling tests (won screen)
    // -----------------------------------------------------------------------

    #[test]
    fn won_enter_advances_level() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        app.completed[0] = true;
        app.handle_key(&key_event(Key::Enter));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.current_level, 1);
    }

    #[test]
    fn won_enter_last_level_goes_to_select() {
        let mut app = SokobanApp::new();
        let last = app.level_count() - 1;
        app.start_level(last);
        app.screen = Screen::Won;
        app.completed[last] = true;
        app.handle_key(&key_event(Key::Enter));
        assert_eq!(app.screen, Screen::LevelSelect);
    }

    #[test]
    fn won_r_replays() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        app.handle_key(&key_event(Key::R));
        assert_eq!(app.screen, Screen::Playing);
        assert_eq!(app.current_level, 0);
        assert_eq!(app.move_count, 0);
    }

    #[test]
    fn won_escape_goes_to_select() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        app.handle_key(&key_event(Key::Escape));
        assert_eq!(app.screen, Screen::LevelSelect);
    }

    // -----------------------------------------------------------------------
    // Level loading tests
    // -----------------------------------------------------------------------

    #[test]
    fn load_level_sets_width_height() {
        let app = SokobanApp::new();
        assert!(app.level_width > 0);
        assert!(app.level_height > 0);
    }

    #[test]
    fn load_level_sets_player() {
        let app = SokobanApp::new();
        assert!(app.player.row >= 0);
        assert!(app.player.col >= 0);
    }

    #[test]
    fn load_level_sets_boxes() {
        let app = SokobanApp::new();
        assert!(!app.boxes.is_empty());
    }

    #[test]
    fn start_level_changes_screen() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        assert_eq!(app.screen, Screen::Playing);
    }

    // -----------------------------------------------------------------------
    // Builtin levels tests
    // -----------------------------------------------------------------------

    #[test]
    fn builtin_levels_count() {
        let levels = builtin_levels();
        assert!(levels.len() >= 10);
    }

    #[test]
    fn builtin_levels_all_parse() {
        let levels = builtin_levels();
        for (i, src) in levels.iter().enumerate() {
            let level = parse_level(src);
            assert!(level.width > 0, "level {} has zero width", i + 1);
            assert!(level.height > 0, "level {} has zero height", i + 1);
        }
    }

    #[test]
    fn builtin_levels_all_have_player() {
        let levels = builtin_levels();
        for (i, src) in levels.iter().enumerate() {
            let level = parse_level(src);
            // Player should be inside the grid.
            assert!(
                level.start_player.row > 0 || level.start_player.col > 0,
                "level {} has player at origin",
                i + 1
            );
        }
    }

    #[test]
    fn builtin_levels_all_have_boxes() {
        let levels = builtin_levels();
        for (i, src) in levels.iter().enumerate() {
            let level = parse_level(src);
            assert!(
                !level.start_boxes.is_empty(),
                "level {} has no boxes",
                i + 1
            );
        }
    }

    #[test]
    fn builtin_levels_boxes_equal_targets() {
        let levels = builtin_levels();
        for (i, src) in levels.iter().enumerate() {
            let level = parse_level(src);
            let target_count: usize = level
                .tiles
                .iter()
                .flatten()
                .filter(|t| **t == Tile::Target)
                .count();
            assert_eq!(
                level.start_boxes.len(),
                target_count,
                "level {} boxes={} targets={}",
                i + 1,
                level.start_boxes.len(),
                target_count
            );
        }
    }

    // -----------------------------------------------------------------------
    // Rendering tests
    // -----------------------------------------------------------------------

    #[test]
    fn render_playing_non_empty() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_level_select_non_empty() {
        let app = SokobanApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_won_non_empty() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_has_background_rect() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        let cmds = app.render();
        let has_bg = cmds.iter().any(|c| matches!(c, RenderCommand::FillRect { x, y, .. } if *x == 0.0 && *y == 0.0));
        assert!(has_bg);
    }

    #[test]
    fn render_contains_text() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        let cmds = app.render();
        let has_text = cmds.iter().any(|c| matches!(c, RenderCommand::Text { .. }));
        assert!(has_text);
    }

    #[test]
    fn render_select_has_level_text() {
        let app = SokobanApp::new();
        let cmds = app.render();
        let has_level_text = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Level"))
        });
        assert!(has_level_text);
    }

    #[test]
    fn render_won_has_complete_text() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        let cmds = app.render();
        let has_complete = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Complete"))
        });
        assert!(has_complete);
    }

    #[test]
    fn render_playing_shows_move_count() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.move_count = 42;
        let cmds = app.render();
        let has_moves = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("42"))
        });
        assert!(has_moves);
    }

    // -----------------------------------------------------------------------
    // handle_event tests
    // -----------------------------------------------------------------------

    #[test]
    fn handle_event_key() {
        let level = parse_level(concat!(
            "#####\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        let event = Event::Key(key_event(Key::Down));
        app.handle_event(&event);
        assert_eq!(app.player, Pos::new(2, 2));
    }

    // -----------------------------------------------------------------------
    // Direction tests
    // -----------------------------------------------------------------------

    #[test]
    fn direction_up_delta() {
        let (dr, dc) = Direction::Up.delta();
        assert_eq!(dr, -1);
        assert_eq!(dc, 0);
    }

    #[test]
    fn direction_down_delta() {
        let (dr, dc) = Direction::Down.delta();
        assert_eq!(dr, 1);
        assert_eq!(dc, 0);
    }

    #[test]
    fn direction_left_delta() {
        let (dr, dc) = Direction::Left.delta();
        assert_eq!(dr, 0);
        assert_eq!(dc, -1);
    }

    #[test]
    fn direction_right_delta() {
        let (dr, dc) = Direction::Right.delta();
        assert_eq!(dr, 0);
        assert_eq!(dc, 1);
    }

    // -----------------------------------------------------------------------
    // Pos tests
    // -----------------------------------------------------------------------

    #[test]
    fn pos_moved() {
        let p = Pos::new(3, 4);
        let up = p.moved(Direction::Up);
        assert_eq!(up, Pos::new(2, 4));
    }

    #[test]
    fn pos_equality() {
        assert_eq!(Pos::new(1, 2), Pos::new(1, 2));
        assert_ne!(Pos::new(1, 2), Pos::new(2, 1));
    }

    // -----------------------------------------------------------------------
    // Completion tracking tests
    // -----------------------------------------------------------------------

    #[test]
    fn completion_initially_false() {
        let app = SokobanApp::new();
        for c in &app.completed {
            assert!(!c);
        }
    }

    #[test]
    fn completion_set_on_solve() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$.#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Right);
        assert!(app.completed[0]);
    }

    #[test]
    fn screen_transitions_to_won_on_solve() {
        let level = parse_level(concat!(
            "#####\n",
            "#@$.#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Right);
        assert_eq!(app.screen, Screen::Won);
    }

    // -----------------------------------------------------------------------
    // Miscellaneous / edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn level_count_matches_builtin() {
        let app = SokobanApp::new();
        assert_eq!(app.level_count(), builtin_levels().len());
    }

    #[test]
    fn grid_dimensions_positive() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        assert!(app.grid_pixel_width() > 0.0);
        assert!(app.grid_pixel_height() > 0.0);
    }

    #[test]
    fn window_dimensions_positive() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        assert!(app.window_width() > 0.0);
        assert!(app.window_height() > 0.0);
    }

    #[test]
    fn cell_screen_position_increases() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        let x0 = app.cell_screen_x(0);
        let x1 = app.cell_screen_x(1);
        assert!(x1 > x0);
    }

    #[test]
    fn multiple_undos_then_redo_via_move() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-@-#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Up);
        app.try_move(Direction::Right);
        app.undo();
        // After undo, redo by moving right again.
        app.try_move(Direction::Right);
        assert_eq!(app.player, Pos::new(1, 3));
    }

    #[test]
    fn push_box_upward() {
        let level = parse_level(concat!(
            "#####\n",
            "#---#\n",
            "#-$-#\n",
            "#-@-#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Up);
        assert_eq!(app.player, Pos::new(2, 2));
        assert!(app.has_box(Pos::new(1, 2)));
    }

    #[test]
    fn push_box_leftward() {
        let level = parse_level(concat!(
            "#####\n",
            "#-$@#\n",
            "#---#\n",
            "#####\n",
        ));
        let mut app = SokobanApp::new();
        app.levels = vec![level];
        app.completed = vec![false];
        app.load_level(0);
        app.screen = Screen::Playing;

        app.try_move(Direction::Left);
        assert_eq!(app.player, Pos::new(1, 2));
        assert!(app.has_box(Pos::new(1, 1)));
    }

    #[test]
    fn won_advance_updates_select_cursor() {
        let mut app = SokobanApp::new();
        app.start_level(0);
        app.screen = Screen::Won;
        app.completed[0] = true;
        app.handle_key(&key_event(Key::Enter));
        assert_eq!(app.select_cursor, 1);
    }
}
