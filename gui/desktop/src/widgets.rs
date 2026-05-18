//! Desktop widget system.
//!
//! Allows small, always-visible widget panels on the desktop surface.
//! Widgets can show live information (clock, weather, CPU, calendar, notes,
//! RSS, stocks, etc.) without opening a full application window.
//!
//! Each widget occupies a fixed-size slot on a grid overlay. The user can
//! add, remove, move, and resize widgets. Third-party apps can provide widgets
//! via a capability-gated registration API.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Widget types
// ============================================================================

/// Unique widget instance ID.
pub type WidgetInstanceId = u64;

/// Size of a widget in grid cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetSize {
    pub cols: u32,
    pub rows: u32,
}

impl WidgetSize {
    pub const SMALL: Self = Self { cols: 1, rows: 1 };
    pub const MEDIUM: Self = Self { cols: 2, rows: 1 };
    pub const WIDE: Self = Self { cols: 2, rows: 2 };
    pub const TALL: Self = Self { cols: 1, rows: 2 };
    pub const LARGE: Self = Self { cols: 3, rows: 2 };

    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }

    /// Pixel dimensions given a cell size.
    pub fn pixels(&self, cell_w: f32, cell_h: f32, gap: f32) -> (f32, f32) {
        let w = self.cols as f32 * cell_w + (self.cols.saturating_sub(1)) as f32 * gap;
        let h = self.rows as f32 * cell_h + (self.rows.saturating_sub(1)) as f32 * gap;
        (w, h)
    }
}

/// Grid position for a widget (column, row — 0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPos {
    pub col: u32,
    pub row: u32,
}

impl GridPos {
    pub fn new(col: u32, row: u32) -> Self {
        Self { col, row }
    }

    /// Pixel position given cell size, gap, and origin.
    pub fn pixels(&self, origin_x: f32, origin_y: f32, cell_w: f32, cell_h: f32, gap: f32) -> (f32, f32) {
        let x = origin_x + self.col as f32 * (cell_w + gap);
        let y = origin_y + self.row as f32 * (cell_h + gap);
        (x, y)
    }
}

/// The type of built-in widget content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    /// Digital clock with date.
    Clock,
    /// Weather summary (current conditions).
    Weather,
    /// CPU/memory/disk usage.
    SystemMonitor,
    /// Small calendar (month view).
    Calendar,
    /// Quick notes / sticky text.
    Notes,
    /// RSS feed headlines.
    RssFeed,
    /// Music player controls.
    MusicPlayer,
    /// Photo slideshow.
    PhotoFrame,
    /// World clocks (multiple timezones).
    WorldClock,
    /// Upcoming events/reminders.
    Reminders,
    /// Disk usage summary.
    DiskUsage,
    /// Network traffic monitor.
    NetworkMonitor,
    /// Battery status.
    BatteryStatus,
    /// Custom widget from a third-party app.
    Custom { app_name: String },
}

impl WidgetKind {
    /// Human-readable label.
    pub fn label(&self) -> &str {
        match self {
            Self::Clock => "Clock",
            Self::Weather => "Weather",
            Self::SystemMonitor => "System Monitor",
            Self::Calendar => "Calendar",
            Self::Notes => "Quick Notes",
            Self::RssFeed => "RSS Feed",
            Self::MusicPlayer => "Music Player",
            Self::PhotoFrame => "Photo Frame",
            Self::WorldClock => "World Clock",
            Self::Reminders => "Reminders",
            Self::DiskUsage => "Disk Usage",
            Self::NetworkMonitor => "Network",
            Self::BatteryStatus => "Battery",
            Self::Custom { .. } => "Custom Widget",
        }
    }

