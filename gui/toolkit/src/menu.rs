//! Context menu and tooltip widgets.
//!
//! Provides a popup context menu (with submenus, keyboard navigation,
//! separators, icons, and check marks) and a tooltip that appears after
//! a configurable hover delay. Both produce `RenderCommand` lists and
//! use the Catppuccin Mocha dark theme.

use crate::color::Color;
use crate::event::{Key, KeyEvent};
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;

// ─── Catppuccin Mocha palette ───────────────────────────────────────────────

/// Dark background for menus and tooltips.
const BG_COLOR: Color = Color::from_hex(0x1E1E2E);
/// Slightly lighter surface for hover highlights.
const HOVER_COLOR: Color = Color::from_hex(0x313244);
/// Primary text color (light).
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
/// Dimmed text for disabled items and secondary info.
const DIM_TEXT_COLOR: Color = Color::from_hex(0x6C7086);
/// Accent color for checkmarks and active indicators.
const ACCENT_COLOR: Color = Color::from_hex(0x89B4FA);
/// Separator line color.
const SEPARATOR_COLOR: Color = Color::from_hex(0x45475A);
/// Shadow color (semi-transparent black).
const SHADOW_COLOR: Color = Color::rgba(0, 0, 0, 160);
/// Border color for menu outline.
const BORDER_COLOR: Color = Color::from_hex(0x45475A);

// ─── Layout constants ───────────────────────────────────────────────────────

const ITEM_HEIGHT: f32 = 28.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const ICON_COLUMN_WIDTH: f32 = 28.0;
const SHORTCUT_PADDING: f32 = 40.0;
const HORIZONTAL_PADDING: f32 = 8.0;
const VERTICAL_PADDING: f32 = 4.0;
const FONT_SIZE: f32 = 13.0;
const CORNER_RADIUS: f32 = 6.0;
const SHADOW_BLUR: f32 = 12.0;
const SHADOW_OFFSET: f32 = 4.0;
const SUBMENU_ARROW_WIDTH: f32 = 20.0;
const MIN_MENU_WIDTH: f32 = 160.0;

// ─── Viewport bounds (used for edge-flip logic) ─────────────────────────────

/// Default viewport width used for edge detection when flipping menu position.
const DEFAULT_VIEWPORT_WIDTH: f32 = 1920.0;
/// Default viewport height used for edge detection when flipping menu position.
const DEFAULT_VIEWPORT_HEIGHT: f32 = 1080.0;

// ─── Menu types ─────────────────────────────────────────────────────────────

/// Unique identifier for a menu item.
pub type MenuItemId = u64;

/// A single item in a context menu.
#[derive(Clone, Debug)]
pub enum MenuItem {
    /// Regular clickable item.
    Action {
        id: MenuItemId,
        label: String,
        shortcut: Option<String>,
        icon: Option<String>,
        enabled: bool,
        /// `None` means not checkable; `Some(true/false)` means checkbox state.
        checked: Option<bool>,
    },
    /// Visual separator line between groups of items.
    Separator,
    /// Submenu that opens on hover.
    Submenu {
        id: MenuItemId,
        label: String,
        icon: Option<String>,
        enabled: bool,
        children: Vec<MenuItem>,
    },
}

/// Result of handling a menu interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// An item was selected.
    Selected(MenuItemId),
    /// Menu was closed (e.g., Escape).
    Closed,
    /// No action taken.
    None,
}

/// A context menu or dropdown menu that renders as a popup overlay.
pub struct ContextMenu {
    items: Vec<MenuItem>,
    x: f32,
    y: f32,
    visible: bool,
    hover_index: Option<usize>,
    open_submenu: Option<(usize, Box<ContextMenu>)>,
    /// Auto-calculated width based on content.
    width: f32,
}

impl ContextMenu {
    /// Create a new context menu with the given items.
    pub fn new(items: Vec<MenuItem>) -> Self {
        let width = Self::calculate_width(&items);
        Self {
            items,
            x: 0.0,
            y: 0.0,
            visible: false,
            hover_index: None,
            open_submenu: None,
            width,
        }
    }

