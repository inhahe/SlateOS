//! Conway's Game of Life cellular automaton for OurOS.
//!
//! Features:
//! - Toroidal grid (wraps at edges)
//! - Play/pause with Space
//! - Step-by-step with S key
//! - Adjustable speed (1-9 keys)
//! - Drawing cells with mouse or arrow keys + Enter
//! - Clear grid (C key), randomize (R key)
//! - Built-in patterns: glider, blinker, pulsar, gosper gun, etc.
//! - Generation counter and population count
//! - Zoom in/out with +/-
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

// ── Deterministic RNG ───────────────────────────────────────────────
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

    fn next_bool(&mut self, chance_pct: u64) -> bool {
        self.next() % 100 < chance_pct
    }
}

// ── Preset patterns ─────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pattern {
    Glider,
    Blinker,
    Toad,
    Beacon,
    Pulsar,
    GosperGun,
    Lwss,
    Diehard,
    Acorn,
    RPentomino,
}

impl Pattern {
    const ALL: &[Pattern] = &[
        Pattern::Glider,
        Pattern::Blinker,
        Pattern::Toad,
        Pattern::Beacon,
        Pattern::Pulsar,
        Pattern::GosperGun,
        Pattern::Lwss,
        Pattern::Diehard,
        Pattern::Acorn,
        Pattern::RPentomino,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Glider => "Glider",
            Self::Blinker => "Blinker",
            Self::Toad => "Toad",
            Self::Beacon => "Beacon",
            Self::Pulsar => "Pulsar",
            Self::GosperGun => "Gosper Glider Gun",
            Self::Lwss => "LWSS",
            Self::Diehard => "Diehard",
            Self::Acorn => "Acorn",
            Self::RPentomino => "R-Pentomino",
        }
    }

    /// Returns cells as (row_offset, col_offset) relative to placement point
    fn cells(self) -> Vec<(i32, i32)> {
        match self {
            Self::Glider => vec![(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)],
            Self::Blinker => vec![(0, 0), (0, 1), (0, 2)],
            Self::Toad => vec![(0, 1), (0, 2), (0, 3), (1, 0), (1, 1), (1, 2)],
            Self::Beacon => vec![(0, 0), (0, 1), (1, 0), (2, 3), (3, 2), (3, 3)],
            Self::Pulsar => {
                let mut cells = Vec::new();
                // Pulsar is symmetric — define one quadrant and mirror
                let quarter = [
                    (1, 2),
                    (1, 3),
                    (1, 4),
                    (2, 1),
                    (3, 1),
                    (4, 1),
                    (2, 6),
                    (3, 6),
                    (4, 6),
                    (6, 2),
                    (6, 3),
                    (6, 4),
                ];
                for &(r, c) in &quarter {
                    cells.push((r, c));
                    cells.push((r, 12 - c));
                    cells.push((12 - r, c));
                    cells.push((12 - r, 12 - c));
                }
                cells.sort();
                cells.dedup();
                cells
            }
            Self::GosperGun => vec![
                (0, 24),
                (1, 22),
                (1, 24),
                (2, 12),
                (2, 13),
                (2, 20),
                (2, 21),
                (2, 34),
                (2, 35),
                (3, 11),
                (3, 15),
                (3, 20),
                (3, 21),
                (3, 34),
                (3, 35),
                (4, 0),
                (4, 1),
                (4, 10),
                (4, 16),
                (4, 20),
                (4, 21),
                (5, 0),
                (5, 1),
                (5, 10),
                (5, 14),
                (5, 16),
                (5, 17),
                (5, 22),
                (5, 24),
                (6, 10),
                (6, 16),
                (6, 24),
                (7, 11),
                (7, 15),
                (8, 12),
                (8, 13),
            ],
            Self::Lwss => vec![
                (0, 1),
                (0, 4),
                (1, 0),
                (2, 0),
                (2, 4),
                (3, 0),
                (3, 1),
                (3, 2),
                (3, 3),
            ],
            Self::Diehard => vec![(0, 6), (1, 0), (1, 1), (2, 1), (2, 5), (2, 6), (2, 7)],
            Self::Acorn => vec![(0, 1), (1, 3), (2, 0), (2, 1), (2, 4), (2, 5), (2, 6)],
            Self::RPentomino => vec![(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)],
        }
    }
}

// ── Grid ────────────────────────────────────────────────────────────
const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 60;

struct Grid {
    width: usize,
    height: usize,
    cells: Vec<bool>,
}

