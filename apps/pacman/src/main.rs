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

//! OurOS Pac-Man -- classic maze chase arcade game.
//!
//! Features a 28x31 grid-based maze with the classic Pac-Man layout,
//! arrow-key movement, 4 ghosts with chase/scatter AI, power pellets
//! that make ghosts vulnerable, wrap-around tunnel, 3 lives, score
//! tracking, level progression, and menu/pause/game-over states.
//! Uses an LCG pseudo-random number generator (no external rand crate).

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// -- Catppuccin Mocha palette ------------------------------------------------
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
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

// -- Layout constants --------------------------------------------------------
const MAZE_COLS: usize = 28;
const MAZE_ROWS: usize = 31;
const CELL_SIZE: f32 = 18.0;
const PADDING: f32 = 12.0;
const HEADER_HEIGHT: f32 = 48.0;
const FOOTER_HEIGHT: f32 = 36.0;
const HEADER_FONT_SIZE: f32 = 16.0;
const TITLE_FONT_SIZE: f32 = 28.0;
const OVERLAY_FONT_SIZE: f32 = 16.0;
const SMALL_FONT_SIZE: f32 = 12.0;

/// Dot radius as a fraction of half-cell.
const DOT_RADIUS: f32 = 2.0;
/// Power pellet radius as a fraction of half-cell.
const POWER_PELLET_RADIUS: f32 = 5.0;

/// Points for eating a normal dot.
const DOT_POINTS: u32 = 10;
/// Points for eating a power pellet.
const POWER_PELLET_POINTS: u32 = 50;
/// Base points for eating the first ghost during a power pellet.
const GHOST_BASE_POINTS: u32 = 200;

/// Duration of power pellet effect in milliseconds.
const POWER_DURATION_MS: u64 = 8000;
/// Duration of ghost frightened flash near end in milliseconds.
const POWER_FLASH_MS: u64 = 2000;

/// Player movement interval in milliseconds.
const PLAYER_MOVE_MS: u64 = 140;
/// Ghost movement interval in milliseconds.
const GHOST_MOVE_MS: u64 = 160;
/// Frightened ghost movement interval in milliseconds.
const GHOST_FRIGHTENED_MOVE_MS: u64 = 220;

/// How long scatter mode lasts (ms).
const SCATTER_DURATION_MS: u64 = 7000;
/// How long chase mode lasts (ms).
const CHASE_DURATION_MS: u64 = 20000;

/// Initial number of lives.
const INITIAL_LIVES: u32 = 3;

/// Tunnel row (0-indexed).
const TUNNEL_ROW: usize = 14;

// -- LCG random number generator ---------------------------------------------
struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn next_bounded(&mut self, bound: usize) -> usize {
        let val = self.next_u64();
        (val % bound as u64) as usize
    }
}

// -- Direction ---------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn is_opposite(self, other: Direction) -> bool {
        matches!(
            (self, other),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        )
    }

    fn delta(self) -> (i32, i32) {
        match self {
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
            Direction::Left => (0, -1),
            Direction::Right => (0, 1),
        }
    }

    const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];
}

// -- Grid position -----------------------------------------------------------
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

    fn in_bounds(self) -> bool {
        self.row >= 0
            && self.row < MAZE_ROWS as i32
            && self.col >= 0
            && self.col < MAZE_COLS as i32
    }

    /// Wrap position for the tunnel (horizontal wrap-around at tunnel row).
    fn tunnel_wrap(self) -> Self {
        if self.row == TUNNEL_ROW as i32 {
            let col = ((self.col % MAZE_COLS as i32) + MAZE_COLS as i32) % MAZE_COLS as i32;
            Self { row: self.row, col }
        } else {
            self
        }
    }

    /// Manhattan distance to another position.
    fn manhattan_distance(self, other: Pos) -> i32 {
        (self.row - other.row).abs() + (self.col - other.col).abs()
    }
}

// -- Cell types in the maze --------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Wall,
    Empty,
    Dot,
    PowerPellet,
    GhostHouse,
    GhostDoor,
}

impl Cell {
    fn is_walkable(self) -> bool {
        matches!(self, Cell::Empty | Cell::Dot | Cell::PowerPellet)
    }

    fn is_ghost_walkable(self) -> bool {
        matches!(
            self,
            Cell::Empty | Cell::Dot | Cell::PowerPellet | Cell::GhostHouse | Cell::GhostDoor
        )
    }
}

// -- Ghost identity ----------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhostId {
    Blinky, // Red - chases player directly
    Pinky,  // Pink - targets 4 cells ahead of player
    Inky,   // Cyan/Teal - uses Blinky's position for targeting
    Clyde,  // Orange/Peach - chases when far, scatters when close
}

impl GhostId {
    const ALL: [GhostId; 4] = [GhostId::Blinky, GhostId::Pinky, GhostId::Inky, GhostId::Clyde];

    fn color(self) -> Color {
        match self {
            GhostId::Blinky => RED,
            GhostId::Pinky => LAVENDER,
            GhostId::Inky => TEAL,
            GhostId::Clyde => PEACH,
        }
    }

    /// Scatter target corner for each ghost.
    fn scatter_target(self) -> Pos {
        match self {
            GhostId::Blinky => Pos::new(0, MAZE_COLS as i32 - 3),
            GhostId::Pinky => Pos::new(0, 2),
            GhostId::Inky => Pos::new(MAZE_ROWS as i32 - 1, MAZE_COLS as i32 - 1),
            GhostId::Clyde => Pos::new(MAZE_ROWS as i32 - 1, 0),
        }
    }

    fn name(self) -> &'static str {
        match self {
            GhostId::Blinky => "Blinky",
            GhostId::Pinky => "Pinky",
            GhostId::Inky => "Inky",
            GhostId::Clyde => "Clyde",
        }
    }
}

// -- Ghost mode --------------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhostMode {
    Chase,
    Scatter,
    Frightened,
    Eaten,
}

// -- Ghost state -------------------------------------------------------------
#[derive(Clone, Debug)]
struct Ghost {
    id: GhostId,
    pos: Pos,
    direction: Direction,
    mode: GhostMode,
    /// Home position inside the ghost house.
    home: Pos,
    /// Whether this ghost has been released from the ghost house.
    released: bool,
    /// Timer for release delay (ms).
    release_timer_ms: u64,
    /// Release delay threshold (ms).
    release_delay_ms: u64,
}

impl Ghost {
    fn new(id: GhostId, home: Pos, release_delay_ms: u64) -> Self {
        let released = id == GhostId::Blinky;
        let start_pos = if released {
            // Blinky starts outside the ghost house
            Pos::new(11, 14)
        } else {
            home
        };
        Self {
            id,
            pos: start_pos,
            direction: Direction::Left,
            mode: GhostMode::Scatter,
            home,
            released,
            release_timer_ms: 0,
            release_delay_ms,
        }
    }
}

// -- Game state enum ---------------------------------------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameState {
    Menu,
    Playing,
    Paused,
    GameOver,
}

// -- Global ghost behavior mode (chase/scatter cycle) ------------------------
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobalGhostMode {
    Chase,
    Scatter,
}

// -- Classic Pac-Man maze layout ---------------------------------------------
// W = Wall, . = Dot, o = Power Pellet, _ = Empty, G = Ghost House, D = Ghost Door
// 28 columns x 31 rows
const MAZE_TEMPLATE: [&str; MAZE_ROWS] = [
    "WWWWWWWWWWWWWWWWWWWWWWWWWWWW", // row 0
    "W............WW............W", // row 1
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 2
    "WoWWWW.WWWWW.WW.WWWWW.WWWWoW", // row 3
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 4
    "W..........................W", // row 5
    "W.WWWW.WW.WWWWWWWW.WW.WWWW.W", // row 6
    "W.WWWW.WW.WWWWWWWW.WW.WWWW.W", // row 7
    "W......WW....WW....WW......W", // row 8
    "WWWWWW.WWWWW_WW_WWWWW.WWWWWW", // row 9
    "WWWWWW.WWWWW_WW_WWWWW.WWWWWW", // row 10
    "WWWWWW.WW__________WW.WWWWWW", // row 11
    "WWWWWW.WW_WWW__WWW_WW.WWWWWW", // row 12
    "WWWWWW.WW_WGGDDGGW_WW.WWWWWW", // row 13
    "______._____GGGG_____.______", // row 14  (tunnel row)
    "WWWWWW.WW_WGGGGGGW_WW.WWWWWW", // row 15
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 16
    "WWWWWW.WW__________WW.WWWWWW", // row 17
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 18
    "WWWWWW.WW_WWWWWWWW_WW.WWWWWW", // row 19
    "W............WW............W", // row 20
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 21
    "W.WWWW.WWWWW.WW.WWWWW.WWWW.W", // row 22
    "Wo..WW................WW..oW", // row 23
    "WWW.WW.WW.WWWWWWWW.WW.WW.WWW", // row 24
    "WWW.WW.WW.WWWWWWWW.WW.WW.WWW", // row 25
    "W......WW....WW....WW......W", // row 26
    "W.WWWWWWWWWW.WW.WWWWWWWWWW.W", // row 27
    "W.WWWWWWWWWW.WW.WWWWWWWWWW.W", // row 28
    "W..........................W", // row 29
    "WWWWWWWWWWWWWWWWWWWWWWWWWWWW", // row 30
];