    /// Icon character.
    pub fn icon(&self) -> &str {
        match self {
            Self::Clock => "\u{1F552}",
            Self::Weather => "\u{2600}",
            Self::SystemMonitor => "\u{1F4CA}",
            Self::Calendar => "\u{1F4C5}",
            Self::Notes => "\u{1F4DD}",
            Self::RssFeed => "\u{1F4F0}",
            Self::MusicPlayer => "\u{1F3B5}",
            Self::PhotoFrame => "\u{1F5BC}",
            Self::WorldClock => "\u{1F30D}",
            Self::Reminders => "\u{1F514}",
            Self::DiskUsage => "\u{1F4BE}",
            Self::NetworkMonitor => "\u{1F310}",
            Self::BatteryStatus => "\u{1F50B}",
            Self::Custom { .. } => "\u{1F50C}",
        }
    }

    /// Default size.
    pub fn default_size(&self) -> WidgetSize {
        match self {
            Self::Clock => WidgetSize::SMALL,
            Self::Weather => WidgetSize::MEDIUM,
            Self::SystemMonitor => WidgetSize::MEDIUM,
            Self::Calendar => WidgetSize::WIDE,
            Self::Notes => WidgetSize::MEDIUM,
            Self::RssFeed => WidgetSize::TALL,
            Self::MusicPlayer => WidgetSize::MEDIUM,
            Self::PhotoFrame => WidgetSize::WIDE,
            Self::WorldClock => WidgetSize::MEDIUM,
            Self::Reminders => WidgetSize::TALL,
            Self::DiskUsage => WidgetSize::SMALL,
            Self::NetworkMonitor => WidgetSize::SMALL,
            Self::BatteryStatus => WidgetSize::SMALL,
            Self::Custom { .. } => WidgetSize::MEDIUM,
        }
    }

    /// All built-in widget types (for the add-widget picker).
    pub fn all_builtin() -> Vec<Self> {
        vec![
            Self::Clock,
            Self::Weather,
            Self::SystemMonitor,
            Self::Calendar,
            Self::Notes,
            Self::RssFeed,
            Self::MusicPlayer,
            Self::PhotoFrame,
            Self::WorldClock,
            Self::Reminders,
            Self::DiskUsage,
            Self::NetworkMonitor,
            Self::BatteryStatus,
        ]
    }
}

// ============================================================================
// Widget instance
// ============================================================================

/// A placed widget on the desktop.
#[derive(Clone, Debug)]
pub struct WidgetInstance {
    /// Unique ID.
    pub id: WidgetInstanceId,
    /// What kind of widget.
    pub kind: WidgetKind,
    /// Grid position.
    pub position: GridPos,
    /// Grid size.
    pub size: WidgetSize,
    /// Whether the widget is visible.
    pub visible: bool,
    /// Background opacity (0–255).
    pub bg_opacity: u8,
    /// Custom title override.
    pub title_override: Option<String>,
    /// Last updated timestamp (ms since epoch).
    pub last_updated: u64,
    /// Update interval in ms (0 = static).
    pub update_interval_ms: u64,
    /// Widget-specific state (text for Notes, timezone list for WorldClock, etc.).
    pub state_text: String,
    /// Whether the widget is currently being dragged.
    pub dragging: bool,
}

impl WidgetInstance {
    pub fn new(id: WidgetInstanceId, kind: WidgetKind, position: GridPos) -> Self {
        let size = kind.default_size();
        let update_interval = match &kind {
            WidgetKind::Clock | WidgetKind::SystemMonitor | WidgetKind::NetworkMonitor => 1000,
            WidgetKind::Weather => 600_000,
            WidgetKind::RssFeed => 300_000,
            WidgetKind::BatteryStatus => 30_000,
            _ => 0,
        };
        Self {
            id,
            kind,
            position,
            size,
            visible: true,
            bg_opacity: 200,
            title_override: None,
            last_updated: 0,
            update_interval_ms: update_interval,
            state_text: String::new(),
            dragging: false,
        }
    }

    /// Display title.
    pub fn title(&self) -> &str {
        self.title_override.as_deref().unwrap_or_else(|| self.kind.label())
    }

    /// Whether the widget needs an update tick.
    pub fn needs_update(&self, now_ms: u64) -> bool {
        if self.update_interval_ms == 0 {
            return false;
        }
        now_ms.saturating_sub(self.last_updated) >= self.update_interval_ms
    }