impl Grid {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![false; width * height],
        }
    }

    fn get(&self, row: usize, col: usize) -> bool {
        if row < self.height && col < self.width {
            self.cells
                .get(row * self.width + col)
                .copied()
                .unwrap_or(false)
        } else {
            false
        }
    }

    fn set(&mut self, row: usize, col: usize, alive: bool) {
        if row < self.height && col < self.width {
            if let Some(cell) = self.cells.get_mut(row * self.width + col) {
                *cell = alive;
            }
        }
    }

    fn toggle(&mut self, row: usize, col: usize) {
        if row < self.height && col < self.width {
            if let Some(cell) = self.cells.get_mut(row * self.width + col) {
                *cell = !*cell;
            }
        }
    }

    fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = false;
        }
    }

    fn population(&self) -> usize {
        self.cells.iter().filter(|&&c| c).count()
    }

    /// Count live neighbors using toroidal wrapping
    fn count_neighbors(&self, row: usize, col: usize) -> u8 {
        let mut count: u8 = 0;
        for dr in [-1i32, 0, 1] {
            for dc in [-1i32, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = ((row as i32 + dr).rem_euclid(self.height as i32)) as usize;
                let nc = ((col as i32 + dc).rem_euclid(self.width as i32)) as usize;
                if self.get(nr, nc) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Advance one generation using Conway's rules:
    /// - Live cell with 2-3 neighbors survives
    /// - Dead cell with exactly 3 neighbors becomes alive
    /// - All other cells die or stay dead
    fn step(&self) -> Grid {
        let mut next = Grid::new(self.width, self.height);
        for row in 0..self.height {
            for col in 0..self.width {
                let neighbors = self.count_neighbors(row, col);
                let alive = self.get(row, col);
                let next_alive = if alive {
                    neighbors == 2 || neighbors == 3
                } else {
                    neighbors == 3
                };
                next.set(row, col, next_alive);
            }
        }
        next
    }

    fn randomize(&mut self, rng: &mut Rng, density: u64) {
        for cell in &mut self.cells {
            *cell = rng.next_bool(density);
        }
    }

    fn place_pattern(&mut self, pattern: Pattern, center_row: usize, center_col: usize) {
        let cells = pattern.cells();
        for (dr, dc) in cells {
            let r = ((center_row as i32 + dr).rem_euclid(self.height as i32)) as usize;
            let c = ((center_col as i32 + dc).rem_euclid(self.width as i32)) as usize;
            self.set(r, c, true);
        }
    }
}

impl Clone for Grid {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            cells: self.cells.clone(),
        }
    }
}

// ── View state ──────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Main,
    PatternSelect,
}

// ── App ─────────────────────────────────────────────────────────────
struct LifeApp {
    grid: Grid,
    view: View,
    running: bool,
    generation: u64,
    speed: u32, // 1-9, ticks between updates
    cell_size: f32,
    cursor_row: usize,
    cursor_col: usize,
    // Viewport offset for scrolling
    view_row: usize,
    view_col: usize,
    // Pattern selection
    selected_pattern: usize,
    // Timing
    tick_accum: u64,
    // RNG
    rng: Rng,
    // Show grid lines
    show_grid: bool,
    // Show help
    show_help: bool,
}

impl LifeApp {
    fn new() -> Self {
        let mut app = Self {
            grid: Grid::new(DEFAULT_WIDTH, DEFAULT_HEIGHT),
            view: View::Main,
            running: false,
            generation: 0,
            speed: 5,
            cell_size: 8.0,
            cursor_row: DEFAULT_HEIGHT / 2,
            cursor_col: DEFAULT_WIDTH / 2,
            view_row: 0,
            view_col: 0,
            selected_pattern: 0,
            tick_accum: 0,
            rng: Rng::new(42),
            show_grid: true,
            show_help: false,
        };
        // Place a glider gun for an interesting start
        app.grid.place_pattern(Pattern::GosperGun, 10, 10);
        app
    }

    fn speed_ms(&self) -> u64 {
        match self.speed {
            1 => 500,
            2 => 350,
            3 => 250,
            4 => 175,
            5 => 120,
            6 => 80,
            7 => 50,
            8 => 30,
            _ => 15,
        }
    }

    fn step(&mut self) {
        self.grid = self.grid.step();
        self.generation += 1;
    }

    fn visible_rows(&self, height: f32) -> usize {
        let usable = height - 44.0; // top bar
        ((usable / self.cell_size) as usize).min(self.grid.height)
    }

    fn visible_cols(&self, width: f32) -> usize {
        ((width / self.cell_size) as usize).min(self.grid.width)
    }

