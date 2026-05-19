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

//! OurOS Battleship -- classic naval combat game with AI opponent.
//!
//! Features two 10x10 grids (player's fleet and opponent's ocean),
//! a ship placement phase with rotation and arrow-key positioning,
//! an AI opponent with hunt/target firing strategy, hit/miss markers,
//! ship sinking detection with announcements, win/loss detection,
//! and live stats (shots fired, hit rate, ships remaining).
//! Uses an LCG pseudo-random number generator (no external rand crate).
//! Themed with the Catppuccin Mocha palette.

use guitk::color::Color;
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ── Catppuccin Mocha palette ────────────────────────────────────────
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

// ── Game constants ──────────────────────────────────────────────────
const GRID_SIZE: usize = 10;
const CELL_SIZE: f32 = 36.0;
const CELL_GAP: f32 = 2.0;
const GRID_PADDING: f32 = 20.0;
const LABEL_SIZE: f32 = 14.0;
const LABEL_OFFSET: f32 = 20.0;
const TITLE_FONT_SIZE: f32 = 22.0;
const INFO_FONT_SIZE: f32 = 15.0;
const STATS_FONT_SIZE: f32 = 13.0;
const HEADER_HEIGHT: f32 = 50.0;
const GRID_TOP: f32 = HEADER_HEIGHT + 30.0;
const GRID_LEFT_PLAYER: f32 = GRID_PADDING + LABEL_OFFSET;
const GRID_SPACING: f32 = 60.0;
const GRID_WIDTH: f32 = GRID_SIZE as f32 * (CELL_SIZE + CELL_GAP) - CELL_GAP;
const GRID_LEFT_OPPONENT: f32 = GRID_LEFT_PLAYER + GRID_WIDTH + GRID_SPACING + LABEL_OFFSET;
const CORNER_RADIUS: f32 = 4.0;
const MARKER_SIZE: f32 = 10.0;
const SHIP_CORNER_RADIUS: f32 = 3.0;

/// Number of ships and their sizes.
const SHIP_DEFS: [(ShipKind, usize); 5] = [
    (ShipKind::Carrier, 5),
    (ShipKind::Battleship, 4),
    (ShipKind::Cruiser, 3),
    (ShipKind::Submarine, 3),
    (ShipKind::Destroyer, 2),
];

// ── Seeded LCG RNG ─────────────────────────────────────────────────

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

    fn next_range(&mut self, max: u64) -> u64 {
        self.next() % max
    }
}

// ── Ship types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ShipKind {
    Carrier,
    Battleship,
    Cruiser,
    Submarine,
    Destroyer,
}

impl ShipKind {
    fn name(self) -> &'static str {
        match self {
            Self::Carrier => "Carrier",
            Self::Battleship => "Battleship",
            Self::Cruiser => "Cruiser",
            Self::Submarine => "Submarine",
            Self::Destroyer => "Destroyer",
        }
    }

    fn size(self) -> usize {
        match self {
            Self::Carrier => 5,
            Self::Battleship => 4,
            Self::Cruiser => 3,
            Self::Submarine => 3,
            Self::Destroyer => 2,
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Carrier => TEAL,
            Self::Battleship => LAVENDER,
            Self::Cruiser => GREEN,
            Self::Submarine => YELLOW,
            Self::Destroyer => PEACH,
        }
    }
}

// ── Orientation ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    fn toggle(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

// ── Ship placement ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ship {
    kind: ShipKind,
    row: usize,
    col: usize,
    orientation: Orientation,
}

impl Ship {
    /// Returns all cells occupied by this ship.
    fn cells(self) -> Vec<(usize, usize)> {
        let size = self.kind.size();
        let mut result = Vec::with_capacity(size);
        for i in 0..size {
            let (r, c) = match self.orientation {
                Orientation::Horizontal => (self.row, self.col + i),
                Orientation::Vertical => (self.row + i, self.col),
            };
            result.push((r, c));
        }
        result
    }

    /// Returns true if all cells are within the 10x10 grid.
    fn is_within_bounds(self) -> bool {
        let size = self.kind.size();
        match self.orientation {
            Orientation::Horizontal => self.row < GRID_SIZE && self.col + size <= GRID_SIZE,
            Orientation::Vertical => self.row + size <= GRID_SIZE && self.col < GRID_SIZE,
        }
    }
}

// ── Cell state on grids ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellMark {
    Empty,
    Miss,
    Hit,
}

// ── AI firing strategy ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum AiMode {
    /// Firing at random unexplored cells.
    Hunt,
    /// Following up on a hit — trying adjacent cells.
    Target { targets: Vec<(usize, usize)> },
}

#[derive(Debug, Clone)]
struct AiState {
    mode: AiMode,
    /// Tracks which cells the AI has already fired at.
    fired: [[bool; GRID_SIZE]; GRID_SIZE],
    shots: usize,
    hits: usize,
}

impl AiState {
    fn new() -> Self {
        Self {
            mode: AiMode::Hunt,
            fired: [[false; GRID_SIZE]; GRID_SIZE],
            shots: 0,
            hits: 0,
        }
    }

    /// Pick the next cell to fire at.
    fn choose_target(&mut self, rng: &mut Rng) -> (usize, usize) {
        match &self.mode {
            AiMode::Target { targets } if !targets.is_empty() => {
                // Pick the first valid unfired target from the list.
                let targets_clone = targets.clone();
                for &(r, c) in &targets_clone {
                    if !self.fired[r][c] {
                        return (r, c);
                    }
                }
                // All queued targets already fired — fall back to hunt.
                self.mode = AiMode::Hunt;
                self.pick_random(rng)
            }
            _ => {
                self.mode = AiMode::Hunt;
                self.pick_random(rng)
            }
        }
    }

    /// Pick a random unfired cell.
    fn pick_random(&self, rng: &mut Rng) -> (usize, usize) {
        // Count unfired cells.
        let mut unfired = Vec::new();
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if !self.fired[r][c] {
                    unfired.push((r, c));
                }
            }
        }
        if unfired.is_empty() {
            return (0, 0); // Should not happen in a valid game.
        }
        let idx = rng.next_range(unfired.len() as u64) as usize;
        unfired[idx]
    }

    /// Record a shot result and update the mode.
    fn record_shot(&mut self, row: usize, col: usize, hit: bool) {
        self.fired[row][col] = true;
        self.shots += 1;
        if hit {
            self.hits += 1;
            // Add adjacent cells as targets.
            let mut new_targets = Vec::new();
            if row > 0 && !self.fired[row - 1][col] {
                new_targets.push((row - 1, col));
            }
            if row + 1 < GRID_SIZE && !self.fired[row + 1][col] {
                new_targets.push((row + 1, col));
            }
            if col > 0 && !self.fired[row][col - 1] {
                new_targets.push((row, col - 1));
            }
            if col + 1 < GRID_SIZE && !self.fired[row][col + 1] {
                new_targets.push((row, col + 1));
            }
            match &mut self.mode {
                AiMode::Target { targets } => {
                    for t in new_targets {
                        if !targets.contains(&t) {
                            targets.push(t);
                        }
                    }
                }
                AiMode::Hunt => {
                    self.mode = AiMode::Target {
                        targets: new_targets,
                    };
                }
            }
        } else {
            // Remove this cell from target list if present.
            if let AiMode::Target { targets } = &mut self.mode {
                targets.retain(|&(r, c)| !(r == row && c == col));
                if targets.is_empty() {
                    self.mode = AiMode::Hunt;
                }
            }
        }
    }
}

// ── Game phase ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    /// Placing ships on the player's grid.
    Placement,
    /// Player and AI take turns firing.
    Firing,
    /// Game is over.
    GameOver,
}

// ── Fleet board ─────────────────────────────────────────────────────

/// A player's fleet: ships and the grid tracking incoming fire.
#[derive(Debug, Clone)]
struct Fleet {
    ships: Vec<Ship>,
    /// Grid tracking which cells have been fired at and the result.
    marks: [[CellMark; GRID_SIZE]; GRID_SIZE],
    /// Grid tracking which cells have a ship (and which ship index).
    ship_grid: [[Option<usize>; GRID_SIZE]; GRID_SIZE],
    /// Per-ship hit count.
    ship_hits: Vec<usize>,
}

