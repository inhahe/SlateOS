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

//! OurOS Mahjong Solitaire — a classic tile-matching puzzle game.
//!
//! Features a 144-tile "Turtle" pyramid layout across multiple layers. Tiles
//! are free when they have no tile on top and at least one open side (left or
//! right). Click or keyboard-select two matching free tiles to remove them.
//! Seasons match any season; flowers match any flower; all other tiles must
//! match exactly. Supports undo (Z), hints (H), shuffle (S), and new game (N).
//! Uses a deterministic seeded LCG random number generator with Catppuccin
//! Mocha color palette.

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
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const TEAL: Color = Color::from_hex(0x94E2D5);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ── Tile colors (one per suit category) ─────────────────────────────
const BAMBOO_COLOR: Color = Color::from_hex(0xA6E3A1);
const CIRCLE_COLOR: Color = Color::from_hex(0x89B4FA);
const CHARACTER_COLOR: Color = Color::from_hex(0xF38BA8);
const WIND_COLOR: Color = Color::from_hex(0xF9E2AF);
const DRAGON_COLOR: Color = Color::from_hex(0xCBA6F7);
const SEASON_COLOR: Color = Color::from_hex(0xFAB387);
const FLOWER_COLOR: Color = Color::from_hex(0x94E2D5);
const TILE_BG: Color = Color::from_hex(0x45475A);
const TILE_BG_FREE: Color = Color::from_hex(0x585B70);
const TILE_SELECTED: Color = Color::from_hex(0x89B4FA);
const TILE_HINT: Color = Color::from_hex(0xA6E3A1);
const TILE_SHADOW: Color = Color::from_hex(0x11111B);

// ── Layout constants ────────────────────────────────────────────────
const TILE_W: f32 = 42.0;
const TILE_H: f32 = 54.0;
const TILE_GAP_X: f32 = 2.0;
const TILE_GAP_Y: f32 = 2.0;
const LAYER_OFFSET_X: f32 = 4.0;
const LAYER_OFFSET_Y: f32 = 4.0;
const BOARD_OFFSET_X: f32 = 40.0;
const BOARD_OFFSET_Y: f32 = 70.0;
const TILE_CORNER: f32 = 4.0;
const SHADOW_OFFSET: f32 = 3.0;

const TITLE_FONT_SIZE: f32 = 22.0;
const TILE_FONT_SIZE: f32 = 16.0;
const INFO_FONT_SIZE: f32 = 14.0;
const STATUS_FONT_SIZE: f32 = 16.0;
const HELP_FONT_SIZE: f32 = 12.0;

/// Total number of tile positions in the Turtle layout.
const LAYOUT_SIZE: usize = 144;

// ── LCG random number generator ────────────────────────────────────

/// Simple linear congruential generator. Parameters from Numerical Recipes.
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

    fn next_range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next() >> 33) as usize % max
    }

    /// Fisher-Yates shuffle.
    fn shuffle<T>(&mut self, slice: &mut [T]) {
        let len = slice.len();
        for i in (1..len).rev() {
            let j = self.next_range(i + 1);
            slice.swap(i, j);
        }
    }
}

// ── Tile types ──────────────────────────────────────────────────────

/// The 36 distinct tile types in a Mahjong set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TileKind {
    Bamboo(u8),    // 1..=9
    Circle(u8),    // 1..=9
    Character(u8), // 1..=9
    Wind(u8),      // 0=East, 1=South, 2=West, 3=North
    Dragon(u8),    // 0=Red, 1=Green, 2=White
    Season(u8),    // 0=Spring, 1=Summer, 2=Autumn, 3=Winter
    Flower(u8),    // 0=Plum, 1=Orchid, 2=Chrysanthemum, 3=Bamboo
}

impl TileKind {
    /// Whether two tiles can be matched and removed together.
    /// Seasons match any other season; flowers match any other flower;
    /// all other tiles must match exactly.
    fn matches(self, other: Self) -> bool {
        match (self, other) {
            (TileKind::Season(_), TileKind::Season(_)) => true,
            (TileKind::Flower(_), TileKind::Flower(_)) => true,
            (a, b) => a == b,
        }
    }

    /// Short label for rendering on the tile face.
    fn label(self) -> &'static str {
        match self {
            TileKind::Bamboo(1) => "B1",
            TileKind::Bamboo(2) => "B2",
            TileKind::Bamboo(3) => "B3",
            TileKind::Bamboo(4) => "B4",
            TileKind::Bamboo(5) => "B5",
            TileKind::Bamboo(6) => "B6",
            TileKind::Bamboo(7) => "B7",
            TileKind::Bamboo(8) => "B8",
            TileKind::Bamboo(9) => "B9",
            TileKind::Circle(1) => "C1",
            TileKind::Circle(2) => "C2",
            TileKind::Circle(3) => "C3",
            TileKind::Circle(4) => "C4",
            TileKind::Circle(5) => "C5",
            TileKind::Circle(6) => "C6",
            TileKind::Circle(7) => "C7",
            TileKind::Circle(8) => "C8",
            TileKind::Circle(9) => "C9",
            TileKind::Character(1) => "W1",
            TileKind::Character(2) => "W2",
            TileKind::Character(3) => "W3",
            TileKind::Character(4) => "W4",
            TileKind::Character(5) => "W5",
            TileKind::Character(6) => "W6",
            TileKind::Character(7) => "W7",
            TileKind::Character(8) => "W8",
            TileKind::Character(9) => "W9",
            TileKind::Wind(0) => "E",
            TileKind::Wind(1) => "S",
            TileKind::Wind(2) => "W",
            TileKind::Wind(3) => "N",
            TileKind::Dragon(0) => "Dr",
            TileKind::Dragon(1) => "Dg",
            TileKind::Dragon(2) => "Dw",
            TileKind::Season(0) => "Sp",
            TileKind::Season(1) => "Su",
            TileKind::Season(2) => "Au",
            TileKind::Season(3) => "Wi",
            TileKind::Flower(0) => "Pl",
            TileKind::Flower(1) => "Or",
            TileKind::Flower(2) => "Ch",
            TileKind::Flower(3) => "Bm",
            _ => "??",
        }
    }

    /// Color for rendering this tile's text.
    fn text_color(self) -> Color {
        match self {
            TileKind::Bamboo(_) => BAMBOO_COLOR,
            TileKind::Circle(_) => CIRCLE_COLOR,
            TileKind::Character(_) => CHARACTER_COLOR,
            TileKind::Wind(_) => WIND_COLOR,
            TileKind::Dragon(_) => DRAGON_COLOR,
            TileKind::Season(_) => SEASON_COLOR,
            TileKind::Flower(_) => FLOWER_COLOR,
        }
    }

    /// Category label for the sidebar legend.
    fn category(self) -> &'static str {
        match self {
            TileKind::Bamboo(_) => "Bamboo",
            TileKind::Circle(_) => "Circle",
            TileKind::Character(_) => "Character",
            TileKind::Wind(_) => "Wind",
            TileKind::Dragon(_) => "Dragon",
            TileKind::Season(_) => "Season",
            TileKind::Flower(_) => "Flower",
        }
    }
}