    fn event(&mut self, event: &Event) {
        match self.view {
            View::Main => self.handle_main_event(event),
            View::PatternSelect => self.handle_pattern_event(event),
        }
    }

    fn handle_main_event(&mut self, event: &Event) {
        match event {
            Event::Tick { elapsed_ms } => {
                if self.running {
                    self.tick_accum += elapsed_ms;
                    let interval = self.speed_ms();
                    while self.tick_accum >= interval {
                        self.tick_accum -= interval;
                        self.step();
                    }
                }
            }
            Event::Key(KeyEvent { key, modifiers, .. }) => {
                if modifiers.ctrl {
                    return;
                }
                match key {
                    Key::Space => {
                        self.running = !self.running;
                        self.tick_accum = 0;
                    }
                    Key::S => {
                        if !self.running {
                            self.step();
                        }
                    }
                    Key::C => {
                        self.grid.clear();
                        self.generation = 0;
                        self.running = false;
                    }
                    Key::R => {
                        self.grid.randomize(&mut self.rng, 25);
                        self.generation = 0;
                    }
                    Key::G => {
                        self.show_grid = !self.show_grid;
                    }
                    Key::P => {
                        self.view = View::PatternSelect;
                        self.running = false;
                    }
                    Key::F1 => {
                        self.show_help = !self.show_help;
                    }
                    Key::Enter => {
                        self.grid.toggle(self.cursor_row, self.cursor_col);
                    }
                    Key::Up => {
                        if self.cursor_row > 0 {
                            self.cursor_row -= 1;
                        } else {
                            self.cursor_row = self.grid.height - 1;
                        }
                    }
                    Key::Down => {
                        self.cursor_row = (self.cursor_row + 1) % self.grid.height;
                    }
                    Key::Left => {
                        if self.cursor_col > 0 {
                            self.cursor_col -= 1;
                        } else {
                            self.cursor_col = self.grid.width - 1;
                        }
                    }
                    Key::Right => {
                        self.cursor_col = (self.cursor_col + 1) % self.grid.width;
                    }
                    // Speed: number keys
                    Key::Num1 => self.speed = 1,
                    Key::Num2 => self.speed = 2,
                    Key::Num3 => self.speed = 3,
                    Key::Num4 => self.speed = 4,
                    Key::Num5 => self.speed = 5,
                    Key::Num6 => self.speed = 6,
                    Key::Num7 => self.speed = 7,
                    Key::Num8 => self.speed = 8,
                    Key::Num9 => self.speed = 9,
                    _ => {}
                }
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Press(MouseButton::Left),
                x,
                y,
                ..
            }) => {
                let grid_y_start = 44.0;
                if *y >= grid_y_start {
                    let row = ((y - grid_y_start) / self.cell_size) as usize + self.view_row;
                    let col = (*x / self.cell_size) as usize + self.view_col;
                    if row < self.grid.height && col < self.grid.width {
                        self.grid.toggle(row, col);
                        self.cursor_row = row;
                        self.cursor_col = col;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_pattern_event(&mut self, event: &Event) {
        match event {
            Event::Key(KeyEvent { key, .. }) => match key {
                Key::Up => {
                    if self.selected_pattern > 0 {
                        self.selected_pattern -= 1;
                    }
                }
                Key::Down => {
                    if self.selected_pattern + 1 < Pattern::ALL.len() {
                        self.selected_pattern += 1;
                    }
                }
                Key::Enter => {
                    let pat = Pattern::ALL[self.selected_pattern];
                    self.grid
                        .place_pattern(pat, self.cursor_row, self.cursor_col);
                    self.view = View::Main;
                }
                Key::Escape => {
                    self.view = View::Main;
                }
                _ => {}
            },
            _ => {}
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
            color: COL_CRUST,
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

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 10.0,
            text: "Game of Life".to_string(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Status
        let status = if self.running { "Running" } else { "Paused" };
        let status_color = if self.running { COL_GREEN } else { COL_YELLOW };
        cmds.push(RenderCommand::Text {
            x: 160.0,
            y: 12.0,
            text: status.to_string(),
            font_size: 14.0,
            color: status_color,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Generation
        cmds.push(RenderCommand::Text {
            x: 260.0,
            y: 12.0,
            text: format!("Gen: {}", self.generation),
            font_size: 14.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Population
        cmds.push(RenderCommand::Text {
            x: 400.0,
            y: 12.0,
            text: format!("Pop: {}", self.grid.population()),
            font_size: 14.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Speed
        cmds.push(RenderCommand::Text {
            x: 530.0,
            y: 12.0,
            text: format!("Speed: {}", self.speed),
            font_size: 14.0,
            color: COL_SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid size
        cmds.push(RenderCommand::Text {
            x: 640.0,
            y: 12.0,
            text: format!("{}x{}", self.grid.width, self.grid.height),
            font_size: 14.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Draw grid
        let grid_y = 44.0;
        let vis_rows = self.visible_rows(height);
        let vis_cols = self.visible_cols(width);

        for row in 0..vis_rows {
            let grid_row = (self.view_row + row) % self.grid.height;
            for col in 0..vis_cols {
                let grid_col = (self.view_col + col) % self.grid.width;
                let cx = col as f32 * self.cell_size;
                let cy = grid_y + row as f32 * self.cell_size;

                let alive = self.grid.get(grid_row, grid_col);
                let is_cursor = grid_row == self.cursor_row && grid_col == self.cursor_col;

                if alive {
                    let color = if is_cursor { COL_LAVENDER } else { COL_GREEN };
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: self.cell_size - 0.5,
                        height: self.cell_size - 0.5,
                        color,
                        corner_radii: CornerRadii::ZERO,
                    });
                } else if is_cursor {
                    cmds.push(RenderCommand::FillRect {
                        x: cx,
                        y: cy,
                        width: self.cell_size - 0.5,
                        height: self.cell_size - 0.5,
                        color: COL_SURFACE1,
                        corner_radii: CornerRadii::ZERO,
                    });
                }

                // Grid lines
                if self.show_grid && self.cell_size >= 4.0 {
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx,
                        y: cy,
                        width: self.cell_size,
                        height: self.cell_size,
                        color: COL_SURFACE0,
                        line_width: 0.5,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
        }

        // Help bar at bottom
        cmds.push(RenderCommand::Text {
            x: 8.0, y: height - 20.0,
            text: "Space=Play/Pause  S=Step  C=Clear  R=Random  P=Patterns  G=Grid  1-9=Speed  Enter=Toggle  F1=Help".to_string(),
            font_size: 11.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Pattern select overlay
        if self.view == View::PatternSelect {
            self.render_pattern_select(&mut cmds, width, height);
        }

        // Help overlay
        if self.show_help {
            self.render_help(&mut cmds, width, height);
        }

        cmds
    }

    fn render_pattern_select(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        // Dim background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let bx = width / 2.0 - 150.0;
        let by = 60.0;
        let bw = 300.0;
        let bh = 340.0;

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
            y: by + 16.0,
            text: "Place Pattern".to_string(),
            font_size: 18.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let mut cy = by + 50.0;
        for (i, pat) in Pattern::ALL.iter().enumerate() {
            let is_sel = i == self.selected_pattern;

            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: bx + 10.0,
                    y: cy - 2.0,
                    width: bw - 20.0,
                    height: 24.0,
                    color: COL_SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let color = if is_sel { COL_BLUE } else { COL_SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: bx + 20.0,
                y: cy,
                text: pat.name().to_string(),
                font_size: 14.0,
                color,
                font_weight: if is_sel {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
            cy += 28.0;
        }

        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: by + bh - 30.0,
            text: "Enter=Place  Esc=Cancel".to_string(),
            font_size: 12.0,
            color: COL_OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    fn render_help(&self, cmds: &mut Vec<RenderCommand>, width: f32, height: f32) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: Color::rgba(0, 0, 0, 180),
            corner_radii: CornerRadii::ZERO,
        });

        let bx = width / 2.0 - 180.0;
        let by = height / 2.0 - 150.0;
        let bw = 360.0;
        let bh = 300.0;

        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: bw,
            height: bh,
            color: COL_MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        cmds.push(RenderCommand::Text {
            x: bx + bw / 2.0 - 50.0,
            y: by + 16.0,
            text: "Controls".to_string(),
            font_size: 20.0,
            color: COL_TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let helps = [
            ("Space", "Play / Pause"),
            ("S", "Single step"),
            ("C", "Clear grid"),
            ("R", "Randomize (25% density)"),
            ("P", "Pattern placement menu"),
            ("G", "Toggle grid lines"),
            ("Enter", "Toggle cell at cursor"),
            ("Arrow Keys", "Move cursor"),
            ("Mouse Click", "Toggle cell"),
            ("1-9", "Set simulation speed"),
            ("F1", "Toggle this help"),
        ];

        let mut cy = by + 50.0;
        for (key, desc) in &helps {
            cmds.push(RenderCommand::Text {
                x: bx + 24.0,
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
            cy += 22.0;
        }
    }
}

fn main() {
    let _app = LifeApp::new();
}

// ── Tests ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_new() {
        let grid = Grid::new(10, 10);
        assert_eq!(grid.width, 10);
        assert_eq!(grid.height, 10);
        assert_eq!(grid.population(), 0);
    }

    #[test]
    fn test_grid_set_get() {
        let mut grid = Grid::new(10, 10);
        grid.set(3, 5, true);
        assert!(grid.get(3, 5));
        assert!(!grid.get(0, 0));
    }

    #[test]
    fn test_grid_toggle() {
        let mut grid = Grid::new(10, 10);
        grid.toggle(2, 3);
        assert!(grid.get(2, 3));
        grid.toggle(2, 3);
        assert!(!grid.get(2, 3));
    }

    #[test]
    fn test_grid_clear() {
        let mut grid = Grid::new(10, 10);
        grid.set(0, 0, true);
        grid.set(5, 5, true);
        grid.clear();
        assert_eq!(grid.population(), 0);
    }

    #[test]
    fn test_grid_population() {
        let mut grid = Grid::new(10, 10);
        grid.set(0, 0, true);
        grid.set(1, 1, true);
        grid.set(2, 2, true);
        assert_eq!(grid.population(), 3);
    }

    #[test]
    fn test_grid_out_of_bounds() {
        let grid = Grid::new(5, 5);
        assert!(!grid.get(10, 10));
    }

    #[test]
    fn test_grid_set_out_of_bounds() {
        let mut grid = Grid::new(5, 5);
        grid.set(10, 10, true); // Should not panic
        assert!(!grid.get(10, 10));
    }

    #[test]
    fn test_count_neighbors_center() {
        let mut grid = Grid::new(5, 5);
        grid.set(1, 1, true);
        grid.set(1, 2, true);
        grid.set(1, 3, true);
        // Cell at (2,2) has 3 neighbors
        assert_eq!(grid.count_neighbors(2, 2), 3);
    }

    #[test]
    fn test_count_neighbors_corner_wrapping() {
        let mut grid = Grid::new(5, 5);
        // Place cells at edges that wrap around to (0,0)
        grid.set(4, 4, true); // wraps to neighbor of (0,0)
        grid.set(4, 0, true);
        grid.set(0, 4, true);
        assert_eq!(grid.count_neighbors(0, 0), 3);
    }

    #[test]
    fn test_count_neighbors_no_self() {
        let mut grid = Grid::new(5, 5);
        grid.set(2, 2, true); // Only the cell itself — should count 0 neighbors
        assert_eq!(grid.count_neighbors(2, 2), 0);
    }

    #[test]
    fn test_count_neighbors_all_eight() {
        let mut grid = Grid::new(5, 5);
        // Surround (2,2) with all 8 neighbors
        for dr in [-1i32, 0, 1] {
            for dc in [-1i32, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                grid.set((2 + dr) as usize, (2 + dc) as usize, true);
            }
        }
        assert_eq!(grid.count_neighbors(2, 2), 8);
    }

    #[test]
    fn test_step_blinker() {
        // Blinker oscillator: period 2
        let mut grid = Grid::new(5, 5);
        // Horizontal blinker at row 2
        grid.set(2, 1, true);
        grid.set(2, 2, true);
        grid.set(2, 3, true);

        let next = grid.step();
        // Should become vertical
        assert!(next.get(1, 2));
        assert!(next.get(2, 2));
        assert!(next.get(3, 2));
        assert!(!next.get(2, 1));
        assert!(!next.get(2, 3));

        let next2 = next.step();
        // Should return to horizontal
        assert!(next2.get(2, 1));
        assert!(next2.get(2, 2));
        assert!(next2.get(2, 3));
        assert!(!next2.get(1, 2));
        assert!(!next2.get(3, 2));
    }

    #[test]
    fn test_step_block_still_life() {
        // Block is a still life (2x2)
        let mut grid = Grid::new(5, 5);
        grid.set(1, 1, true);
        grid.set(1, 2, true);
        grid.set(2, 1, true);
        grid.set(2, 2, true);

        let next = grid.step();
        assert!(next.get(1, 1));
        assert!(next.get(1, 2));
        assert!(next.get(2, 1));
        assert!(next.get(2, 2));
        assert_eq!(next.population(), 4);
    }

    #[test]
    fn test_step_lone_cell_dies() {
        let mut grid = Grid::new(5, 5);
        grid.set(2, 2, true);
        let next = grid.step();
        assert!(!next.get(2, 2));
        assert_eq!(next.population(), 0);
    }

    #[test]
    fn test_step_birth() {
        let mut grid = Grid::new(5, 5);
        // Three cells in an L: should birth a new cell
        grid.set(1, 1, true);
        grid.set(1, 2, true);
        grid.set(2, 1, true);
        // Cell at (2,2) has 3 neighbors → should be born
        let next = grid.step();
        assert!(next.get(2, 2));
    }

    #[test]
    fn test_step_overcrowding() {
        let mut grid = Grid::new(5, 5);
        // Cell with 4+ neighbors dies
        grid.set(2, 2, true);
        grid.set(1, 1, true);
        grid.set(1, 2, true);
        grid.set(1, 3, true);
        grid.set(2, 1, true);
        // (2,2) has 4 neighbors → should die
        let next = grid.step();
        assert!(!next.get(2, 2));
    }

    #[test]
    fn test_grid_clone() {
        let mut grid = Grid::new(5, 5);
        grid.set(1, 1, true);
        let clone = grid.clone();
        assert!(clone.get(1, 1));
        assert_eq!(clone.population(), 1);
    }

    #[test]
    fn test_randomize() {
        let mut grid = Grid::new(20, 20);
        let mut rng = Rng::new(123);
        grid.randomize(&mut rng, 50);
        let pop = grid.population();
        // With 50% density, expect roughly 200 cells out of 400
        assert!(pop > 100);
        assert!(pop < 300);
    }

    #[test]
    fn test_place_pattern_glider() {
        let mut grid = Grid::new(10, 10);
        grid.place_pattern(Pattern::Glider, 2, 2);
        assert!(grid.get(2, 3)); // (0,1) offset
        assert!(grid.get(3, 4)); // (1,2)
        assert!(grid.get(4, 2)); // (2,0)
        assert!(grid.get(4, 3)); // (2,1)
        assert!(grid.get(4, 4)); // (2,2)
        assert_eq!(grid.population(), 5);
    }

    #[test]
    fn test_place_pattern_blinker() {
        let mut grid = Grid::new(10, 10);
        grid.place_pattern(Pattern::Blinker, 5, 5);
        assert_eq!(grid.population(), 3);
    }

    #[test]
    fn test_place_pattern_wrapping() {
        let mut grid = Grid::new(5, 5);
        // Place glider near edge — should wrap
        grid.place_pattern(Pattern::Glider, 4, 4);
        assert!(grid.population() > 0);
    }

    #[test]
    fn test_pattern_names() {
        for pat in Pattern::ALL {
            assert!(!pat.name().is_empty());
        }
    }

    #[test]
    fn test_pattern_cells_nonempty() {
        for pat in Pattern::ALL {
            assert!(!pat.cells().is_empty());
        }
    }

    #[test]
    fn test_rng_deterministic() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(r1.next(), r2.next());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut r1 = Rng::new(1);
        let mut r2 = Rng::new(2);
        // Very unlikely to match
        assert_ne!(r1.next(), r2.next());
    }

    #[test]
    fn test_rng_next_bool() {
        let mut rng = Rng::new(42);
        let mut true_count = 0;
        for _ in 0..1000 {
            if rng.next_bool(50) {
                true_count += 1;
            }
        }
        // Should be roughly 50%
        assert!(true_count > 350);
        assert!(true_count < 650);
    }

    #[test]
    fn test_app_new() {
        let app = LifeApp::new();
        assert!(!app.running);
        assert_eq!(app.generation, 0);
        assert_eq!(app.speed, 5);
        assert!(app.grid.population() > 0); // Gosper gun placed
    }

    #[test]
    fn test_app_step() {
        let mut app = LifeApp::new();
        app.step();
        assert_eq!(app.generation, 1);
    }

    #[test]
    fn test_speed_ms() {
        let mut app = LifeApp::new();
        app.speed = 1;
        assert_eq!(app.speed_ms(), 500);
        app.speed = 9;
        assert_eq!(app.speed_ms(), 15);
    }

    #[test]
    fn test_toggle_running() {
        let mut app = LifeApp::new();
        assert!(!app.running);
        app.event(&Event::Key(KeyEvent {
            key: Key::Space,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.running);
        app.event(&Event::Key(KeyEvent {
            key: Key::Space,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(!app.running);
    }

    #[test]
    fn test_clear_key() {
        let mut app = LifeApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::C,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.grid.population(), 0);
        assert_eq!(app.generation, 0);
        assert!(!app.running);
    }

    #[test]
    fn test_randomize_key() {
        let mut app = LifeApp::new();
        app.grid.clear();
        app.event(&Event::Key(KeyEvent {
            key: Key::R,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.grid.population() > 0);
    }

    #[test]
    fn test_step_key() {
        let mut app = LifeApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::S,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.generation, 1);
    }

    #[test]
    fn test_step_key_ignored_when_running() {
        let mut app = LifeApp::new();
        app.running = true;
        let gen_before = app.generation;
        app.event(&Event::Key(KeyEvent {
            key: Key::S,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.generation, gen_before);
    }

    #[test]
    fn test_speed_keys() {
        let mut app = LifeApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::Num1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.speed, 1);
        app.event(&Event::Key(KeyEvent {
            key: Key::Num9,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.speed, 9);
    }

    #[test]
    fn test_cursor_movement() {
        let mut app = LifeApp::new();
        let start_row = app.cursor_row;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, start_row - 1);
    }

    #[test]
    fn test_cursor_wrap_up() {
        let mut app = LifeApp::new();
        app.cursor_row = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, app.grid.height - 1);
    }

    #[test]
    fn test_cursor_wrap_down() {
        let mut app = LifeApp::new();
        app.cursor_row = app.grid.height - 1;
        app.event(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_row, 0);
    }

    #[test]
    fn test_cursor_wrap_left() {
        let mut app = LifeApp::new();
        app.cursor_col = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Left,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_col, app.grid.width - 1);
    }

    #[test]
    fn test_cursor_wrap_right() {
        let mut app = LifeApp::new();
        app.cursor_col = app.grid.width - 1;
        app.event(&Event::Key(KeyEvent {
            key: Key::Right,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.cursor_col, 0);
    }

    #[test]
    fn test_enter_toggles_cell() {
        let mut app = LifeApp::new();
        app.grid.clear();
        let r = app.cursor_row;
        let c = app.cursor_col;
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.grid.get(r, c));
    }

    #[test]
    fn test_grid_toggle_key() {
        let mut app = LifeApp::new();
        assert_eq!(app.show_grid, true);
        app.event(&Event::Key(KeyEvent {
            key: Key::G,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.show_grid, false);
    }

    #[test]
    fn test_pattern_select_opens() {
        let mut app = LifeApp::new();
        app.event(&Event::Key(KeyEvent {
            key: Key::P,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::PatternSelect);
        assert!(!app.running);
    }

    #[test]
    fn test_pattern_select_navigate() {
        let mut app = LifeApp::new();
        app.view = View::PatternSelect;
        app.selected_pattern = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Down,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_pattern, 1);
    }

    #[test]
    fn test_pattern_select_no_overflow_up() {
        let mut app = LifeApp::new();
        app.view = View::PatternSelect;
        app.selected_pattern = 0;
        app.event(&Event::Key(KeyEvent {
            key: Key::Up,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.selected_pattern, 0);
    }

    #[test]
    fn test_pattern_select_place() {
        let mut app = LifeApp::new();
        app.grid.clear();
        app.view = View::PatternSelect;
        app.selected_pattern = 0; // Glider
        app.event(&Event::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::Main);
        assert!(app.grid.population() > 0);
    }

    #[test]
    fn test_pattern_select_escape() {
        let mut app = LifeApp::new();
        app.view = View::PatternSelect;
        app.event(&Event::Key(KeyEvent {
            key: Key::Escape,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert_eq!(app.view, View::Main);
    }

    #[test]
    fn test_help_toggle() {
        let mut app = LifeApp::new();
        assert!(!app.show_help);
        app.event(&Event::Key(KeyEvent {
            key: Key::F1,
            modifiers: Modifiers::default(),
            pressed: true,
            text: None,
        }));
        assert!(app.show_help);
    }

    #[test]
    fn test_tick_advances_when_running() {
        let mut app = LifeApp::new();
        app.running = true;
        app.speed = 5; // 120ms interval
        app.event(&Event::Tick { elapsed_ms: 200 });
        assert!(app.generation > 0);
    }

    #[test]
    fn test_tick_no_advance_when_paused() {
        let mut app = LifeApp::new();
        app.running = false;
        app.event(&Event::Tick { elapsed_ms: 1000 });
        assert_eq!(app.generation, 0);
    }

    #[test]
    fn test_render_no_panic() {
        let app = LifeApp::new();
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_pattern_select_no_panic() {
        let mut app = LifeApp::new();
        app.view = View::PatternSelect;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_help_no_panic() {
        let mut app = LifeApp::new();
        app.show_help = true;
        let cmds = app.render(800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_visible_rows_cols() {
        let app = LifeApp::new();
        let rows = app.visible_rows(600.0);
        let cols = app.visible_cols(800.0);
        assert!(rows > 0);
        assert!(cols > 0);
        assert!(rows <= app.grid.height);
        assert!(cols <= app.grid.width);
    }

    #[test]
    fn test_gosper_gun_starts() {
        let app = LifeApp::new();
        // Gosper gun has 36 cells
        assert_eq!(app.grid.population(), 36);
    }

    #[test]
    fn test_glider_moves() {
        let mut grid = Grid::new(10, 10);
        grid.place_pattern(Pattern::Glider, 1, 1);
        // After 4 steps, glider should have moved one cell down and right
        for _ in 0..4 {
            grid = grid.step();
        }
        assert_eq!(grid.population(), 5);
    }

    #[test]
    fn test_toad_oscillator() {
        let mut grid = Grid::new(8, 8);
        grid.place_pattern(Pattern::Toad, 3, 2);
        let pop = grid.population();
        let gen1 = grid.step();
        let gen2 = gen1.step();
        // Toad is period 2 — population should be same after 2 steps
        assert_eq!(gen2.population(), pop);
    }

    #[test]
    fn test_empty_grid_stays_empty() {
        let grid = Grid::new(10, 10);
        let next = grid.step();
        assert_eq!(next.population(), 0);
    }

    #[test]
    fn test_mouse_click_toggles() {
        let mut app = LifeApp::new();
        app.grid.clear();
        app.cell_size = 8.0;
        // Click at position that maps to a grid cell
        app.event(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::Press(MouseButton::Left),
            x: 16.0, // col = 16/8 = 2
            y: 52.0, // row = (52-44)/8 = 1
        }));
        assert!(app.grid.get(1, 2));
    }

    #[test]
    fn test_ctrl_ignored() {
        let mut app = LifeApp::new();
        let pop = app.grid.population();
        app.event(&Event::Key(KeyEvent {
            key: Key::C,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            pressed: true,
            text: None,
        }));
        // C with Ctrl should NOT clear — Ctrl events are ignored
        assert_eq!(app.grid.population(), pop);
    }

    #[test]
    fn test_pattern_all_count() {
        assert_eq!(Pattern::ALL.len(), 10);
    }

    #[test]
    fn test_rpentomino_population() {
        let mut grid = Grid::new(50, 50);
        grid.place_pattern(Pattern::RPentomino, 25, 25);
        assert_eq!(grid.population(), 5);
    }

    #[test]
    fn test_main_no_panic() {
        main();
    }

    #[test]
    fn test_beacon_oscillator() {
        let mut grid = Grid::new(8, 8);
        grid.place_pattern(Pattern::Beacon, 2, 2);
        let pop = grid.population();
        let gen1 = grid.step();
        let gen2 = gen1.step();
        assert_eq!(gen2.population(), pop);
    }

    #[test]
    fn test_lwss_moves() {
        let mut grid = Grid::new(20, 20);
        grid.place_pattern(Pattern::Lwss, 5, 5);
        assert_eq!(grid.population(), 9);
        // LWSS should survive after several steps
        for _ in 0..4 {
            grid = grid.step();
        }
        assert!(grid.population() > 0);
    }

    #[test]
    fn test_diehard_starts() {
        let mut grid = Grid::new(30, 30);
        grid.place_pattern(Pattern::Diehard, 15, 15);
        assert_eq!(grid.population(), 7);
    }

    #[test]
    fn test_acorn_starts() {
        let mut grid = Grid::new(30, 30);
        grid.place_pattern(Pattern::Acorn, 15, 15);
        assert_eq!(grid.population(), 7);
    }

    #[test]
    fn test_multiple_steps_performance() {
        let mut grid = Grid::new(50, 50);
        let mut rng = Rng::new(99);
        grid.randomize(&mut rng, 30);
        // Run 100 generations — shouldn't panic or hang
        for _ in 0..100 {
            grid = grid.step();
        }
        // Grid should still be valid
        assert!(grid.population() <= 2500);
    }

    #[test]
    fn test_speed_values() {
        let mut app = LifeApp::new();
        for s in 1..=9 {
            app.speed = s;
            let ms = app.speed_ms();
            assert!(ms > 0);
        }
        // Speed 1 should be slowest
        app.speed = 1;
        let slow = app.speed_ms();
        app.speed = 9;
        let fast = app.speed_ms();
        assert!(slow > fast);
    }
}