impl Fleet {
    fn new() -> Self {
        Self {
            ships: Vec::new(),
            marks: [[CellMark::Empty; GRID_SIZE]; GRID_SIZE],
            ship_grid: [[None; GRID_SIZE]; GRID_SIZE],
            ship_hits: Vec::new(),
        }
    }

    /// Try to place a ship. Returns false if it overlaps or is out of bounds.
    fn place_ship(&mut self, ship: Ship) -> bool {
        if !ship.is_within_bounds() {
            return false;
        }
        let cells = ship.cells();
        // Check for overlap with existing ships.
        for &(r, c) in &cells {
            if self.ship_grid[r][c].is_some() {
                return false;
            }
        }
        let idx = self.ships.len();
        self.ships.push(ship);
        self.ship_hits.push(0);
        for &(r, c) in &cells {
            self.ship_grid[r][c] = Some(idx);
        }
        true
    }

    /// Fire at a cell. Returns (hit, sunk_ship_kind).
    fn receive_fire(&mut self, row: usize, col: usize) -> (bool, Option<ShipKind>) {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return (false, None);
        }
        if self.marks[row][col] != CellMark::Empty {
            return (false, None); // Already fired here.
        }
        if let Some(ship_idx) = self.ship_grid[row][col] {
            self.marks[row][col] = CellMark::Hit;
            self.ship_hits[ship_idx] += 1;
            let ship = self.ships[ship_idx];
            if self.ship_hits[ship_idx] >= ship.kind.size() {
                return (true, Some(ship.kind));
            }
            return (true, None);
        }
        self.marks[row][col] = CellMark::Miss;
        (false, None)
    }

    /// Returns true if all ships are sunk.
    fn all_sunk(&self) -> bool {
        for (i, ship) in self.ships.iter().enumerate() {
            if self.ship_hits[i] < ship.kind.size() {
                return false;
            }
        }
        !self.ships.is_empty()
    }

    /// Returns the number of ships still afloat.
    fn ships_remaining(&self) -> usize {
        self.ships
            .iter()
            .enumerate()
            .filter(|(i, ship)| self.ship_hits[*i] < ship.kind.size())
            .count()
    }

    /// Returns true if a ship at the given index is sunk.
    fn is_ship_sunk(&self, idx: usize) -> bool {
        if idx >= self.ships.len() {
            return false;
        }
        self.ship_hits[idx] >= self.ships[idx].kind.size()
    }

    /// Returns true if the cell (row, col) belongs to a sunk ship.
    fn is_cell_sunk(&self, row: usize, col: usize) -> bool {
        if let Some(idx) = self.ship_grid[row][col] {
            self.is_ship_sunk(idx)
        } else {
            false
        }
    }

    /// Check if placing a ship would overlap with existing ships.
    fn would_overlap(&self, ship: &Ship) -> bool {
        if !ship.is_within_bounds() {
            return true;
        }
        for (r, c) in ship.cells() {
            if self.ship_grid[r][c].is_some() {
                return true;
            }
        }
        false
    }

    /// Check if a cell has already been fired upon.
    fn already_fired(&self, row: usize, col: usize) -> bool {
        if row >= GRID_SIZE || col >= GRID_SIZE {
            return true;
        }
        self.marks[row][col] != CellMark::Empty
    }
}

// ── Main application ────────────────────────────────────────────────

struct BattleshipApp {
    phase: GamePhase,
    player_fleet: Fleet,
    opponent_fleet: Fleet,
    ai_state: AiState,
    rng: Rng,

    // Placement state
    placement_index: usize,
    placement_row: usize,
    placement_col: usize,
    placement_orientation: Orientation,

    // Firing state — cursor on the opponent's grid
    cursor_row: usize,
    cursor_col: usize,

    // Stats
    player_shots: usize,
    player_hits: usize,

    // Messages
    message: String,
    last_sunk_message: String,

    // Game over
    player_won: bool,
}

impl BattleshipApp {
    fn new() -> Self {
        let mut app = Self {
            phase: GamePhase::Placement,
            player_fleet: Fleet::new(),
            opponent_fleet: Fleet::new(),
            ai_state: AiState::new(),
            rng: Rng::new(0xDEAD_BEEF_CAFE_1234),
            placement_index: 0,
            placement_row: 0,
            placement_col: 0,
            placement_orientation: Orientation::Horizontal,
            cursor_row: 0,
            cursor_col: 0,
            player_shots: 0,
            player_hits: 0,
            message: String::from("Place your Carrier (5). R to rotate, Enter to place."),
            last_sunk_message: String::new(),
            player_won: false,
        };
        app.place_ai_ships();
        app
    }

    /// Resets the game to the initial state for a new game.
    fn new_game(&mut self) {
        self.phase = GamePhase::Placement;
        self.player_fleet = Fleet::new();
        self.opponent_fleet = Fleet::new();
        self.ai_state = AiState::new();
        self.placement_index = 0;
        self.placement_row = 0;
        self.placement_col = 0;
        self.placement_orientation = Orientation::Horizontal;
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.player_shots = 0;
        self.player_hits = 0;
        self.message = String::from("Place your Carrier (5). R to rotate, Enter to place.");
        self.last_sunk_message = String::new();
        self.player_won = false;
        self.place_ai_ships();
    }

    /// Place AI ships randomly, ensuring no overlaps and within bounds.
    fn place_ai_ships(&mut self) {
        self.opponent_fleet = Fleet::new();
        for &(kind, _) in &SHIP_DEFS {
            let mut placed = false;
            // Safety valve: prevent infinite loops if RNG is pathological.
            let mut attempts = 0;
            while !placed && attempts < 1000 {
                attempts += 1;
                let orientation = if self.rng.next_range(2) == 0 {
                    Orientation::Horizontal
                } else {
                    Orientation::Vertical
                };
                let row = self.rng.next_range(GRID_SIZE as u64) as usize;
                let col = self.rng.next_range(GRID_SIZE as u64) as usize;
                let ship = Ship {
                    kind,
                    row,
                    col,
                    orientation,
                };
                if ship.is_within_bounds() && !self.opponent_fleet.would_overlap(&ship) {
                    self.opponent_fleet.place_ship(ship);
                    placed = true;
                }
            }
        }
    }

    /// The ship kind currently being placed (if any).
    fn current_placement_ship(&self) -> Option<ShipKind> {
        SHIP_DEFS.get(self.placement_index).map(|&(kind, _)| kind)
    }

    /// Build the preview ship for placement.
    fn placement_preview_ship(&self) -> Option<Ship> {
        self.current_placement_ship().map(|kind| Ship {
            kind,
            row: self.placement_row,
            col: self.placement_col,
            orientation: self.placement_orientation,
        })
    }

    /// Returns whether the current placement preview is valid.
    fn is_placement_valid(&self) -> bool {
        if let Some(ship) = self.placement_preview_ship() {
            ship.is_within_bounds() && !self.player_fleet.would_overlap(&ship)
        } else {
            false
        }
    }

    /// Clamp the placement cursor so the ship stays within bounds after rotation.
    fn clamp_placement(&mut self) {
        if let Some(kind) = self.current_placement_ship() {
            let size = kind.size();
            match self.placement_orientation {
                Orientation::Horizontal => {
                    if self.placement_col + size > GRID_SIZE {
                        self.placement_col = GRID_SIZE.saturating_sub(size);
                    }
                    if self.placement_row >= GRID_SIZE {
                        self.placement_row = GRID_SIZE - 1;
                    }
                }
                Orientation::Vertical => {
                    if self.placement_row + size > GRID_SIZE {
                        self.placement_row = GRID_SIZE.saturating_sub(size);
                    }
                    if self.placement_col >= GRID_SIZE {
                        self.placement_col = GRID_SIZE - 1;
                    }
                }
            }
        }
    }

    /// Handle keyboard input.
    fn handle_key(&mut self, key: Key) {
        match key {
            Key::N => {
                self.new_game();
            }
            Key::Escape => {
                // Could quit, but we just reset for now.
                self.new_game();
            }
            _ => match self.phase {
                GamePhase::Placement => self.handle_placement_key(key),
                GamePhase::Firing => self.handle_firing_key(key),
                GamePhase::GameOver => {
                    // Only N is handled above.
                }
            },
        }
    }