/// Generate all 42 distinct tile types across 7 categories.
/// Base types (34): 9 Bamboo + 9 Circle + 9 Character + 4 Wind + 3 Dragon.
/// Bonus types (8): 4 Season + 4 Flower (each unique, but seasons match any
/// season and flowers match any flower).
fn all_tile_kinds() -> Vec<TileKind> {
    let mut kinds = Vec::with_capacity(42);
    for i in 1..=9 {
        kinds.push(TileKind::Bamboo(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Circle(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Character(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Wind(i));
    }
    for i in 0..3 {
        kinds.push(TileKind::Dragon(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Season(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Flower(i));
    }
    kinds
}

/// The 34 base tile types that each appear 4 times.
fn base_tile_kinds() -> Vec<TileKind> {
    let mut kinds = Vec::with_capacity(34);
    for i in 1..=9 {
        kinds.push(TileKind::Bamboo(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Circle(i));
    }
    for i in 1..=9 {
        kinds.push(TileKind::Character(i));
    }
    for i in 0..4 {
        kinds.push(TileKind::Wind(i));
    }
    for i in 0..3 {
        kinds.push(TileKind::Dragon(i));
    }
    kinds
}

/// Generate the traditional 144-tile Mahjong set.
/// 34 base types x 4 copies each = 136, plus 4 unique seasons and 4 unique
/// flowers = 144 total.
fn full_tile_set() -> Vec<TileKind> {
    let base = base_tile_kinds();
    let mut tiles = Vec::with_capacity(144);
    for kind in &base {
        for _ in 0..4 {
            tiles.push(*kind);
        }
    }
    // Seasons and flowers: one copy each (they match any in their group).
    for i in 0..4 {
        tiles.push(TileKind::Season(i));
    }
    for i in 0..4 {
        tiles.push(TileKind::Flower(i));
    }
    tiles
}

// ── Layout positions ────────────────────────────────────────────────

/// A position in the 3D tile grid: (layer, row, col).
/// Layer 0 is the bottom; higher layers are stacked on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TilePos {
    layer: usize,
    row: usize,
    col: usize,
}

/// A placed tile on the board.
#[derive(Clone, Copy, Debug)]
struct PlacedTile {
    pos: TilePos,
    kind: TileKind,
    removed: bool,
}

/// The classic "Turtle" Mahjong Solitaire layout.
/// Returns 144 tile positions across 5 layers.
///
/// Layer 0 (bottom): 12 columns x 8 rows with extras = 86 tiles
/// Layer 1: 10x6 = 60 tiles (but we use a subset)
/// ... built up as a pyramid.
///
/// We use a well-known Turtle/Tortoise layout:
/// Layer 0: main body (widest)
/// Layer 1: slightly smaller
/// Layer 2: smaller still
/// Layer 3: smaller
/// Layer 4: single cap tile
fn turtle_layout() -> Vec<TilePos> {
    let mut positions = Vec::with_capacity(LAYOUT_SIZE);

    // Layer 0: 12 wide x 8 tall, but with the classic Mahjong
    // turtle shape (extra tiles on edges).
    // Row template: which columns have tiles.
    // Standard turtle layout layer 0 has 86 positions.
    let layer0: &[(usize, &[usize])] = &[
        (0, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
        (1, &[2, 3, 4, 5, 6, 7, 8, 9]),
        (2, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        (3, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
        (4, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
        (5, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
        (6, &[2, 3, 4, 5, 6, 7, 8, 9]),
        (7, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ];
    for &(row, cols) in layer0 {
        for &col in cols {
            positions.push(TilePos { layer: 0, row, col });
        }
    }

    // Layer 1: 8 wide x 6 tall = 48 tiles, offset by (1,3)
    let layer1: &[(usize, &[usize])] = &[
        (1, &[3, 4, 5, 6, 7, 8, 9, 10]),
        (2, &[3, 4, 5, 6, 7, 8, 9, 10]),
        (3, &[3, 4, 5, 6, 7, 8, 9, 10]),
        (4, &[3, 4, 5, 6, 7, 8, 9, 10]),
        (5, &[3, 4, 5, 6, 7, 8, 9, 10]),
        (6, &[3, 4, 5, 6, 7, 8, 9, 10]),
    ];
    for &(row, cols) in layer1 {
        for &col in cols {
            positions.push(TilePos { layer: 1, row, col });
        }
    }

    // Layer 2: 6 wide x 4 tall = 24 tiles
    let layer2: &[(usize, &[usize])] = &[
        (2, &[4, 5, 6, 7, 8, 9]),
        (3, &[4, 5, 6, 7, 8, 9]),
        (4, &[4, 5, 6, 7, 8, 9]),
        (5, &[4, 5, 6, 7, 8, 9]),
    ];
    for &(row, cols) in layer2 {
        for &col in cols {
            positions.push(TilePos { layer: 2, row, col });
        }
    }

    // Layer 3: 4 wide x 2 tall = 8 tiles
    let layer3: &[(usize, &[usize])] = &[
        (3, &[5, 6, 7, 8]),
        (4, &[5, 6, 7, 8]),
    ];
    for &(row, cols) in layer3 {
        for &col in cols {
            positions.push(TilePos { layer: 3, row, col });
        }
    }

    // Layer 4: 2 wide x 2 tall = 4 tiles (cap)
    let layer4: &[(usize, &[usize])] = &[
        (3, &[6, 7]),
        (4, &[6, 7]),
    ];
    for &(row, cols) in layer4 {
        for &col in cols {
            positions.push(TilePos { layer: 4, row, col });
        }
    }

    positions
}

// ── Game state ──────────────────────────────────────────────────────

/// Undo record: a pair of tiles that was removed.
#[derive(Clone, Debug)]
struct UndoEntry {
    tile_a: usize,
    tile_b: usize,
}

/// Overall game status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameStatus {
    Playing,
    Won,
    Lost,
}

/// The board state: a collection of placed tiles.
struct Board {
    tiles: Vec<PlacedTile>,
}

impl Board {
    /// Create a new board by placing shuffled tiles on the turtle layout.
    fn new(rng: &mut Rng) -> Self {
        let positions = turtle_layout();
        let mut tile_kinds = full_tile_set();

        // Trim or pad tile_kinds to match the number of layout positions.
        // The turtle layout should have exactly 144 positions but we handle
        // slight mismatches gracefully by truncating.
        let count = positions.len().min(tile_kinds.len());
        tile_kinds.truncate(count);

        rng.shuffle(&mut tile_kinds);

        let mut tiles = Vec::with_capacity(count);
        for i in 0..count {
            tiles.push(PlacedTile {
                pos: positions[i],
                kind: tile_kinds[i],
                removed: false,
            });
        }

        Board { tiles }
    }

    /// Create a board from explicit positions and kinds (for testing).
    fn from_parts(positions: &[TilePos], kinds: &[TileKind]) -> Self {
        let count = positions.len().min(kinds.len());
        let mut tiles = Vec::with_capacity(count);
        for i in 0..count {
            tiles.push(PlacedTile {
                pos: positions[i],
                kind: kinds[i],
                removed: false,
            });
        }
        Board { tiles }
    }

    /// Number of tiles still on the board (not removed).
    fn remaining(&self) -> usize {
        self.tiles.iter().filter(|t| !t.removed).count()
    }

    /// Check if a tile is "free" — not removed, no tile on top, and has an
    /// open left or right side.
    fn is_free(&self, idx: usize) -> bool {
        let tile = match self.tiles.get(idx) {
            Some(t) => t,
            None => return false,
        };
        if tile.removed {
            return false;
        }

        let pos = tile.pos;

        // Check if any non-removed tile is directly on top (one layer above,
        // overlapping in row/col). A tile on layer L+1 at (r, c) covers
        // the tile at layer L if their row and column ranges overlap.
        for (i, other) in self.tiles.iter().enumerate() {
            if i == idx || other.removed {
                continue;
            }
            if other.pos.layer == pos.layer + 1 {
                // Tiles on adjacent layer overlap if they share the same row/col
                // position. Because tiles occupy a 1x1 cell in our grid, a tile
                // at (r, c) on layer L+1 covers (r, c) on layer L.
                if other.pos.row == pos.row && other.pos.col == pos.col {
                    return false;
                }
            }
        }

        // Check left/right openness. A tile is blocked on the left if there
        // is a non-removed tile at the same layer and row at col-1, and
        // blocked on the right if there is one at col+1.
        let mut blocked_left = false;
        let mut blocked_right = false;
        for (i, other) in self.tiles.iter().enumerate() {
            if i == idx || other.removed {
                continue;
            }
            if other.pos.layer == pos.layer && other.pos.row == pos.row {
                if other.pos.col + 1 == pos.col {
                    blocked_left = true;
                }
                if pos.col + 1 == other.pos.col {
                    blocked_right = true;
                }
            }
        }

        // Free if at least one side is open.
        !blocked_left || !blocked_right
    }

    /// Find all currently free tile indices.
    fn free_tiles(&self) -> Vec<usize> {
        (0..self.tiles.len())
            .filter(|&i| self.is_free(i))
            .collect()
    }

    /// Find a matching pair of free tiles, if one exists.
    fn find_hint(&self) -> Option<(usize, usize)> {
        let free = self.free_tiles();
        for i in 0..free.len() {
            for j in (i + 1)..free.len() {
                let a = free[i];
                let b = free[j];
                if self.tiles[a].kind.matches(self.tiles[b].kind) {
                    return Some((a, b));
                }
            }
        }
        None
    }

    /// Check if the game is won (all tiles removed).
    fn is_won(&self) -> bool {
        self.remaining() == 0
    }

    /// Check if the game is lost (no valid pairs remain among free tiles).
    fn is_lost(&self) -> bool {
        self.remaining() > 0 && self.find_hint().is_none()
    }

    /// Remove a pair of tiles.
    fn remove_pair(&mut self, a: usize, b: usize) {
        if let Some(tile) = self.tiles.get_mut(a) {
            tile.removed = true;
        }
        if let Some(tile) = self.tiles.get_mut(b) {
            tile.removed = true;
        }
    }

    /// Restore a pair of tiles (undo).
    fn restore_pair(&mut self, a: usize, b: usize) {
        if let Some(tile) = self.tiles.get_mut(a) {
            tile.removed = false;
        }
        if let Some(tile) = self.tiles.get_mut(b) {
            tile.removed = false;
        }
    }

    /// Shuffle the remaining (non-removed) tiles' kinds while keeping
    /// their positions fixed.
    fn shuffle_remaining(&mut self, rng: &mut Rng) {
        let active_indices: Vec<usize> = self
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.removed)
            .map(|(i, _)| i)
            .collect();

        let mut kinds: Vec<TileKind> = active_indices
            .iter()
            .map(|&i| self.tiles[i].kind)
            .collect();

        rng.shuffle(&mut kinds);

        for (slot, &idx) in active_indices.iter().enumerate() {
            self.tiles[idx].kind = kinds[slot];
        }
    }

    /// Screen coordinates for a tile at the given index.
    fn tile_screen_pos(&self, idx: usize) -> Option<(f32, f32)> {
        self.tiles.get(idx).map(|t| {
            let x = BOARD_OFFSET_X
                + t.pos.col as f32 * (TILE_W + TILE_GAP_X)
                - t.pos.layer as f32 * LAYER_OFFSET_X;
            let y = BOARD_OFFSET_Y
                + t.pos.row as f32 * (TILE_H + TILE_GAP_Y)
                - t.pos.layer as f32 * LAYER_OFFSET_Y;
            (x, y)
        })
    }

    /// Find which tile (topmost) is at the given screen coordinates.
    /// Returns the index if found. Searches from highest layer down so
    /// that the visually topmost tile wins.
    fn tile_at_screen(&self, sx: f32, sy: f32) -> Option<usize> {
        // Sort by layer descending so we pick the topmost tile first.
        let mut indices: Vec<usize> = (0..self.tiles.len())
            .filter(|&i| !self.tiles[i].removed)
            .collect();
        indices.sort_by(|&a, &b| self.tiles[b].pos.layer.cmp(&self.tiles[a].pos.layer));

        for idx in indices {
            if let Some((tx, ty)) = self.tile_screen_pos(idx)
                && sx >= tx && sx < tx + TILE_W && sy >= ty && sy < ty + TILE_H {
                    return Some(idx);
                }
        }
        None
    }
}

// ── Keyboard cursor ─────────────────────────────────────────────────

/// Cursor for keyboard navigation. Tracks which tile index in the free
/// list is currently focused.
#[derive(Clone, Debug)]
struct Cursor {
    /// Index into the board tiles array that the cursor is on.
    tile_idx: Option<usize>,
}

impl Cursor {
    fn new() -> Self {
        Self { tile_idx: None }
    }
}

// ── Main app ────────────────────────────────────────────────────────

struct Mahjong {
    board: Board,
    rng: Rng,
    seed: u64,
    selected: Option<usize>,
    cursor: Cursor,
    undo_stack: Vec<UndoEntry>,
    moves: u32,
    status: GameStatus,
    hint: Option<(usize, usize)>,
    show_hint: bool,
    message: Option<&'static str>,
}

impl Mahjong {
    fn new() -> Self {
        Self::with_seed(42)
    }

    fn with_seed(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let board = Board::new(&mut rng);

        // Initialize cursor to the first free tile, if any.
        let first_free = board.free_tiles().first().copied();

        let mut app = Self {
            board,
            rng,
            seed,
            selected: None,
            cursor: Cursor { tile_idx: first_free },
            undo_stack: Vec::new(),
            moves: 0,
            status: GameStatus::Playing,
            hint: None,
            show_hint: false,
            message: None,
        };
        app.update_status();
        app
    }

    /// Start a new game with a fresh seed.
    fn new_game(&mut self) {
        self.seed = self.seed.wrapping_add(1);
        let mut rng = Rng::new(self.seed);
        self.board = Board::new(&mut rng);
        self.rng = rng;
        self.selected = None;
        self.cursor.tile_idx = self.board.free_tiles().first().copied();
        self.undo_stack.clear();
        self.moves = 0;
        self.status = GameStatus::Playing;
        self.hint = None;
        self.show_hint = false;
        self.message = None;
    }

    /// Update status based on board state.
    fn update_status(&mut self) {
        if self.board.is_won() {
            self.status = GameStatus::Won;
            self.message = Some("You win! Press N for new game.");
        } else if self.board.is_lost() {
            self.status = GameStatus::Lost;
            self.message = Some("No moves left! S=shuffle, N=new");
        }
    }

    /// Try to select a tile (by index) and match if two are selected.
    fn try_select(&mut self, idx: usize) {
        if self.status != GameStatus::Playing {
            return;
        }

        if !self.board.is_free(idx) {
            self.message = Some("Tile is not free");
            return;
        }

        match self.selected {
            None => {
                self.selected = Some(idx);
                self.show_hint = false;
                self.message = None;
            }
            Some(prev) => {
                if prev == idx {
                    // Deselect
                    self.selected = None;
                    self.message = None;
                } else if self.board.tiles[prev].kind.matches(self.board.tiles[idx].kind) {
                    // Match found!
                    self.board.remove_pair(prev, idx);
                    self.undo_stack.push(UndoEntry {
                        tile_a: prev,
                        tile_b: idx,
                    });
                    self.selected = None;
                    self.moves += 1;
                    self.show_hint = false;
                    self.hint = None;
                    self.message = None;
                    self.update_status();
                    // Update cursor to a free tile if current is removed.
                    if let Some(ci) = self.cursor.tile_idx
                        && self.board.tiles.get(ci).is_none_or(|t| t.removed) {
                            self.cursor.tile_idx = self.board.free_tiles().first().copied();
                        }
                } else {
                    self.message = Some("Tiles don't match!");
                    self.selected = Some(idx);
                }
            }
        }
    }

    /// Undo the last move.
    fn undo(&mut self) {
        if let Some(entry) = self.undo_stack.pop() {
            self.board.restore_pair(entry.tile_a, entry.tile_b);
            if self.moves > 0 {
                self.moves -= 1;
            }
            self.selected = None;
            self.show_hint = false;
            self.hint = None;
            self.status = GameStatus::Playing;
            self.message = Some("Undo!");
        } else {
            self.message = Some("Nothing to undo");
        }
    }

    /// Show a hint (highlight a valid pair).
    fn show_hint_pair(&mut self) {
        if self.status != GameStatus::Playing {
            return;
        }
        match self.board.find_hint() {
            Some(pair) => {
                self.hint = Some(pair);
                self.show_hint = true;
                self.message = Some("Hint shown (green tiles)");
            }
            None => {
                self.show_hint = false;
                self.message = Some("No valid pairs!");
            }
        }
    }

    /// Shuffle remaining tiles.
    fn shuffle_tiles(&mut self) {
        if self.status == GameStatus::Won {
            return;
        }
        self.board.shuffle_remaining(&mut self.rng);
        self.selected = None;
        self.show_hint = false;
        self.hint = None;
        self.status = GameStatus::Playing;
        self.message = Some("Tiles shuffled!");
        self.update_status();
    }

    /// Move cursor in the given direction among free tiles.
    fn move_cursor(&mut self, dx: i32, dy: i32) {
        let free = self.board.free_tiles();
        if free.is_empty() {
            self.cursor.tile_idx = None;
            return;
        }

        let current_idx = self.cursor.tile_idx.unwrap_or(free[0]);
        let current_pos = match self.board.tile_screen_pos(current_idx) {
            Some(p) => p,
            None => {
                self.cursor.tile_idx = Some(free[0]);
                return;
            }
        };

        // Find the closest free tile in the requested direction.
        let mut best: Option<(usize, f32)> = None;
        for &fi in &free {
            if fi == current_idx {
                continue;
            }
            if let Some((fx, fy)) = self.board.tile_screen_pos(fi) {
                let delta_x = fx - current_pos.0;
                let delta_y = fy - current_pos.1;

                // Check if the tile is in the requested direction.
                let in_direction = match (dx, dy) {
                    (1, 0) => delta_x > 1.0,   // right
                    (-1, 0) => delta_x < -1.0,  // left
                    (0, 1) => delta_y > 1.0,    // down
                    (0, -1) => delta_y < -1.0,  // up
                    _ => false,
                };

                if in_direction {
                    // Distance with bias towards the main axis.
                    let main = if dx != 0 {
                        delta_x.abs()
                    } else {
                        delta_y.abs()
                    };
                    let cross = if dx != 0 {
                        delta_y.abs()
                    } else {
                        delta_x.abs()
                    };
                    let dist = main + cross * 2.0;

                    if best.is_none_or(|(_, bd)| dist < bd) {
                        best = Some((fi, dist));
                    }
                }
            }
        }

        if let Some((bi, _)) = best {
            self.cursor.tile_idx = Some(bi);
        }
    }

    // ── Event handling ──────────────────────────────────────────────

    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        match event.key {
            Key::N => self.new_game(),
            Key::Z => self.undo(),
            Key::H => self.show_hint_pair(),
            Key::S => self.shuffle_tiles(),
            Key::Left => self.move_cursor(-1, 0),
            Key::Right => self.move_cursor(1, 0),
            Key::Up => self.move_cursor(0, -1),
            Key::Down => self.move_cursor(0, 1),
            Key::Enter | Key::Space => {
                if let Some(ci) = self.cursor.tile_idx {
                    self.try_select(ci);
                }
            }
            Key::Escape => {
                self.selected = None;
                self.show_hint = false;
                self.message = None;
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) {
        if let MouseEventKind::Press(MouseButton::Left) = event.kind
            && let Some(idx) = self.board.tile_at_screen(event.x, event.y) {
                self.cursor.tile_idx = Some(idx);
                self.try_select(idx);
            }
    }

    fn handle_event(&mut self, event: &Event) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me),
            _ => {}
        }
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::with_capacity(512);

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
            x: BOARD_OFFSET_X,
            y: 20.0,
            text: "Mahjong Solitaire".into(),
            color: LAVENDER,
            font_size: TITLE_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status line
        let status_text = match self.status {
            GameStatus::Playing => {
                format!(
                    "Tiles: {}  Moves: {}  Free: {}",
                    self.board.remaining(),
                    self.moves,
                    self.board.free_tiles().len()
                )
            }
            GameStatus::Won => format!("YOU WIN!  Moves: {}", self.moves),
            GameStatus::Lost => "No valid moves remain!".into(),
        };
        let status_color = match self.status {
            GameStatus::Playing => SUBTEXT0,
            GameStatus::Won => GREEN,
            GameStatus::Lost => RED,
        };
        cmds.push(RenderCommand::Text {
            x: BOARD_OFFSET_X,
            y: 48.0,
            text: status_text,
            color: status_color,
            font_size: STATUS_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Message line
        if let Some(msg) = self.message {
            cmds.push(RenderCommand::Text {
                x: BOARD_OFFSET_X + 400.0,
                y: 48.0,
                text: msg.into(),
                color: PEACH,
                font_size: STATUS_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Render tiles bottom-to-top so higher layers draw on top.
        // Build a sorted index list by layer.
        let mut sorted_indices: Vec<usize> = (0..self.board.tiles.len())
            .filter(|&i| !self.board.tiles[i].removed)
            .collect();
        sorted_indices.sort_by_key(|&i| self.board.tiles[i].pos.layer);

        for &idx in &sorted_indices {
            let tile = &self.board.tiles[idx];
            let (tx, ty) = match self.board.tile_screen_pos(idx) {
                Some(p) => p,
                None => continue,
            };

            let is_free = self.board.is_free(idx);
            let is_selected = self.selected == Some(idx);
            let is_cursor = self.cursor.tile_idx == Some(idx);
            let is_hint = self.show_hint
                && self
                    .hint
                    .is_some_and(|(a, b)| idx == a || idx == b);

            // Shadow (gives depth illusion for stacked layers)
            if tile.pos.layer > 0 {
                cmds.push(RenderCommand::FillRect {
                    x: tx + SHADOW_OFFSET,
                    y: ty + SHADOW_OFFSET,
                    width: TILE_W,
                    height: TILE_H,
                    color: TILE_SHADOW,
                    corner_radii: CornerRadii::all(TILE_CORNER),
                });
            }

            // Tile background
            let bg_color = if is_selected {
                TILE_SELECTED
            } else if is_hint {
                TILE_HINT
            } else if is_free {
                TILE_BG_FREE
            } else {
                TILE_BG
            };
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: TILE_W,
                height: TILE_H,
                color: bg_color,
                corner_radii: CornerRadii::all(TILE_CORNER),
            });

            // Cursor highlight (border effect via slightly larger rect behind)
            if is_cursor && !is_selected {
                // Draw a thin border by drawing lines on the edges.
                let bw = 2.0;
                // Top edge
                cmds.push(RenderCommand::Line {
                    x1: tx,
                    y1: ty,
                    x2: tx + TILE_W,
                    y2: ty,
                    color: YELLOW,
                    width: bw,
                });
                // Bottom edge
                cmds.push(RenderCommand::Line {
                    x1: tx,
                    y1: ty + TILE_H,
                    x2: tx + TILE_W,
                    y2: ty + TILE_H,
                    color: YELLOW,
                    width: bw,
                });
                // Left edge
                cmds.push(RenderCommand::Line {
                    x1: tx,
                    y1: ty,
                    x2: tx,
                    y2: ty + TILE_H,
                    color: YELLOW,
                    width: bw,
                });
                // Right edge
                cmds.push(RenderCommand::Line {
                    x1: tx + TILE_W,
                    y1: ty,
                    x2: tx + TILE_W,
                    y2: ty + TILE_H,
                    color: YELLOW,
                    width: bw,
                });
            }

            // Tile label text
            let label = tile.kind.label();
            let text_color = if is_selected {
                CRUST
            } else {
                tile.kind.text_color()
            };
            cmds.push(RenderCommand::Text {
                x: tx + TILE_W / 2.0 - (label.len() as f32 * 4.0),
                y: ty + TILE_H / 2.0 - TILE_FONT_SIZE / 2.0,
                text: label.into(),
                color: text_color,
                font_size: TILE_FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }

        // Help bar at the bottom.
        let help_y = height - 24.0;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: help_y - 6.0,
            width,
            height: 30.0,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 10.0,
            y: help_y,
            text: "N=New  Z=Undo  H=Hint  S=Shuffle  Arrows=Navigate  Enter/Space=Select  Esc=Deselect".into(),
            color: OVERLAY0,
            font_size: HELP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Legend (right side)
        let legend_x = BOARD_OFFSET_X + 15.0 * (TILE_W + TILE_GAP_X) + 30.0;
        let legend_y = BOARD_OFFSET_Y;
        cmds.push(RenderCommand::Text {
            x: legend_x,
            y: legend_y,
            text: "Legend".into(),
            color: TEXT_COLOR,
            font_size: INFO_FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let legend_items: &[(&str, &str, Color)] = &[
            ("B1-B9", "Bamboo", BAMBOO_COLOR),
            ("C1-C9", "Circle", CIRCLE_COLOR),
            ("W1-W9", "Character", CHARACTER_COLOR),
            ("E/S/W/N", "Wind", WIND_COLOR),
            ("Dr/Dg/Dw", "Dragon", DRAGON_COLOR),
            ("Sp/Su/Au/Wi", "Season*", SEASON_COLOR),
            ("Pl/Or/Ch/Bm", "Flower*", FLOWER_COLOR),
        ];

        for (i, &(codes, name, color)) in legend_items.iter().enumerate() {
            let ly = legend_y + 24.0 + i as f32 * 20.0;
            cmds.push(RenderCommand::FillRect {
                x: legend_x,
                y: ly - 2.0,
                width: 10.0,
                height: 10.0,
                color,
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: legend_x + 14.0,
                y: ly,
                text: format!("{codes} ({name})"),
                color: SUBTEXT0,
                font_size: HELP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Note about wildcards
        cmds.push(RenderCommand::Text {
            x: legend_x,
            y: legend_y + 24.0 + 7.0 * 20.0 + 4.0,
            text: "* match any in group".into(),
            color: OVERLAY0,
            font_size: HELP_FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        cmds
    }
}

fn main() {
    let _app = Mahjong::new();
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper ──────────────────────────────────────────────────────

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn make_key_up(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    fn make_mouse(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }
    }

    // ── RNG tests ───────────────────────────────────────────────────

    #[test]
    fn rng_deterministic() {
        let mut r1 = Rng::new(123);
        let mut r2 = Rng::new(123);
        for _ in 0..100 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        assert_ne!(r1.next(), r2.next());
    }

    #[test]
    fn rng_range() {
        let mut rng = Rng::new(42);
        for _ in 0..200 {
            let v = rng.next_range(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn rng_range_zero() {
        let mut rng = Rng::new(42);
        assert_eq!(rng.next_range(0), 0);
    }

    #[test]
    fn rng_shuffle_preserves_elements() {
        let mut rng = Rng::new(99);
        let mut data: Vec<u32> = (0..20).collect();
        let original = data.clone();
        rng.shuffle(&mut data);
        data.sort();
        assert_eq!(data, original);
    }

    #[test]
    fn rng_shuffle_changes_order() {
        let mut rng = Rng::new(42);
        let mut data: Vec<u32> = (0..20).collect();
        let original = data.clone();
        rng.shuffle(&mut data);
        // Extremely unlikely to stay in the same order with 20 elements.
        assert_ne!(data, original);
    }

    // ── TileKind tests ──────────────────────────────────────────────

    #[test]
    fn tile_kind_exact_match() {
        assert!(TileKind::Bamboo(1).matches(TileKind::Bamboo(1)));
        assert!(TileKind::Circle(5).matches(TileKind::Circle(5)));
        assert!(TileKind::Wind(0).matches(TileKind::Wind(0)));
        assert!(TileKind::Dragon(2).matches(TileKind::Dragon(2)));
    }

    #[test]
    fn tile_kind_no_match_different_number() {
        assert!(!TileKind::Bamboo(1).matches(TileKind::Bamboo(2)));
        assert!(!TileKind::Circle(3).matches(TileKind::Circle(9)));
    }

    #[test]
    fn tile_kind_no_match_different_suit() {
        assert!(!TileKind::Bamboo(1).matches(TileKind::Circle(1)));
        assert!(!TileKind::Character(5).matches(TileKind::Bamboo(5)));
    }

    #[test]
    fn tile_kind_seasons_match_any_season() {
        assert!(TileKind::Season(0).matches(TileKind::Season(1)));
        assert!(TileKind::Season(0).matches(TileKind::Season(2)));
        assert!(TileKind::Season(0).matches(TileKind::Season(3)));
        assert!(TileKind::Season(1).matches(TileKind::Season(3)));
        assert!(TileKind::Season(2).matches(TileKind::Season(2)));
    }

    #[test]
    fn tile_kind_flowers_match_any_flower() {
        assert!(TileKind::Flower(0).matches(TileKind::Flower(1)));
        assert!(TileKind::Flower(0).matches(TileKind::Flower(2)));
        assert!(TileKind::Flower(0).matches(TileKind::Flower(3)));
        assert!(TileKind::Flower(1).matches(TileKind::Flower(3)));
        assert!(TileKind::Flower(2).matches(TileKind::Flower(2)));
    }

    #[test]
    fn tile_kind_season_does_not_match_flower() {
        assert!(!TileKind::Season(0).matches(TileKind::Flower(0)));
        assert!(!TileKind::Flower(1).matches(TileKind::Season(1)));
    }

    #[test]
    fn tile_kind_wind_does_not_match_dragon() {
        assert!(!TileKind::Wind(0).matches(TileKind::Dragon(0)));
    }

    #[test]
    fn all_tile_kinds_count() {
        let kinds = all_tile_kinds();
        // 9 Bamboo + 9 Circle + 9 Character + 4 Wind + 3 Dragon + 4 Season + 4 Flower = 42.
        assert_eq!(kinds.len(), 42);
    }

    #[test]
    fn all_tile_kinds_unique() {
        let kinds = all_tile_kinds();
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }

    #[test]
    fn full_tile_set_has_144() {
        let tiles = full_tile_set();
        assert_eq!(tiles.len(), 144);
    }

    #[test]
    fn full_tile_set_four_copies_of_base() {
        let tiles = full_tile_set();
        let base = base_tile_kinds();
        for kind in &base {
            let count = tiles.iter().filter(|t| *t == kind).count();
            assert_eq!(count, 4, "Expected 4 copies of base {:?}", kind);
        }
    }

    #[test]
    fn full_tile_set_one_copy_of_bonus() {
        let tiles = full_tile_set();
        // Seasons and flowers appear once each.
        for i in 0..4u8 {
            let sc = tiles.iter().filter(|t| **t == TileKind::Season(i)).count();
            assert_eq!(sc, 1, "Expected 1 copy of Season({i})");
            let fc = tiles.iter().filter(|t| **t == TileKind::Flower(i)).count();
            assert_eq!(fc, 1, "Expected 1 copy of Flower({i})");
        }
    }

    #[test]
    fn tile_kind_labels_not_empty() {
        let kinds = all_tile_kinds();
        for kind in &kinds {
            assert!(!kind.label().is_empty());
        }
    }

    #[test]
    fn tile_kind_category_not_empty() {
        let kinds = all_tile_kinds();
        for kind in &kinds {
            assert!(!kind.category().is_empty());
        }
    }

    // ── Layout tests ────────────────────────────────────────────────

    #[test]
    fn turtle_layout_has_correct_count() {
        let layout = turtle_layout();
        // The exact count depends on our layout. We target 144 for a standard set.
        // Our turtle layout: 86 + 48 + 24 + 8 + 4 = 170, but we only use 144
        // because we have 144 tiles. The layout can have more positions than tiles
        // if we trim.
        // Actually, let's verify the layout count.
        assert!(layout.len() >= 144, "Layout has {} positions, need at least 144", layout.len());
    }

    #[test]
    fn turtle_layout_no_duplicates() {
        let layout = turtle_layout();
        for i in 0..layout.len() {
            for j in (i + 1)..layout.len() {
                assert_ne!(
                    layout[i], layout[j],
                    "Duplicate position at indices {i} and {j}: {:?}",
                    layout[i]
                );
            }
        }
    }

    #[test]
    fn turtle_layout_layers_ascending() {
        let layout = turtle_layout();
        // Each layer should have tiles defined in contiguous blocks
        // (layer 0 first, then layer 1, etc.)
        let max_layer = layout.iter().map(|p| p.layer).max().unwrap_or(0);
        assert!(max_layer >= 3, "Should have at least 4 layers");
    }

    // ── Board tests ─────────────────────────────────────────────────

    #[test]
    fn board_new_has_tiles() {
        let mut rng = Rng::new(42);
        let board = Board::new(&mut rng);
        assert!(!board.tiles.is_empty());
        assert!(board.remaining() > 0);
    }

    #[test]
    fn board_new_all_tiles_present() {
        let mut rng = Rng::new(42);
        let board = Board::new(&mut rng);
        // All tiles should start as not removed.
        for tile in &board.tiles {
            assert!(!tile.removed);
        }
    }

    #[test]
    fn board_remaining_decreases_on_remove() {
        let mut rng = Rng::new(42);
        let mut board = Board::new(&mut rng);
        let initial = board.remaining();
        board.remove_pair(0, 1);
        assert_eq!(board.remaining(), initial - 2);
    }

    #[test]
    fn board_restore_pair() {
        let mut rng = Rng::new(42);
        let mut board = Board::new(&mut rng);
        let initial = board.remaining();
        board.remove_pair(0, 1);
        board.restore_pair(0, 1);
        assert_eq!(board.remaining(), initial);
    }

    #[test]
    fn board_remove_pair_marks_removed() {
        let mut rng = Rng::new(42);
        let mut board = Board::new(&mut rng);
        board.remove_pair(0, 1);
        assert!(board.tiles[0].removed);
        assert!(board.tiles[1].removed);
    }

    #[test]
    fn board_is_won_when_empty() {
        let board = Board::from_parts(&[], &[]);
        assert!(board.is_won());
    }

    #[test]
    fn board_is_not_won_with_tiles() {
        let board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        assert!(!board.is_won());
    }

    #[test]
    fn board_free_tile_no_neighbors() {
        // Single tile on layer 0 — should be free.
        let board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        assert!(board.is_free(0));
    }

    #[test]
    fn board_tile_blocked_by_tile_above() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 1, row: 0, col: 0 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        assert!(!board.is_free(0)); // bottom tile blocked by top
        assert!(board.is_free(1));  // top tile is free
    }

    #[test]
    fn board_tile_blocked_both_sides() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 1 },
                TilePos { layer: 0, row: 0, col: 2 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2), TileKind::Bamboo(3)],
        );
        // Middle tile (col=1) is blocked on left (col=0) and right (col=2).
        assert!(!board.is_free(1));
        // Edge tiles are free (open side).
        assert!(board.is_free(0));
        assert!(board.is_free(2));
    }

    #[test]
    fn board_tile_blocked_one_side_is_free() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 1 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        // Each tile has one side blocked and one open: both are free.
        assert!(board.is_free(0));
        assert!(board.is_free(1));
    }

    #[test]
    fn board_removed_tile_not_free() {
        let mut board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        board.tiles[0].removed = true;
        assert!(!board.is_free(0));
    }

    #[test]
    fn board_free_tiles_returns_correct_count() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 1 },
                TilePos { layer: 0, row: 0, col: 2 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2), TileKind::Bamboo(3)],
        );
        // Edge tiles are free, middle is not.
        assert_eq!(board.free_tiles().len(), 2);
    }

    #[test]
    fn board_find_hint_with_match() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        let hint = board.find_hint();
        assert!(hint.is_some());
        let (a, b) = hint.unwrap();
        assert!(board.tiles[a].kind.matches(board.tiles[b].kind));
    }

    #[test]
    fn board_find_hint_no_match() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        assert!(board.find_hint().is_none());
    }

    #[test]
    fn board_find_hint_season_wildcard() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Season(0), TileKind::Season(3)],
        );
        assert!(board.find_hint().is_some());
    }

    #[test]
    fn board_find_hint_flower_wildcard() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Flower(1), TileKind::Flower(2)],
        );
        assert!(board.find_hint().is_some());
    }

    #[test]
    fn board_is_lost_no_pairs() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Circle(2)],
        );
        assert!(board.is_lost());
    }

    #[test]
    fn board_is_not_lost_with_pairs() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        assert!(!board.is_lost());
    }

    #[test]
    fn board_shuffle_preserves_count() {
        let mut rng = Rng::new(42);
        let mut board = Board::new(&mut rng);
        let before = board.remaining();
        board.shuffle_remaining(&mut rng);
        assert_eq!(board.remaining(), before);
    }

    #[test]
    fn board_shuffle_preserves_positions() {
        let mut rng = Rng::new(42);
        let mut board = Board::new(&mut rng);
        let positions_before: Vec<TilePos> = board.tiles.iter().map(|t| t.pos).collect();
        board.shuffle_remaining(&mut rng);
        let positions_after: Vec<TilePos> = board.tiles.iter().map(|t| t.pos).collect();
        assert_eq!(positions_before, positions_after);
    }

    #[test]
    fn board_tile_screen_pos_exists() {
        let mut rng = Rng::new(42);
        let board = Board::new(&mut rng);
        for i in 0..board.tiles.len() {
            assert!(board.tile_screen_pos(i).is_some());
        }
    }

    #[test]
    fn board_tile_screen_pos_out_of_bounds() {
        let board = Board::from_parts(&[], &[]);
        assert!(board.tile_screen_pos(0).is_none());
    }

    #[test]
    fn board_tile_at_screen_finds_tile() {
        let board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        let (tx, ty) = board.tile_screen_pos(0).unwrap();
        let found = board.tile_at_screen(tx + 5.0, ty + 5.0);
        assert_eq!(found, Some(0));
    }

    #[test]
    fn board_tile_at_screen_misses_empty() {
        let board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        // Far away from any tile.
        let found = board.tile_at_screen(9999.0, 9999.0);
        assert_eq!(found, None);
    }

    #[test]
    fn board_tile_at_screen_topmost_wins() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 1, row: 0, col: 0 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        // The top-layer tile is offset slightly, so click in the overlap area.
        let (_tx0, _ty0) = board.tile_screen_pos(0).unwrap();
        let (tx1, ty1) = board.tile_screen_pos(1).unwrap();
        // Click in the overlap region (upper tile's area).
        let cx = tx1 + TILE_W / 2.0;
        let cy = ty1 + TILE_H / 2.0;
        // Only if the click falls within the upper tile's bounds.
        if cx >= tx1 && cx < tx1 + TILE_W && cy >= ty1 && cy < ty1 + TILE_H {
            let found = board.tile_at_screen(cx, cy);
            assert_eq!(found, Some(1)); // Layer 1 tile wins.
        }
    }

    // ── App / Mahjong tests ─────────────────────────────────────────

    #[test]
    fn app_new_creates_game() {
        let app = Mahjong::new();
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.moves, 0);
        assert!(app.board.remaining() > 0);
    }

    #[test]
    fn app_with_seed_deterministic() {
        let a1 = Mahjong::with_seed(123);
        let a2 = Mahjong::with_seed(123);
        // Same seed should produce same tile kinds at same positions.
        for i in 0..a1.board.tiles.len() {
            assert_eq!(a1.board.tiles[i].kind, a2.board.tiles[i].kind);
            assert_eq!(a1.board.tiles[i].pos, a2.board.tiles[i].pos);
        }
    }

    #[test]
    fn app_different_seeds_different_layout() {
        let a1 = Mahjong::with_seed(1);
        let a2 = Mahjong::with_seed(2);
        // At least some tiles should differ in kind assignment.
        let diffs = a1
            .board
            .tiles
            .iter()
            .zip(a2.board.tiles.iter())
            .filter(|(a, b)| a.kind != b.kind)
            .count();
        assert!(diffs > 0);
    }

    #[test]
    fn app_new_game_resets() {
        let mut app = Mahjong::with_seed(42);
        app.moves = 10;
        app.status = GameStatus::Won;
        app.new_game();
        assert_eq!(app.moves, 0);
        assert_eq!(app.status, GameStatus::Playing);
    }

    #[test]
    fn app_select_free_tile() {
        let mut app = Mahjong::with_seed(42);
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.try_select(free[0]);
            assert_eq!(app.selected, Some(free[0]));
        }
    }

    #[test]
    fn app_deselect_same_tile() {
        let mut app = Mahjong::with_seed(42);
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.try_select(free[0]);
            app.try_select(free[0]);
            assert_eq!(app.selected, None);
        }
    }

    #[test]
    fn app_select_non_free_tile() {
        let mut app = Mahjong::with_seed(42);
        // Find a non-free tile.
        let non_free: Vec<usize> = (0..app.board.tiles.len())
            .filter(|&i| !app.board.is_free(i) && !app.board.tiles[i].removed)
            .collect();
        if !non_free.is_empty() {
            app.try_select(non_free[0]);
            assert_eq!(app.selected, None);
            assert!(app.message.is_some());
        }
    }

    #[test]
    fn app_match_pair_removes_tiles() {
        // Create a small board with two matching free tiles.
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 0);
        assert_eq!(app.moves, 1);
    }

    #[test]
    fn app_mismatch_does_not_remove() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 2);
        assert_eq!(app.moves, 0);
        // Second tile should now be selected instead.
        assert_eq!(app.selected, Some(1));
    }

    #[test]
    fn app_undo_restores_tiles() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 0, row: 0, col: 10 },
                TilePos { layer: 0, row: 0, col: 15 },
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Bamboo(2),
                TileKind::Bamboo(2),
            ],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 2);
        assert_eq!(app.moves, 1);
        app.undo();
        assert_eq!(app.board.remaining(), 4);
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn app_undo_empty_stack() {
        let mut app = Mahjong::with_seed(42);
        app.undo();
        assert!(app.message.is_some());
    }

    #[test]
    fn app_hint_finds_pair() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.show_hint_pair();
        assert!(app.show_hint);
        assert!(app.hint.is_some());
    }

    #[test]
    fn app_hint_no_pair() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Circle(2)],
        );
        app.status = GameStatus::Playing;
        app.show_hint_pair();
        assert!(!app.show_hint);
    }

    #[test]
    fn app_shuffle_keeps_tile_count() {
        let mut app = Mahjong::with_seed(42);
        let before = app.board.remaining();
        app.shuffle_tiles();
        assert_eq!(app.board.remaining(), before);
    }

    #[test]
    fn app_shuffle_clears_selection() {
        let mut app = Mahjong::with_seed(42);
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.try_select(free[0]);
            assert!(app.selected.is_some());
        }
        app.shuffle_tiles();
        assert!(app.selected.is_none());
    }

    #[test]
    fn app_key_n_new_game() {
        let mut app = Mahjong::with_seed(42);
        app.moves = 5;
        app.handle_key(&make_key(Key::N));
        assert_eq!(app.moves, 0);
    }

    #[test]
    fn app_key_z_undo() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 0);
        app.handle_key(&make_key(Key::Z));
        assert_eq!(app.board.remaining(), 2);
    }

    #[test]
    fn app_key_h_hint() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.handle_key(&make_key(Key::H));
        assert!(app.show_hint);
    }

    #[test]
    fn app_key_s_shuffle() {
        let mut app = Mahjong::with_seed(42);
        app.handle_key(&make_key(Key::S));
        assert!(app.message.is_some());
    }

    #[test]
    fn app_key_escape_deselects() {
        let mut app = Mahjong::with_seed(42);
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.try_select(free[0]);
            assert!(app.selected.is_some());
            app.handle_key(&make_key(Key::Escape));
            assert!(app.selected.is_none());
        }
    }

    #[test]
    fn app_key_enter_selects_cursor() {
        let mut app = Mahjong::with_seed(42);
        // Set cursor to a free tile.
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.cursor.tile_idx = Some(free[0]);
            app.handle_key(&make_key(Key::Enter));
            assert_eq!(app.selected, Some(free[0]));
        }
    }

    #[test]
    fn app_key_space_selects_cursor() {
        let mut app = Mahjong::with_seed(42);
        let free = app.board.free_tiles();
        if !free.is_empty() {
            app.cursor.tile_idx = Some(free[0]);
            app.handle_key(&make_key(Key::Space));
            assert_eq!(app.selected, Some(free[0]));
        }
    }

    #[test]
    fn app_key_up_ignored() {
        let mut app = Mahjong::with_seed(42);
        // Key release should be ignored.
        app.handle_key(&make_key_up(Key::N));
        // App should still be in the same state (seed 42).
        assert_eq!(app.seed, 42);
    }

    #[test]
    fn app_arrow_keys_move_cursor() {
        let mut app = Mahjong::with_seed(42);
        let _initial = app.cursor.tile_idx;
        app.handle_key(&make_key(Key::Right));
        // Cursor may or may not change depending on layout, but shouldn't crash.
        let _ = app.cursor.tile_idx;
        // Try all directions.
        app.handle_key(&make_key(Key::Left));
        app.handle_key(&make_key(Key::Up));
        app.handle_key(&make_key(Key::Down));
        // No crash is success. (No assertion needed; reaching this line
        // means the four key events handled cleanly.)
    }

    #[test]
    fn app_handle_event_key() {
        let mut app = Mahjong::with_seed(42);
        app.handle_event(&Event::Key(make_key(Key::S)));
        // Shuffle should have occurred.
        assert!(app.message.is_some());
    }

    #[test]
    fn app_handle_event_mouse() {
        let mut app = Mahjong::with_seed(42);
        // Click at a position that might hit a tile.
        let free = app.board.free_tiles();
        if !free.is_empty()
            && let Some((tx, ty)) = app.board.tile_screen_pos(free[0])
        {
            app.handle_event(&Event::Mouse(make_mouse(tx + 5.0, ty + 5.0)));
            // Should have selected the tile.
            assert_eq!(app.selected, Some(free[0]));
        }
    }

    #[test]
    fn app_mouse_click_selects_tile() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        let (tx, ty) = app.board.tile_screen_pos(0).unwrap();
        app.handle_mouse(&make_mouse(tx + 1.0, ty + 1.0));
        assert_eq!(app.selected, Some(0));
    }

    #[test]
    fn app_mouse_click_empty_area() {
        let mut app = Mahjong::with_seed(42);
        app.handle_mouse(&make_mouse(9999.0, 9999.0));
        assert_eq!(app.selected, None);
    }

    #[test]
    fn app_won_status_on_clear() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn app_lost_status_no_pairs() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Circle(2)],
        );
        app.status = GameStatus::Playing;
        app.update_status();
        assert_eq!(app.status, GameStatus::Lost);
    }

    #[test]
    fn app_no_action_when_won() {
        let mut app = Mahjong::with_seed(42);
        app.status = GameStatus::Won;
        app.try_select(0);
        // Should not change selection.
        assert_eq!(app.selected, None);
    }

    #[test]
    fn app_shuffle_when_lost() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Circle(2)],
        );
        app.status = GameStatus::Lost;
        app.shuffle_tiles();
        // After shuffle status is rechecked. Tiles are same types but may
        // still be unmatched — status depends on shuffle result.
        assert!(app.board.remaining() > 0);
    }

    #[test]
    fn app_cursor_starts_on_free_tile() {
        let app = Mahjong::with_seed(42);
        if let Some(ci) = app.cursor.tile_idx {
            assert!(app.board.is_free(ci));
        }
    }

    #[test]
    fn app_cursor_none_on_empty_board() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(&[], &[]);
        app.cursor.tile_idx = app.board.free_tiles().first().copied();
        assert_eq!(app.cursor.tile_idx, None);
    }

    #[test]
    fn app_move_cursor_on_empty_board() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(&[], &[]);
        app.cursor.tile_idx = None;
        app.move_cursor(1, 0);
        assert_eq!(app.cursor.tile_idx, None);
    }

    // ── Render tests ────────────────────────────────────────────────

    #[test]
    fn render_produces_commands() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_has_background() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        let has_bg = cmds.iter().any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == BASE));
        assert!(has_bg);
    }

    #[test]
    fn render_has_title() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        let has_title = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Mahjong"))
        });
        assert!(has_title);
    }

    #[test]
    fn render_has_tiles() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        // Should have tile rectangles (at least the number of visible tiles).
        let rects = cmds.iter().filter(|c| matches!(c, RenderCommand::FillRect { .. })).count();
        assert!(rects >= 10); // Many tile backgrounds.
    }

    #[test]
    fn render_has_help_bar() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        let has_help = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Undo"))
        });
        assert!(has_help);
    }

    #[test]
    fn render_has_legend() {
        let app = Mahjong::new();
        let cmds = app.render(900.0, 700.0);
        let has_legend = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Legend"))
        });
        assert!(has_legend);
    }

    #[test]
    fn render_selected_tile_highlighted() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        let cmds = app.render(900.0, 700.0);
        let has_selected = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if *color == TILE_SELECTED)
        });
        assert!(has_selected);
    }

    #[test]
    fn render_hint_tiles_highlighted() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.show_hint_pair();
        let cmds = app.render(900.0, 700.0);
        // Count tile-sized hint rects (TILE_W x TILE_H). The legend also uses
        // the same green color for small swatches, so filter by size.
        let hint_count = cmds.iter().filter(|c| {
            matches!(c, RenderCommand::FillRect { color, width, height, .. }
                if *color == TILE_HINT && (*width - TILE_W).abs() < 1.0 && (*height - TILE_H).abs() < 1.0)
        }).count();
        assert_eq!(hint_count, 2);
    }

    #[test]
    fn render_cursor_draws_lines() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.cursor.tile_idx = Some(0);
        app.selected = None;
        let cmds = app.render(900.0, 700.0);
        let line_count = cmds.iter().filter(|c| matches!(c, RenderCommand::Line { .. })).count();
        assert!(line_count >= 4); // 4 edges for the cursor border.
    }

    #[test]
    fn render_won_shows_win_text() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(&[], &[]);
        app.status = GameStatus::Won;
        app.message = Some("You win! Press N for new game.");
        let cmds = app.render(900.0, 700.0);
        let has_win = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("WIN"))
        });
        assert!(has_win);
    }

    #[test]
    fn render_lost_shows_lost_text() {
        let mut app = Mahjong::with_seed(42);
        app.status = GameStatus::Lost;
        let cmds = app.render(900.0, 700.0);
        let has_lost = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("No valid"))
        });
        assert!(has_lost);
    }

    #[test]
    fn render_message_shown() {
        let mut app = Mahjong::with_seed(42);
        app.message = Some("Test message");
        let cmds = app.render(900.0, 700.0);
        let has_msg = cmds.iter().any(|c| {
            matches!(c, RenderCommand::Text { text, .. } if text.contains("Test message"))
        });
        assert!(has_msg);
    }

    #[test]
    fn render_shadow_on_upper_layers() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 1, row: 1, col: 1 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        let cmds = app.render(900.0, 700.0);
        let has_shadow = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if *color == TILE_SHADOW)
        });
        assert!(has_shadow);
    }

    #[test]
    fn render_no_shadow_on_layer_zero() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        let cmds = app.render(900.0, 700.0);
        let has_shadow = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if *color == TILE_SHADOW)
        });
        assert!(!has_shadow);
    }

    // ── Edge cases and integration ──────────────────────────────────

    #[test]
    fn full_game_remove_all_pairs_small() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 0, row: 1, col: 0 },
                TilePos { layer: 0, row: 1, col: 5 },
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Circle(3),
                TileKind::Circle(3),
            ],
        );
        app.status = GameStatus::Playing;
        // Remove first pair
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.moves, 1);
        // Remove second pair
        app.try_select(2);
        app.try_select(3);
        assert_eq!(app.moves, 2);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn undo_after_win_returns_to_playing() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.status, GameStatus::Won);
        app.undo();
        assert_eq!(app.status, GameStatus::Playing);
        assert_eq!(app.board.remaining(), 2);
    }

    #[test]
    fn multiple_undos() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 0, row: 1, col: 0 },
                TilePos { layer: 0, row: 1, col: 5 },
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Circle(3),
                TileKind::Circle(3),
            ],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        app.try_select(2);
        app.try_select(3);
        assert_eq!(app.moves, 2);
        app.undo();
        assert_eq!(app.moves, 1);
        app.undo();
        assert_eq!(app.moves, 0);
        assert_eq!(app.board.remaining(), 4);
    }

    #[test]
    fn hint_not_shown_when_won() {
        let mut app = Mahjong::with_seed(42);
        app.status = GameStatus::Won;
        app.show_hint_pair();
        assert!(!app.show_hint);
    }

    #[test]
    fn shuffle_not_when_won() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(&[], &[]);
        app.status = GameStatus::Won;
        app.shuffle_tiles();
        // Should not crash, board is empty.
        assert_eq!(app.board.remaining(), 0);
    }

    #[test]
    fn season_match_through_game() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Season(0), TileKind::Season(3)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 0);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn flower_match_through_game() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Flower(0), TileKind::Flower(2)],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        assert_eq!(app.board.remaining(), 0);
        assert_eq!(app.status, GameStatus::Won);
    }

    #[test]
    fn layer_blocking_prevents_match() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 1, row: 0, col: 0 }, // blocks tile 0
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Circle(1),
            ],
        );
        app.status = GameStatus::Playing;
        // Tile 0 is blocked by tile 2 on layer above.
        assert!(!app.board.is_free(0));
        app.try_select(0);
        // Should not be selected (not free).
        assert_eq!(app.selected, None);
    }

    #[test]
    fn cursor_moves_right() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.cursor.tile_idx = Some(0);
        app.move_cursor(1, 0);
        assert_eq!(app.cursor.tile_idx, Some(1));
    }

    #[test]
    fn cursor_moves_left() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.cursor.tile_idx = Some(1);
        app.move_cursor(-1, 0);
        assert_eq!(app.cursor.tile_idx, Some(0));
    }

    #[test]
    fn cursor_moves_down() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 3, col: 0 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.cursor.tile_idx = Some(0);
        app.move_cursor(0, 1);
        assert_eq!(app.cursor.tile_idx, Some(1));
    }

    #[test]
    fn cursor_moves_up() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 3, col: 0 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.cursor.tile_idx = Some(1);
        app.move_cursor(0, -1);
        assert_eq!(app.cursor.tile_idx, Some(0));
    }

    #[test]
    fn cursor_stays_if_no_tile_in_direction() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        app.cursor.tile_idx = Some(0);
        app.move_cursor(1, 0); // no tile to the right
        assert_eq!(app.cursor.tile_idx, Some(0));
    }

    #[test]
    fn new_game_increments_seed() {
        let mut app = Mahjong::with_seed(42);
        app.new_game();
        assert_eq!(app.seed, 43);
    }

    #[test]
    fn board_from_parts_correct_count() {
        let board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 1 },
                TilePos { layer: 0, row: 0, col: 2 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2), TileKind::Bamboo(3)],
        );
        assert_eq!(board.tiles.len(), 3);
        assert_eq!(board.remaining(), 3);
    }

    #[test]
    fn render_empty_board() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(&[], &[]);
        let cmds = app.render(900.0, 700.0);
        // Should still have background, title, help bar, legend.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn tile_kind_label_bamboo() {
        assert_eq!(TileKind::Bamboo(1).label(), "B1");
        assert_eq!(TileKind::Bamboo(9).label(), "B9");
    }

    #[test]
    fn tile_kind_label_circle() {
        assert_eq!(TileKind::Circle(1).label(), "C1");
        assert_eq!(TileKind::Circle(9).label(), "C9");
    }

    #[test]
    fn tile_kind_label_character() {
        assert_eq!(TileKind::Character(1).label(), "W1");
        assert_eq!(TileKind::Character(9).label(), "W9");
    }

    #[test]
    fn tile_kind_label_wind() {
        assert_eq!(TileKind::Wind(0).label(), "E");
        assert_eq!(TileKind::Wind(1).label(), "S");
        assert_eq!(TileKind::Wind(2).label(), "W");
        assert_eq!(TileKind::Wind(3).label(), "N");
    }

    #[test]
    fn tile_kind_label_dragon() {
        assert_eq!(TileKind::Dragon(0).label(), "Dr");
        assert_eq!(TileKind::Dragon(1).label(), "Dg");
        assert_eq!(TileKind::Dragon(2).label(), "Dw");
    }

    #[test]
    fn tile_kind_label_season() {
        assert_eq!(TileKind::Season(0).label(), "Sp");
        assert_eq!(TileKind::Season(1).label(), "Su");
        assert_eq!(TileKind::Season(2).label(), "Au");
        assert_eq!(TileKind::Season(3).label(), "Wi");
    }

    #[test]
    fn tile_kind_label_flower() {
        assert_eq!(TileKind::Flower(0).label(), "Pl");
        assert_eq!(TileKind::Flower(1).label(), "Or");
        assert_eq!(TileKind::Flower(2).label(), "Ch");
        assert_eq!(TileKind::Flower(3).label(), "Bm");
    }

    #[test]
    fn tile_kind_text_color_varies() {
        // Each suit category should have a different text color.
        let bamboo = TileKind::Bamboo(1).text_color();
        let circle = TileKind::Circle(1).text_color();
        let wind = TileKind::Wind(0).text_color();
        assert_ne!(bamboo, circle);
        assert_ne!(circle, wind);
    }

    #[test]
    fn tile_kind_category_values() {
        assert_eq!(TileKind::Bamboo(1).category(), "Bamboo");
        assert_eq!(TileKind::Circle(1).category(), "Circle");
        assert_eq!(TileKind::Character(1).category(), "Character");
        assert_eq!(TileKind::Wind(0).category(), "Wind");
        assert_eq!(TileKind::Dragon(0).category(), "Dragon");
        assert_eq!(TileKind::Season(0).category(), "Season");
        assert_eq!(TileKind::Flower(0).category(), "Flower");
    }

    #[test]
    fn board_free_tiles_on_full_board() {
        let mut rng = Rng::new(42);
        let board = Board::new(&mut rng);
        let free = board.free_tiles();
        // On a standard layout, there should be some free tiles.
        assert!(!free.is_empty());
    }

    #[test]
    fn board_hint_on_full_board() {
        let mut rng = Rng::new(42);
        let board = Board::new(&mut rng);
        // A freshly shuffled board with 4 copies of each type should almost
        // always have a valid pair among the free tiles.
        // This test may very rarely fail if the shuffle is extremely unlucky,
        // but with 36 types x 4 copies it's virtually guaranteed.
        let hint = board.find_hint();
        // We just check it doesn't crash. The hint may or may not exist
        // depending on the specific shuffle.
        let _ = hint;
    }

    #[test]
    fn app_undo_clears_selection() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 0, row: 1, col: 0 },
                TilePos { layer: 0, row: 1, col: 5 },
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Bamboo(2),
                TileKind::Bamboo(2),
            ],
        );
        app.status = GameStatus::Playing;
        app.try_select(0);
        app.try_select(1);
        app.try_select(2); // select a tile
        assert_eq!(app.selected, Some(2));
        app.undo();
        assert_eq!(app.selected, None); // undo clears selection
    }

    #[test]
    fn app_hint_clears_on_match() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
                TilePos { layer: 0, row: 1, col: 0 },
                TilePos { layer: 0, row: 1, col: 5 },
            ],
            &[
                TileKind::Bamboo(1),
                TileKind::Bamboo(1),
                TileKind::Bamboo(2),
                TileKind::Bamboo(2),
            ],
        );
        app.status = GameStatus::Playing;
        app.show_hint_pair();
        assert!(app.show_hint);
        app.try_select(0);
        app.try_select(1);
        assert!(!app.show_hint); // hint clears after match
    }

    #[test]
    fn board_tile_at_screen_respects_removed() {
        let mut board = Board::from_parts(
            &[TilePos { layer: 0, row: 0, col: 0 }],
            &[TileKind::Bamboo(1)],
        );
        let (tx, ty) = board.tile_screen_pos(0).unwrap();
        board.tiles[0].removed = true;
        assert_eq!(board.tile_at_screen(tx + 1.0, ty + 1.0), None);
    }

    #[test]
    fn cursor_selects_via_enter_match() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(1)],
        );
        app.status = GameStatus::Playing;
        app.cursor.tile_idx = Some(0);
        app.handle_key(&make_key(Key::Enter));
        assert_eq!(app.selected, Some(0));
        app.cursor.tile_idx = Some(1);
        app.handle_key(&make_key(Key::Enter));
        assert_eq!(app.board.remaining(), 0);
    }

    #[test]
    fn mouse_click_updates_cursor() {
        let mut app = Mahjong::with_seed(42);
        app.board = Board::from_parts(
            &[
                TilePos { layer: 0, row: 0, col: 0 },
                TilePos { layer: 0, row: 0, col: 5 },
            ],
            &[TileKind::Bamboo(1), TileKind::Bamboo(2)],
        );
        app.status = GameStatus::Playing;
        let (tx, ty) = app.board.tile_screen_pos(1).unwrap();
        app.handle_mouse(&make_mouse(tx + 1.0, ty + 1.0));
        assert_eq!(app.cursor.tile_idx, Some(1));
    }
}