    /// Check if a position (in grid cells) overlaps this widget.
    pub fn occupies(&self, col: u32, row: u32) -> bool {
        col >= self.position.col
            && col < self.position.col + self.size.cols
            && row >= self.position.row
            && row < self.position.row + self.size.rows
    }
}

// ============================================================================
// Widget grid / manager
// ============================================================================

/// Configuration for the widget grid.
#[derive(Clone, Debug)]
pub struct WidgetGridConfig {
    /// Number of columns.
    pub columns: u32,
    /// Number of rows.
    pub rows: u32,
    /// Cell width in pixels.
    pub cell_width: f32,
    /// Cell height in pixels.
    pub cell_height: f32,
    /// Gap between cells in pixels.
    pub gap: f32,
    /// Grid origin (top-left of widget area).
    pub origin_x: f32,
    pub origin_y: f32,
    /// Corner radius for widget panels.
    pub corner_radius: f32,
}

impl Default for WidgetGridConfig {
    fn default() -> Self {
        Self {
            columns: 8,
            rows: 6,
            cell_width: 180.0,
            cell_height: 150.0,
            gap: 12.0,
            origin_x: 40.0,
            origin_y: 40.0,
            corner_radius: 12.0,
        }
    }
}

/// Manages all desktop widgets.
pub struct DesktopWidgetManager {
    /// All widget instances.
    widgets: Vec<WidgetInstance>,
    /// Grid configuration.
    pub grid: WidgetGridConfig,
    /// Whether the widget layer is visible.
    pub layer_visible: bool,
    /// Whether in edit mode (can move/resize/add/remove widgets).
    pub edit_mode: bool,
    /// Next widget instance ID.
    next_id: WidgetInstanceId,
    /// Maximum number of widgets.
    pub max_widgets: usize,
    /// Whether the add-widget picker is open.
    pub picker_open: bool,
    /// Currently selected widget for editing.
    pub selected_widget: Option<WidgetInstanceId>,
}

impl DesktopWidgetManager {
    pub fn new() -> Self {
        Self {
            widgets: Vec::new(),
            grid: WidgetGridConfig::default(),
            layer_visible: true,
            edit_mode: false,
            next_id: 1,
            max_widgets: 20,
            picker_open: false,
            selected_widget: None,
        }
    }

    /// Add a widget. Returns the instance ID, or None if rejected.
    pub fn add_widget(&mut self, kind: WidgetKind, position: GridPos) -> Option<WidgetInstanceId> {
        if self.widgets.len() >= self.max_widgets {
            return None;
        }

        let size = kind.default_size();

        // Check bounds.
        if position.col + size.cols > self.grid.columns
            || position.row + size.rows > self.grid.rows
        {
            return None;
        }

        // Check overlap.
        if self.overlaps_any(position, size, None) {
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;
        self.widgets.push(WidgetInstance::new(id, kind, position));
        Some(id)
    }

    /// Remove a widget by ID.
    pub fn remove_widget(&mut self, id: WidgetInstanceId) -> bool {
        let len_before = self.widgets.len();
        self.widgets.retain(|w| w.id != id);
        if self.selected_widget == Some(id) {
            self.selected_widget = None;
        }
        self.widgets.len() < len_before
    }

    /// Move a widget to a new grid position.
    pub fn move_widget(&mut self, id: WidgetInstanceId, new_pos: GridPos) -> bool {
        // Get the widget's size first.
        let size = match self.widgets.iter().find(|w| w.id == id) {
            Some(w) => w.size,
            None => return false,
        };

        // Check bounds.
        if new_pos.col + size.cols > self.grid.columns
            || new_pos.row + size.rows > self.grid.rows
        {
            return false;
        }

        // Check overlap (excluding self).
        if self.overlaps_any(new_pos, size, Some(id)) {
            return false;
        }

        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.position = new_pos;
            true
        } else {
            false
        }
    }

