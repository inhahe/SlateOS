//! Desktop Icons — icon layer sitting beneath windows, above the wallpaper.
//!
//! This module manages a grid of desktop icons (files, folders, shortcuts,
//! system items) that users can click, drag, rename, and double-click to open.
//!
//! # Integration
//!
//! ```ignore
//! let mut icon_layer = DesktopIconLayer::new(1920, 1080, 40); // screen_w, screen_h, taskbar_h
//! icon_layer.populate_defaults();
//!
//! // Each frame:
//! let commands = icon_layer.render();
//!
//! // Forward mouse/key events:
//! icon_layer.handle_mouse_down(x, y, button, modifiers);
//! icon_layer.handle_mouse_move(x, y);
//! icon_layer.handle_mouse_up(x, y, button);
//! icon_layer.handle_key(key_event);
//! icon_layer.handle_double_click(x, y);
//! ```

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Theme — Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const SAPPHIRE: Color = Color::from_hex(0x74C7EC);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const MAUVE: Color = Color::from_hex(0xCBA6F7);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);

    /// Selection highlight (translucent blue).
    pub const SELECTION_BG: Color = Color::rgba(137, 180, 250, 50);
    /// Selection border.
    pub const SELECTION_BORDER: Color = Color::rgba(137, 180, 250, 150);
    /// Rubber-band selection fill.
    pub const RUBBERBAND_FILL: Color = Color::rgba(137, 180, 250, 30);
    /// Rubber-band selection border.
    pub const RUBBERBAND_BORDER: Color = Color::rgba(137, 180, 250, 120);
    /// Drop target highlight.
    pub const DROP_TARGET: Color = Color::rgba(166, 227, 161, 60);
    /// Icon label shadow for readability.
    pub const LABEL_SHADOW: Color = Color::rgba(0, 0, 0, 180);
}

// ============================================================================
// Constants
// ============================================================================

/// Default grid cell width in pixels.
const DEFAULT_GRID_WIDTH: u32 = 80;
/// Default grid cell height in pixels.
const DEFAULT_GRID_HEIGHT: u32 = 90;
/// Icon glyph size (large character).
const ICON_GLYPH_SIZE: f32 = 32.0;
/// Label font size.
const LABEL_FONT_SIZE: f32 = 11.0;
/// Maximum label width (for centering and truncation).
const LABEL_MAX_WIDTH: f32 = 72.0;
/// Maximum characters per label line before ellipsis.
const LABEL_MAX_CHARS_PER_LINE: usize = 12;
/// Maximum number of label lines.
const LABEL_MAX_LINES: usize = 2;
/// Drag threshold in pixels (squared, to avoid sqrt).
const DRAG_THRESHOLD_SQ: f32 = 25.0;
/// Padding from top of grid cell to icon glyph.
const ICON_TOP_PADDING: f32 = 8.0;
/// Padding from screen edges.
const EDGE_PADDING: u32 = 8;

// ============================================================================
// Types
// ============================================================================

/// Unique identifier for a desktop icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconId(pub u32);

/// The type/category of a desktop icon, determining its visual glyph and behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Folder,
    File,
    Shortcut,
    Drive,
    RecycleBin,
    Computer,
    Document,
    Image,
    Executable,
}

impl IconType {
    /// Unicode glyph representing this icon type.
    fn glyph(self) -> &'static str {
        match self {
            Self::Folder => "\u{1F4C1}",    // folder
            Self::File => "\u{1F4C4}",      // page facing up
            Self::Shortcut => "\u{1F517}",  // link
            Self::Drive => "\u{1F4BE}",     // floppy disk (drive)
            Self::RecycleBin => "\u{1F5D1}", // wastebasket
            Self::Computer => "\u{1F4BB}",  // laptop
            Self::Document => "\u{1F4DD}",  // memo
            Self::Image => "\u{1F5BC}",     // framed picture
            Self::Executable => "\u{2699}", // gear
        }
    }

    /// Color accent for the icon type (used for the glyph background).
    fn color(self) -> Color {
        match self {
            Self::Folder => theme::YELLOW,
            Self::File => theme::TEXT,
            Self::Shortcut => theme::SAPPHIRE,
            Self::Drive => theme::OVERLAY0,
            Self::RecycleBin => theme::RED,
            Self::Computer => theme::BLUE,
            Self::Document => theme::GREEN,
            Self::Image => theme::MAUVE,
            Self::Executable => theme::PEACH,
        }
    }
}

/// Action associated with a desktop icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconAction {
    /// Open a path (file or directory).
    OpenPath(String),
    /// Launch a system tool/dialog.
    LaunchSystem(String),
    /// Custom action string.
    Custom(String),
}

/// A single desktop icon.
#[derive(Clone, Debug)]
pub struct DesktopIcon {
    pub id: IconId,
    /// Position on the desktop (top-left of the icon cell).
    pub x: i32,
    pub y: i32,
    /// Display label (file/folder name).
    pub label: String,
    /// Type determines the glyph and accent color.
    pub icon_type: IconType,
    /// What happens when the icon is activated (double-click / Enter).
    pub action: IconAction,
    /// Whether this icon is currently selected.
    pub selected: bool,
}