    /// Show the menu at the given position, adjusting for viewport edges.
    pub fn show(&mut self, x: f32, y: f32) {
        let total_height = self.total_height();

        // Flip horizontally if menu would overflow right edge.
        self.x = if x + self.width > DEFAULT_VIEWPORT_WIDTH {
            (x - self.width).max(0.0)
        } else {
            x
        };

        // Flip vertically if menu would overflow bottom edge.
        self.y = if y + total_height > DEFAULT_VIEWPORT_HEIGHT {
            (y - total_height).max(0.0)
        } else {
            y
        };

        self.visible = true;
        self.hover_index = None;
        self.open_submenu = None;
    }

    /// Hide the menu and any open submenus.
    pub fn hide(&mut self) {
        self.visible = false;
        self.hover_index = None;
        self.open_submenu = None;
    }

    /// Whether the menu is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Handle a mouse click. Returns the selected item ID if an action item was clicked.
    pub fn handle_click(&mut self, mx: f32, my: f32) -> Option<MenuItemId> {
        if !self.visible {
            return None;
        }

        // Check if click is in open submenu first.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && let Some(id) = submenu.handle_click(mx, my) {
                self.hide();
                return Some(id);
            }

        // Check if click is within our bounds.
        if !self.point_in_bounds(mx, my) {
            self.hide();
            return None;
        }

        let idx = self.index_at_y(my)?;