    /// Resize a widget.
    pub fn resize_widget(&mut self, id: WidgetInstanceId, new_size: WidgetSize) -> bool {
        let pos = match self.widgets.iter().find(|w| w.id == id) {
            Some(w) => w.position,
            None => return false,
        };

        // Check bounds.
        if pos.col + new_size.cols > self.grid.columns || pos.row + new_size.rows > self.grid.rows {
            return false;
        }

        // Check overlap.
        if self.overlaps_any(pos, new_size, Some(id)) {
            return false;
        }

        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.size = new_size;
            true
        } else {
            false
        }
    }

    /// Toggle visibility of a widget.
    pub fn toggle_visibility(&mut self, id: WidgetInstanceId) -> bool {
        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.visible = !w.visible;
            true
        } else {
            false
        }
    }

    /// Get a widget by ID.
    pub fn get(&self, id: WidgetInstanceId) -> Option<&WidgetInstance> {
        self.widgets.iter().find(|w| w.id == id)
    }

    /// Get a mutable widget by ID.
    pub fn get_mut(&mut self, id: WidgetInstanceId) -> Option<&mut WidgetInstance> {
        self.widgets.iter_mut().find(|w| w.id == id)
    }

    /// All widgets.
    pub fn all_widgets(&self) -> &[WidgetInstance] {
        &self.widgets
    }

    /// Visible widgets.
    pub fn visible_widgets(&self) -> Vec<&WidgetInstance> {
        self.widgets.iter().filter(|w| w.visible).collect()
    }

    /// Count.
    pub fn count(&self) -> usize {
        self.widgets.len()
    }

    /// Hit-test: which widget (if any) is at a pixel coordinate?
    pub fn hit_test(&self, px: f32, py: f32) -> Option<WidgetInstanceId> {
        for w in self.widgets.iter().rev() {
            if !w.visible {
                continue;
            }
            let (wx, wy) = w.position.pixels(
                self.grid.origin_x,
                self.grid.origin_y,
                self.grid.cell_width,
                self.grid.cell_height,
                self.grid.gap,
            );
            let (ww, wh) = w.size.pixels(self.grid.cell_width, self.grid.cell_height, self.grid.gap);
            if px >= wx && px < wx + ww && py >= wy && py < wy + wh {
                return Some(w.id);
            }
        }
        None
    }

    /// Which grid cell is at a pixel coordinate?
    pub fn pixel_to_grid(&self, px: f32, py: f32) -> Option<GridPos> {
        let rel_x = px - self.grid.origin_x;
        let rel_y = py - self.grid.origin_y;
        if rel_x < 0.0 || rel_y < 0.0 {
            return None;
        }
        let step_x = self.grid.cell_width + self.grid.gap;
        let step_y = self.grid.cell_height + self.grid.gap;
        let col = (rel_x / step_x) as u32;
        let row = (rel_y / step_y) as u32;
        if col < self.grid.columns && row < self.grid.rows {
            Some(GridPos::new(col, row))
        } else {
            None
        }
    }

    /// Find the first available position for a widget of the given size.
    pub fn find_free_position(&self, size: WidgetSize) -> Option<GridPos> {
        for row in 0..self.grid.rows {
            for col in 0..self.grid.columns {
                let pos = GridPos::new(col, row);
                if pos.col + size.cols <= self.grid.columns
                    && pos.row + size.rows <= self.grid.rows
                    && !self.overlaps_any(pos, size, None)
                {
                    return Some(pos);
                }
            }
        }
        None
    }

    /// Tick all widgets (update those that need it).
    pub fn tick(&mut self, now_ms: u64) {
        for w in &mut self.widgets {
            if w.needs_update(now_ms) {
                w.last_updated = now_ms;
            }
        }
    }

    /// Render all visible widgets into render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.layer_visible {
            return Vec::new();
        }

        let mut commands = Vec::new();

        // In edit mode, render the grid.
        if self.edit_mode {
            self.render_grid(&mut commands);
        }

        // Render each visible widget.
        for w in &self.widgets {
            if !w.visible {
                continue;
            }
            self.render_widget(w, &mut commands);
        }

        // Widget picker overlay.
        if self.picker_open {
            self.render_picker(&mut commands);
        }

        commands
    }

    // ========================================================================
    // Private
    // ========================================================================

    fn overlaps_any(&self, pos: GridPos, size: WidgetSize, exclude: Option<WidgetInstanceId>) -> bool {
        for w in &self.widgets {
            if exclude == Some(w.id) {
                continue;
            }
            // Check rectangle overlap.
            let a_left = pos.col;
            let a_right = pos.col + size.cols;
            let a_top = pos.row;
            let a_bottom = pos.row + size.rows;

            let b_left = w.position.col;
            let b_right = w.position.col + w.size.cols;
            let b_top = w.position.row;
            let b_bottom = w.position.row + w.size.rows;

            if a_left < b_right && a_right > b_left && a_top < b_bottom && a_bottom > b_top {
                return true;
            }
        }
        false
    }

    fn render_grid(&self, commands: &mut Vec<RenderCommand>) {
        for row in 0..self.grid.rows {
            for col in 0..self.grid.columns {
                let (x, y) = GridPos::new(col, row).pixels(
                    self.grid.origin_x,
                    self.grid.origin_y,
                    self.grid.cell_width,
                    self.grid.cell_height,
                    self.grid.gap,
                );
                commands.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: self.grid.cell_width,
                    height: self.grid.cell_height,
                    color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, 80),
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }
    }

    fn render_widget(&self, w: &WidgetInstance, commands: &mut Vec<RenderCommand>) {
        let (x, y) = w.position.pixels(
            self.grid.origin_x,
            self.grid.origin_y,
            self.grid.cell_width,
            self.grid.cell_height,
            self.grid.gap,
        );
        let (width, height) = w.size.pixels(self.grid.cell_width, self.grid.cell_height, self.grid.gap);
        let cr = self.grid.corner_radius;

        // Shadow.
        commands.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, w.bg_opacity / 3),
            corner_radii: CornerRadii::all(cr),
        });

        // Background.
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: Color::rgba(BASE.r, BASE.g, BASE.b, w.bg_opacity),
            corner_radii: CornerRadii::all(cr),
        });

        // Selection highlight in edit mode.
        if self.edit_mode && self.selected_widget == Some(w.id) {
            commands.push(RenderCommand::StrokeRect {
                x: x - 2.0,
                y: y - 2.0,
                width: width + 4.0,
                height: height + 4.0,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(cr + 2.0),
            });
        }

        // Title bar.
        let title_h = 24.0;
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: title_h,
            color: Color::rgba(SURFACE0.r, SURFACE0.g, SURFACE0.b, w.bg_opacity),
            corner_radii: CornerRadii {
                top_left: cr,
                top_right: cr,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Icon and title.
        commands.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 4.0,
            text: w.kind.icon().to_string(),
            font_size: 12.0,
            color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, (w.bg_opacity as f32 * 1.2) as u8),
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        commands.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 5.0,
            text: w.title().to_string(),
            font_size: 11.0,
            color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, (w.bg_opacity as f32 * 1.2) as u8),
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
        });

        // Content area.
        let content_y = y + title_h + 4.0;
        let content_h = height - title_h - 8.0;
        self.render_widget_content(w, x + 8.0, content_y, width - 16.0, content_h, w.bg_opacity, commands);
    }

    fn render_widget_content(
        &self,
        w: &WidgetInstance,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        alpha: u8,
        commands: &mut Vec<RenderCommand>,
    ) {
        match &w.kind {
            WidgetKind::Clock => {
                // Large time display.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 10.0,
                    text: "12:34".to_string(),
                    font_size: 36.0,
                    color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width),
                });
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 55.0,
                    text: "Sunday, May 18".to_string(),
                    font_size: 12.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                });
            }
            WidgetKind::SystemMonitor => {
                // CPU bar.
                let bar_h = 8.0;
                commands.push(RenderCommand::Text {
                    x,
                    y,
                    text: "CPU".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 14.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(SURFACE1.r, SURFACE1.g, SURFACE1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 14.0,
                    width: width * 0.45,
                    height: bar_h,
                    color: Color::rgba(BLUE.r, BLUE.g, BLUE.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                // Memory bar.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 32.0,
                    text: "Memory".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 46.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(SURFACE1.r, SURFACE1.g, SURFACE1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 46.0,
                    width: width * 0.62,
                    height: bar_h,
                    color: Color::rgba(GREEN.r, GREEN.g, GREEN.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                // Disk bar.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 64.0,
                    text: "Disk".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 78.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(SURFACE1.r, SURFACE1.g, SURFACE1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 78.0,
                    width: width * 0.38,
                    height: bar_h,
                    color: Color::rgba(PEACH.r, PEACH.g, PEACH.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            WidgetKind::Notes => {
                let display = if w.state_text.is_empty() {
                    "Click to add a note..."
                } else {
                    &w.state_text
                };
                commands.push(RenderCommand::Text {
                    x,
                    y,
                    text: display.to_string(),
                    font_size: 12.0,
                    color: Color::rgba(
                        if w.state_text.is_empty() { OVERLAY0.r } else { TEXT.r },
                        if w.state_text.is_empty() { OVERLAY0.g } else { TEXT.g },
                        if w.state_text.is_empty() { OVERLAY0.b } else { TEXT.b },
                        alpha,
                    ),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                });
            }
            WidgetKind::BatteryStatus => {
                // Battery bar placeholder.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 10.0,
                    text: "\u{1F50B}".to_string(),
                    font_size: 28.0,
                    color: Color::rgba(GREEN.r, GREEN.g, GREEN.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                commands.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: y + 16.0,
                    text: "85%".to_string(),
                    font_size: 20.0,
                    color: Color::rgba(TEXT.r, TEXT.g, TEXT.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 55.0,
                    text: "3h 42m remaining".to_string(),
                    font_size: 11.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                });
            }
            _ => {
                // Generic placeholder for other widget types.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + height / 2.0 - 10.0,
                    text: w.kind.icon().to_string(),
                    font_size: 32.0,
                    color: Color::rgba(SURFACE2.r, SURFACE2.g, SURFACE2.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                commands.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: y + height / 2.0 - 4.0,
                    text: w.kind.label().to_string(),
                    font_size: 13.0,
                    color: Color::rgba(SUBTEXT0.r, SUBTEXT0.g, SUBTEXT0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 44.0),
                });
            }
        }
    }

    fn render_picker(&self, commands: &mut Vec<RenderCommand>) {
        let picker_w = 300.0;
        let picker_h = 400.0;
        let px = self.grid.origin_x + 50.0;
        let py = self.grid.origin_y + 50.0;

        // Backdrop.
        commands.push(RenderCommand::BoxShadow {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            offset_x: 0.0,
            offset_y: 6.0,
            blur: 20.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(12.0),
        });
        commands.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });
        commands.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title.
        commands.push(RenderCommand::Text {
            x: px + 16.0,
            y: py + 14.0,
            text: "Add Widget".to_string(),
            font_size: 16.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Widget list.
        let mut cy = py + 48.0;
        for kind in WidgetKind::all_builtin() {
            if cy + 32.0 > py + picker_h {
                break;
            }
            commands.push(RenderCommand::Text {
                x: px + 16.0,
                y: cy + 4.0,
                text: kind.icon().to_string(),
                font_size: 16.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            commands.push(RenderCommand::Text {
                x: px + 40.0,
                y: cy + 6.0,
                text: kind.label().to_string(),
                font_size: 13.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            let sz = kind.default_size();
            commands.push(RenderCommand::Text {
                x: px + picker_w - 60.0,
                y: cy + 8.0,
                text: format!("{}x{}", sz.cols, sz.rows),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Light,
                max_width: None,
            });
            cy += 26.0;
        }
    }
}

impl Default for DesktopWidgetManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mgr() -> DesktopWidgetManager {
        DesktopWidgetManager::new()
    }

    // ---- WidgetSize ----

    #[test]
    fn widget_size_pixels() {
        let size = WidgetSize::MEDIUM; // 2x1
        let (w, h) = size.pixels(180.0, 150.0, 12.0);
        assert!((w - 372.0).abs() < 0.01); // 2*180 + 1*12
        assert!((h - 150.0).abs() < 0.01); // 1*150 + 0*12
    }

    #[test]
    fn widget_size_small_pixels() {
        let size = WidgetSize::SMALL;
        let (w, h) = size.pixels(100.0, 100.0, 10.0);
        assert!((w - 100.0).abs() < 0.01);
        assert!((h - 100.0).abs() < 0.01);
    }

    #[test]
    fn widget_size_new_clamps() {
        let size = WidgetSize::new(0, 0);
        assert_eq!(size.cols, 1);
        assert_eq!(size.rows, 1);
    }

    // ---- GridPos ----

    #[test]
    fn grid_pos_pixels() {
        let pos = GridPos::new(2, 1);
        let (x, y) = pos.pixels(40.0, 40.0, 180.0, 150.0, 12.0);
        assert!((x - 424.0).abs() < 0.01); // 40 + 2*(180+12)
        assert!((y - 202.0).abs() < 0.01); // 40 + 1*(150+12)
    }

    // ---- WidgetKind ----

    #[test]
    fn all_builtin_kinds() {
        let kinds = WidgetKind::all_builtin();
        assert_eq!(kinds.len(), 13);
    }

    #[test]
    fn kind_labels_not_empty() {
        for kind in WidgetKind::all_builtin() {
            assert!(!kind.label().is_empty());
            assert!(!kind.icon().is_empty());
        }
    }

    #[test]
    fn kind_default_sizes() {
        assert_eq!(WidgetKind::Clock.default_size(), WidgetSize::SMALL);
        assert_eq!(WidgetKind::Calendar.default_size(), WidgetSize::WIDE);
        assert_eq!(WidgetKind::SystemMonitor.default_size(), WidgetSize::MEDIUM);
    }

    // ---- WidgetInstance ----

    #[test]
    fn widget_instance_new() {
        let w = WidgetInstance::new(1, WidgetKind::Clock, GridPos::new(0, 0));
        assert_eq!(w.id, 1);
        assert_eq!(w.size, WidgetSize::SMALL);
        assert!(w.visible);
        assert!(!w.dragging);
    }

    #[test]
    fn widget_title_default() {
        let w = WidgetInstance::new(1, WidgetKind::Weather, GridPos::new(0, 0));
        assert_eq!(w.title(), "Weather");
    }

    #[test]
    fn widget_title_override() {
        let mut w = WidgetInstance::new(1, WidgetKind::Weather, GridPos::new(0, 0));
        w.title_override = Some("My Weather".to_string());
        assert_eq!(w.title(), "My Weather");
    }

    #[test]
    fn widget_needs_update() {
        let w = WidgetInstance::new(1, WidgetKind::Clock, GridPos::new(0, 0));
        assert!(w.update_interval_ms > 0);
        assert!(w.needs_update(2000));
        assert!(!WidgetInstance::new(2, WidgetKind::Notes, GridPos::new(0, 0)).needs_update(1000));
    }

    #[test]
    fn widget_occupies() {
        let w = WidgetInstance::new(1, WidgetKind::Calendar, GridPos::new(1, 1));
        // Calendar is 2x2.
        assert!(w.occupies(1, 1));
        assert!(w.occupies(2, 2));
        assert!(!w.occupies(0, 0));
        assert!(!w.occupies(3, 1));
    }

    // ---- DesktopWidgetManager ----

    #[test]
    fn add_widget() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        assert!(id.is_some());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn add_widget_out_of_bounds() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Calendar, GridPos::new(7, 5)); // 2x2 at (7,5) exceeds 8x6
        assert!(id.is_none());
    }

    #[test]
    fn add_widget_overlap_rejected() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Calendar, GridPos::new(0, 0)); // 2x2
        let id2 = mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 1)); // overlaps
        assert!(id2.is_none());
    }

    #[test]
    fn add_widget_adjacent_ok() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)); // 1x1
        let id2 = mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0)); // 1x1 adjacent
        assert!(id2.is_some());
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn max_widgets_enforced() {
        let mut mgr = make_mgr();
        mgr.max_widgets = 2;
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        let id3 = mgr.add_widget(WidgetKind::Clock, GridPos::new(2, 0));
        assert!(id3.is_none());
    }

    #[test]
    fn remove_widget() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(mgr.remove_widget(id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn remove_nonexistent() {
        let mut mgr = make_mgr();
        assert!(!mgr.remove_widget(999));
    }

    #[test]
    fn move_widget() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(mgr.move_widget(id, GridPos::new(3, 3)));
        assert_eq!(mgr.get(id).unwrap().position, GridPos::new(3, 3));
    }

    #[test]
    fn move_widget_out_of_bounds() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(!mgr.move_widget(id, GridPos::new(8, 0)));
    }

    #[test]
    fn move_widget_overlap() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(2, 2));
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(!mgr.move_widget(id, GridPos::new(2, 2))); // occupied
    }

    #[test]
    fn resize_widget() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(mgr.resize_widget(id, WidgetSize::MEDIUM));
        assert_eq!(mgr.get(id).unwrap().size, WidgetSize::MEDIUM);
    }

    #[test]
    fn resize_widget_blocked_by_overlap() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        assert!(!mgr.resize_widget(id, WidgetSize::MEDIUM)); // would overlap
    }

    #[test]
    fn toggle_visibility() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        assert!(mgr.get(id).unwrap().visible);
        mgr.toggle_visibility(id);
        assert!(!mgr.get(id).unwrap().visible);
        assert_eq!(mgr.visible_widgets().len(), 0);
    }

    #[test]
    fn hit_test() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        // Clock is 1x1 at (40,40) with 180x150 cell.
        assert_eq!(mgr.hit_test(50.0, 50.0), Some(id));
        assert_eq!(mgr.hit_test(300.0, 300.0), None);
    }

    #[test]
    fn pixel_to_grid() {
        let mgr = make_mgr();
        // Origin at (40,40), cell 180x150, gap 12.
        let pos = mgr.pixel_to_grid(50.0, 50.0);
        assert_eq!(pos, Some(GridPos::new(0, 0)));
        let pos2 = mgr.pixel_to_grid(250.0, 50.0);
        assert_eq!(pos2, Some(GridPos::new(1, 0)));
    }

    #[test]
    fn pixel_to_grid_out_of_bounds() {
        let mgr = make_mgr();
        assert_eq!(mgr.pixel_to_grid(0.0, 0.0), None);
    }

    #[test]
    fn find_free_position() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        let free = mgr.find_free_position(WidgetSize::SMALL);
        assert!(free.is_some());
        assert_ne!(free.unwrap(), GridPos::new(0, 0));
    }

    #[test]
    fn find_free_position_none_when_full() {
        let mut mgr = make_mgr();
        mgr.grid.columns = 2;
        mgr.grid.rows = 1;
        mgr.max_widgets = 10;
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        let free = mgr.find_free_position(WidgetSize::SMALL);
        assert!(free.is_none());
    }

    #[test]
    fn tick_updates_timestamps() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)).unwrap();
        mgr.tick(5000);
        assert_eq!(mgr.get(id).unwrap().last_updated, 5000);
    }

    // ---- Rendering ----

    #[test]
    fn render_empty() {
        let mgr = make_mgr();
        let cmds = mgr.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_with_widget() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_hidden_layer() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.layer_visible = false;
        let cmds = mgr.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_edit_mode_shows_grid() {
        let mut mgr = make_mgr();
        mgr.edit_mode = true;
        let cmds = mgr.render();
        // Should have grid cells rendered.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_picker() {
        let mut mgr = make_mgr();
        mgr.picker_open = true;
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_system_monitor() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(0, 0));
        let cmds = mgr.render();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_notes_empty() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Notes, GridPos::new(0, 0));
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_notes_with_text() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Notes, GridPos::new(0, 0)).unwrap();
        mgr.get_mut(id).unwrap().state_text = "Hello world".to_string();
        let cmds = mgr.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_battery_status() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(0, 0));
        let cmds = mgr.render();
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_multiple_widgets() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(2, 0));
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(4, 0));
        let cmds = mgr.render();
        assert!(cmds.len() > 15);
    }
}