/// Describes the current interaction state of the icon layer.
#[derive(Clone, Debug)]
enum InteractionState {
    /// No interaction in progress.
    Idle,
    /// Mouse is down, waiting to see if it becomes a drag.
    PendingDrag {
        start_x: f32,
        start_y: f32,
        /// Icons being considered for drag.
        icon_ids: Vec<IconId>,
    },
    /// Dragging selected icons.
    Dragging {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
        /// Original positions of dragged icons (id, orig_x, orig_y).
        originals: Vec<(IconId, i32, i32)>,
    },
    /// Rubber-band selection in progress (started on empty desktop area).
    RubberBand {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
    },
}

/// Arrangement mode for desktop icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangementMode {
    /// Icons snap to grid when dropped but stay where placed.
    FreeWithSnap,
    /// Icons are automatically sorted and placed from top-left.
    AutoArrange,
}

/// Result of an icon interaction (returned to the desktop shell).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconEvent {
    /// An icon was activated (double-click or Enter).
    Activate(IconId, IconAction),
    /// A context menu was requested at a position.
    ContextMenu { x: i32, y: i32, icon_id: Option<IconId> },
    /// Icons were deleted (moved to recycle bin).
    Delete(Vec<IconId>),
    /// Rename was initiated for an icon.
    BeginRename(IconId),
    /// No event.
    None,
}

/// Grid configuration for icon placement.
#[derive(Clone, Copy, Debug)]
pub struct GridConfig {
    pub cell_width: u32,
    pub cell_height: u32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            cell_width: DEFAULT_GRID_WIDTH,
            cell_height: DEFAULT_GRID_HEIGHT,
        }
    }
}

impl GridConfig {
    /// Snap a position to the nearest grid cell origin.
    pub fn snap(&self, x: i32, y: i32) -> (i32, i32) {
        let col = if x >= 0 {
            x / self.cell_width as i32
        } else {
            (x - self.cell_width as i32 + 1) / self.cell_width as i32
        };
        let row = if y >= 0 {
            y / self.cell_height as i32
        } else {
            (y - self.cell_height as i32 + 1) / self.cell_height as i32
        };
        (col * self.cell_width as i32, row * self.cell_height as i32)
    }

    /// Convert pixel position to grid column/row.
    pub fn to_cell(&self, x: i32, y: i32) -> (i32, i32) {
        let col = if x >= 0 {
            x / self.cell_width as i32
        } else {
            (x - self.cell_width as i32 + 1) / self.cell_width as i32
        };
        let row = if y >= 0 {
            y / self.cell_height as i32
        } else {
            (y - self.cell_height as i32 + 1) / self.cell_height as i32
        };
        (col, row)
    }

    /// Convert grid column/row to pixel position.
    pub fn from_cell(&self, col: i32, row: i32) -> (i32, i32) {
        (col * self.cell_width as i32, row * self.cell_height as i32)
    }

    /// Number of columns that fit within a given width.
    pub fn columns_in(&self, width: u32) -> u32 {
        if self.cell_width == 0 {
            return 0;
        }
        width / self.cell_width
    }

    /// Number of rows that fit within a given height.
    pub fn rows_in(&self, height: u32) -> u32 {
        if self.cell_height == 0 {
            return 0;
        }
        height / self.cell_height
    }
}

// ============================================================================
// Desktop Icon Layer
// ============================================================================

/// The desktop icon layer — manages placement, selection, drag, and rendering.
pub struct DesktopIconLayer {
    /// All icons on the desktop.
    icons: Vec<DesktopIcon>,
    /// Next ID to assign.
    next_id: u32,
    /// Grid configuration.
    pub grid: GridConfig,
    /// Arrangement mode.
    pub arrangement: ArrangementMode,
    /// Current interaction state.
    interaction: InteractionState,
    /// Screen dimensions.
    screen_width: u32,
    screen_height: u32,
    /// Taskbar height (icons must not overlap the taskbar).
    taskbar_height: u32,
}

impl DesktopIconLayer {
    /// Create a new icon layer for the given screen dimensions.
    pub fn new(screen_width: u32, screen_height: u32, taskbar_height: u32) -> Self {
        Self {
            icons: Vec::new(),
            next_id: 1,
            grid: GridConfig::default(),
            arrangement: ArrangementMode::FreeWithSnap,
            interaction: InteractionState::Idle,
            screen_width,
            screen_height,
            taskbar_height,
        }
    }

    /// Usable area height (excluding taskbar).
    fn usable_height(&self) -> u32 {
        self.screen_height.saturating_sub(self.taskbar_height)
    }

    // ======================================================================
    // Icon management
    // ======================================================================

    /// Add an icon and return its ID.
    pub fn add_icon(
        &mut self,
        label: &str,
        icon_type: IconType,
        action: IconAction,
        x: i32,
        y: i32,
    ) -> IconId {
        let id = IconId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);

        let (snapped_x, snapped_y) = self.grid.snap(x, y);

        self.icons.push(DesktopIcon {
            id,
            x: snapped_x,
            y: snapped_y,
            label: label.to_string(),
            icon_type,
            action,
            selected: false,
        });