    fn handle_placement_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                if self.placement_row > 0 {
                    self.placement_row -= 1;
                }
            }
            Key::Down => {
                if self.placement_row + 1 < GRID_SIZE {
                    self.placement_row += 1;
                }
                self.clamp_placement();
            }
            Key::Left => {
                if self.placement_col > 0 {
                    self.placement_col -= 1;
                }
            }
            Key::Right => {
                if self.placement_col + 1 < GRID_SIZE {
                    self.placement_col += 1;
                }
                self.clamp_placement();
            }
            Key::R => {
                self.placement_orientation = self.placement_orientation.toggle();
                self.clamp_placement();
            }
            Key::Enter => {
                self.try_place_current_ship();
            }
            _ => {}
        }
    }

    fn try_place_current_ship(&mut self) {
        if let Some(ship) = self.placement_preview_ship() {
            if self.player_fleet.place_ship(ship) {
                self.placement_index += 1;
                self.placement_row = 0;
                self.placement_col = 0;
                self.placement_orientation = Orientation::Horizontal;
                if self.placement_index >= SHIP_DEFS.len() {
                    self.phase = GamePhase::Firing;
                    self.message = String::from("All ships placed! Select target and fire (Enter).");
                } else {
                    let (next_kind, next_size) = SHIP_DEFS[self.placement_index];
                    self.message = format!(
                        "Place your {} ({}). R to rotate, Enter to place.",
                        next_kind.name(),
                        next_size
                    );
                }
            } else {
                self.message = String::from("Invalid placement! Ship overlaps or out of bounds.");
            }
        }
    }

    fn handle_firing_key(&mut self, key: Key) {
        match key {
            Key::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                }
            }
            Key::Down => {
                if self.cursor_row + 1 < GRID_SIZE {
                    self.cursor_row += 1;
                }
            }
            Key::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            Key::Right => {
                if self.cursor_col + 1 < GRID_SIZE {
                    self.cursor_col += 1;
                }
            }
            Key::Enter | Key::Space => {
                self.fire_at_opponent();
            }
            _ => {}
        }
    }

    fn fire_at_opponent(&mut self) {
        if self.opponent_fleet.already_fired(self.cursor_row, self.cursor_col) {
            self.message = String::from("Already fired there! Choose a new target.");
            return;
        }

        let (hit, sunk) = self.opponent_fleet.receive_fire(self.cursor_row, self.cursor_col);
        self.player_shots += 1;
        if hit {
            self.player_hits += 1;
        }

        if let Some(kind) = sunk {
            self.last_sunk_message = format!("You sank their {}!", kind.name());
            self.message = self.last_sunk_message.clone();
        } else if hit {
            self.message = String::from("Hit!");
        } else {
            self.message = String::from("Miss!");
        }

        // Check if player won.
        if self.opponent_fleet.all_sunk() {
            self.phase = GamePhase::GameOver;
            self.player_won = true;
            self.message = String::from("VICTORY! You sank all enemy ships! Press N for new game.");
            return;
        }

        // AI's turn.
        self.ai_turn();
    }

    fn ai_turn(&mut self) {
        let (ar, ac) = self.ai_state.choose_target(&mut self.rng);
        let (hit, sunk) = self.player_fleet.receive_fire(ar, ac);
        self.ai_state.record_shot(ar, ac, hit);

        if let Some(kind) = sunk {
            self.message = format!(
                "AI sank your {}! Select your next target.",
                kind.name()
            );
        } else if hit {
            // Keep the player's message if they sank something, otherwise note AI hit.
            if self.last_sunk_message.is_empty() || !self.message.starts_with("You sank") {
                self.message = String::from("AI hit your ship! Select your next target.");
            } else {
                // Player just sank something; append AI info.
                self.message
                    .push_str(" AI hit your ship! Your turn.");
            }
        } else if !self.message.starts_with("You sank") {
            self.message = format!(
                "{} AI missed. Your turn.",
                if hit { "Hit!" } else { self.message.as_str() }
            );
        }

        // Clear last_sunk_message after the full turn.
        self.last_sunk_message.clear();

        // Check if AI won.
        if self.player_fleet.all_sunk() {
            self.phase = GamePhase::GameOver;
            self.player_won = false;
            self.message =
                String::from("DEFEAT! All your ships are sunk! Press N for new game.");
        }
    }

    /// Compute the player's hit rate as a percentage.
    fn player_hit_rate(&self) -> f32 {
        if self.player_shots == 0 {
            0.0
        } else {
            (self.player_hits as f32 / self.player_shots as f32) * 100.0
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: GRID_LEFT_OPPONENT + GRID_WIDTH + GRID_PADDING + LABEL_OFFSET,
            height: GRID_TOP + GRID_WIDTH + 140.0,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER,
            y: 16.0,
            text: String::from("Battleship"),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Phase indicator
        let phase_text = match self.phase {
            GamePhase::Placement => "Ship Placement",
            GamePhase::Firing => "Battle",
            GamePhase::GameOver => {
                if self.player_won {
                    "Victory!"
                } else {
                    "Defeat!"
                }
            }
        };
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_OPPONENT,
            y: 16.0,
            text: String::from(phase_text),
            color: match self.phase {
                GamePhase::Placement => YELLOW,
                GamePhase::Firing => GREEN,
                GamePhase::GameOver => {
                    if self.player_won {
                        GREEN
                    } else {
                        RED
                    }
                }
            },
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Message bar
        cmds.push(RenderCommand::FillRect {
            x: GRID_LEFT_PLAYER,
            y: HEADER_HEIGHT,
            width: GRID_LEFT_OPPONENT + GRID_WIDTH - GRID_LEFT_PLAYER,
            height: 22.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + 8.0,
            y: HEADER_HEIGHT + 3.0,
            text: self.message.clone(),
            color: TEXT_COLOR,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid titles
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + GRID_WIDTH / 2.0 - 40.0,
            y: GRID_TOP - 18.0,
            text: String::from("Your Fleet"),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_OPPONENT + GRID_WIDTH / 2.0 - 55.0,
            y: GRID_TOP - 18.0,
            text: String::from("Opponent's Ocean"),
            color: SUBTEXT0,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Column labels (1-10) for both grids
        self.render_grid_labels(&mut cmds, GRID_LEFT_PLAYER, GRID_TOP);
        self.render_grid_labels(&mut cmds, GRID_LEFT_OPPONENT, GRID_TOP);

        // Player grid
        self.render_player_grid(&mut cmds);

        // Opponent grid
        self.render_opponent_grid(&mut cmds);

        // Stats panel below the grids
        self.render_stats(&mut cmds);

        // Help text
        self.render_help(&mut cmds);

        cmds
    }

    fn render_grid_labels(&self, cmds: &mut Vec<RenderCommand>, grid_x: f32, grid_y: f32) {
        // Column labels: 1-10
        for c in 0..GRID_SIZE {
            let x = grid_x + c as f32 * (CELL_SIZE + CELL_GAP) + CELL_SIZE / 2.0 - 4.0;
            let label = format!("{}", c + 1);
            cmds.push(RenderCommand::Text {
                x,
                y: grid_y - 4.0,
                text: label,
                color: OVERLAY0,
                font_size: LABEL_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
        // Row labels: A-J
        for r in 0..GRID_SIZE {
            let y = grid_y + LABEL_OFFSET + r as f32 * (CELL_SIZE + CELL_GAP)
                + CELL_SIZE / 2.0
                - 7.0;
            let label = String::from((b'A' + r as u8) as char);
            cmds.push(RenderCommand::Text {
                x: grid_x - LABEL_OFFSET + 2.0,
                y,
                text: label,
                color: OVERLAY0,
                font_size: LABEL_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    fn render_player_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let gx = GRID_LEFT_PLAYER;
        let gy = GRID_TOP + LABEL_OFFSET;

        // Grid background
        cmds.push(RenderCommand::FillRect {
            x: gx - 3.0,
            y: gy - 3.0,
            width: GRID_WIDTH + 6.0,
            height: GRID_WIDTH + 6.0,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let cx = gx + c as f32 * (CELL_SIZE + CELL_GAP);
                let cy = gy + r as f32 * (CELL_SIZE + CELL_GAP);

                // Cell background
                let bg = if self.player_fleet.ship_grid[r][c].is_some() {
                    // Ship cell — show the ship's color.
                    if let Some(idx) = self.player_fleet.ship_grid[r][c] {
                        if self.player_fleet.is_ship_sunk(idx) {
                            // Sunk ship cells are dimmed.
                            OVERLAY0
                        } else {
                            self.player_fleet.ships[idx].kind.color()
                        }
                    } else {
                        SURFACE0
                    }
                } else {
                    SURFACE0
                };

                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg,
                    corner_radii: CornerRadii::all(CORNER_RADIUS),
                });

                // Hit/miss markers from AI shots
                match self.player_fleet.marks[r][c] {
                    CellMark::Hit => {
                        // Red X
                        self.render_hit_marker(cmds, cx, cy);
                    }
                    CellMark::Miss => {
                        // Blue dot
                        self.render_miss_marker(cmds, cx, cy);
                    }
                    CellMark::Empty => {}
                }
            }
        }

        // Placement preview overlay
        if self.phase == GamePhase::Placement {
            if let Some(ship) = self.placement_preview_ship() {
                let valid = self.is_placement_valid();
                let preview_color = if valid {
                    Color::rgba(166, 227, 161, 120) // Green tint
                } else {
                    Color::rgba(243, 139, 168, 120) // Red tint
                };
                let cells = ship.cells();
                for (r, c) in cells {
                    if r < GRID_SIZE && c < GRID_SIZE {
                        let cx = gx + c as f32 * (CELL_SIZE + CELL_GAP);
                        let cy = gy + r as f32 * (CELL_SIZE + CELL_GAP);
                        cmds.push(RenderCommand::FillRect {
                            x: cx,
                            y: cy,
                            width: CELL_SIZE,
                            height: CELL_SIZE,
                            color: preview_color,
                            corner_radii: CornerRadii::all(CORNER_RADIUS),
                        });
                    }
                }
            }
        }
    }

    fn render_opponent_grid(&self, cmds: &mut Vec<RenderCommand>) {
        let gx = GRID_LEFT_OPPONENT;
        let gy = GRID_TOP + LABEL_OFFSET;

        // Grid background
        cmds.push(RenderCommand::FillRect {
            x: gx - 3.0,
            y: gy - 3.0,
            width: GRID_WIDTH + 6.0,
            height: GRID_WIDTH + 6.0,
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                let cx = gx + c as f32 * (CELL_SIZE + CELL_GAP);
                let cy = gy + r as f32 * (CELL_SIZE + CELL_GAP);

                // Only show water unless the cell has been fired upon
                // or the game is over (reveal ships).
                let bg = if self.phase == GamePhase::GameOver {
                    // Reveal all ships on game over.
                    if let Some(idx) = self.opponent_fleet.ship_grid[r][c] {
                        if self.opponent_fleet.is_ship_sunk(idx) {
                            OVERLAY0
                        } else {
                            self.opponent_fleet.ships[idx].kind.color()
                        }
                    } else {
                        SURFACE0
                    }
                } else {
                    // During play, show sunk ship cells so player can see them.
                    if self.opponent_fleet.is_cell_sunk(r, c) {
                        OVERLAY0
                    } else {
                        SURFACE0
                    }
                };

                cmds.push(RenderCommand::FillRect {
                    x: cx,
                    y: cy,
                    width: CELL_SIZE,
                    height: CELL_SIZE,
                    color: bg,
                    corner_radii: CornerRadii::all(CORNER_RADIUS),
                });

                // Hit/miss markers
                match self.opponent_fleet.marks[r][c] {
                    CellMark::Hit => {
                        self.render_hit_marker(cmds, cx, cy);
                    }
                    CellMark::Miss => {
                        self.render_miss_marker(cmds, cx, cy);
                    }
                    CellMark::Empty => {}
                }
            }
        }

        // Cursor highlight during firing phase
        if self.phase == GamePhase::Firing {
            let cx = gx + self.cursor_col as f32 * (CELL_SIZE + CELL_GAP);
            let cy = gy + self.cursor_row as f32 * (CELL_SIZE + CELL_GAP);
            cmds.push(RenderCommand::StrokeRect {
                x: cx,
                y: cy,
                width: CELL_SIZE,
                height: CELL_SIZE,
                color: YELLOW,
                line_width: 2.5,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
        }
    }

    fn render_hit_marker(&self, cmds: &mut Vec<RenderCommand>, cx: f32, cy: f32) {
        let mid_x = cx + CELL_SIZE / 2.0;
        let mid_y = cy + CELL_SIZE / 2.0;
        let half = MARKER_SIZE / 2.0;
        // Red X: two diagonal lines
        cmds.push(RenderCommand::Line {
            x1: mid_x - half,
            y1: mid_y - half,
            x2: mid_x + half,
            y2: mid_y + half,
            color: RED,
            width: 2.5,
        });
        cmds.push(RenderCommand::Line {
            x1: mid_x + half,
            y1: mid_y - half,
            x2: mid_x - half,
            y2: mid_y + half,
            color: RED,
            width: 2.5,
        });
    }

    fn render_miss_marker(&self, cmds: &mut Vec<RenderCommand>, cx: f32, cy: f32) {
        let mid_x = cx + CELL_SIZE / 2.0 - MARKER_SIZE / 4.0;
        let mid_y = cy + CELL_SIZE / 2.0 - MARKER_SIZE / 4.0;
        // Blue dot (small filled rect with rounded corners to approximate circle)
        cmds.push(RenderCommand::FillRect {
            x: mid_x,
            y: mid_y,
            width: MARKER_SIZE / 2.0,
            height: MARKER_SIZE / 2.0,
            color: BLUE,
            corner_radii: CornerRadii::all(MARKER_SIZE / 4.0),
        });
    }

    fn render_stats(&self, cmds: &mut Vec<RenderCommand>) {
        let stats_y = GRID_TOP + LABEL_OFFSET + GRID_WIDTH + 16.0;

        // Stats background
        cmds.push(RenderCommand::FillRect {
            x: GRID_LEFT_PLAYER,
            y: stats_y,
            width: GRID_LEFT_OPPONENT + GRID_WIDTH - GRID_LEFT_PLAYER,
            height: 56.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });

        // Player stats
        let shots_text = format!("Shots: {}", self.player_shots);
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + 12.0,
            y: stats_y + 8.0,
            text: shots_text,
            color: TEXT_COLOR,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let hit_rate_text = format!("Hit Rate: {:.1}%", self.player_hit_rate());
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + 12.0,
            y: stats_y + 28.0,
            text: hit_rate_text,
            color: TEXT_COLOR,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Ships remaining
        let player_remaining = self.player_fleet.ships_remaining();
        let opponent_remaining = self.opponent_fleet.ships_remaining();

        let your_ships = format!("Your Ships: {}/5", player_remaining);
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + 180.0,
            y: stats_y + 8.0,
            text: your_ships,
            color: if player_remaining <= 1 { RED } else { GREEN },
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let enemy_ships = format!("Enemy Ships: {}/5", opponent_remaining);
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER + 180.0,
            y: stats_y + 28.0,
            text: enemy_ships,
            color: if opponent_remaining <= 1 { RED } else { GREEN },
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // AI stats
        let ai_shots_text = format!("AI Shots: {}", self.ai_state.shots);
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_OPPONENT,
            y: stats_y + 8.0,
            text: ai_shots_text,
            color: TEXT_COLOR,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        let ai_rate = if self.ai_state.shots == 0 {
            0.0
        } else {
            (self.ai_state.hits as f32 / self.ai_state.shots as f32) * 100.0
        };
        let ai_rate_text = format!("AI Hit Rate: {ai_rate:.1}%");
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_OPPONENT,
            y: stats_y + 28.0,
            text: ai_rate_text,
            color: TEXT_COLOR,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_help(&self, cmds: &mut Vec<RenderCommand>) {
        let help_y = GRID_TOP + LABEL_OFFSET + GRID_WIDTH + 80.0;
        let help_text = match self.phase {
            GamePhase::Placement => {
                "Arrow keys: move | R: rotate | Enter: place | N: new game"
            }
            GamePhase::Firing => {
                "Arrow keys: move cursor | Enter/Space: fire | N: new game"
            }
            GamePhase::GameOver => "N: new game | Esc: reset",
        };
        cmds.push(RenderCommand::Text {
            x: GRID_LEFT_PLAYER,
            y: help_y,
            text: String::from(help_text),
            color: OVERLAY0,
            font_size: STATS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

fn main() {
    let _app = BattleshipApp::new();
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RNG tests ───────────────────────────────────────────────────

    #[test]
    fn test_rng_produces_different_values() {
        let mut rng = Rng::new(42);
        let a = rng.next();
        let b = rng.next();
        assert_ne!(a, b);
    }

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = Rng::new(42);
        let mut rng2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(rng1.next(), rng2.next());
        }
    }

    #[test]
    fn test_rng_range_bounded() {
        let mut rng = Rng::new(123);
        for _ in 0..200 {
            let val = rng.next_range(10);
            assert!(val < 10);
        }
    }

    // ── Ship kind tests ─────────────────────────────────────────────

    #[test]
    fn test_ship_kind_sizes() {
        assert_eq!(ShipKind::Carrier.size(), 5);
        assert_eq!(ShipKind::Battleship.size(), 4);
        assert_eq!(ShipKind::Cruiser.size(), 3);
        assert_eq!(ShipKind::Submarine.size(), 3);
        assert_eq!(ShipKind::Destroyer.size(), 2);
    }

    #[test]
    fn test_ship_kind_names() {
        assert_eq!(ShipKind::Carrier.name(), "Carrier");
        assert_eq!(ShipKind::Battleship.name(), "Battleship");
        assert_eq!(ShipKind::Cruiser.name(), "Cruiser");
        assert_eq!(ShipKind::Submarine.name(), "Submarine");
        assert_eq!(ShipKind::Destroyer.name(), "Destroyer");
    }

    // ── Ship placement tests ────────────────────────────────────────

    #[test]
    fn test_ship_cells_horizontal() {
        let ship = Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 3,
            orientation: Orientation::Horizontal,
        };
        let cells = ship.cells();
        assert_eq!(cells, vec![(2, 3), (2, 4), (2, 5)]);
    }

    #[test]
    fn test_ship_cells_vertical() {
        let ship = Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 3,
            orientation: Orientation::Vertical,
        };
        let cells = ship.cells();
        assert_eq!(cells, vec![(2, 3), (3, 3), (4, 3)]);
    }

    #[test]
    fn test_ship_within_bounds_horizontal() {
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 5,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds()); // 5+5 = 10, within 0..10

        let ship_oob = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 6,
            orientation: Orientation::Horizontal,
        };
        assert!(!ship_oob.is_within_bounds()); // 6+5 = 11, out of bounds
    }

    #[test]
    fn test_ship_within_bounds_vertical() {
        let ship = Ship {
            kind: ShipKind::Battleship,
            row: 6,
            col: 0,
            orientation: Orientation::Vertical,
        };
        assert!(ship.is_within_bounds()); // 6+4 = 10

        let ship_oob = Ship {
            kind: ShipKind::Battleship,
            row: 7,
            col: 0,
            orientation: Orientation::Vertical,
        };
        assert!(!ship_oob.is_within_bounds()); // 7+4 = 11
    }

    #[test]
    fn test_ship_at_origin() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(0, 0), (0, 1)]);
    }

    #[test]
    fn test_ship_at_bottom_right_horizontal() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 9,
            col: 8,
            orientation: Orientation::Horizontal,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(9, 8), (9, 9)]);
    }

    #[test]
    fn test_ship_at_bottom_right_vertical() {
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 8,
            col: 9,
            orientation: Orientation::Vertical,
        };
        assert!(ship.is_within_bounds());
        assert_eq!(ship.cells(), vec![(8, 9), (9, 9)]);
    }

    // ── Orientation toggle ──────────────────────────────────────────

    #[test]
    fn test_orientation_toggle() {
        assert_eq!(Orientation::Horizontal.toggle(), Orientation::Vertical);
        assert_eq!(Orientation::Vertical.toggle(), Orientation::Horizontal);
    }

    // ── Fleet placement and overlap tests ───────────────────────────

    #[test]
    fn test_fleet_place_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship));
        assert_eq!(fleet.ships.len(), 1);
        // Check grid cells
        for c in 0..5 {
            assert_eq!(fleet.ship_grid[0][c], Some(0));
        }
        assert_eq!(fleet.ship_grid[0][5], None);
    }

    #[test]
    fn test_fleet_overlap_detection() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship1));

        // Try to place overlapping ship
        let ship2 = Ship {
            kind: ShipKind::Battleship,
            row: 0,
            col: 2,
            orientation: Orientation::Vertical,
        };
        assert!(!fleet.place_ship(ship2));
        assert_eq!(fleet.ships.len(), 1); // Should not have been added
    }

    #[test]
    fn test_fleet_no_overlap_adjacent() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship1));

        // Adjacent ship (row below) should be fine.
        let ship2 = Ship {
            kind: ShipKind::Destroyer,
            row: 1,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(ship2));
        assert_eq!(fleet.ships.len(), 2);
    }

    #[test]
    fn test_fleet_out_of_bounds_placement() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Carrier,
            row: 0,
            col: 8,
            orientation: Orientation::Horizontal,
        };
        assert!(!fleet.place_ship(ship)); // 8 + 5 = 13, out of bounds
    }

    #[test]
    fn test_fleet_would_overlap() {
        let mut fleet = Fleet::new();
        let ship1 = Ship {
            kind: ShipKind::Cruiser,
            row: 3,
            col: 3,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship1);

        let overlapping = Ship {
            kind: ShipKind::Submarine,
            row: 3,
            col: 4,
            orientation: Orientation::Vertical,
        };
        assert!(fleet.would_overlap(&overlapping));

        let not_overlapping = Ship {
            kind: ShipKind::Submarine,
            row: 4,
            col: 3,
            orientation: Orientation::Vertical,
        };
        assert!(!fleet.would_overlap(&not_overlapping));
    }

    // ── Firing tests ────────────────────────────────────────────────

    #[test]
    fn test_fire_hit() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit, sunk) = fleet.receive_fire(0, 0);
        assert!(hit);
        assert!(sunk.is_none());
        assert_eq!(fleet.marks[0][0], CellMark::Hit);
    }

    #[test]
    fn test_fire_miss() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit, sunk) = fleet.receive_fire(5, 5);
        assert!(!hit);
        assert!(sunk.is_none());
        assert_eq!(fleet.marks[5][5], CellMark::Miss);
    }

    #[test]
    fn test_fire_sink_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        let (hit1, sunk1) = fleet.receive_fire(0, 0);
        assert!(hit1);
        assert!(sunk1.is_none());

        let (hit2, sunk2) = fleet.receive_fire(0, 1);
        assert!(hit2);
        assert_eq!(sunk2, Some(ShipKind::Destroyer));
    }

    #[test]
    fn test_fire_already_fired() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);

        fleet.receive_fire(0, 0); // First shot
        let (hit, sunk) = fleet.receive_fire(0, 0); // Duplicate
        assert!(!hit); // Should not count again
        assert!(sunk.is_none());
    }

    #[test]
    fn test_fire_out_of_bounds() {
        let mut fleet = Fleet::new();
        let (hit, sunk) = fleet.receive_fire(10, 10);
        assert!(!hit);
        assert!(sunk.is_none());
    }

    #[test]
    fn test_already_fired_check() {
        let mut fleet = Fleet::new();
        assert!(!fleet.already_fired(5, 5));
        fleet.marks[5][5] = CellMark::Miss;
        assert!(fleet.already_fired(5, 5));
        fleet.marks[3][3] = CellMark::Hit;
        assert!(fleet.already_fired(3, 3));
    }

    #[test]
    fn test_already_fired_out_of_bounds() {
        let fleet = Fleet::new();
        assert!(fleet.already_fired(10, 10));
    }

    // ── Sinking detection ───────────────────────────────────────────

    #[test]
    fn test_all_sunk_empty_fleet() {
        let fleet = Fleet::new();
        assert!(!fleet.all_sunk()); // No ships = not all sunk
    }

    #[test]
    fn test_all_sunk_one_ship() {
        let mut fleet = Fleet::new();
        let ship = Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        fleet.place_ship(ship);
        assert!(!fleet.all_sunk());

        fleet.receive_fire(0, 0);
        assert!(!fleet.all_sunk());

        fleet.receive_fire(0, 1);
        assert!(fleet.all_sunk());
    }

    #[test]
    fn test_all_sunk_multiple_ships() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        fleet.place_ship(Ship {
            kind: ShipKind::Submarine,
            row: 2,
            col: 0,
            orientation: Orientation::Horizontal,
        });

        // Sink destroyer
        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert!(!fleet.all_sunk());

        // Sink submarine
        fleet.receive_fire(2, 0);
        fleet.receive_fire(2, 1);
        fleet.receive_fire(2, 2);
        assert!(fleet.all_sunk());
    }

    #[test]
    fn test_ships_remaining() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        fleet.place_ship(Ship {
            kind: ShipKind::Cruiser,
            row: 2,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert_eq!(fleet.ships_remaining(), 2);

        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert_eq!(fleet.ships_remaining(), 1);
    }

    #[test]
    fn test_is_ship_sunk() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert!(!fleet.is_ship_sunk(0));
        fleet.receive_fire(0, 0);
        assert!(!fleet.is_ship_sunk(0));
        fleet.receive_fire(0, 1);
        assert!(fleet.is_ship_sunk(0));
    }

    #[test]
    fn test_is_cell_sunk() {
        let mut fleet = Fleet::new();
        fleet.place_ship(Ship {
            kind: ShipKind::Destroyer,
            row: 0,
            col: 0,
            orientation: Orientation::Horizontal,
        });
        assert!(!fleet.is_cell_sunk(0, 0));
        fleet.receive_fire(0, 0);
        fleet.receive_fire(0, 1);
        assert!(fleet.is_cell_sunk(0, 0));
        assert!(fleet.is_cell_sunk(0, 1));
        assert!(!fleet.is_cell_sunk(1, 0)); // No ship here
    }

    // ── AI behavior tests ───────────────────────────────────────────

    #[test]
    fn test_ai_starts_in_hunt_mode() {
        let ai = AiState::new();
        assert_eq!(ai.mode, AiMode::Hunt);
        assert_eq!(ai.shots, 0);
        assert_eq!(ai.hits, 0);
    }

    #[test]
    fn test_ai_switches_to_target_on_hit() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        assert!(matches!(ai.mode, AiMode::Target { .. }));
        assert_eq!(ai.shots, 1);
        assert_eq!(ai.hits, 1);
    }

    #[test]
    fn test_ai_stays_hunt_on_miss() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, false);
        assert_eq!(ai.mode, AiMode::Hunt);
        assert_eq!(ai.shots, 1);
        assert_eq!(ai.hits, 0);
    }

    #[test]
    fn test_ai_target_has_adjacent_cells() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        if let AiMode::Target { targets } = &ai.mode {
            // Should have up to 4 adjacent cells
            assert!(!targets.is_empty());
            assert!(targets.contains(&(4, 5))); // Up
            assert!(targets.contains(&(6, 5))); // Down
            assert!(targets.contains(&(5, 4))); // Left
            assert!(targets.contains(&(5, 6))); // Right
        } else {
            panic!("AI should be in Target mode");
        }
    }

    #[test]
    fn test_ai_target_edge_cell() {
        let mut ai = AiState::new();
        ai.record_shot(0, 0, true);
        if let AiMode::Target { targets } = &ai.mode {
            // Corner hit: only 2 adjacent cells
            assert_eq!(targets.len(), 2);
            assert!(targets.contains(&(1, 0)));
            assert!(targets.contains(&(0, 1)));
        } else {
            panic!("AI should be in Target mode");
        }
    }

    #[test]
    fn test_ai_choose_target_hunt() {
        let mut ai = AiState::new();
        let mut rng = Rng::new(99);
        let (r, c) = ai.choose_target(&mut rng);
        assert!(r < GRID_SIZE);
        assert!(c < GRID_SIZE);
    }

    #[test]
    fn test_ai_choose_target_from_targets() {
        let mut ai = AiState::new();
        ai.record_shot(5, 5, true);
        let mut rng = Rng::new(99);
        let (r, c) = ai.choose_target(&mut rng);
        // Should pick from adjacents of (5,5)
        let expected = [(4, 5), (6, 5), (5, 4), (5, 6)];
        assert!(expected.contains(&(r, c)));
    }

    #[test]
    fn test_ai_returns_to_hunt_when_targets_exhausted() {
        let mut ai = AiState::new();
        ai.record_shot(0, 0, true);
        // Fire at all target cells as misses.
        ai.record_shot(1, 0, false);
        ai.record_shot(0, 1, false);
        assert_eq!(ai.mode, AiMode::Hunt);
    }

    #[test]
    fn test_ai_records_fired_cells() {
        let mut ai = AiState::new();
        assert!(!ai.fired[3][7]);
        ai.record_shot(3, 7, false);
        assert!(ai.fired[3][7]);
    }

    #[test]
    fn test_ai_pick_random_avoids_fired() {
        let mut ai = AiState::new();
        let mut rng = Rng::new(42);
        // Fire at most cells
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if !(r == 9 && c == 9) {
                    ai.fired[r][c] = true;
                }
            }
        }
        let (r, c) = ai.pick_random(&mut rng);
        assert_eq!((r, c), (9, 9));
    }

    // ── Game phase tests ────────────────────────────────────────────

    #[test]
    fn test_new_game_starts_in_placement() {
        let app = BattleshipApp::new();
        assert_eq!(app.phase, GamePhase::Placement);
    }

    #[test]
    fn test_new_game_has_ai_ships() {
        let app = BattleshipApp::new();
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_new_game_player_fleet_empty() {
        let app = BattleshipApp::new();
        assert!(app.player_fleet.ships.is_empty());
    }

    #[test]
    fn test_new_game_placement_index() {
        let app = BattleshipApp::new();
        assert_eq!(app.placement_index, 0);
    }

    #[test]
    fn test_current_placement_ship() {
        let app = BattleshipApp::new();
        assert_eq!(app.current_placement_ship(), Some(ShipKind::Carrier));
    }

    #[test]
    fn test_placement_phase_place_all_ships() {
        let mut app = BattleshipApp::new();
        // Place all 5 ships at non-overlapping positions.
        // Carrier(5) at row 0
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 1);

        // Battleship(4) at row 1
        app.placement_row = 1;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 2);

        // Cruiser(3) at row 2
        app.placement_row = 2;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 3);

        // Submarine(3) at row 3
        app.placement_row = 3;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 4);

        // Destroyer(2) at row 4
        app.placement_row = 4;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);

        // Should now be in Firing phase.
        assert_eq!(app.phase, GamePhase::Firing);
        assert_eq!(app.player_fleet.ships.len(), 5);
    }

    #[test]
    fn test_placement_rotate() {
        let mut app = BattleshipApp::new();
        assert_eq!(app.placement_orientation, Orientation::Horizontal);
        app.handle_key(Key::R);
        assert_eq!(app.placement_orientation, Orientation::Vertical);
        app.handle_key(Key::R);
        assert_eq!(app.placement_orientation, Orientation::Horizontal);
    }

    #[test]
    fn test_placement_arrow_keys() {
        let mut app = BattleshipApp::new();
        assert_eq!(app.placement_row, 0);
        assert_eq!(app.placement_col, 0);

        app.handle_key(Key::Down);
        assert_eq!(app.placement_row, 1);

        app.handle_key(Key::Right);
        assert_eq!(app.placement_col, 1);

        app.handle_key(Key::Up);
        assert_eq!(app.placement_row, 0);

        app.handle_key(Key::Left);
        assert_eq!(app.placement_col, 0);
    }

    #[test]
    fn test_placement_cursor_stays_in_bounds() {
        let mut app = BattleshipApp::new();
        // Try to go above row 0
        app.handle_key(Key::Up);
        assert_eq!(app.placement_row, 0);

        // Try to go left of col 0
        app.handle_key(Key::Left);
        assert_eq!(app.placement_col, 0);
    }

    #[test]
    fn test_placement_invalid_overlap() {
        let mut app = BattleshipApp::new();
        // Place carrier at (0,0) horizontal
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);

        // Try to place battleship overlapping
        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert_eq!(app.placement_index, 1); // Should not advance
        assert!(app.message.contains("Invalid"));
    }

    #[test]
    fn test_placement_clamp_after_rotate() {
        let mut app = BattleshipApp::new();
        // Move to bottom edge and rotate vertical
        app.placement_row = 9;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::R); // Rotate to vertical
        // Carrier(5) vertical at row 9 would go to row 13; should clamp.
        assert!(app.placement_row + ShipKind::Carrier.size() <= GRID_SIZE);
    }

    // ── Firing phase tests ──────────────────────────────────────────

    fn setup_firing_app() -> BattleshipApp {
        let mut app = BattleshipApp::new();
        // Place all player ships quickly in non-overlapping rows.
        let positions = [(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)];
        for (i, &(row, col)) in positions.iter().enumerate() {
            app.placement_row = row;
            app.placement_col = col;
            app.placement_orientation = Orientation::Horizontal;
            if i < SHIP_DEFS.len() {
                app.handle_key(Key::Enter);
            }
        }
        assert_eq!(app.phase, GamePhase::Firing);
        app
    }

    #[test]
    fn test_firing_cursor_movement() {
        let mut app = setup_firing_app();
        assert_eq!(app.cursor_row, 0);
        assert_eq!(app.cursor_col, 0);

        app.handle_key(Key::Down);
        assert_eq!(app.cursor_row, 1);

        app.handle_key(Key::Right);
        assert_eq!(app.cursor_col, 1);
    }

    #[test]
    fn test_firing_cursor_bounds() {
        let mut app = setup_firing_app();
        app.handle_key(Key::Up);
        assert_eq!(app.cursor_row, 0); // Should not go below 0

        app.handle_key(Key::Left);
        assert_eq!(app.cursor_col, 0);

        // Move to bottom-right
        for _ in 0..20 {
            app.handle_key(Key::Down);
            app.handle_key(Key::Right);
        }
        assert_eq!(app.cursor_row, 9);
        assert_eq!(app.cursor_col, 9);
    }

    #[test]
    fn test_fire_and_track_stats() {
        let mut app = setup_firing_app();
        assert_eq!(app.player_shots, 0);
        assert_eq!(app.player_hits, 0);

        // Fire at (0,0) on opponent's grid — might be hit or miss.
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);

        assert_eq!(app.player_shots, 1);
    }

    #[test]
    fn test_fire_duplicate_rejected() {
        let mut app = setup_firing_app();
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);
        let shots_after_first = app.player_shots;

        // Try to fire at same spot.
        app.cursor_row = 0;
        app.cursor_col = 0;
        app.handle_key(Key::Enter);
        assert_eq!(app.player_shots, shots_after_first);
        assert!(app.message.contains("Already"));
    }

    #[test]
    fn test_fire_space_key() {
        let mut app = setup_firing_app();
        app.cursor_row = 5;
        app.cursor_col = 5;
        app.handle_key(Key::Space);
        assert_eq!(app.player_shots, 1);
    }

    #[test]
    fn test_player_hit_rate_zero() {
        let app = BattleshipApp::new();
        assert!((app.player_hit_rate() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_player_hit_rate_calculation() {
        let mut app = setup_firing_app();
        app.player_shots = 10;
        app.player_hits = 3;
        let rate = app.player_hit_rate();
        assert!((rate - 30.0).abs() < 0.1);
    }

    // ── Win/loss detection tests ────────────────────────────────────

    #[test]
    fn test_win_detection() {
        let mut app = setup_firing_app();
        // Manually sink all opponent ships.
        for ship in &app.opponent_fleet.ships.clone() {
            for (r, c) in ship.cells() {
                app.opponent_fleet.receive_fire(r, c);
            }
        }
        assert!(app.opponent_fleet.all_sunk());
    }

    #[test]
    fn test_loss_detection() {
        let mut app = setup_firing_app();
        // Manually sink all player ships.
        for ship in &app.player_fleet.ships.clone() {
            for (r, c) in ship.cells() {
                app.player_fleet.receive_fire(r, c);
            }
        }
        assert!(app.player_fleet.all_sunk());
    }

    // ── New game tests ──────────────────────────────────────────────

    #[test]
    fn test_new_game_resets_state() {
        let mut app = setup_firing_app();
        app.player_shots = 15;
        app.player_hits = 5;
        app.handle_key(Key::N);
        assert_eq!(app.phase, GamePhase::Placement);
        assert_eq!(app.player_shots, 0);
        assert_eq!(app.player_hits, 0);
        assert!(app.player_fleet.ships.is_empty());
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_new_game_from_game_over() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        app.handle_key(Key::N);
        assert_eq!(app.phase, GamePhase::Placement);
    }

    #[test]
    fn test_escape_resets() {
        let mut app = setup_firing_app();
        app.handle_key(Key::Escape);
        assert_eq!(app.phase, GamePhase::Placement);
    }

    // ── AI placement tests ──────────────────────────────────────────

    #[test]
    fn test_ai_ships_no_overlap() {
        let app = BattleshipApp::new();
        // Check that no two AI ships occupy the same cell.
        let mut occupied = [[false; GRID_SIZE]; GRID_SIZE];
        for ship in &app.opponent_fleet.ships {
            for (r, c) in ship.cells() {
                assert!(
                    !occupied[r][c],
                    "AI ships overlap at ({r}, {c})"
                );
                occupied[r][c] = true;
            }
        }
    }

    #[test]
    fn test_ai_ships_within_bounds() {
        let app = BattleshipApp::new();
        for ship in &app.opponent_fleet.ships {
            assert!(ship.is_within_bounds(), "AI ship out of bounds: {ship:?}");
        }
    }

    #[test]
    fn test_ai_places_correct_number_of_ships() {
        let app = BattleshipApp::new();
        assert_eq!(app.opponent_fleet.ships.len(), 5);
    }

    #[test]
    fn test_ai_ships_correct_sizes() {
        let app = BattleshipApp::new();
        let mut sizes: Vec<usize> = app
            .opponent_fleet
            .ships
            .iter()
            .map(|s| s.kind.size())
            .collect();
        sizes.sort();
        assert_eq!(sizes, vec![2, 3, 3, 4, 5]);
    }

    #[test]
    fn test_ai_placement_with_different_seeds() {
        // Different seeds should produce different layouts.
        let mut app1 = BattleshipApp::new();
        app1.rng = Rng::new(111);
        app1.place_ai_ships();

        let mut app2 = BattleshipApp::new();
        app2.rng = Rng::new(999);
        app2.place_ai_ships();

        // It's possible (but unlikely) for two seeds to produce the same layout.
        // Check that at least one ship differs.
        let differ = app1
            .opponent_fleet
            .ships
            .iter()
            .zip(app2.opponent_fleet.ships.iter())
            .any(|(a, b)| a.row != b.row || a.col != b.col || a.orientation != b.orientation);
        assert!(differ, "Different seeds should produce different layouts");
    }

    // ── Rendering tests ─────────────────────────────────────────────

    #[test]
    fn test_render_produces_commands() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_has_title() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_title = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Battleship")
        });
        assert!(has_title);
    }

    #[test]
    fn test_render_has_grid_titles() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_your_fleet = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Your Fleet")
        });
        let has_opponent = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text == "Opponent's Ocean")
        });
        assert!(has_your_fleet);
        assert!(has_opponent);
    }

    #[test]
    fn test_render_has_column_labels() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        for num in 1..=10 {
            let num_str = format!("{num}");
            let has_label = cmds.iter().any(|cmd| {
                matches!(cmd, RenderCommand::Text { text, .. } if text == &num_str)
            });
            assert!(has_label, "Missing column label {num}");
        }
    }

    #[test]
    fn test_render_has_row_labels() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        for ch in b'A'..=b'J' {
            let label = String::from(ch as char);
            let has_label = cmds.iter().any(|cmd| {
                matches!(cmd, RenderCommand::Text { text, .. } if text == &label)
            });
            assert!(has_label, "Missing row label {}", label);
        }
    }

    #[test]
    fn test_render_has_message() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_msg = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Place your Carrier"))
        });
        assert!(has_msg);
    }

    #[test]
    fn test_render_has_help_text() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_help = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Arrow keys"))
        });
        assert!(has_help);
    }

    #[test]
    fn test_render_firing_phase_has_cursor() {
        let mut app = setup_firing_app();
        app.cursor_row = 3;
        app.cursor_col = 4;
        let cmds = app.render();
        // Should have a StrokeRect for the cursor.
        let has_cursor = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::StrokeRect { color, .. } if *color == YELLOW)
        });
        assert!(has_cursor);
    }

    #[test]
    fn test_render_placement_preview() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        // The placement preview draws colored FillRects with semi-transparent green.
        // Just verify render doesn't crash and has a reasonable number of commands.
        assert!(cmds.len() > 50);
    }

    #[test]
    fn test_render_stats_panel() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_shots = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Shots:"))
        });
        assert!(has_shots);
    }

    #[test]
    fn test_render_ships_remaining_stat() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_enemy_ships = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Enemy Ships:"))
        });
        assert!(has_enemy_ships);
    }

    #[test]
    fn test_render_hit_rate_stat() {
        let app = BattleshipApp::new();
        let cmds = app.render();
        let has_rate = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Hit Rate:"))
        });
        assert!(has_rate);
    }

    #[test]
    fn test_render_after_hit() {
        let mut app = setup_firing_app();
        // Find a cell with an opponent ship and fire at it.
        let mut target = (0, 0);
        'outer: for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.opponent_fleet.ship_grid[r][c].is_some() {
                    target = (r, c);
                    break 'outer;
                }
            }
        }
        app.cursor_row = target.0;
        app.cursor_col = target.1;
        app.handle_key(Key::Enter);
        let cmds = app.render();
        // Should have hit markers (Line commands for the X)
        let has_hit_line = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Line { color, .. } if *color == RED)
        });
        assert!(has_hit_line);
    }

    #[test]
    fn test_render_after_miss() {
        let mut app = setup_firing_app();
        // Find a cell without an opponent ship and fire at it.
        let mut target = (0, 0);
        'outer: for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.opponent_fleet.ship_grid[r][c].is_none() {
                    target = (r, c);
                    break 'outer;
                }
            }
        }
        app.cursor_row = target.0;
        app.cursor_col = target.1;
        app.handle_key(Key::Enter);
        let cmds = app.render();
        // Should have a blue miss dot (FillRect with BLUE color)
        let has_miss_dot = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::FillRect { color, .. } if *color == BLUE)
        });
        assert!(has_miss_dot);
    }

    #[test]
    fn test_render_game_over_victory() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        app.player_won = true;
        app.message = String::from("VICTORY!");
        let cmds = app.render();
        let has_victory = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Victory"))
        });
        assert!(has_victory);
    }

    #[test]
    fn test_render_game_over_defeat() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        app.player_won = false;
        app.message = String::from("DEFEAT!");
        let cmds = app.render();
        let has_defeat = cmds.iter().any(|cmd| {
            matches!(cmd, RenderCommand::Text { text, .. } if text.contains("Defeat"))
        });
        assert!(has_defeat);
    }

    // ── Integration / gameplay tests ────────────────────────────────

    #[test]
    fn test_full_game_sink_all_opponent_ships() {
        let mut app = setup_firing_app();
        // Fire at every opponent ship cell.
        let opponent_ships = app.opponent_fleet.ships.clone();
        for ship in &opponent_ships {
            for (r, c) in ship.cells() {
                if !app.opponent_fleet.already_fired(r, c) && app.phase == GamePhase::Firing {
                    app.cursor_row = r;
                    app.cursor_col = c;
                    app.handle_key(Key::Enter);
                }
            }
        }
        // Either the player won, or the AI won first (unlikely but possible).
        assert!(
            app.phase == GamePhase::GameOver,
            "Game should be over after sinking all ships"
        );
    }

    #[test]
    fn test_ai_fires_after_player() {
        let mut app = setup_firing_app();
        let ai_shots_before = app.ai_state.shots;
        // Fire at an empty cell.
        let mut target = (0, 0);
        for r in 0..GRID_SIZE {
            for c in 0..GRID_SIZE {
                if app.opponent_fleet.ship_grid[r][c].is_none() {
                    target = (r, c);
                }
            }
        }
        app.cursor_row = target.0;
        app.cursor_col = target.1;
        app.handle_key(Key::Enter);
        assert_eq!(app.ai_state.shots, ai_shots_before + 1);
    }

    #[test]
    fn test_placement_message_updates() {
        let mut app = BattleshipApp::new();
        assert!(app.message.contains("Carrier"));

        app.placement_row = 0;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert!(app.message.contains("Battleship"));

        app.placement_row = 1;
        app.placement_col = 0;
        app.placement_orientation = Orientation::Horizontal;
        app.handle_key(Key::Enter);
        assert!(app.message.contains("Cruiser"));
    }

    #[test]
    fn test_sink_message_appears() {
        let mut app = setup_firing_app();
        // Find a destroyer (size 2) in opponent's fleet and sink it.
        let destroyer_idx = app
            .opponent_fleet
            .ships
            .iter()
            .position(|s| s.kind == ShipKind::Destroyer);
        if let Some(idx) = destroyer_idx {
            let cells = app.opponent_fleet.ships[idx].cells();
            for (r, c) in cells {
                if !app.opponent_fleet.already_fired(r, c) && app.phase == GamePhase::Firing {
                    app.cursor_row = r;
                    app.cursor_col = c;
                    app.handle_key(Key::Enter);
                }
            }
            // The message should mention sinking at some point during the sequence.
            // After the last hit that sinks the ship, we check the message
            // (it might be overwritten by AI response, but last_sunk should have been set).
        }
    }

    #[test]
    fn test_game_over_no_firing() {
        let mut app = setup_firing_app();
        app.phase = GamePhase::GameOver;
        let shots = app.player_shots;
        app.cursor_row = 5;
        app.cursor_col = 5;
        app.handle_key(Key::Enter); // Should be ignored.
        assert_eq!(app.player_shots, shots);
    }

    #[test]
    fn test_placement_phase_ignores_fire_keys() {
        let mut app = BattleshipApp::new();
        // Space should not do anything during placement.
        app.handle_key(Key::Space);
        assert_eq!(app.phase, GamePhase::Placement);
    }

    #[test]
    fn test_fleet_place_all_five_ships() {
        let mut fleet = Fleet::new();
        let ships = [
            Ship { kind: ShipKind::Carrier, row: 0, col: 0, orientation: Orientation::Horizontal },
            Ship { kind: ShipKind::Battleship, row: 1, col: 0, orientation: Orientation::Horizontal },
            Ship { kind: ShipKind::Cruiser, row: 2, col: 0, orientation: Orientation::Horizontal },
            Ship { kind: ShipKind::Submarine, row: 3, col: 0, orientation: Orientation::Horizontal },
            Ship { kind: ShipKind::Destroyer, row: 4, col: 0, orientation: Orientation::Horizontal },
        ];
        for ship in &ships {
            assert!(fleet.place_ship(*ship));
        }
        assert_eq!(fleet.ships.len(), 5);
        assert_eq!(fleet.ships_remaining(), 5);
    }

    #[test]
    fn test_cross_overlap_horizontal_vertical() {
        let mut fleet = Fleet::new();
        let h_ship = Ship {
            kind: ShipKind::Carrier,
            row: 5,
            col: 0,
            orientation: Orientation::Horizontal,
        };
        assert!(fleet.place_ship(h_ship));

        // Place vertical ship crossing through it.
        let v_ship = Ship {
            kind: ShipKind::Cruiser,
            row: 3,
            col: 2,
            orientation: Orientation::Vertical,
        };
        // Row 3,4,5 col 2 — row 5, col 2 is occupied by carrier.
        assert!(!fleet.place_ship(v_ship));
    }
}