        match self.items.get(idx) {
            Some(MenuItem::Action { id, enabled: true, .. }) => {
                let id = *id;
                self.hide();
                Some(id)
            }
            Some(MenuItem::Submenu { enabled: true, children, .. }) => {
                // Clicking a submenu item opens it (same as hover).
                let mut sub = ContextMenu::new(children.clone());
                let sub_x = self.x + self.width;
                let sub_y = self.y + self.y_offset_for_index(idx);
                sub.show(sub_x, sub_y);
                self.open_submenu = Some((idx, Box::new(sub)));
                None
            }
            _ => None, // Separator, disabled item
        }
    }

    /// Handle mouse movement for hover highlighting and submenu opening.
    pub fn handle_mouse_move(&mut self, mx: f32, my: f32) {
        if !self.visible {
            return;
        }

        // Delegate to submenu if mouse is within it.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && submenu.point_in_bounds(mx, my) {
                submenu.handle_mouse_move(mx, my);
                return;
            }

        if !self.point_in_bounds(mx, my) {
            // Don't clear hover if mouse moved to a submenu.
            if self.open_submenu.as_ref().is_some_and(|(_, sub)| sub.point_in_bounds(mx, my)) {
                return;
            }
            self.hover_index = None;
            return;
        }

        let new_index = self.index_at_y(my);
        self.hover_index = new_index;

        // Open submenu if hovering over a submenu item.
        if let Some(idx) = new_index {
            match self.items.get(idx) {
                Some(MenuItem::Submenu { enabled: true, children, .. }) => {
                    // Only open if not already open for this index.
                    let already_open = self.open_submenu.as_ref().is_some_and(|(i, _)| *i == idx);
                    if !already_open {
                        let mut sub = ContextMenu::new(children.clone());
                        let sub_x = self.x + self.width;
                        let sub_y = self.y + self.y_offset_for_index(idx);
                        sub.show(sub_x, sub_y);
                        self.open_submenu = Some((idx, Box::new(sub)));
                    }
                }
                _ => {
                    // Close submenu if hovering over a non-submenu item.
                    self.open_submenu = None;
                }
            }
        }
    }

    /// Handle keyboard input for menu navigation.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<MenuAction> {
        if !self.visible || !key.pressed {
            return Some(MenuAction::None);
        }

        // Delegate to open submenu first.
        if let Some((_, ref mut submenu)) = self.open_submenu
            && submenu.is_visible() {
                let result = submenu.handle_key(key);
                if let Some(MenuAction::Selected(id)) = result {
                    self.hide();
                    return Some(MenuAction::Selected(id));
                }
                if let Some(MenuAction::Closed) = result {
                    // Left arrow or Escape in submenu closes it, returns focus to parent.
                    self.open_submenu = None;
                    return Some(MenuAction::None);
                }
                return result;
            }

        match key.key {
            Key::Escape => {
                self.hide();
                Some(MenuAction::Closed)
            }
            Key::Up => {
                self.move_hover(-1);
                Some(MenuAction::None)
            }
            Key::Down => {
                self.move_hover(1);
                Some(MenuAction::None)
            }
            Key::Enter => {
                if let Some(idx) = self.hover_index {
                    match self.items.get(idx) {
                        Some(MenuItem::Action { id, enabled: true, .. }) => {
                            let id = *id;
                            self.hide();
                            Some(MenuAction::Selected(id))
                        }
                        Some(MenuItem::Submenu { enabled: true, children, .. }) => {
                            let mut sub = ContextMenu::new(children.clone());
                            let sub_x = self.x + self.width;
                            let sub_y = self.y + self.y_offset_for_index(idx);
                            sub.show(sub_x, sub_y);
                            self.open_submenu = Some((idx, Box::new(sub)));
                            Some(MenuAction::None)
                        }
                        _ => Some(MenuAction::None),
                    }
                } else {
                    Some(MenuAction::None)
                }
            }
            Key::Right => {
                // Open submenu if hover is on a submenu item.
                if let Some(idx) = self.hover_index
                    && let Some(MenuItem::Submenu { enabled: true, children, .. }) = self.items.get(idx) {
                        let mut sub = ContextMenu::new(children.clone());
                        let sub_x = self.x + self.width;
                        let sub_y = self.y + self.y_offset_for_index(idx);
                        sub.show(sub_x, sub_y);
                        self.open_submenu = Some((idx, Box::new(sub)));
                    }
                Some(MenuAction::None)
            }
            Key::Left => {
                // Close submenu (handled by parent delegation above).
                Some(MenuAction::Closed)
            }
            _ => Some(MenuAction::None),
        }
    }

    /// Produce render commands for this menu and any open submenus.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let total_height = self.total_height();
        let radii = CornerRadii::all(CORNER_RADIUS);

        // Shadow behind menu.
        cmds.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width: self.width,
            height: total_height,
            offset_x: SHADOW_OFFSET,
            offset_y: SHADOW_OFFSET,
            blur: SHADOW_BLUR,
            spread: 0.0,
            color: SHADOW_COLOR,
            corner_radii: radii,
        });

        // Menu background.
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: total_height,
            color: BG_COLOR,
            corner_radii: radii,
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: total_height,
            color: BORDER_COLOR,
            line_width: 1.0,
            corner_radii: radii,
        });

        // Render each item.
        let mut current_y = self.y + VERTICAL_PADDING;
        for (i, item) in self.items.iter().enumerate() {
            match item {
                MenuItem::Separator => {
                    let line_y = current_y + SEPARATOR_HEIGHT / 2.0;
                    cmds.push(RenderCommand::Line {
                        x1: self.x + HORIZONTAL_PADDING,
                        y1: line_y,
                        x2: self.x + self.width - HORIZONTAL_PADDING,
                        y2: line_y,
                        color: SEPARATOR_COLOR,
                        width: 1.0,
                    });
                    current_y += SEPARATOR_HEIGHT;
                }
                MenuItem::Action { label, shortcut, enabled, checked, .. } => {
                    // Hover highlight.
                    if self.hover_index == Some(i) && *enabled {
                        cmds.push(RenderCommand::FillRect {
                            x: self.x + 4.0,
                            y: current_y,
                            width: self.width - 8.0,
                            height: ITEM_HEIGHT,
                            color: HOVER_COLOR,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    let text_color = if *enabled { TEXT_COLOR } else { DIM_TEXT_COLOR };
                    let text_y = current_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                    // Check mark.
                    if let Some(true) = checked {
                        cmds.push(RenderCommand::Text {
                            x: self.x + HORIZONTAL_PADDING + 4.0,
                            y: text_y,
                            text: "\u{2713}".to_string(), // checkmark
                            color: ACCENT_COLOR,
                            font_size: FONT_SIZE,
                            font_weight: FontWeightHint::Bold,
                            max_width: None,
                        });
                    }

                    // Label.
                    cmds.push(RenderCommand::Text {
                        x: self.x + HORIZONTAL_PADDING + ICON_COLUMN_WIDTH,
                        y: text_y,
                        text: label.clone(),
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });

                    // Shortcut text (right-aligned).
                    if let Some(shortcut_text) = shortcut {
                        cmds.push(RenderCommand::Text {
                            x: self.x + self.width - HORIZONTAL_PADDING - Self::estimate_text_width(shortcut_text, FONT_SIZE),
                            y: text_y,
                            text: shortcut_text.clone(),
                            color: DIM_TEXT_COLOR,
                            font_size: FONT_SIZE,
                            font_weight: FontWeightHint::Regular,
                            max_width: None,
                        });
                    }

                    current_y += ITEM_HEIGHT;
                }
                MenuItem::Submenu { label, enabled, .. } => {
                    // Hover highlight.
                    if self.hover_index == Some(i) && *enabled {
                        cmds.push(RenderCommand::FillRect {
                            x: self.x + 4.0,
                            y: current_y,
                            width: self.width - 8.0,
                            height: ITEM_HEIGHT,
                            color: HOVER_COLOR,
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }

                    let text_color = if *enabled { TEXT_COLOR } else { DIM_TEXT_COLOR };
                    let text_y = current_y + (ITEM_HEIGHT - FONT_SIZE) / 2.0;

                    // Label.
                    cmds.push(RenderCommand::Text {
                        x: self.x + HORIZONTAL_PADDING + ICON_COLUMN_WIDTH,
                        y: text_y,
                        text: label.clone(),
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });

                    // Submenu arrow indicator.
                    cmds.push(RenderCommand::Text {
                        x: self.x + self.width - HORIZONTAL_PADDING - SUBMENU_ARROW_WIDTH,
                        y: text_y,
                        text: "\u{25B8}".to_string(), // right-pointing triangle
                        color: text_color,
                        font_size: FONT_SIZE,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                    });

                    current_y += ITEM_HEIGHT;
                }
            }
        }

        // Render open submenu on top.
        if let Some((_, ref submenu)) = self.open_submenu {
            cmds.extend(submenu.render());
        }

        cmds
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    fn calculate_width(items: &[MenuItem]) -> f32 {
        let mut max_label_w: f32 = 0.0;
        let mut max_shortcut_w: f32 = 0.0;

        for item in items {
            match item {
                MenuItem::Action { label, shortcut, .. } => {
                    let label_w = Self::estimate_text_width(label, FONT_SIZE);
                    max_label_w = max_label_w.max(label_w);
                    if let Some(sc) = shortcut {
                        let sc_w = Self::estimate_text_width(sc, FONT_SIZE);
                        max_shortcut_w = max_shortcut_w.max(sc_w);
                    }
                }
                MenuItem::Submenu { label, .. } => {
                    let label_w = Self::estimate_text_width(label, FONT_SIZE);
                    max_label_w = max_label_w.max(label_w);
                    // Account for arrow indicator.
                    max_shortcut_w = max_shortcut_w.max(SUBMENU_ARROW_WIDTH);
                }
                MenuItem::Separator => {}
            }
        }

        let shortcut_space = if max_shortcut_w > 0.0 {
            SHORTCUT_PADDING + max_shortcut_w
        } else {
            0.0
        };

        let width = HORIZONTAL_PADDING * 2.0 + ICON_COLUMN_WIDTH + max_label_w + shortcut_space + HORIZONTAL_PADDING;
        width.max(MIN_MENU_WIDTH)
    }

    /// Rough text width estimation (monospace-ish approximation).
    fn estimate_text_width(text: &str, font_size: f32) -> f32 {
        text.len() as f32 * font_size * 0.6
    }

    fn total_height(&self) -> f32 {
        let content: f32 = self.items.iter().map(|item| match item {
            MenuItem::Separator => SEPARATOR_HEIGHT,
            _ => ITEM_HEIGHT,
        }).sum();
        content + VERTICAL_PADDING * 2.0
    }

    fn point_in_bounds(&self, px: f32, py: f32) -> bool {
        px >= self.x
            && px <= self.x + self.width
            && py >= self.y
            && py <= self.y + self.total_height()
    }

    /// Find which item index the Y coordinate corresponds to.
    fn index_at_y(&self, py: f32) -> Option<usize> {
        let mut current_y = self.y + VERTICAL_PADDING;
        for (i, item) in self.items.iter().enumerate() {
            let h = match item {
                MenuItem::Separator => SEPARATOR_HEIGHT,
                _ => ITEM_HEIGHT,
            };
            if py >= current_y && py < current_y + h {
                // Don't select separators.
                if matches!(item, MenuItem::Separator) {
                    return None;
                }
                return Some(i);
            }
            current_y += h;
        }
        None
    }

    /// Get the Y offset of the item at the given index relative to menu top.
    fn y_offset_for_index(&self, target: usize) -> f32 {
        let mut offset = VERTICAL_PADDING;
        for (i, item) in self.items.iter().enumerate() {
            if i == target {
                return offset;
            }
            offset += match item {
                MenuItem::Separator => SEPARATOR_HEIGHT,
                _ => ITEM_HEIGHT,
            };
        }
        offset
    }

    /// Move hover up or down, skipping separators and disabled items.
    fn move_hover(&mut self, direction: i32) {
        let count = self.items.len();
        if count == 0 {
            return;
        }

        let start = match self.hover_index {
            Some(idx) => idx as i32 + direction,
            None => if direction > 0 { 0 } else { count as i32 - 1 },
        };

        // Scan in the given direction, wrapping around once.
        let mut pos = start;
        for _ in 0..count {
            if pos < 0 {
                pos = count as i32 - 1;
            } else if pos >= count as i32 {
                pos = 0;
            }

            let idx = pos as usize;
            let selectable = matches!(
                self.items.get(idx),
                Some(MenuItem::Action { enabled: true, .. })
                    | Some(MenuItem::Submenu { enabled: true, .. })
            );

            if selectable {
                self.hover_index = Some(idx);
                return;
            }
            pos += direction;
        }
    }
}

// ─── Tooltip ────────────────────────────────────────────────────────────────

const TOOLTIP_BG: Color = Color::from_hex(0x1E1E2E);
const TOOLTIP_TEXT: Color = Color::from_hex(0xCDD6F4);
const TOOLTIP_BORDER: Color = Color::from_hex(0x45475A);
const TOOLTIP_SHADOW: Color = Color::rgba(0, 0, 0, 120);
const TOOLTIP_FONT_SIZE: f32 = 12.0;
const TOOLTIP_PADDING: f32 = 6.0;
const TOOLTIP_CORNER_RADIUS: f32 = 4.0;
const TOOLTIP_OFFSET_X: f32 = 12.0;
const TOOLTIP_OFFSET_Y: f32 = 16.0;
const TOOLTIP_LINE_HEIGHT: f32 = 16.0;
const DEFAULT_TOOLTIP_DELAY_MS: u32 = 500;
const DEFAULT_TOOLTIP_MAX_WIDTH: f32 = 300.0;

/// A tooltip that appears after a configurable hover delay.
pub struct Tooltip {
    text: String,
    x: f32,
    y: f32,
    visible: bool,
    /// Delay in milliseconds before tooltip appears.
    delay_ms: u32,
    /// Timestamp (ms) when hover began; `None` if not hovering.
    hover_start: Option<u64>,
    /// Maximum width before text wraps to a new line.
    max_width: f32,
}

impl Tooltip {
    /// Create a new tooltip with the given text and default settings.
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            x: 0.0,
            y: 0.0,
            visible: false,
            delay_ms: DEFAULT_TOOLTIP_DELAY_MS,
            hover_start: None,
            max_width: DEFAULT_TOOLTIP_MAX_WIDTH,
        }
    }

    /// Set the delay before the tooltip appears.
    pub fn with_delay(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    /// Set the maximum width before text wraps.
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = width;
        self
    }

    /// Call when the mouse enters the tooltip trigger area.
    pub fn start_hover(&mut self, x: f32, y: f32, timestamp_ms: u64) {
        if self.hover_start.is_none() {
            self.hover_start = Some(timestamp_ms);

            // Position with offset, flipping if near viewport edges.
            let tip_width = self.compute_width();
            let tip_height = self.compute_height();

            let mut tip_x = x + TOOLTIP_OFFSET_X;
            let mut tip_y = y + TOOLTIP_OFFSET_Y;

            if tip_x + tip_width > DEFAULT_VIEWPORT_WIDTH {
                tip_x = (x - tip_width - TOOLTIP_OFFSET_X).max(0.0);
            }
            if tip_y + tip_height > DEFAULT_VIEWPORT_HEIGHT {
                tip_y = (y - tip_height - TOOLTIP_OFFSET_Y).max(0.0);
            }

            self.x = tip_x;
            self.y = tip_y;
        }
    }

    /// Call when the mouse leaves the trigger area.
    pub fn end_hover(&mut self) {
        self.hover_start = None;
        self.visible = false;
    }

    /// Call on each frame/tick to check if the hover delay has elapsed.
    pub fn tick(&mut self, timestamp_ms: u64) {
        if let Some(start) = self.hover_start
            && !self.visible && timestamp_ms.saturating_sub(start) >= u64::from(self.delay_ms) {
                self.visible = true;
            }
    }

    /// Whether the tooltip is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Produce render commands for the tooltip.
    pub fn render(&self) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let width = self.compute_width();
        let height = self.compute_height();
        let radii = CornerRadii::all(TOOLTIP_CORNER_RADIUS);

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: self.x,
            y: self.y,
            width,
            height,
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 6.0,
            spread: 0.0,
            color: TOOLTIP_SHADOW,
            corner_radii: radii,
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: self.x,
            y: self.y,
            width,
            height,
            color: TOOLTIP_BG,
            corner_radii: radii,
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: self.x,
            y: self.y,
            width,
            height,
            color: TOOLTIP_BORDER,
            line_width: 1.0,
            corner_radii: radii,
        });

        // Text (each wrapped line).
        let lines = self.wrap_text();
        let mut text_y = self.y + TOOLTIP_PADDING;
        for line in &lines {
            cmds.push(RenderCommand::Text {
                x: self.x + TOOLTIP_PADDING,
                y: text_y,
                text: line.clone(),
                color: TOOLTIP_TEXT,
                font_size: TOOLTIP_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(self.max_width),
            });
            text_y += TOOLTIP_LINE_HEIGHT;
        }

        cmds
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    fn compute_width(&self) -> f32 {
        let lines = self.wrap_text();
        let max_line_width: f32 = lines.iter()
            .map(|l| l.len() as f32 * TOOLTIP_FONT_SIZE * 0.6)
            .fold(0.0_f32, f32::max);
        (max_line_width + TOOLTIP_PADDING * 2.0).min(self.max_width + TOOLTIP_PADDING * 2.0)
    }

    fn compute_height(&self) -> f32 {
        let lines = self.wrap_text();
        let line_count = lines.len().max(1);
        line_count as f32 * TOOLTIP_LINE_HEIGHT + TOOLTIP_PADDING * 2.0
    }

    /// Simple word-wrap at max_width.
    fn wrap_text(&self) -> Vec<String> {
        let max_chars = (self.max_width / (TOOLTIP_FONT_SIZE * 0.6)) as usize;
        if max_chars == 0 {
            return vec![self.text.clone()];
        }

        let mut lines = Vec::new();
        for paragraph in self.text.split('\n') {
            let words: Vec<&str> = paragraph.split_whitespace().collect();
            if words.is_empty() {
                lines.push(String::new());
                continue;
            }

            let mut current_line = String::new();
            for word in words {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.len() + 1 + word.len() <= max_chars {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                lines.push(current_line);
            }
        }

        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modifiers;

    fn sample_items() -> Vec<MenuItem> {
        vec![
            MenuItem::Action {
                id: 1,
                label: "Cut".to_string(),
                shortcut: Some("Ctrl+X".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Action {
                id: 2,
                label: "Copy".to_string(),
                shortcut: Some("Ctrl+C".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
            MenuItem::Separator,
            MenuItem::Action {
                id: 3,
                label: "Paste".to_string(),
                shortcut: Some("Ctrl+V".to_string()),
                icon: None,
                enabled: false,
                checked: None,
            },
            MenuItem::Action {
                id: 4,
                label: "Select All".to_string(),
                shortcut: Some("Ctrl+A".to_string()),
                icon: None,
                enabled: true,
                checked: None,
            },
        ]
    }

    fn make_key(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        }
    }

    #[test]
    fn menu_initially_hidden() {
        let menu = ContextMenu::new(sample_items());
        assert!(!menu.is_visible());
    }

    #[test]
    fn menu_show_and_hide() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(100.0, 200.0);
        assert!(menu.is_visible());
        menu.hide();
        assert!(!menu.is_visible());
    }

    #[test]
    fn menu_click_selects_item() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Click within the first item area (after padding).
        let click_y = VERTICAL_PADDING + ITEM_HEIGHT / 2.0;
        let result = menu.handle_click(50.0, click_y);
        assert_eq!(result, Some(1)); // "Cut" has id 1
        assert!(!menu.is_visible()); // Menu closes after selection
    }

    #[test]
    fn menu_click_disabled_item_does_nothing() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Item 3 ("Paste") is disabled, it's at index 3 (after separator).
        // Offset: items 0,1 = 2*ITEM_HEIGHT, separator = SEPARATOR_HEIGHT, then half of item 3.
        let click_y = VERTICAL_PADDING + 2.0 * ITEM_HEIGHT + SEPARATOR_HEIGHT + ITEM_HEIGHT / 2.0;
        let result = menu.handle_click(50.0, click_y);
        assert_eq!(result, None);
        assert!(menu.is_visible()); // Menu stays open
    }

    #[test]
    fn menu_click_outside_closes() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(100.0, 100.0);

        let result = menu.handle_click(0.0, 0.0);
        assert_eq!(result, None);
        assert!(!menu.is_visible());
    }

    #[test]
    fn keyboard_down_moves_hover() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Press Down — should select first selectable item (index 0).
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(0));

        // Press Down again — should move to index 1.
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(1));

        // Press Down again — should skip separator (index 2), skip disabled (index 3), land on 4.
        menu.handle_key(&make_key(Key::Down));
        assert_eq!(menu.hover_index, Some(4));
    }

    #[test]
    fn keyboard_up_wraps_around() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        // Press Up from no selection — should wrap to last selectable item (index 4).
        menu.handle_key(&make_key(Key::Up));
        assert_eq!(menu.hover_index, Some(4));
    }

    #[test]
    fn keyboard_enter_selects_hovered() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        menu.handle_key(&make_key(Key::Down)); // hover index 0
        let result = menu.handle_key(&make_key(Key::Enter));
        assert_eq!(result, Some(MenuAction::Selected(1)));
        assert!(!menu.is_visible());
    }

    #[test]
    fn keyboard_escape_closes_menu() {
        let mut menu = ContextMenu::new(sample_items());
        menu.show(0.0, 0.0);

        let result = menu.handle_key(&make_key(Key::Escape));
        assert_eq!(result, Some(MenuAction::Closed));
        assert!(!menu.is_visible());
    }

    #[test]
    fn submenu_opens_on_hover() {
        let items = vec![
            MenuItem::Submenu {
                id: 10,
                label: "More".to_string(),
                icon: None,
                enabled: true,
                children: vec![
                    MenuItem::Action {
                        id: 11,
                        label: "Sub Item".to_string(),
                        shortcut: None,
                        icon: None,
                        enabled: true,
                        checked: None,
                    },
                ],
            },
        ];

        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        // Move mouse over the submenu item.
        let hover_y = VERTICAL_PADDING + ITEM_HEIGHT / 2.0;
        menu.handle_mouse_move(50.0, hover_y);

        assert!(menu.open_submenu.is_some());
        let (idx, ref sub) = *menu.open_submenu.as_ref().expect("submenu should be open");
        assert_eq!(idx, 0);
        assert!(sub.is_visible());
    }

    #[test]
    fn submenu_keyboard_right_opens() {
        let items = vec![
            MenuItem::Submenu {
                id: 20,
                label: "View".to_string(),
                icon: None,
                enabled: true,
                children: vec![
                    MenuItem::Action {
                        id: 21,
                        label: "Zoom In".to_string(),
                        shortcut: None,
                        icon: None,
                        enabled: true,
                        checked: None,
                    },
                ],
            },
        ];

        let mut menu = ContextMenu::new(items);
        menu.show(0.0, 0.0);

        menu.handle_key(&make_key(Key::Down)); // hover on submenu item
        menu.handle_key(&make_key(Key::Right)); // open submenu

        assert!(menu.open_submenu.is_some());
    }

    #[test]
    fn menu_edge_flip_horizontal() {
        let mut menu = ContextMenu::new(sample_items());
        // Show near right edge — should flip to left.
        menu.show(DEFAULT_VIEWPORT_WIDTH - 10.0, 100.0);
        assert!(menu.x < DEFAULT_VIEWPORT_WIDTH - 10.0);
    }

    #[test]
    fn menu_edge_flip_vertical() {
        let mut menu = ContextMenu::new(sample_items());
        // Show near bottom edge — should flip upward.
        menu.show(100.0, DEFAULT_VIEWPORT_HEIGHT - 10.0);
        assert!(menu.y < DEFAULT_VIEWPORT_HEIGHT - 10.0);
    }

    // ─── Tooltip tests ──────────────────────────────────────────────────────

    #[test]
    fn tooltip_initially_hidden() {
        let tooltip = Tooltip::new("Hello");
        assert!(!tooltip.is_visible());
    }

    #[test]
    fn tooltip_appears_after_delay() {
        let mut tooltip = Tooltip::new("Tooltip text").with_delay(200);

        tooltip.start_hover(100.0, 100.0, 1000);
        tooltip.tick(1100); // 100ms elapsed — not enough
        assert!(!tooltip.is_visible());

        tooltip.tick(1200); // 200ms elapsed — should appear
        assert!(tooltip.is_visible());
    }

    #[test]
    fn tooltip_disappears_on_leave() {
        let mut tooltip = Tooltip::new("Tip");
        tooltip.start_hover(50.0, 50.0, 0);
        tooltip.tick(600); // Past default delay
        assert!(tooltip.is_visible());

        tooltip.end_hover();
        assert!(!tooltip.is_visible());
    }

    #[test]
    fn tooltip_render_empty_when_hidden() {
        let tooltip = Tooltip::new("Hidden tooltip");
        let cmds = tooltip.render();
        assert!(cmds.is_empty());
    }

    #[test]
    fn tooltip_render_produces_commands_when_visible() {
        let mut tooltip = Tooltip::new("Visible tooltip");
        tooltip.start_hover(50.0, 50.0, 0);
        tooltip.tick(600);
        assert!(tooltip.is_visible());

        let cmds = tooltip.render();
        // Should have shadow, background, border, and at least one text command.
        assert!(cmds.len() >= 4);
    }

    #[test]
    fn tooltip_edge_flip() {
        let mut tooltip = Tooltip::new("Near edge");
        // Start hover near bottom-right — should flip position.
        tooltip.start_hover(DEFAULT_VIEWPORT_WIDTH - 5.0, DEFAULT_VIEWPORT_HEIGHT - 5.0, 0);
        assert!(tooltip.x < DEFAULT_VIEWPORT_WIDTH - 5.0);
        assert!(tooltip.y < DEFAULT_VIEWPORT_HEIGHT - 5.0);
    }
}