        id
    }

    /// Add an icon at the next available grid position.
    pub fn add_icon_auto(&mut self, label: &str, icon_type: IconType, action: IconAction) -> IconId {
        let pos = self.next_free_cell();
        self.add_icon(label, icon_type, action, pos.0, pos.1)
    }

    /// Remove an icon by ID.
    pub fn remove_icon(&mut self, id: IconId) {
        self.icons.retain(|icon| icon.id != id);
    }

    /// Get a reference to an icon by ID.
    pub fn get_icon(&self, id: IconId) -> Option<&DesktopIcon> {
        self.icons.iter().find(|icon| icon.id == id)
    }

    /// Get a mutable reference to an icon by ID.
    pub fn get_icon_mut(&mut self, id: IconId) -> Option<&mut DesktopIcon> {
        self.icons.iter_mut().find(|icon| icon.id == id)
    }

    /// Populate default desktop icons (This PC, Recycle Bin, Documents, Home).
    pub fn populate_defaults(&mut self) {
        let cw = self.grid.cell_width as i32;
        let y_start = EDGE_PADDING as i32;

        self.add_icon(
            "This PC",
            IconType::Computer,
            IconAction::LaunchSystem("explorer --computer".to_string()),
            EDGE_PADDING as i32,
            y_start,
        );

        self.add_icon(
            "Recycle Bin",
            IconType::RecycleBin,
            IconAction::LaunchSystem("explorer --recycle-bin".to_string()),
            EDGE_PADDING as i32,
            y_start + self.grid.cell_height as i32,
        );

        self.add_icon(
            "Documents",
            IconType::Folder,
            IconAction::OpenPath("/home/user/Documents".to_string()),
            EDGE_PADDING as i32,
            y_start + self.grid.cell_height as i32 * 2,
        );

        self.add_icon(
            "Home",
            IconType::Folder,
            IconAction::OpenPath("/home/user".to_string()),
            EDGE_PADDING as i32,
            y_start + self.grid.cell_height as i32 * 3,
        );

        // Suppress unused variable warning — cw reserved for multi-column layouts.
        let _ = cw;
    }

    // ======================================================================
    // Selection
    // ======================================================================

    /// Select a single icon, deselecting all others.
    pub fn select_single(&mut self, id: IconId) {
        for icon in &mut self.icons {
            icon.selected = icon.id == id;
        }
    }

    /// Toggle selection of a single icon (Ctrl+Click behavior).
    pub fn toggle_selection(&mut self, id: IconId) {
        if let Some(icon) = self.icons.iter_mut().find(|i| i.id == id) {
            icon.selected = !icon.selected;
        }
    }

    /// Select all icons.
    pub fn select_all(&mut self) {
        for icon in &mut self.icons {
            icon.selected = true;
        }
    }

    /// Deselect all icons.
    pub fn deselect_all(&mut self) {
        for icon in &mut self.icons {
            icon.selected = false;
        }
    }

    /// Select icons within a rectangle (rubber-band selection).
    pub fn select_in_rect(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, additive: bool) {
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);

        for icon in &mut self.icons {
            let icon_cx = icon.x as f32 + self.grid.cell_width as f32 / 2.0;
            let icon_cy = icon.y as f32 + self.grid.cell_height as f32 / 2.0;

            let in_rect =
                icon_cx >= min_x && icon_cx <= max_x && icon_cy >= min_y && icon_cy <= max_y;

            if additive {
                if in_rect {
                    icon.selected = true;
                }
            } else {
                icon.selected = in_rect;
            }
        }
    }

    /// Get all currently selected icon IDs.
    pub fn selected_ids(&self) -> Vec<IconId> {
        self.icons
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.id)
            .collect()
    }

    // ======================================================================
    // Hit testing
    // ======================================================================

    /// Find the icon at a given pixel position, if any.
    pub fn icon_at(&self, x: f32, y: f32) -> Option<IconId> {
        // Iterate in reverse so topmost (last-added) icon wins on overlap.
        for icon in self.icons.iter().rev() {
            let ix = icon.x as f32;
            let iy = icon.y as f32;
            let iw = self.grid.cell_width as f32;
            let ih = self.grid.cell_height as f32;

            if x >= ix && x < ix + iw && y >= iy && y < iy + ih {
                return Some(icon.id);
            }
        }
        None
    }

    // ======================================================================
    // Arrangement
    // ======================================================================

    /// Find the next free grid cell (scanning top-to-bottom, left-to-right).
    pub fn next_free_cell(&self) -> (i32, i32) {
        let cols = self.grid.columns_in(self.screen_width.saturating_sub(EDGE_PADDING * 2));
        let rows = self.grid.rows_in(self.usable_height().saturating_sub(EDGE_PADDING * 2));

        // Map each icon's stored top-left back to its grid cell.  Icons
        // placed via `add_icon` are snapped (no padding added), and icons
        // placed via `auto_arrange` get `EDGE_PADDING` added on top — both
        // still resolve to the same cell here because `EDGE_PADDING` (8) is
        // far smaller than the cell size (80x90).
        let occupied: Vec<(i32, i32)> = self
            .icons
            .iter()
            .map(|i| self.grid.to_cell(i.x, i.y))
            .collect();

        // Scan columns first (top to bottom within each column, then next column).
        for col in 0..cols as i32 {
            for row in 0..rows as i32 {
                if !occupied.contains(&(col, row)) {
                    let (px, py) = self.grid.from_cell(col, row);
                    return (px + EDGE_PADDING as i32, py + EDGE_PADDING as i32);
                }
            }
        }

        // Fallback: just place at origin.
        (EDGE_PADDING as i32, EDGE_PADDING as i32)
    }

    /// Auto-arrange all icons into a grid, sorted alphabetically.
    pub fn auto_arrange(&mut self) {
        // Sort by label (case-insensitive).
        self.icons.sort_by(|a, b| {
            a.label
                .to_lowercase()
                .cmp(&b.label.to_lowercase())
        });

        let cols = self
            .grid
            .columns_in(self.screen_width.saturating_sub(EDGE_PADDING * 2));
        let rows = self
            .grid
            .rows_in(self.usable_height().saturating_sub(EDGE_PADDING * 2));

        for (idx, icon) in self.icons.iter_mut().enumerate() {
            // Fill column-first (top to bottom, then next column).
            let col = if rows == 0 {
                idx as u32
            } else {
                (idx as u32) / rows
            };
            let row = if rows == 0 { 0 } else { (idx as u32) % rows };

            if col >= cols {
                // Overflow — wrap or leave at last valid column.
                let wrapped_col = col % cols.max(1);
                let (px, py) = GridConfig::default().from_cell(wrapped_col as i32, row as i32);
                icon.x = px + EDGE_PADDING as i32;
                icon.y = py + EDGE_PADDING as i32;
            } else {
                let (px, py) = GridConfig::default().from_cell(col as i32, row as i32);
                icon.x = px + EDGE_PADDING as i32;
                icon.y = py + EDGE_PADDING as i32;
            }
        }
    }

    // ======================================================================
    // Mouse interaction
    // ======================================================================

    /// Handle mouse button press. Returns an event if one is generated.
    pub fn handle_mouse_down(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        ctrl_held: bool,
    ) -> IconEvent {
        if button == MouseButton::Right {
            let hit = self.icon_at(x, y);
            if let Some(id) = hit {
                // If right-clicking an unselected icon, select it alone.
                if !self.icons.iter().any(|i| i.id == id && i.selected) {
                    self.select_single(id);
                }
            }
            return IconEvent::ContextMenu {
                x: x as i32,
                y: y as i32,
                icon_id: hit,
            };
        }

        if button != MouseButton::Left {
            return IconEvent::None;
        }

        if let Some(id) = self.icon_at(x, y) {
            // Clicked on an icon.
            if ctrl_held {
                self.toggle_selection(id);
            } else if !self.icons.iter().any(|i| i.id == id && i.selected) {
                self.select_single(id);
            }

            // Begin pending drag.
            let selected = self.selected_ids();
            self.interaction = InteractionState::PendingDrag {
                start_x: x,
                start_y: y,
                icon_ids: selected,
            };
        } else {
            // Clicked on empty desktop — start rubber-band.
            if !ctrl_held {
                self.deselect_all();
            }
            self.interaction = InteractionState::RubberBand {
                start_x: x,
                start_y: y,
                current_x: x,
                current_y: y,
            };
        }

        IconEvent::None
    }

    /// Handle mouse movement (drag tracking).
    pub fn handle_mouse_move(&mut self, x: f32, y: f32, ctrl_held: bool) {
        match &self.interaction {
            InteractionState::PendingDrag {
                start_x,
                start_y,
                icon_ids,
            } => {
                let dx = x - start_x;
                let dy = y - start_y;
                if dx * dx + dy * dy >= DRAG_THRESHOLD_SQ {
                    // Exceeded drag threshold — transition to dragging.
                    let originals: Vec<(IconId, i32, i32)> = icon_ids
                        .iter()
                        .filter_map(|id| {
                            self.icons
                                .iter()
                                .find(|i| i.id == *id)
                                .map(|i| (i.id, i.x, i.y))
                        })
                        .collect();

                    self.interaction = InteractionState::Dragging {
                        start_x: *start_x,
                        start_y: *start_y,
                        current_x: x,
                        current_y: y,
                        originals,
                    };
                }
            }
            InteractionState::Dragging {
                start_x,
                start_y,
                originals,
                ..
            } => {
                // Update current drag position. Clone to satisfy borrow checker.
                let sx = *start_x;
                let sy = *start_y;
                let orig = originals.clone();
                self.interaction = InteractionState::Dragging {
                    start_x: sx,
                    start_y: sy,
                    current_x: x,
                    current_y: y,
                    originals: orig,
                };
            }
            InteractionState::RubberBand {
                start_x, start_y, ..
            } => {
                let sx = *start_x;
                let sy = *start_y;
                self.interaction = InteractionState::RubberBand {
                    start_x: sx,
                    start_y: sy,
                    current_x: x,
                    current_y: y,
                };
                // Update selection based on rubber-band rectangle.
                self.select_in_rect(sx, sy, x, y, ctrl_held);
            }
            InteractionState::Idle => {}
        }
    }

    /// Handle mouse button release.
    pub fn handle_mouse_up(&mut self, _x: f32, _y: f32, button: MouseButton) {
        if button != MouseButton::Left {
            return;
        }

        match &self.interaction {
            InteractionState::Dragging {
                start_x,
                start_y,
                current_x,
                current_y,
                originals,
            } => {
                // Drop: move icons by the delta, snapping to grid.
                let dx = *current_x - *start_x;
                let dy = *current_y - *start_y;

                let originals_snapshot = originals.clone();
                self.interaction = InteractionState::Idle;

                for (id, orig_x, orig_y) in &originals_snapshot {
                    let new_x = *orig_x + dx as i32;
                    let new_y = *orig_y + dy as i32;
                    let (snapped_x, snapped_y) = self.grid.snap(new_x, new_y);

                    if let Some(icon) = self.icons.iter_mut().find(|i| i.id == *id) {
                        icon.x = snapped_x;
                        icon.y = snapped_y;
                    }
                }

                if self.arrangement == ArrangementMode::AutoArrange {
                    self.auto_arrange();
                }
            }
            _ => {
                self.interaction = InteractionState::Idle;
            }
        }
    }

    /// Handle double-click at a position.
    pub fn handle_double_click(&mut self, x: f32, y: f32) -> IconEvent {
        if let Some(id) = self.icon_at(x, y) {
            if let Some(icon) = self.icons.iter().find(|i| i.id == id) {
                return IconEvent::Activate(id, icon.action.clone());
            }
        }
        IconEvent::None
    }

    /// Handle keyboard input. Returns an event if one is generated.
    pub fn handle_key(&mut self, key: DesktopKey, ctrl_held: bool) -> IconEvent {
        match key {
            DesktopKey::SelectAll if ctrl_held => {
                self.select_all();
                IconEvent::None
            }
            DesktopKey::Delete => {
                let selected = self.selected_ids();
                if selected.is_empty() {
                    return IconEvent::None;
                }
                IconEvent::Delete(selected)
            }
            DesktopKey::F2 => {
                let selected = self.selected_ids();
                if selected.len() == 1 {
                    IconEvent::BeginRename(selected[0])
                } else {
                    IconEvent::None
                }
            }
            DesktopKey::Enter => {
                let selected = self.selected_ids();
                if selected.len() == 1 {
                    if let Some(icon) = self.icons.iter().find(|i| i.id == selected[0]) {
                        return IconEvent::Activate(selected[0], icon.action.clone());
                    }
                }
                IconEvent::None
            }
            _ => IconEvent::None,
        }
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Produce render commands for the entire icon layer.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::new();

        // Render each icon.
        for icon in &self.icons {
            self.render_icon(icon, &mut cmds);
        }

        // Render drag ghosts (translucent copies at drag position).
        if let InteractionState::Dragging {
            start_x,
            start_y,
            current_x,
            current_y,
            originals,
        } = &self.interaction
        {
            let dx = *current_x - *start_x;
            let dy = *current_y - *start_y;

            for (id, orig_x, orig_y) in originals {
                if let Some(icon) = self.icons.iter().find(|i| i.id == *id) {
                    let ghost_x = *orig_x as f32 + dx;
                    let ghost_y = *orig_y as f32 + dy;

                    // Ghost background (translucent).
                    cmds.push(RenderCommand::FillRect {
                        x: ghost_x,
                        y: ghost_y,
                        width: self.grid.cell_width as f32,
                        height: self.grid.cell_height as f32,
                        color: Color::rgba(137, 180, 250, 30),
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Ghost glyph.
                    let glyph_x = ghost_x + (self.grid.cell_width as f32 - ICON_GLYPH_SIZE) / 2.0;
                    let glyph_y = ghost_y + ICON_TOP_PADDING;
                    cmds.push(RenderCommand::Text {
                        x: glyph_x,
                        y: glyph_y,
                        text: icon.icon_type.glyph().to_string(),
                        color: Color::rgba(
                            icon.icon_type.color().r,
                            icon.icon_type.color().g,
                            icon.icon_type.color().b,
                            120,
                        ),
                        font_size: ICON_GLYPH_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });
                }
            }

            // Drop target highlight at snapped position.
            if let Some((_id, orig_x, orig_y)) = originals.first() {
                let target_x = *orig_x as f32 + dx;
                let target_y = *orig_y as f32 + dy;
                let (snap_x, snap_y) = self.grid.snap(target_x as i32, target_y as i32);

                cmds.push(RenderCommand::StrokeRect {
                    x: snap_x as f32,
                    y: snap_y as f32,
                    width: self.grid.cell_width as f32,
                    height: self.grid.cell_height as f32,
                    color: theme::DROP_TARGET,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // Render rubber-band selection rectangle.
        if let InteractionState::RubberBand {
            start_x,
            start_y,
            current_x,
            current_y,
        } = &self.interaction
        {
            let rx = start_x.min(*current_x);
            let ry = start_y.min(*current_y);
            let rw = (current_x - start_x).abs();
            let rh = (current_y - start_y).abs();

            cmds.push(RenderCommand::FillRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: theme::RUBBERBAND_FILL,
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: theme::RUBBERBAND_BORDER,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds
    }

    /// Render a single icon into the command list.
    fn render_icon(&self, icon: &DesktopIcon, cmds: &mut Vec<RenderCommand>) {
        let ix = icon.x as f32;
        let iy = icon.y as f32;
        let cw = self.grid.cell_width as f32;
        let ch = self.grid.cell_height as f32;

        // Selection highlight.
        if icon.selected {
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: iy,
                width: cw,
                height: ch,
                color: theme::SELECTION_BG,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: ix,
                y: iy,
                width: cw,
                height: ch,
                color: theme::SELECTION_BORDER,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Icon glyph (centered horizontally within the cell).
        let glyph_x = ix + (cw - ICON_GLYPH_SIZE) / 2.0;
        let glyph_y = iy + ICON_TOP_PADDING;

        cmds.push(RenderCommand::Text {
            x: glyph_x,
            y: glyph_y,
            text: icon.icon_type.glyph().to_string(),
            color: icon.icon_type.color(),
            font_size: ICON_GLYPH_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Label text below icon (centered, 2-line max with ellipsis).
        let label_y = iy + ICON_TOP_PADDING + ICON_GLYPH_SIZE + 6.0;
        let lines = wrap_label(&icon.label, LABEL_MAX_CHARS_PER_LINE, LABEL_MAX_LINES);

        for (line_idx, line) in lines.iter().enumerate() {
            let ly = label_y + line_idx as f32 * (LABEL_FONT_SIZE + 2.0);

            // Shadow for readability against varied backgrounds.
            cmds.push(RenderCommand::Text {
                x: ix + 1.0,
                y: ly + 1.0,
                text: line.clone(),
                color: theme::LABEL_SHADOW,
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(LABEL_MAX_WIDTH),
            });

            // Actual label text.
            cmds.push(RenderCommand::Text {
                x: ix,
                y: ly,
                text: line.clone(),
                color: if icon.selected {
                    theme::TEXT
                } else {
                    theme::SUBTEXT0
                },
                font_size: LABEL_FONT_SIZE,
                font_weight: if icon.selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(LABEL_MAX_WIDTH),
            });
        }
    }

    /// Update screen dimensions (e.g., on resolution change).
    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
    }
}

// ============================================================================
// Helper types
// ============================================================================

/// Mouse button (simplified).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Simplified key events relevant to the icon layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopKey {
    Delete,
    F2,
    Enter,
    SelectAll,
    Escape,
}

// ============================================================================
// Label wrapping
// ============================================================================

/// Wrap a label into at most `max_lines` lines, each at most `max_chars`.
/// The last line gets ellipsis if the text overflows.
fn wrap_label(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    if text.is_empty() || max_lines == 0 {
        return vec![String::new()];
    }

    let chars: Vec<char> = text.chars().collect();

    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut pos = 0;

    while pos < chars.len() && lines.len() < max_lines {
        let is_last_allowed_line = lines.len() + 1 == max_lines;
        let remaining = chars.len() - pos;

        if remaining <= max_chars {
            // Fits on this line.
            lines.push(chars[pos..].iter().collect());
            break;
        }

        if is_last_allowed_line {
            // Last line — truncate with ellipsis.
            let end = pos + max_chars.saturating_sub(1);
            let mut line: String = chars[pos..end.min(chars.len())].iter().collect();
            line.push('\u{2026}'); // ellipsis character
            lines.push(line);
            break;
        }

        // Try to break at a word boundary.
        let chunk_end = (pos + max_chars).min(chars.len());
        let chunk = &chars[pos..chunk_end];

        // Look for last space in the chunk to break at.
        let break_at = chunk
            .iter()
            .rposition(|c| *c == ' ' || *c == '-' || *c == '_' || *c == '.')
            .map(|i| i + 1)
            .unwrap_or(max_chars);

        let line: String = chars[pos..pos + break_at].iter().collect();
        lines.push(line.trim_end().to_string());
        pos += break_at;
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Grid snapping tests
    // ------------------------------------------------------------------

    #[test]
    fn grid_snap_positive_aligned() {
        let grid = GridConfig {
            cell_width: 80,
            cell_height: 90,
        };
        assert_eq!(grid.snap(0, 0), (0, 0));
        assert_eq!(grid.snap(80, 90), (80, 90));
        assert_eq!(grid.snap(160, 180), (160, 180));
    }

    #[test]
    fn grid_snap_positive_unaligned() {
        let grid = GridConfig {
            cell_width: 80,
            cell_height: 90,
        };
        // Should snap to nearest lower-left cell origin.
        assert_eq!(grid.snap(10, 10), (0, 0));
        assert_eq!(grid.snap(79, 89), (0, 0));
        assert_eq!(grid.snap(81, 91), (80, 90));
        assert_eq!(grid.snap(120, 135), (80, 90));
        assert_eq!(grid.snap(159, 179), (80, 90));
    }

    #[test]
    fn grid_snap_negative_coords() {
        let grid = GridConfig {
            cell_width: 80,
            cell_height: 90,
        };
        assert_eq!(grid.snap(-1, -1), (-80, -90));
        assert_eq!(grid.snap(-80, -90), (-80, -90));
        assert_eq!(grid.snap(-81, -91), (-160, -180));
    }

    #[test]
    fn grid_to_cell_and_back() {
        let grid = GridConfig {
            cell_width: 80,
            cell_height: 90,
        };
        assert_eq!(grid.to_cell(0, 0), (0, 0));
        assert_eq!(grid.to_cell(80, 90), (1, 1));
        assert_eq!(grid.to_cell(160, 270), (2, 3));
        assert_eq!(grid.from_cell(2, 3), (160, 270));
    }

    #[test]
    fn grid_columns_and_rows() {
        let grid = GridConfig {
            cell_width: 80,
            cell_height: 90,
        };
        assert_eq!(grid.columns_in(1920), 24);
        assert_eq!(grid.rows_in(1040), 11); // 1080 - 40 taskbar
        assert_eq!(grid.columns_in(0), 0);
        assert_eq!(grid.rows_in(0), 0);
    }

    #[test]
    fn grid_zero_cell_size() {
        let grid = GridConfig {
            cell_width: 0,
            cell_height: 0,
        };
        assert_eq!(grid.columns_in(1920), 0);
        assert_eq!(grid.rows_in(1080), 0);
    }

    // ------------------------------------------------------------------
    // Selection tests
    // ------------------------------------------------------------------

    #[test]
    fn select_single_deselects_others() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);
        let id2 = layer.add_icon("B", IconType::File, IconAction::OpenPath("/b".into()), 80, 0);

        layer.select_all();
        assert_eq!(layer.selected_ids().len(), 2);

        layer.select_single(id1);
        assert_eq!(layer.selected_ids(), vec![id1]);

        let icon2 = layer.get_icon(id2).unwrap();
        assert!(!icon2.selected);
    }

    #[test]
    fn toggle_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);

        assert!(!layer.get_icon(id1).unwrap().selected);

        layer.toggle_selection(id1);
        assert!(layer.get_icon(id1).unwrap().selected);

        layer.toggle_selection(id1);
        assert!(!layer.get_icon(id1).unwrap().selected);
    }

    #[test]
    fn select_all_and_deselect_all() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);
        layer.add_icon("B", IconType::File, IconAction::OpenPath("/b".into()), 80, 0);
        layer.add_icon("C", IconType::File, IconAction::OpenPath("/c".into()), 160, 0);

        layer.select_all();
        assert_eq!(layer.selected_ids().len(), 3);

        layer.deselect_all();
        assert_eq!(layer.selected_ids().len(), 0);
    }

    #[test]
    fn rubber_band_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Place icons in a known grid.
        layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);
        layer.add_icon("B", IconType::File, IconAction::OpenPath("/b".into()), 80, 0);
        layer.add_icon("C", IconType::File, IconAction::OpenPath("/c".into()), 0, 90);

        // Select a rectangle that covers A and C (first column).
        // Center of A = (40, 45), center of C = (40, 135), center of B = (120, 45).
        layer.select_in_rect(0.0, 0.0, 79.0, 180.0, false);

        let selected = layer.selected_ids();
        assert_eq!(selected.len(), 2);
        // B should not be selected (its center is at x=120, outside rect).
        assert!(!layer.get_icon(IconId(2)).unwrap().selected);
    }

    // ------------------------------------------------------------------
    // Arrangement tests
    // ------------------------------------------------------------------

    #[test]
    fn auto_arrange_sorts_alphabetically() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon("Zebra", IconType::File, IconAction::OpenPath("/z".into()), 500, 500);
        layer.add_icon("Apple", IconType::File, IconAction::OpenPath("/a".into()), 300, 300);
        layer.add_icon("Mango", IconType::File, IconAction::OpenPath("/m".into()), 100, 100);

        layer.auto_arrange();

        // After auto-arrange, alphabetical order: Apple, Mango, Zebra.
        assert_eq!(layer.icons[0].label, "Apple");
        assert_eq!(layer.icons[1].label, "Mango");
        assert_eq!(layer.icons[2].label, "Zebra");
    }

    #[test]
    fn auto_arrange_places_in_grid() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        for i in 0..5 {
            layer.add_icon(
                &format!("Icon{i}"),
                IconType::File,
                IconAction::OpenPath(format!("/{i}")),
                999,
                999,
            );
        }

        layer.auto_arrange();

        // With default grid (80x90), screen 1920x1080, taskbar 40:
        // usable height = 1040, minus 16 edge padding = 1024
        // rows = 1024 / 90 = 11
        // So 5 icons should fill first column (rows 0..4).
        let grid = GridConfig::default();
        for (idx, icon) in layer.icons.iter().enumerate() {
            let expected_col = idx as u32 / 11;
            let expected_row = idx as u32 % 11;
            let (ex, ey) = grid.from_cell(expected_col as i32, expected_row as i32);
            assert_eq!(icon.x, ex + EDGE_PADDING as i32, "icon {idx} x mismatch");
            assert_eq!(icon.y, ey + EDGE_PADDING as i32, "icon {idx} y mismatch");
        }
    }

    #[test]
    fn next_free_cell_avoids_occupied() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Occupy the first cell.
        layer.add_icon(
            "First",
            IconType::File,
            IconAction::OpenPath("/first".into()),
            EDGE_PADDING as i32,
            EDGE_PADDING as i32,
        );

        let next = layer.next_free_cell();
        // Should be second row, first column.
        let (expected_x, expected_y) = layer.grid.from_cell(0, 1);
        assert_eq!(next, (expected_x + EDGE_PADDING as i32, expected_y + EDGE_PADDING as i32));
    }

    // ------------------------------------------------------------------
    // Hit testing
    // ------------------------------------------------------------------

    #[test]
    fn icon_at_hit() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon("Test", IconType::File, IconAction::OpenPath("/t".into()), 0, 0);

        // Click inside the cell (grid snaps to 0,0).
        assert_eq!(layer.icon_at(40.0, 45.0), Some(id));
        // Click outside.
        assert_eq!(layer.icon_at(200.0, 200.0), None);
    }

    #[test]
    fn icon_at_returns_topmost() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Two icons at the same position (overlapping).
        let _id1 = layer.add_icon("Under", IconType::File, IconAction::OpenPath("/u".into()), 0, 0);
        let id2 = layer.add_icon("Over", IconType::File, IconAction::OpenPath("/o".into()), 0, 0);

        // Should return the later-added (topmost) icon.
        assert_eq!(layer.icon_at(40.0, 45.0), Some(id2));
    }

    // ------------------------------------------------------------------
    // Label wrapping tests
    // ------------------------------------------------------------------

    #[test]
    fn wrap_short_label() {
        let lines = wrap_label("Hello", 12, 2);
        assert_eq!(lines, vec!["Hello"]);
    }

    #[test]
    fn wrap_long_label_two_lines() {
        let lines = wrap_label("My Documents Folder", 12, 2);
        assert_eq!(lines.len(), 2);
        // First line should break at a word boundary.
        assert!(lines[0].chars().count() <= 12);
        // Second line should end with ellipsis if it overflows.  Length
        // is measured in chars (display cells), not bytes — the ellipsis
        // is a single Unicode '…' which occupies multiple UTF-8 bytes.
        assert!(lines[1].chars().count() <= 12);
        assert!(lines[1].ends_with('\u{2026}'));
    }

    #[test]
    fn wrap_exact_boundary() {
        let lines = wrap_label("ABCDEFGHIJKL", 12, 2);
        // Exactly 12 chars = fits on one line.
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "ABCDEFGHIJKL");
    }

    #[test]
    fn wrap_empty_label() {
        let lines = wrap_label("", 12, 2);
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn wrap_single_line_truncation() {
        let lines = wrap_label("VeryLongFileNameThatExceedsLimit", 12, 1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with('\u{2026}')); // ellipsis
        assert!(lines[0].chars().count() <= 12);
    }

    // ------------------------------------------------------------------
    // Double-click / action tests
    // ------------------------------------------------------------------

    #[test]
    fn double_click_activates_icon() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "Test",
            IconType::Computer,
            IconAction::LaunchSystem("explorer --computer".into()),
            0,
            0,
        );

        let event = layer.handle_double_click(40.0, 45.0);
        assert_eq!(
            event,
            IconEvent::Activate(id, IconAction::LaunchSystem("explorer --computer".into()))
        );
    }

    #[test]
    fn double_click_on_empty_returns_none() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon("Test", IconType::File, IconAction::OpenPath("/t".into()), 0, 0);

        let event = layer.handle_double_click(500.0, 500.0);
        assert_eq!(event, IconEvent::None);
    }

    // ------------------------------------------------------------------
    // Keyboard action tests
    // ------------------------------------------------------------------

    #[test]
    fn delete_key_returns_selected_ids() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);
        let id2 = layer.add_icon("B", IconType::File, IconAction::OpenPath("/b".into()), 80, 0);

        layer.select_all();
        let event = layer.handle_key(DesktopKey::Delete, false);
        assert_eq!(event, IconEvent::Delete(vec![id1, id2]));
    }

    #[test]
    fn f2_begins_rename_for_single_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);

        layer.select_single(id);
        let event = layer.handle_key(DesktopKey::F2, false);
        assert_eq!(event, IconEvent::BeginRename(id));
    }

    #[test]
    fn f2_does_nothing_for_multi_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon("A", IconType::File, IconAction::OpenPath("/a".into()), 0, 0);
        layer.add_icon("B", IconType::File, IconAction::OpenPath("/b".into()), 80, 0);

        layer.select_all();
        let event = layer.handle_key(DesktopKey::F2, false);
        assert_eq!(event, IconEvent::None);
    }

    // ------------------------------------------------------------------
    // Default icons test
    // ------------------------------------------------------------------

    #[test]
    fn populate_defaults_creates_four_icons() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.populate_defaults();
        assert_eq!(layer.icons.len(), 4);

        // Verify expected icons exist.
        assert!(layer.icons.iter().any(|i| i.label == "This PC"));
        assert!(layer.icons.iter().any(|i| i.label == "Recycle Bin"));
        assert!(layer.icons.iter().any(|i| i.label == "Documents"));
        assert!(layer.icons.iter().any(|i| i.label == "Home"));
    }

    // ------------------------------------------------------------------
    // Render produces output
    // ------------------------------------------------------------------

    #[test]
    fn render_produces_commands_for_icons() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.populate_defaults();

        let cmds = layer.render();
        // Each icon produces: glyph text + label shadow + label text (minimum).
        // Selected icons also get highlight rect + border.
        assert!(!cmds.is_empty());
        // At least 3 commands per icon (glyph + shadow + text) * 4 icons = 12.
        assert!(cmds.len() >= 12);
    }

    #[test]
    fn render_selected_icon_has_highlight() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon("Sel", IconType::File, IconAction::OpenPath("/s".into()), 0, 0);
        layer.select_single(id);

        let cmds = layer.render();
        // First command for a selected icon should be the selection FillRect.
        let has_fill = cmds.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if *color == theme::SELECTION_BG)
        });
        assert!(has_fill, "Selected icon should have a selection highlight");
    }
}