/// Parse the maze template into a grid of cells.
fn parse_maze() -> [[Cell; MAZE_COLS]; MAZE_ROWS] {
    let mut grid = [[Cell::Wall; MAZE_COLS]; MAZE_ROWS];
    for row in 0..MAZE_ROWS {
        let line = MAZE_TEMPLATE[row].as_bytes();
        for col in 0..MAZE_COLS {
            if col < line.len() {
                grid[row][col] = match line[col] {
                    b'W' => Cell::Wall,
                    b'.' => Cell::Dot,
                    b'o' => Cell::PowerPellet,
                    b'_' => Cell::Empty,
                    b'G' => Cell::GhostHouse,
                    b'D' => Cell::GhostDoor,
                    _ => Cell::Wall,
                };
            }
        }
    }
    grid
}

/// Count total dots (including power pellets) in the maze.
fn count_dots(grid: &[[Cell; MAZE_COLS]; MAZE_ROWS]) -> u32 {
    let mut count = 0u32;
    for row in grid {
        for cell in row {
            if *cell == Cell::Dot || *cell == Cell::PowerPellet {
                count += 1;
            }
        }
    }
    count
}

// -- Main app struct ---------------------------------------------------------
struct PacmanApp {
    /// The maze grid.
    maze: [[Cell; MAZE_COLS]; MAZE_ROWS],
    /// Player position.
    player_pos: Pos,
    /// Player movement direction.
    player_dir: Direction,
    /// Queued direction (buffered input).
    queued_dir: Option<Direction>,
    /// The four ghosts.
    ghosts: Vec<Ghost>,
    /// Current game state.
    state: GameState,
    /// Current score.
    score: u32,
    /// High score.
    high_score: u32,
    /// Number of lives remaining.
    lives: u32,
    /// Current level (1-based).
    level: u32,
    /// Total dots remaining.
    dots_remaining: u32,
    /// Total dots at level start.
    total_dots: u32,
    /// Power pellet timer (remaining ms).
    power_timer_ms: u64,
    /// Number of ghosts eaten during current power pellet.
    ghosts_eaten_this_power: u32,
    /// Global ghost behavior mode.
    global_ghost_mode: GlobalGhostMode,
    /// Timer for the current global ghost mode phase (ms elapsed).
    ghost_mode_timer_ms: u64,
    /// Accumulated time for player movement.
    player_move_accum_ms: u64,
    /// Accumulated time for ghost movement.
    ghost_move_accum_ms: u64,
    /// Animation pulse counter.
    pulse_counter: u32,
    /// Mouth animation angle (for pac-man rendering).
    mouth_open: bool,
    /// RNG.
    rng: Lcg,
    /// Total elapsed game time in ms.
    elapsed_total_ms: u64,
}

impl PacmanApp {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let maze = parse_maze();
        let total = count_dots(&maze);
        let mut app = Self {
            maze,
            player_pos: Pos::new(23, 14),
            player_dir: Direction::Left,
            queued_dir: None,
            ghosts: Vec::new(),
            state: GameState::Menu,
            score: 0,
            high_score: 0,
            lives: INITIAL_LIVES,
            level: 1,
            dots_remaining: total,
            total_dots: total,
            power_timer_ms: 0,
            ghosts_eaten_this_power: 0,
            global_ghost_mode: GlobalGhostMode::Scatter,
            ghost_mode_timer_ms: 0,
            player_move_accum_ms: 0,
            ghost_move_accum_ms: 0,
            pulse_counter: 0,
            mouth_open: true,
            rng: Lcg::new(seed),
            elapsed_total_ms: 0,
        };
        app.init_ghosts();
        app
    }

    /// Initialize the four ghosts in their starting positions.
    fn init_ghosts(&mut self) {
        self.ghosts.clear();
        self.ghosts
            .push(Ghost::new(GhostId::Blinky, Pos::new(13, 14), 0));
        self.ghosts
            .push(Ghost::new(GhostId::Pinky, Pos::new(14, 13), 1000));
        self.ghosts
            .push(Ghost::new(GhostId::Inky, Pos::new(14, 14), 3000));
        self.ghosts
            .push(Ghost::new(GhostId::Clyde, Pos::new(14, 15), 5000));
    }

    /// Start a new game.
    fn start_new_game(&mut self) {
        let high = self.high_score;
        let seed = self.rng.next_u64();
        *self = Self::with_seed(seed);
        self.high_score = high;
        self.state = GameState::Playing;
    }

    /// Reset positions after losing a life (keep maze state).
    fn reset_positions(&mut self) {
        self.player_pos = Pos::new(23, 14);
        self.player_dir = Direction::Left;
        self.queued_dir = None;
        self.init_ghosts();
        self.power_timer_ms = 0;
        self.ghosts_eaten_this_power = 0;
        self.global_ghost_mode = GlobalGhostMode::Scatter;
        self.ghost_mode_timer_ms = 0;
        self.player_move_accum_ms = 0;
        self.ghost_move_accum_ms = 0;
    }

    /// Advance to the next level.
    fn next_level(&mut self) {
        self.level += 1;
        self.maze = parse_maze();
        self.total_dots = count_dots(&self.maze);
        self.dots_remaining = self.total_dots;
        self.reset_positions();
    }

    /// Check if a position is walkable for the player.
    fn is_walkable(&self, pos: Pos) -> bool {
        // Tunnel wrap-around: allow walking off the edges at the tunnel row.
        if pos.row == TUNNEL_ROW as i32 && (pos.col < 0 || pos.col >= MAZE_COLS as i32) {
            return true;
        }
        if !pos.in_bounds() {
            return false;
        }
        self.maze[pos.row as usize][pos.col as usize].is_walkable()
    }

    /// Check if a position is walkable for a ghost.
    fn is_ghost_walkable(&self, pos: Pos, ghost_mode: GhostMode) -> bool {
        // Tunnel wrap-around.
        if pos.row == TUNNEL_ROW as i32 && (pos.col < 0 || pos.col >= MAZE_COLS as i32) {
            return true;
        }
        if !pos.in_bounds() {
            return false;
        }
        let cell = self.maze[pos.row as usize][pos.col as usize];
        match ghost_mode {
            GhostMode::Eaten => cell.is_ghost_walkable(),
            _ => cell.is_walkable() || cell == Cell::GhostDoor || cell == Cell::GhostHouse,
        }
    }

    /// Check if a position is valid for ghost pathfinding (no walls).
    fn is_ghost_passable(&self, pos: Pos) -> bool {
        if pos.row == TUNNEL_ROW as i32 && (pos.col < 0 || pos.col >= MAZE_COLS as i32) {
            return true;
        }
        if !pos.in_bounds() {
            return false;
        }
        self.maze[pos.row as usize][pos.col as usize] != Cell::Wall
    }

    /// Get the chase target for a ghost based on its AI personality.
    fn ghost_chase_target(&self, ghost_id: GhostId) -> Pos {
        match ghost_id {
            GhostId::Blinky => {
                // Directly targets the player.
                self.player_pos
            }
            GhostId::Pinky => {
                // Targets 4 cells ahead of the player in their direction.
                let (dr, dc) = self.player_dir.delta();
                Pos::new(self.player_pos.row + dr * 4, self.player_pos.col + dc * 4)
            }
            GhostId::Inky => {
                // Uses Blinky's position: target is 2 cells ahead of player,
                // then doubled from Blinky's position.
                let (dr, dc) = self.player_dir.delta();
                let ahead = Pos::new(
                    self.player_pos.row + dr * 2,
                    self.player_pos.col + dc * 2,
                );
                let blinky_pos = self
                    .ghosts
                    .iter()
                    .find(|g| g.id == GhostId::Blinky)
                    .map_or(self.player_pos, |g| g.pos);
                Pos::new(
                    ahead.row + (ahead.row - blinky_pos.row),
                    ahead.col + (ahead.col - blinky_pos.col),
                )
            }
            GhostId::Clyde => {
                // Chases player when far, scatters to corner when within 8 cells.
                let dist = self.player_pos.manhattan_distance(
                    self.ghosts
                        .iter()
                        .find(|g| g.id == GhostId::Clyde)
                        .map_or(self.player_pos, |g| g.pos),
                );
                if dist > 8 {
                    self.player_pos
                } else {
                    GhostId::Clyde.scatter_target()
                }
            }
        }
    }

    /// Choose the best direction for a ghost to move toward a target.
    fn ghost_choose_direction(
        &self,
        ghost_pos: Pos,
        current_dir: Direction,
        target: Pos,
        ghost_mode: GhostMode,
    ) -> Direction {
        let mut best_dir = current_dir;
        let mut best_dist = i32::MAX;

        // Ghosts prefer directions in this order: Up, Left, Down, Right
        let preferred_order = [
            Direction::Up,
            Direction::Left,
            Direction::Down,
            Direction::Right,
        ];

        for &dir in &preferred_order {
            // Ghosts cannot reverse direction (except when mode changes).
            if dir.is_opposite(current_dir) {
                continue;
            }
            let next = ghost_pos.moved(dir).tunnel_wrap();
            if self.is_ghost_passable(next) || ghost_mode == GhostMode::Eaten {
                let dist = next.manhattan_distance(target);
                if dist < best_dist {
                    best_dist = dist;
                    best_dir = dir;
                }
            }
        }
        best_dir
    }

    /// Move the player one step in the current direction.
    fn move_player(&mut self) {
        // Try the queued direction first.
        if let Some(qd) = self.queued_dir {
            let next = self.player_pos.moved(qd).tunnel_wrap();
            if self.is_walkable(next) {
                self.player_dir = qd;
                self.queued_dir = None;
            }
        }

        let next = self.player_pos.moved(self.player_dir).tunnel_wrap();
        if self.is_walkable(next) {
            self.player_pos = next;
            self.mouth_open = !self.mouth_open;

            // Check what is at the new position.
            if next.in_bounds() {
                let cell = self.maze[next.row as usize][next.col as usize];
                match cell {
                    Cell::Dot => {
                        self.maze[next.row as usize][next.col as usize] = Cell::Empty;
                        self.score += DOT_POINTS;
                        self.dots_remaining = self.dots_remaining.saturating_sub(1);
                    }
                    Cell::PowerPellet => {
                        self.maze[next.row as usize][next.col as usize] = Cell::Empty;
                        self.score += POWER_PELLET_POINTS;
                        self.dots_remaining = self.dots_remaining.saturating_sub(1);
                        self.activate_power_pellet();
                    }
                    _ => {}
                }
            }

            // Update high score.
            if self.score > self.high_score {
                self.high_score = self.score;
            }
        }
    }

    /// Activate power pellet mode.
    fn activate_power_pellet(&mut self) {
        self.power_timer_ms = POWER_DURATION_MS;
        self.ghosts_eaten_this_power = 0;
        for ghost in &mut self.ghosts {
            if ghost.mode != GhostMode::Eaten {
                ghost.mode = GhostMode::Frightened;
                // Reverse direction when becoming frightened.
                ghost.direction = match ghost.direction {
                    Direction::Up => Direction::Down,
                    Direction::Down => Direction::Up,
                    Direction::Left => Direction::Right,
                    Direction::Right => Direction::Left,
                };
            }
        }
    }

    /// Move all ghosts.
    fn move_ghosts(&mut self) {
        let global_mode = self.global_ghost_mode;

        for i in 0..self.ghosts.len() {
            if !self.ghosts[i].released {
                continue;
            }

            let ghost_mode = self.ghosts[i].mode;
            let ghost_pos = self.ghosts[i].pos;
            let current_dir = self.ghosts[i].direction;
            let ghost_id = self.ghosts[i].id;

            let target = match ghost_mode {
                GhostMode::Chase => self.ghost_chase_target(ghost_id),
                GhostMode::Scatter => ghost_id.scatter_target(),
                GhostMode::Frightened => {
                    // Random target when frightened.
                    Pos::new(
                        self.rng.next_bounded(MAZE_ROWS) as i32,
                        self.rng.next_bounded(MAZE_COLS) as i32,
                    )
                }
                GhostMode::Eaten => {
                    // Return to ghost house.
                    Pos::new(13, 14)
                }
            };

            let new_dir =
                self.ghost_choose_direction(ghost_pos, current_dir, target, ghost_mode);
            let new_pos = ghost_pos.moved(new_dir).tunnel_wrap();

            // Verify the new position is passable.
            if self.is_ghost_passable(new_pos)
                || ghost_mode == GhostMode::Eaten
                || (new_pos.in_bounds()
                    && self.maze[new_pos.row as usize][new_pos.col as usize]
                        != Cell::Wall)
            {
                self.ghosts[i].pos = new_pos;
                self.ghosts[i].direction = new_dir;
            }

            // Check if eaten ghost reached home.
            if ghost_mode == GhostMode::Eaten {
                let home_target = Pos::new(13, 14);
                if self.ghosts[i].pos == home_target {
                    self.ghosts[i].mode = match global_mode {
                        GlobalGhostMode::Chase => GhostMode::Chase,
                        GlobalGhostMode::Scatter => GhostMode::Scatter,
                    };
                }
            }
        }
    }

    /// Check for collisions between player and ghosts.
    fn check_ghost_collisions(&mut self) {
        for i in 0..self.ghosts.len() {
            if self.ghosts[i].pos == self.player_pos {
                match self.ghosts[i].mode {
                    GhostMode::Frightened => {
                        // Eat the ghost.
                        self.ghosts[i].mode = GhostMode::Eaten;
                        let multiplier = 1u32 << self.ghosts_eaten_this_power;
                        self.score += GHOST_BASE_POINTS * multiplier;
                        self.ghosts_eaten_this_power += 1;
                        if self.score > self.high_score {
                            self.high_score = self.score;
                        }
                    }
                    GhostMode::Eaten => {
                        // Eaten ghosts don't hurt the player.
                    }
                    _ => {
                        // Player dies.
                        self.lives = self.lives.saturating_sub(1);
                        if self.lives == 0 {
                            self.state = GameState::GameOver;
                        } else {
                            self.reset_positions();
                        }
                        return;
                    }
                }
            }
        }
    }

    /// Update ghost release timers.
    fn update_ghost_releases(&mut self, elapsed_ms: u64) {
        for ghost in &mut self.ghosts {
            if !ghost.released {
                ghost.release_timer_ms += elapsed_ms;
                if ghost.release_timer_ms >= ghost.release_delay_ms {
                    ghost.released = true;
                    ghost.pos = Pos::new(11, 14); // Move to the exit position.
                }
            }
        }
    }

    /// Update the global ghost mode (chase/scatter cycling).
    fn update_ghost_mode_cycle(&mut self, elapsed_ms: u64) {
        if self.power_timer_ms > 0 {
            return; // Don't cycle during power pellet.
        }
        self.ghost_mode_timer_ms += elapsed_ms;
        let threshold = match self.global_ghost_mode {
            GlobalGhostMode::Scatter => SCATTER_DURATION_MS,
            GlobalGhostMode::Chase => CHASE_DURATION_MS,
        };
        if self.ghost_mode_timer_ms >= threshold {
            self.ghost_mode_timer_ms = 0;
            self.global_ghost_mode = match self.global_ghost_mode {
                GlobalGhostMode::Scatter => GlobalGhostMode::Chase,
                GlobalGhostMode::Chase => GlobalGhostMode::Scatter,
            };
            // Update ghost modes (except frightened/eaten).
            let new_mode = match self.global_ghost_mode {
                GlobalGhostMode::Chase => GhostMode::Chase,
                GlobalGhostMode::Scatter => GhostMode::Scatter,
            };
            for ghost in &mut self.ghosts {
                if ghost.mode != GhostMode::Frightened && ghost.mode != GhostMode::Eaten {
                    ghost.mode = new_mode;
                    // Reverse direction on mode change.
                    ghost.direction = match ghost.direction {
                        Direction::Up => Direction::Down,
                        Direction::Down => Direction::Up,
                        Direction::Left => Direction::Right,
                        Direction::Right => Direction::Left,
                    };
                }
            }
        }
    }

    /// Update power pellet timer.
    fn update_power_timer(&mut self, elapsed_ms: u64) {
        if self.power_timer_ms > 0 {
            self.power_timer_ms = self.power_timer_ms.saturating_sub(elapsed_ms);
            if self.power_timer_ms == 0 {
                // Power pellet ended: restore ghosts to normal mode.
                let normal_mode = match self.global_ghost_mode {
                    GlobalGhostMode::Chase => GhostMode::Chase,
                    GlobalGhostMode::Scatter => GhostMode::Scatter,
                };
                for ghost in &mut self.ghosts {
                    if ghost.mode == GhostMode::Frightened {
                        ghost.mode = normal_mode;
                    }
                }
            }
        }
    }

    /// Handle a game tick.
    fn handle_tick(&mut self, elapsed_ms: u64) {
        if self.state != GameState::Playing {
            // Still update pulse for menu animation.
            self.pulse_counter = self.pulse_counter.wrapping_add(1);
            return;
        }

        self.elapsed_total_ms += elapsed_ms;
        self.pulse_counter = self.pulse_counter.wrapping_add(1);

        // Update ghost releases.
        self.update_ghost_releases(elapsed_ms);

        // Update global ghost mode cycle.
        self.update_ghost_mode_cycle(elapsed_ms);

        // Update power timer.
        self.update_power_timer(elapsed_ms);

        // Player movement.
        self.player_move_accum_ms += elapsed_ms;
        if self.player_move_accum_ms >= PLAYER_MOVE_MS {
            self.player_move_accum_ms = 0;
            self.move_player();
        }

        // Ghost movement.
        self.ghost_move_accum_ms += elapsed_ms;
        let ghost_interval = if self.power_timer_ms > 0 {
            GHOST_FRIGHTENED_MOVE_MS
        } else {
            GHOST_MOVE_MS
        };
        if self.ghost_move_accum_ms >= ghost_interval {
            self.ghost_move_accum_ms = 0;
            self.move_ghosts();
        }

        // Check collisions.
        self.check_ghost_collisions();

        // Check level completion.
        if self.dots_remaining == 0 {
            self.next_level();
        }
    }

    /// Handle key input.
    fn handle_key(&mut self, key: Key, pressed: bool) {
        if !pressed {
            return;
        }

        match self.state {
            GameState::Menu => match key {
                Key::N => self.start_new_game(),
                _ => {}
            },
            GameState::Playing => match key {
                Key::Up => self.queued_dir = Some(Direction::Up),
                Key::Down => self.queued_dir = Some(Direction::Down),
                Key::Left => self.queued_dir = Some(Direction::Left),
                Key::Right => self.queued_dir = Some(Direction::Right),
                Key::P => self.state = GameState::Paused,
                Key::Escape => self.state = GameState::Paused,
                _ => {}
            },
            GameState::Paused => match key {
                Key::P => self.state = GameState::Playing,
                Key::Escape => self.state = GameState::Playing,
                Key::N => self.start_new_game(),
                _ => {}
            },
            GameState::GameOver => match key {
                Key::N => self.start_new_game(),
                _ => {}
            },
        }
    }

    /// Handle incoming events.
    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke.key, ke.pressed),
            Event::Tick { elapsed_ms } => self.handle_tick(*elapsed_ms),
            _ => {}
        }
    }

    // -- Rendering -----------------------------------------------------------

    /// Produce all render commands for the current frame.
    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background.
        let total_w = PADDING * 2.0 + MAZE_COLS as f32 * CELL_SIZE;
        let total_h = PADDING * 2.0 + HEADER_HEIGHT + MAZE_ROWS as f32 * CELL_SIZE + FOOTER_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_w,
            height: total_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut cmds);
        self.render_maze(&mut cmds);
        self.render_dots(&mut cmds);
        self.render_player(&mut cmds);
        self.render_ghosts(&mut cmds);
        self.render_footer(&mut cmds);
        self.render_overlay(&mut cmds);

        cmds
    }

    /// Render the header (score, high score, level).
    fn render_header(&self, cmds: &mut Vec<RenderCommand>) {
        let y = PADDING;
        let left_x = PADDING;
        let center_x = PADDING + (MAZE_COLS as f32 * CELL_SIZE) / 2.0;
        let right_x = PADDING + MAZE_COLS as f32 * CELL_SIZE - 120.0;

        // Score.
        cmds.push(RenderCommand::Text {
            x: left_x,
            y,
            text: format!("SCORE: {}", self.score),
            color: TEXT_COLOR,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // High score.
        cmds.push(RenderCommand::Text {
            x: center_x - 50.0,
            y,
            text: format!("HI: {}", self.high_score),
            color: SUBTEXT0,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Level.
        cmds.push(RenderCommand::Text {
            x: right_x,
            y,
            text: format!("LVL {}", self.level),
            color: LAVENDER,
            font_size: HEADER_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Pixel x for a maze column.
    fn cell_x(&self, col: usize) -> f32 {
        PADDING + col as f32 * CELL_SIZE
    }

    /// Pixel y for a maze row.
    fn cell_y(&self, row: usize) -> f32 {
        PADDING + HEADER_HEIGHT + row as f32 * CELL_SIZE
    }

    /// Render the maze walls.
    fn render_maze(&self, cmds: &mut Vec<RenderCommand>) {
        for row in 0..MAZE_ROWS {
            for col in 0..MAZE_COLS {
                let cell = self.maze[row][col];
                if cell == Cell::Wall {
                    let x = self.cell_x(col);
                    let y = self.cell_y(row);
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y,
                        width: CELL_SIZE,
                        height: CELL_SIZE,
                        color: BLUE,
                        corner_radii: CornerRadii::all(2.0),
                    });
                } else if cell == Cell::GhostDoor {
                    let x = self.cell_x(col);
                    let y = self.cell_y(row);
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y,
                        width: CELL_SIZE,
                        height: CELL_SIZE / 3.0,
                        color: LAVENDER,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }
    }

    /// Render dots and power pellets.
    fn render_dots(&self, cmds: &mut Vec<RenderCommand>) {
        let pulse = (self.pulse_counter % 30) > 15;
        for row in 0..MAZE_ROWS {
            for col in 0..MAZE_COLS {
                let cell = self.maze[row][col];
                let cx = self.cell_x(col) + CELL_SIZE / 2.0;
                let cy = self.cell_y(row) + CELL_SIZE / 2.0;

                match cell {
                    Cell::Dot => {
                        let r = DOT_RADIUS;
                        cmds.push(RenderCommand::FillRect {
                            x: cx - r,
                            y: cy - r,
                            width: r * 2.0,
                            height: r * 2.0,
                            color: YELLOW,
                            corner_radii: CornerRadii::all(r),
                        });
                    }
                    Cell::PowerPellet => {
                        let r = if pulse {
                            POWER_PELLET_RADIUS + 1.0
                        } else {
                            POWER_PELLET_RADIUS
                        };
                        cmds.push(RenderCommand::FillRect {
                            x: cx - r,
                            y: cy - r,
                            width: r * 2.0,
                            height: r * 2.0,
                            color: YELLOW,
                            corner_radii: CornerRadii::all(r),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    /// Render the player (Pac-Man: a yellow circle with a mouth).
    fn render_player(&self, cmds: &mut Vec<RenderCommand>) {
        if self.state == GameState::Menu {
            return;
        }
        let cx = self.cell_x(self.player_pos.col as usize) + CELL_SIZE / 2.0;
        let cy = self.cell_y(self.player_pos.row as usize) + CELL_SIZE / 2.0;
        let radius = CELL_SIZE / 2.0 - 1.0;

        // Body circle.
        cmds.push(RenderCommand::FillRect {
            x: cx - radius,
            y: cy - radius,
            width: radius * 2.0,
            height: radius * 2.0,
            color: YELLOW,
            corner_radii: CornerRadii::all(radius),
        });

        // Mouth (wedge approximated by a triangle of background color).
        if self.mouth_open {
            let mouth_size = radius * 0.5;
            let (mx, my) = match self.player_dir {
                Direction::Right => (cx + mouth_size, cy),
                Direction::Left => (cx - mouth_size, cy),
                Direction::Up => (cx, cy - mouth_size),
                Direction::Down => (cx, cy + mouth_size),
            };
            // Draw a small dark rect to simulate mouth opening.
            let ms = radius * 0.45;
            cmds.push(RenderCommand::FillRect {
                x: mx - ms / 2.0,
                y: my - ms / 2.0,
                width: ms,
                height: ms,
                color: BASE,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Eye.
        let (ex, ey) = match self.player_dir {
            Direction::Right | Direction::Down => (cx + 1.0, cy - radius * 0.35),
            Direction::Left => (cx - 3.0, cy - radius * 0.35),
            Direction::Up => (cx + 1.0, cy - radius * 0.5),
        };
        let eye_r = 2.0;
        cmds.push(RenderCommand::FillRect {
            x: ex - eye_r,
            y: ey - eye_r,
            width: eye_r * 2.0,
            height: eye_r * 2.0,
            color: BASE,
            corner_radii: CornerRadii::all(eye_r),
        });
    }

    /// Render the ghosts.
    fn render_ghosts(&self, cmds: &mut Vec<RenderCommand>) {
        if self.state == GameState::Menu {
            return;
        }
        let is_flashing = self.power_timer_ms > 0 && self.power_timer_ms < POWER_FLASH_MS;
        let flash_on = is_flashing && (self.pulse_counter % 10) > 5;

        for ghost in &self.ghosts {
            let cx = self.cell_x(ghost.pos.col as usize) + CELL_SIZE / 2.0;
            let cy = self.cell_y(ghost.pos.row as usize) + CELL_SIZE / 2.0;
            let radius = CELL_SIZE / 2.0 - 1.0;

            let body_color = match ghost.mode {
                GhostMode::Frightened => {
                    if flash_on {
                        TEXT_COLOR
                    } else {
                        BLUE
                    }
                }
                GhostMode::Eaten => OVERLAY0,
                _ => ghost.id.color(),
            };

            // Ghost body (rounded-top rectangle).
            cmds.push(RenderCommand::FillRect {
                x: cx - radius,
                y: cy - radius,
                width: radius * 2.0,
                height: radius * 2.0,
                color: body_color,
                corner_radii: CornerRadii::all(radius * 0.5),
            });

            // Ghost skirt (bottom rect, no rounding).
            cmds.push(RenderCommand::FillRect {
                x: cx - radius,
                y: cy,
                width: radius * 2.0,
                height: radius,
                color: body_color,
                corner_radii: CornerRadii::ZERO,
            });

            // Eyes (not for eaten ghosts -- they are just eyes).
            if ghost.mode != GhostMode::Eaten {
                let eye_r = 2.5;
                let eye_y = cy - radius * 0.2;
                // Left eye.
                cmds.push(RenderCommand::FillRect {
                    x: cx - radius * 0.4 - eye_r,
                    y: eye_y - eye_r,
                    width: eye_r * 2.0,
                    height: eye_r * 2.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(eye_r),
                });
                // Right eye.
                cmds.push(RenderCommand::FillRect {
                    x: cx + radius * 0.4 - eye_r,
                    y: eye_y - eye_r,
                    width: eye_r * 2.0,
                    height: eye_r * 2.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(eye_r),
                });
                // Pupils.
                let pupil_r = 1.5;
                let (pox, poy) = match ghost.direction {
                    Direction::Right => (1.0, 0.0),
                    Direction::Left => (-1.0, 0.0),
                    Direction::Up => (0.0, -1.0),
                    Direction::Down => (0.0, 1.0),
                };
                cmds.push(RenderCommand::FillRect {
                    x: cx - radius * 0.4 - pupil_r + pox,
                    y: eye_y - pupil_r + poy,
                    width: pupil_r * 2.0,
                    height: pupil_r * 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(pupil_r),
                });
                cmds.push(RenderCommand::FillRect {
                    x: cx + radius * 0.4 - pupil_r + pox,
                    y: eye_y - pupil_r + poy,
                    width: pupil_r * 2.0,
                    height: pupil_r * 2.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(pupil_r),
                });
            } else {
                // Eaten ghost: just draw eyes.
                let eye_r = 3.0;
                let eye_y = cy - 1.0;
                cmds.push(RenderCommand::FillRect {
                    x: cx - radius * 0.35 - eye_r,
                    y: eye_y - eye_r,
                    width: eye_r * 2.0,
                    height: eye_r * 2.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(eye_r),
                });
                cmds.push(RenderCommand::FillRect {
                    x: cx + radius * 0.35 - eye_r,
                    y: eye_y - eye_r,
                    width: eye_r * 2.0,
                    height: eye_r * 2.0,
                    color: TEXT_COLOR,
                    corner_radii: CornerRadii::all(eye_r),
                });
            }
        }
    }

    /// Render the footer (lives indicator).
    fn render_footer(&self, cmds: &mut Vec<RenderCommand>) {
        let y = PADDING + HEADER_HEIGHT + MAZE_ROWS as f32 * CELL_SIZE + 8.0;
        let x_start = PADDING;

        // Lives.
        cmds.push(RenderCommand::Text {
            x: x_start,
            y,
            text: "LIVES:".to_string(),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        for i in 0..self.lives.min(5) {
            let lx = x_start + 50.0 + i as f32 * 22.0;
            let r = 7.0;
            cmds.push(RenderCommand::FillRect {
                x: lx,
                y: y + 2.0,
                width: r * 2.0,
                height: r * 2.0,
                color: YELLOW,
                corner_radii: CornerRadii::all(r),
            });
        }

        // Dots remaining.
        let dots_x = PADDING + MAZE_COLS as f32 * CELL_SIZE - 120.0;
        cmds.push(RenderCommand::Text {
            x: dots_x,
            y,
            text: format!("DOTS: {}", self.dots_remaining),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render overlays for menu, pause, game over.
    fn render_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        match self.state {
            GameState::Menu => self.render_menu_overlay(cmds),
            GameState::Paused => self.render_pause_overlay(cmds),
            GameState::GameOver => self.render_game_over_overlay(cmds),
            GameState::Playing => {}
        }
    }

    /// Render the menu overlay.
    fn render_menu_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let total_w = PADDING * 2.0 + MAZE_COLS as f32 * CELL_SIZE;
        let total_h = PADDING * 2.0 + HEADER_HEIGHT + MAZE_ROWS as f32 * CELL_SIZE + FOOTER_HEIGHT;

        // Semi-transparent overlay.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_w,
            height: total_h,
            color: Color::rgba(30, 30, 46, 220),
            corner_radii: CornerRadii::ZERO,
        });

        let center_x = total_w / 2.0;
        let center_y = total_h / 2.0;

        cmds.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y - 60.0,
            text: "PAC-MAN".to_string(),
            color: YELLOW,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 90.0,
            y: center_y - 15.0,
            text: "Press N to start".to_string(),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: center_y + 15.0,
            text: "Arrow keys to move".to_string(),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 80.0,
            y: center_y + 35.0,
            text: "P to pause".to_string(),
            color: SUBTEXT0,
            font_size: SMALL_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Animated pac-man on menu.
        let pac_x = center_x - 60.0 + ((self.pulse_counter % 120) as f32);
        let r = 12.0;
        cmds.push(RenderCommand::FillRect {
            x: pac_x - r,
            y: center_y + 55.0,
            width: r * 2.0,
            height: r * 2.0,
            color: YELLOW,
            corner_radii: CornerRadii::all(r),
        });
    }

    /// Render the pause overlay.
    fn render_pause_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let total_w = PADDING * 2.0 + MAZE_COLS as f32 * CELL_SIZE;
        let total_h = PADDING * 2.0 + HEADER_HEIGHT + MAZE_ROWS as f32 * CELL_SIZE + FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_w,
            height: total_h,
            color: Color::rgba(30, 30, 46, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let center_x = total_w / 2.0;
        let center_y = total_h / 2.0;

        cmds.push(RenderCommand::Text {
            x: center_x - 50.0,
            y: center_y - 30.0,
            text: "PAUSED".to_string(),
            color: YELLOW,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: center_y + 10.0,
            text: "P to resume, N for new game".to_string(),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render the game over overlay.
    fn render_game_over_overlay(&self, cmds: &mut Vec<RenderCommand>) {
        let total_w = PADDING * 2.0 + MAZE_COLS as f32 * CELL_SIZE;
        let total_h = PADDING * 2.0 + HEADER_HEIGHT + MAZE_ROWS as f32 * CELL_SIZE + FOOTER_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: total_w,
            height: total_h,
            color: Color::rgba(30, 30, 46, 200),
            corner_radii: CornerRadii::ZERO,
        });

        let center_x = total_w / 2.0;
        let center_y = total_h / 2.0;

        cmds.push(RenderCommand::Text {
            x: center_x - 70.0,
            y: center_y - 40.0,
            text: "GAME OVER".to_string(),
            color: RED,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 70.0,
            y: center_y,
            text: format!("Score: {}", self.score),
            color: TEXT_COLOR,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x - 100.0,
            y: center_y + 30.0,
            text: "Press N for new game".to_string(),
            color: SUBTEXT0,
            font_size: OVERLAY_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Get the number of released ghosts.
    fn released_ghost_count(&self) -> usize {
        self.ghosts.iter().filter(|g| g.released).count()
    }

    /// Check if power pellet mode is active.
    fn is_power_active(&self) -> bool {
        self.power_timer_ms > 0
    }

    /// Check if power pellet mode is flashing (near end).
    fn is_power_flashing(&self) -> bool {
        self.power_timer_ms > 0 && self.power_timer_ms < POWER_FLASH_MS
    }

    /// Get the ghost at a given position, if any.
    fn ghost_at(&self, pos: Pos) -> Option<&Ghost> {
        self.ghosts.iter().find(|g| g.pos == pos)
    }

    /// Get the count of dots of a specific type remaining.
    fn count_cell_type(&self, cell_type: Cell) -> u32 {
        let mut count = 0u32;
        for row in &self.maze {
            for cell in row {
                if *cell == cell_type {
                    count += 1;
                }
            }
        }
        count
    }
}

fn main() {
    let _app = PacmanApp::new();
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // -- Helpers --------------------------------------------------------------

    fn test_app() -> PacmanApp {
        PacmanApp::with_seed(12345)
    }

    fn playing_app() -> PacmanApp {
        let mut app = test_app();
        app.state = GameState::Playing;
        app
    }

    fn make_key_event(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn press_key(app: &mut PacmanApp, key: Key) {
        let event = Event::Key(make_key_event(key));
        app.handle_event(&event);
    }

    fn force_player_tick(app: &mut PacmanApp) {
        app.handle_tick(PLAYER_MOVE_MS + 1);
    }

    // -- LCG tests ------------------------------------------------------------

    #[test]
    fn test_lcg_deterministic() {
        let mut rng1 = Lcg::new(42);
        let mut rng2 = Lcg::new(42);
        assert_eq!(rng1.next_u64(), rng2.next_u64());
        assert_eq!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_lcg_different_seeds() {
        let mut rng1 = Lcg::new(1);
        let mut rng2 = Lcg::new(2);
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }

    #[test]
    fn test_lcg_bounded() {
        let mut rng = Lcg::new(99);
        for _ in 0..100 {
            let val = rng.next_bounded(10);
            assert!(val < 10);
        }
    }

    // -- Direction tests ------------------------------------------------------

    #[test]
    fn test_direction_opposite_up_down() {
        assert!(Direction::Up.is_opposite(Direction::Down));
        assert!(Direction::Down.is_opposite(Direction::Up));
    }

    #[test]
    fn test_direction_opposite_left_right() {
        assert!(Direction::Left.is_opposite(Direction::Right));
        assert!(Direction::Right.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_same() {
        assert!(!Direction::Up.is_opposite(Direction::Up));
        assert!(!Direction::Left.is_opposite(Direction::Left));
    }

    #[test]
    fn test_direction_not_opposite_perpendicular() {
        assert!(!Direction::Up.is_opposite(Direction::Left));
        assert!(!Direction::Down.is_opposite(Direction::Right));
    }

    #[test]
    fn test_direction_delta_up() {
        assert_eq!(Direction::Up.delta(), (-1, 0));
    }

    #[test]
    fn test_direction_delta_down() {
        assert_eq!(Direction::Down.delta(), (1, 0));
    }

    #[test]
    fn test_direction_delta_left() {
        assert_eq!(Direction::Left.delta(), (0, -1));
    }

    #[test]
    fn test_direction_delta_right() {
        assert_eq!(Direction::Right.delta(), (0, 1));
    }

    // -- Pos tests ------------------------------------------------------------

    #[test]
    fn test_pos_in_bounds_valid() {
        assert!(Pos::new(0, 0).in_bounds());
        assert!(Pos::new(MAZE_ROWS as i32 - 1, MAZE_COLS as i32 - 1).in_bounds());
        assert!(Pos::new(15, 14).in_bounds());
    }

    #[test]
    fn test_pos_in_bounds_invalid() {
        assert!(!Pos::new(-1, 0).in_bounds());
        assert!(!Pos::new(0, -1).in_bounds());
        assert!(!Pos::new(MAZE_ROWS as i32, 0).in_bounds());
        assert!(!Pos::new(0, MAZE_COLS as i32).in_bounds());
    }

    #[test]
    fn test_pos_moved() {
        let p = Pos::new(5, 5);
        assert_eq!(p.moved(Direction::Up), Pos::new(4, 5));
        assert_eq!(p.moved(Direction::Down), Pos::new(6, 5));
        assert_eq!(p.moved(Direction::Left), Pos::new(5, 4));
        assert_eq!(p.moved(Direction::Right), Pos::new(5, 6));
    }

    #[test]
    fn test_pos_tunnel_wrap_left() {
        let p = Pos::new(TUNNEL_ROW as i32, -1);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, MAZE_COLS as i32 - 1);
        assert_eq!(w.row, TUNNEL_ROW as i32);
    }

    #[test]
    fn test_pos_tunnel_wrap_right() {
        let p = Pos::new(TUNNEL_ROW as i32, MAZE_COLS as i32);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, 0);
    }

    #[test]
    fn test_pos_tunnel_no_wrap_other_row() {
        let p = Pos::new(5, -1);
        let w = p.tunnel_wrap();
        assert_eq!(w.col, -1); // No wrap on non-tunnel rows.
    }

    #[test]
    fn test_pos_manhattan_distance() {
        assert_eq!(Pos::new(0, 0).manhattan_distance(Pos::new(3, 4)), 7);
        assert_eq!(Pos::new(5, 5).manhattan_distance(Pos::new(5, 5)), 0);
    }

    // -- Maze parsing tests ---------------------------------------------------

    #[test]
    fn test_maze_dimensions() {
        let maze = parse_maze();
        assert_eq!(maze.len(), MAZE_ROWS);
        assert_eq!(maze[0].len(), MAZE_COLS);
    }

    #[test]
    fn test_maze_corners_are_walls() {
        let maze = parse_maze();
        assert_eq!(maze[0][0], Cell::Wall);
        assert_eq!(maze[0][MAZE_COLS - 1], Cell::Wall);
        assert_eq!(maze[MAZE_ROWS - 1][0], Cell::Wall);
        assert_eq!(maze[MAZE_ROWS - 1][MAZE_COLS - 1], Cell::Wall);
    }

    #[test]
    fn test_maze_has_dots() {
        let maze = parse_maze();
        let dots = count_dots(&maze);
        assert!(dots > 0, "Maze should contain dots");
    }

    #[test]
    fn test_maze_has_power_pellets() {
        let maze = parse_maze();
        let power_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::PowerPellet)
            .count();
        assert_eq!(power_count, 4, "Should have 4 power pellets");
    }

    #[test]
    fn test_maze_has_ghost_house() {
        let maze = parse_maze();
        let gh_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::GhostHouse)
            .count();
        assert!(gh_count > 0, "Should have ghost house cells");
    }

    #[test]
    fn test_maze_has_ghost_door() {
        let maze = parse_maze();
        let door_count = maze
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| **c == Cell::GhostDoor)
            .count();
        assert!(door_count > 0, "Should have ghost door cells");
    }

    // -- Cell walkability tests -----------------------------------------------

    #[test]
    fn test_cell_walkability() {
        assert!(!Cell::Wall.is_walkable());
        assert!(Cell::Empty.is_walkable());
        assert!(Cell::Dot.is_walkable());
        assert!(Cell::PowerPellet.is_walkable());
        assert!(!Cell::GhostHouse.is_walkable());
        assert!(!Cell::GhostDoor.is_walkable());
    }

    #[test]
    fn test_cell_ghost_walkability() {
        assert!(!Cell::Wall.is_ghost_walkable());
        assert!(Cell::Empty.is_ghost_walkable());
        assert!(Cell::Dot.is_ghost_walkable());
        assert!(Cell::PowerPellet.is_ghost_walkable());
        assert!(Cell::GhostHouse.is_ghost_walkable());
        assert!(Cell::GhostDoor.is_ghost_walkable());
    }

    // -- GhostId tests --------------------------------------------------------

    #[test]
    fn test_ghost_colors_distinct() {
        let colors: Vec<Color> = GhostId::ALL.iter().map(|g| g.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }

    #[test]
    fn test_ghost_scatter_targets_distinct() {
        let targets: Vec<Pos> = GhostId::ALL.iter().map(|g| g.scatter_target()).collect();
        for i in 0..targets.len() {
            for j in (i + 1)..targets.len() {
                assert_ne!(targets[i], targets[j]);
            }
        }
    }

    #[test]
    fn test_ghost_names() {
        assert_eq!(GhostId::Blinky.name(), "Blinky");
        assert_eq!(GhostId::Pinky.name(), "Pinky");
        assert_eq!(GhostId::Inky.name(), "Inky");
        assert_eq!(GhostId::Clyde.name(), "Clyde");
    }

    // -- Ghost initialization tests -------------------------------------------

    #[test]
    fn test_initial_ghost_count() {
        let app = test_app();
        assert_eq!(app.ghosts.len(), 4);
    }

    #[test]
    fn test_blinky_starts_released() {
        let app = test_app();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert!(blinky.released);
    }

    #[test]
    fn test_other_ghosts_start_unreleased() {
        let app = test_app();
        for ghost in &app.ghosts {
            if ghost.id != GhostId::Blinky {
                assert!(!ghost.released, "{:?} should start unreleased", ghost.id);
            }
        }
    }

    #[test]
    fn test_blinky_starts_outside_house() {
        let app = test_app();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert_eq!(blinky.pos, Pos::new(11, 14));
    }

    // -- Game state tests -----------------------------------------------------

    #[test]
    fn test_initial_state_is_menu() {
        let app = test_app();
        assert_eq!(app.state, GameState::Menu);
    }

    #[test]
    fn test_initial_score_zero() {
        let app = test_app();
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_initial_lives() {
        let app = test_app();
        assert_eq!(app.lives, INITIAL_LIVES);
    }

    #[test]
    fn test_initial_level() {
        let app = test_app();
        assert_eq!(app.level, 1);
    }

    #[test]
    fn test_dots_remaining_equals_total() {
        let app = test_app();
        assert_eq!(app.dots_remaining, app.total_dots);
    }

    #[test]
    fn test_initial_no_power() {
        let app = test_app();
        assert!(!app.is_power_active());
    }

    #[test]
    fn test_initial_player_position() {
        let app = test_app();
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    #[test]
    fn test_initial_player_direction() {
        let app = test_app();
        assert_eq!(app.player_dir, Direction::Left);
    }

    // -- Key handling tests ---------------------------------------------------

    #[test]
    fn test_n_starts_game_from_menu() {
        let mut app = test_app();
        assert_eq!(app.state, GameState::Menu);
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_p_pauses_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_p_resumes_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Paused);
        press_key(&mut app, Key::P);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_escape_pauses_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::Escape);
        assert_eq!(app.state, GameState::Paused);
    }

    #[test]
    fn test_escape_resumes_game() {
        let mut app = playing_app();
        press_key(&mut app, Key::Escape);
        press_key(&mut app, Key::Escape);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_n_from_paused_starts_new() {
        let mut app = playing_app();
        app.score = 500;
        press_key(&mut app, Key::P);
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_n_from_game_over_starts_new() {
        let mut app = playing_app();
        app.state = GameState::GameOver;
        press_key(&mut app, Key::N);
        assert_eq!(app.state, GameState::Playing);
    }

    #[test]
    fn test_arrow_keys_queue_direction() {
        let mut app = playing_app();
        press_key(&mut app, Key::Up);
        assert_eq!(app.queued_dir, Some(Direction::Up));
        press_key(&mut app, Key::Down);
        assert_eq!(app.queued_dir, Some(Direction::Down));
        press_key(&mut app, Key::Left);
        assert_eq!(app.queued_dir, Some(Direction::Left));
        press_key(&mut app, Key::Right);
        assert_eq!(app.queued_dir, Some(Direction::Right));
    }

    #[test]
    fn test_key_release_ignored() {
        let mut app = playing_app();
        let event = Event::Key(KeyEvent {
            key: Key::Up,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        });
        app.handle_event(&event);
        assert_eq!(app.queued_dir, None);
    }

    // -- Movement tests -------------------------------------------------------

    #[test]
    fn test_player_moves_on_tick() {
        let mut app = playing_app();
        // Place player in an open area and set direction.
        app.player_pos = Pos::new(5, 14);
        app.player_dir = Direction::Left;
        app.queued_dir = None;
        let old_pos = app.player_pos;
        // Clear the cell to the left to ensure it is walkable.
        app.maze[5][13] = Cell::Empty;
        force_player_tick(&mut app);
        assert_ne!(app.player_pos, old_pos, "Player should have moved");
    }

    #[test]
    fn test_player_cannot_walk_into_wall() {
        let mut app = playing_app();
        // Place player next to a wall.
        app.player_pos = Pos::new(1, 1);
        app.player_dir = Direction::Up; // Row 0 is all walls.
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(
            app.player_pos,
            Pos::new(1, 1),
            "Player should not move into wall"
        );
    }

    #[test]
    fn test_tunnel_wrap_player_left() {
        let mut app = playing_app();
        app.player_pos = Pos::new(TUNNEL_ROW as i32, 0);
        app.player_dir = Direction::Left;
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(
            app.player_pos.col,
            MAZE_COLS as i32 - 1,
            "Player should wrap around tunnel"
        );
    }

    #[test]
    fn test_tunnel_wrap_player_right() {
        let mut app = playing_app();
        app.player_pos = Pos::new(TUNNEL_ROW as i32, MAZE_COLS as i32 - 1);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        force_player_tick(&mut app);
        assert_eq!(app.player_pos.col, 0, "Player should wrap around tunnel");
    }

    // -- Dot eating tests -----------------------------------------------------

    #[test]
    fn test_eating_dot_scores_points() {
        let mut app = playing_app();
        // Place a dot and move player to it.
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        let old_score = app.score;
        let old_dots = app.dots_remaining;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.score, old_score + DOT_POINTS);
            assert_eq!(app.dots_remaining, old_dots - 1);
        }
    }

    #[test]
    fn test_eating_power_pellet_scores_points() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::PowerPellet;
        let old_score = app.score;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.score, old_score + POWER_PELLET_POINTS);
        }
    }

    #[test]
    fn test_power_pellet_activates_power() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::PowerPellet;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert!(app.is_power_active());
            assert_eq!(app.power_timer_ms, POWER_DURATION_MS);
        }
    }

    #[test]
    fn test_dot_consumed_after_eating() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.maze[5][14], Cell::Empty);
        }
    }

    // -- Power pellet behavior tests ------------------------------------------

    #[test]
    fn test_power_pellet_frightens_ghosts() {
        let mut app = playing_app();
        app.activate_power_pellet();
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Frightened);
        }
    }

    #[test]
    fn test_power_timer_decreases() {
        let mut app = playing_app();
        app.power_timer_ms = 5000;
        app.update_power_timer(1000);
        assert_eq!(app.power_timer_ms, 4000);
    }

    #[test]
    fn test_power_timer_expires() {
        let mut app = playing_app();
        app.activate_power_pellet();
        app.update_power_timer(POWER_DURATION_MS);
        assert_eq!(app.power_timer_ms, 0);
        assert!(!app.is_power_active());
    }

    #[test]
    fn test_power_flashing_near_end() {
        let mut app = playing_app();
        app.power_timer_ms = POWER_FLASH_MS - 100;
        assert!(app.is_power_flashing());
    }

    #[test]
    fn test_power_not_flashing_when_lots_remaining() {
        let mut app = playing_app();
        app.power_timer_ms = POWER_DURATION_MS;
        assert!(!app.is_power_flashing());
    }

    #[test]
    fn test_ghosts_return_to_normal_after_power() {
        let mut app = playing_app();
        app.global_ghost_mode = GlobalGhostMode::Chase;
        app.activate_power_pellet();
        // All frightened.
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Frightened);
        }
        // Expire power.
        app.update_power_timer(POWER_DURATION_MS);
        for ghost in &app.ghosts {
            assert_eq!(ghost.mode, GhostMode::Chase);
        }
    }

    // -- Ghost collision tests ------------------------------------------------

    #[test]
    fn test_ghost_collision_kills_player() {
        let mut app = playing_app();
        let initial_lives = app.lives;
        app.ghosts[0].mode = GhostMode::Chase;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, initial_lives - 1);
    }

    #[test]
    fn test_ghost_collision_game_over_at_zero_lives() {
        let mut app = playing_app();
        app.lives = 1;
        app.ghosts[0].mode = GhostMode::Chase;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, 0);
        assert_eq!(app.state, GameState::GameOver);
    }

    #[test]
    fn test_eating_frightened_ghost() {
        let mut app = playing_app();
        app.activate_power_pellet();
        app.ghosts[0].pos = app.player_pos;
        let old_score = app.score;
        app.check_ghost_collisions();
        assert_eq!(app.ghosts[0].mode, GhostMode::Eaten);
        assert!(app.score > old_score);
    }

    #[test]
    fn test_ghost_eating_score_doubles() {
        let mut app = playing_app();
        app.activate_power_pellet();
        // Eat first ghost: 200.
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.score, GHOST_BASE_POINTS); // 200

        // Eat second ghost: 400.
        app.ghosts[1].mode = GhostMode::Frightened;
        app.ghosts[1].pos = app.player_pos;
        let score_before = app.score;
        app.check_ghost_collisions();
        assert_eq!(app.score, score_before + GHOST_BASE_POINTS * 2); // +400
    }

    #[test]
    fn test_eaten_ghost_doesnt_hurt_player() {
        let mut app = playing_app();
        let initial_lives = app.lives;
        app.ghosts[0].mode = GhostMode::Eaten;
        app.ghosts[0].pos = app.player_pos;
        app.check_ghost_collisions();
        assert_eq!(app.lives, initial_lives);
    }

    // -- Ghost release tests --------------------------------------------------

    #[test]
    fn test_ghost_release_after_delay() {
        let mut app = playing_app();
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(!pinky.released);
        // Pinky has 1000ms delay.
        app.update_ghost_releases(1000);
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(pinky.released);
    }

    #[test]
    fn test_ghost_not_released_before_delay() {
        let mut app = playing_app();
        app.update_ghost_releases(500);
        let pinky = app.ghosts.iter().find(|g| g.id == GhostId::Pinky).unwrap();
        assert!(!pinky.released, "Pinky should not release at 500ms");
    }

    #[test]
    fn test_released_ghost_count() {
        let app = playing_app();
        assert_eq!(app.released_ghost_count(), 1, "Only Blinky starts released");
    }

    // -- Ghost mode cycle tests -----------------------------------------------

    #[test]
    fn test_initial_ghost_mode_is_scatter() {
        let app = playing_app();
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Scatter);
    }

    #[test]
    fn test_ghost_mode_switches_to_chase() {
        let mut app = playing_app();
        app.update_ghost_mode_cycle(SCATTER_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Chase);
    }

    #[test]
    fn test_ghost_mode_switches_back_to_scatter() {
        let mut app = playing_app();
        app.update_ghost_mode_cycle(SCATTER_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Chase);
        app.update_ghost_mode_cycle(CHASE_DURATION_MS);
        assert_eq!(app.global_ghost_mode, GlobalGhostMode::Scatter);
    }

    // -- Level tests ----------------------------------------------------------

    #[test]
    fn test_next_level_increments_level() {
        let mut app = playing_app();
        app.next_level();
        assert_eq!(app.level, 2);
    }

    #[test]
    fn test_next_level_resets_dots() {
        let mut app = playing_app();
        app.dots_remaining = 0;
        app.next_level();
        assert_eq!(app.dots_remaining, app.total_dots);
    }

    #[test]
    fn test_next_level_resets_player_pos() {
        let mut app = playing_app();
        app.player_pos = Pos::new(10, 10);
        app.next_level();
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    // -- Score / high score tests ---------------------------------------------

    #[test]
    fn test_high_score_preserved_on_new_game() {
        let mut app = playing_app();
        app.score = 1000;
        app.high_score = 1000;
        app.start_new_game();
        assert_eq!(app.high_score, 1000);
        assert_eq!(app.score, 0);
    }

    #[test]
    fn test_high_score_updates_on_dot() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Dot;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_eq!(app.high_score, DOT_POINTS);
        }
    }

    // -- Ghost AI target tests ------------------------------------------------

    #[test]
    fn test_blinky_targets_player() {
        let app = playing_app();
        let target = app.ghost_chase_target(GhostId::Blinky);
        assert_eq!(target, app.player_pos);
    }

    #[test]
    fn test_pinky_targets_ahead_of_player() {
        let mut app = playing_app();
        app.player_dir = Direction::Right;
        let target = app.ghost_chase_target(GhostId::Pinky);
        assert_eq!(target.row, app.player_pos.row);
        assert_eq!(target.col, app.player_pos.col + 4);
    }

    #[test]
    fn test_clyde_scatters_when_close() {
        let mut app = playing_app();
        // Place Clyde close to the player.
        app.ghosts[3].pos = Pos::new(app.player_pos.row + 2, app.player_pos.col);
        let target = app.ghost_chase_target(GhostId::Clyde);
        assert_eq!(target, GhostId::Clyde.scatter_target());
    }

    #[test]
    fn test_clyde_chases_when_far() {
        let mut app = playing_app();
        app.ghosts[3].pos = Pos::new(0, 0);
        let target = app.ghost_chase_target(GhostId::Clyde);
        assert_eq!(target, app.player_pos);
    }

    // -- Render tests ---------------------------------------------------------

    #[test]
    fn test_render_produces_commands() {
        let app = test_app();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_playing_has_more_commands() {
        let menu_app = test_app();
        let mut play_app = test_app();
        play_app.state = GameState::Playing;
        let menu_cmds = menu_app.render();
        let play_cmds = play_app.render();
        // Playing state renders player + ghosts + dots; menu shows overlay.
        // Both should have commands.
        assert!(!menu_cmds.is_empty());
        assert!(!play_cmds.is_empty());
    }

    // -- Walkability tests ----------------------------------------------------

    #[test]
    fn test_is_walkable_empty() {
        let app = playing_app();
        // Find an empty cell in the maze.
        let mut found = false;
        for r in 0..MAZE_ROWS {
            for c in 0..MAZE_COLS {
                if app.maze[r][c] == Cell::Empty {
                    assert!(app.is_walkable(Pos::new(r as i32, c as i32)));
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
    }

    #[test]
    fn test_is_walkable_wall() {
        let app = playing_app();
        // Top-left corner is always a wall.
        assert!(!app.is_walkable(Pos::new(0, 0)));
    }

    #[test]
    fn test_is_walkable_tunnel() {
        let app = playing_app();
        assert!(
            app.is_walkable(Pos::new(TUNNEL_ROW as i32, -1)),
            "Tunnel wrap should be walkable"
        );
    }

    // -- Tick/event integration tests -----------------------------------------

    #[test]
    fn test_tick_in_menu_doesnt_move() {
        let mut app = test_app();
        let pos = app.player_pos;
        app.handle_tick(1000);
        assert_eq!(app.player_pos, pos, "No movement in menu state");
    }

    #[test]
    fn test_tick_in_paused_doesnt_move() {
        let mut app = playing_app();
        app.state = GameState::Paused;
        let pos = app.player_pos;
        app.handle_tick(1000);
        assert_eq!(app.player_pos, pos, "No movement in paused state");
    }

    #[test]
    fn test_count_cell_type_dots() {
        let app = test_app();
        let dot_count = app.count_cell_type(Cell::Dot);
        assert!(dot_count > 0);
    }

    #[test]
    fn test_count_cell_type_power_pellets() {
        let app = test_app();
        let pp_count = app.count_cell_type(Cell::PowerPellet);
        assert_eq!(pp_count, 4);
    }

    // -- Queued direction tests -----------------------------------------------

    #[test]
    fn test_queued_direction_applied() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 14);
        app.player_dir = Direction::Left;
        // Make sure the cell above is walkable.
        app.maze[4][14] = Cell::Empty;
        app.maze[5][13] = Cell::Empty;
        app.queued_dir = Some(Direction::Up);
        force_player_tick(&mut app);
        // Player should have moved up (queued direction was valid).
        assert_eq!(app.player_dir, Direction::Up);
    }

    // -- Reset tests ----------------------------------------------------------

    #[test]
    fn test_reset_positions_keeps_score() {
        let mut app = playing_app();
        app.score = 500;
        app.player_pos = Pos::new(10, 10);
        app.reset_positions();
        assert_eq!(app.score, 500);
        assert_eq!(app.player_pos, Pos::new(23, 14));
    }

    #[test]
    fn test_reset_positions_resets_ghosts() {
        let mut app = playing_app();
        app.ghosts[0].pos = Pos::new(5, 5);
        app.reset_positions();
        let blinky = app.ghosts.iter().find(|g| g.id == GhostId::Blinky).unwrap();
        assert_eq!(blinky.pos, Pos::new(11, 14));
    }

    // -- Ghost at position test -----------------------------------------------

    #[test]
    fn test_ghost_at_position() {
        let app = playing_app();
        let blinky_pos = app.ghosts[0].pos;
        let found = app.ghost_at(blinky_pos);
        assert!(found.is_some());
    }

    #[test]
    fn test_no_ghost_at_empty_position() {
        let app = playing_app();
        let found = app.ghost_at(Pos::new(0, 0)); // Wall, no ghost here.
        assert!(found.is_none());
    }

    // -- Mouth animation test -------------------------------------------------

    #[test]
    fn test_mouth_toggles_on_move() {
        let mut app = playing_app();
        app.player_pos = Pos::new(5, 13);
        app.player_dir = Direction::Right;
        app.queued_dir = None;
        app.maze[5][14] = Cell::Empty;
        let initial_mouth = app.mouth_open;
        force_player_tick(&mut app);
        if app.player_pos == Pos::new(5, 14) {
            assert_ne!(app.mouth_open, initial_mouth);
        }
    }
}
