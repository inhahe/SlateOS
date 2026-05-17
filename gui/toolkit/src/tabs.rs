//! TabView widget for tabbed content areas.
//!
//! Provides a tab bar with selection, close buttons, dirty indicators,
//! keyboard navigation (Ctrl+Tab / Ctrl+Shift+Tab), scroll arrows for
//! overflow, and dark theme styling.

use crate::color::Color;
use crate::event::{Key, KeyEvent};
use crate::render::{FontWeightHint, RenderCommand};
use crate::style::CornerRadii;

/// A single tab definition.
#[derive(Clone, Debug)]
pub struct Tab {
    /// Unique identifier for this tab.
    pub id: u64,
    /// Display label for the tab.
    pub label: String,
    /// Optional icon name/identifier.
    pub icon: Option<String>,
    /// Whether this tab shows a close button.
    pub closeable: bool,
    /// Whether to show an unsaved/dirty indicator.
    pub dirty: bool,
}

impl Tab {
    /// Create a new tab with the given ID and label.
    pub fn new(id: u64, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            closeable: true,
            dirty: false,
        }
    }
}

/// Tab bar position relative to content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabPosition {
    /// Tab bar above content.
    Top,
    /// Tab bar below content.
    Bottom,
}

/// Events produced by tab interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum TabEvent {
    /// A tab was selected.
    Selected(u64),
    /// A tab's close button was clicked.
    CloseRequested(u64),
    /// A tab was reordered to a new index.
    Reordered { tab_id: u64, new_index: usize },
}

/// How tab widths are calculated.
#[derive(Clone, Copy, Debug)]
pub enum TabWidth {
    /// All tabs have the same fixed width.
    Fixed(f32),
    /// Tabs flex between min and max based on available space.
    Flexible { min: f32, max: f32 },
}

impl Default for TabWidth {
    fn default() -> Self {
        Self::Flexible {
            min: 80.0,
            max: 200.0,
        }
    }
}

// --- Dark theme colors ---

/// Tab bar background.
const BAR_BG: Color = Color::from_hex(0x181825);
/// Active tab background.
const ACTIVE_BG: Color = Color::from_hex(0x1E1E2E);
/// Inactive tab background.
const INACTIVE_BG: Color = Color::from_hex(0x11111B);
/// Hover tab background.
const HOVER_BG: Color = Color::from_hex(0x313244);
/// Active tab text.
const ACTIVE_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Inactive tab text.
const INACTIVE_TEXT: Color = Color::from_hex(0xA6ADC8);
/// Close button color.
const CLOSE_COLOR: Color = Color::from_hex(0x6C7086);
/// Close button hover color.
const CLOSE_HOVER: Color = Color::from_hex(0xF38BA8);
/// Dirty indicator color (warm dot).
const DIRTY_COLOR: Color = Color::from_hex(0xFAB387);
/// Active tab underline accent.
const ACCENT_COLOR: Color = Color::from_hex(0x89B4FA);

/// Height of the tab bar in pixels.
const TAB_BAR_HEIGHT: f32 = 36.0;
/// Padding inside each tab.
const TAB_PADDING_H: f32 = 12.0;
/// Size of the close button hit area.
const CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Tab bar state and logic.
///
/// Manages a collection of tabs with selection, hover state,
/// close buttons, dirty indicators, and overflow scrolling.
pub struct TabView {
    tabs: Vec<Tab>,
    active_id: Option<u64>,
    position: TabPosition,
    tab_width: TabWidth,
    scroll_offset: f32,
    hover_tab: Option<u64>,
    hover_close: Option<u64>,
}

impl TabView {
    /// Create a new tab view with the given tab bar position.
    pub fn new(position: TabPosition) -> Self {
        Self {
            tabs: Vec::new(),
            active_id: None,
            position,
            tab_width: TabWidth::default(),
            scroll_offset: 0.0,
            hover_tab: None,
            hover_close: None,
        }
    }

    /// Add a tab to the end of the tab bar.
    pub fn add_tab(&mut self, tab: Tab) {
        let id = tab.id;
        self.tabs.push(tab);
        // Auto-activate the first tab added
        if self.active_id.is_none() {
            self.active_id = Some(id);
        }
    }

    /// Remove a tab by ID. If the removed tab was active, activates an adjacent tab.
    pub fn remove_tab(&mut self, id: u64) {
        let Some(idx) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        self.tabs.remove(idx);

        if self.active_id == Some(id) {
            // Activate the next tab, or previous, or none
            self.active_id = if !self.tabs.is_empty() {
                let new_idx = idx.min(self.tabs.len() - 1);
                self.tabs.get(new_idx).map(|t| t.id)
            } else {
                None
            };
        }
    }

    /// Set the active (selected) tab by ID.
    pub fn set_active(&mut self, id: u64) {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_id = Some(id);
        }
    }

    /// Get the currently active tab ID.
    pub fn active_id(&self) -> Option<u64> {
        self.active_id
    }

    /// Set the dirty (unsaved) indicator for a tab.
    pub fn mark_dirty(&mut self, id: u64, dirty: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.dirty = dirty;
        }
    }

    /// Update a tab's label.
    pub fn set_label(&mut self, id: u64, label: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.label = label.to_string();
        }
    }

    /// Get the number of tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Get a slice of all tabs.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Set the tab width strategy.
    pub fn set_tab_width(&mut self, tab_width: TabWidth) {
        self.tab_width = tab_width;
    }

    /// Set hover state for external mouse tracking.
    pub fn set_hover(&mut self, tab_id: Option<u64>, close_hover: Option<u64>) {
        self.hover_tab = tab_id;
        self.hover_close = close_hover;
    }

    /// Handle a mouse click at position (x, y) relative to the tab bar origin.
    pub fn handle_click(&mut self, x: f32, y: f32) -> Option<TabEvent> {
        let bar_height = TAB_BAR_HEIGHT;
        // Ignore clicks outside the bar vertically
        if y < 0.0 || y > bar_height {
            return None;
        }

        let click_x = x + self.scroll_offset;
        let mut current_x: f32 = 0.0;

        for tab in &self.tabs {
            let tw = self.compute_tab_width(tab);
            if click_x >= current_x && click_x < current_x + tw {
                // Check if click is on close button
                if tab.closeable {
                    let close_x = current_x + tw - TAB_PADDING_H - CLOSE_BUTTON_SIZE;
                    let close_y = (bar_height - CLOSE_BUTTON_SIZE) / 2.0;
                    if click_x >= close_x
                        && click_x <= close_x + CLOSE_BUTTON_SIZE
                        && y >= close_y
                        && y <= close_y + CLOSE_BUTTON_SIZE
                    {
                        return Some(TabEvent::CloseRequested(tab.id));
                    }
                }
                // Regular tab selection
                if self.active_id != Some(tab.id) {
                    self.active_id = Some(tab.id);
                    return Some(TabEvent::Selected(tab.id));
                }
                return None;
            }
            current_x += tw;
        }
        None
    }

    /// Handle a keyboard event. Supports Ctrl+Tab and Ctrl+Shift+Tab for cycling.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<TabEvent> {
        if !key.pressed || self.tabs.is_empty() {
            return None;
        }

        match key.key {
            Key::Tab if key.modifiers.ctrl && !key.modifiers.shift => {
                // Ctrl+Tab: next tab
                self.cycle_tab(1)
            }
            Key::Tab if key.modifiers.ctrl && key.modifiers.shift => {
                // Ctrl+Shift+Tab: previous tab
                self.cycle_tab(-1)
            }
            Key::W if key.modifiers.ctrl => {
                // Ctrl+W: close current tab
                self.active_id.map(TabEvent::CloseRequested)
            }
            _ => None,
        }
    }

    /// Render the tab bar. Returns (commands, content_y, content_height).
    ///
    /// `content_y` and `content_height` indicate where content should be drawn
    /// below (or above) the tab bar.
    pub fn render(
        &self,
        x: f32,
        y: f32,
        width: f32,
        total_height: f32,
    ) -> (Vec<RenderCommand>, f32, f32) {
        let bar_height = TAB_BAR_HEIGHT;
        let (bar_y, content_y, content_height) = match self.position {
            TabPosition::Top => (y, y + bar_height, total_height - bar_height),
            TabPosition::Bottom => (
                y + total_height - bar_height,
                y,
                total_height - bar_height,
            ),
        };

        let mut commands = Vec::new();

        // Tab bar background
        commands.push(RenderCommand::FillRect {
            x,
            y: bar_y,
            width,
            height: bar_height,
            color: BAR_BG,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip the tab area for overflow
        commands.push(RenderCommand::PushClip {
            x,
            y: bar_y,
            width,
            height: bar_height,
        });

        // Render each tab
        let mut current_x = x - self.scroll_offset;
        for tab in &self.tabs {
            let tw = self.compute_tab_width(tab);
            let is_active = self.active_id == Some(tab.id);
            let is_hovered = self.hover_tab == Some(tab.id);

            // Tab background
            let tab_bg = if is_active {
                ACTIVE_BG
            } else if is_hovered {
                HOVER_BG
            } else {
                INACTIVE_BG
            };

            let corner_radii = match self.position {
                TabPosition::Top => CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
                TabPosition::Bottom => CornerRadii {
                    top_left: 0.0,
                    top_right: 0.0,
                    bottom_left: 4.0,
                    bottom_right: 4.0,
                },
            };

            commands.push(RenderCommand::FillRect {
                x: current_x,
                y: bar_y,
                width: tw,
                height: bar_height,
                color: tab_bg,
                corner_radii,
            });

            // Active tab accent underline/overline
            if is_active {
                let accent_y = match self.position {
                    TabPosition::Top => bar_y + bar_height - 2.0,
                    TabPosition::Bottom => bar_y,
                };
                commands.push(RenderCommand::FillRect {
                    x: current_x,
                    y: accent_y,
                    width: tw,
                    height: 2.0,
                    color: ACCENT_COLOR,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            // Dirty indicator (dot before label)
            let mut label_x = current_x + TAB_PADDING_H;
            if tab.dirty {
                commands.push(RenderCommand::FillRect {
                    x: label_x,
                    y: bar_y + (bar_height - 6.0) / 2.0,
                    width: 6.0,
                    height: 6.0,
                    color: DIRTY_COLOR,
                    corner_radii: CornerRadii::all(3.0),
                });
                label_x += 10.0;
            }

            // Tab label
            let text_color = if is_active { ACTIVE_TEXT } else { INACTIVE_TEXT };
            let max_label_width = tw - TAB_PADDING_H * 2.0
                - if tab.closeable { CLOSE_BUTTON_SIZE + 4.0 } else { 0.0 }
                - if tab.dirty { 10.0 } else { 0.0 };

            commands.push(RenderCommand::Text {
                x: label_x,
                y: bar_y + (bar_height - 13.0) / 2.0,
                text: tab.label.clone(),
                color: text_color,
                font_size: 13.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(max_label_width.max(0.0)),
            });

            // Close button
            if tab.closeable {
                let close_x = current_x + tw - TAB_PADDING_H - CLOSE_BUTTON_SIZE;
                let close_y = bar_y + (bar_height - CLOSE_BUTTON_SIZE) / 2.0;
                let close_color = if self.hover_close == Some(tab.id) {
                    CLOSE_HOVER
                } else {
                    CLOSE_COLOR
                };
                // Render X as text
                commands.push(RenderCommand::Text {
                    x: close_x + 3.0,
                    y: close_y + 1.0,
                    text: "\u{00D7}".to_string(), // multiplication sign as close icon
                    color: close_color,
                    font_size: 14.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            current_x += tw;
        }

        commands.push(RenderCommand::PopClip);

        // Scroll indicators if overflow
        let total_tab_width = self.total_tabs_width();
        if total_tab_width > width {
            // Left arrow indicator
            if self.scroll_offset > 0.0 {
                commands.push(RenderCommand::Text {
                    x: x + 2.0,
                    y: bar_y + (bar_height - 12.0) / 2.0,
                    text: "\u{25C0}".to_string(),
                    color: INACTIVE_TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            // Right arrow indicator
            if self.scroll_offset + width < total_tab_width {
                commands.push(RenderCommand::Text {
                    x: x + width - 14.0,
                    y: bar_y + (bar_height - 12.0) / 2.0,
                    text: "\u{25B6}".to_string(),
                    color: INACTIVE_TEXT,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
        }

        (commands, content_y, content_height)
    }

    /// Set the scroll offset for the tab bar.
    pub fn set_scroll_offset(&mut self, offset: f32) {
        let max = self.max_scroll_offset();
        self.scroll_offset = offset.clamp(0.0, max);
    }

    /// Get the current scroll offset.
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset
    }

    // --- Private helpers ---

    fn cycle_tab(&mut self, direction: i32) -> Option<TabEvent> {
        if self.tabs.is_empty() {
            return None;
        }

        let current_idx = self.active_id.and_then(|aid| {
            self.tabs.iter().position(|t| t.id == aid)
        }).unwrap_or(0);

        let count = self.tabs.len() as i32;
        let new_idx = ((current_idx as i32 + direction).rem_euclid(count)) as usize;

        if let Some(tab) = self.tabs.get(new_idx) {
            let new_id = tab.id;
            self.active_id = Some(new_id);
            Some(TabEvent::Selected(new_id))
        } else {
            None
        }
    }

    fn compute_tab_width(&self, tab: &Tab) -> f32 {
        match self.tab_width {
            TabWidth::Fixed(w) => w,
            TabWidth::Flexible { min, max } => {
                // Estimate width based on label length
                let estimated = TAB_PADDING_H * 2.0
                    + tab.label.len() as f32 * 7.5 // approximate char width
                    + if tab.closeable { CLOSE_BUTTON_SIZE + 4.0 } else { 0.0 }
                    + if tab.dirty { 10.0 } else { 0.0 };
                estimated.clamp(min, max)
            }
        }
    }

    fn total_tabs_width(&self) -> f32 {
        self.tabs.iter().map(|t| self.compute_tab_width(t)).sum()
    }

    fn max_scroll_offset(&self) -> f32 {
        // We don't know the viewport width here, so return a large max.
        // The caller should clamp via set_scroll_offset with the actual width.
        let total = self.total_tabs_width();
        total.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Modifiers;

    #[test]
    fn test_new_tab_view_is_empty() {
        let tv = TabView::new(TabPosition::Top);
        assert_eq!(tv.tab_count(), 0);
        assert_eq!(tv.active_id(), None);
    }

    #[test]
    fn test_add_tab_auto_activates_first() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "First"));
        assert_eq!(tv.active_id(), Some(1));
        tv.add_tab(Tab::new(2, "Second"));
        // First tab remains active
        assert_eq!(tv.active_id(), Some(1));
    }

    #[test]
    fn test_remove_active_tab_activates_adjacent() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "A"));
        tv.add_tab(Tab::new(2, "B"));
        tv.add_tab(Tab::new(3, "C"));
        tv.set_active(2);
        tv.remove_tab(2);
        // Should activate tab at same index (which is now tab 3)
        assert_eq!(tv.active_id(), Some(3));
    }

    #[test]
    fn test_remove_last_tab_clears_active() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "Only"));
        tv.remove_tab(1);
        assert_eq!(tv.active_id(), None);
        assert_eq!(tv.tab_count(), 0);
    }

    #[test]
    fn test_set_active() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "A"));
        tv.add_tab(Tab::new(2, "B"));
        tv.set_active(2);
        assert_eq!(tv.active_id(), Some(2));
    }

    #[test]
    fn test_mark_dirty() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "A"));
        tv.mark_dirty(1, true);
        assert!(tv.tabs()[0].dirty);
        tv.mark_dirty(1, false);
        assert!(!tv.tabs()[0].dirty);
    }

    #[test]
    fn test_set_label() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "Old"));
        tv.set_label(1, "New");
        assert_eq!(tv.tabs()[0].label, "New");
    }

    #[test]
    fn test_ctrl_tab_cycles_forward() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "A"));
        tv.add_tab(Tab::new(2, "B"));
        tv.add_tab(Tab::new(3, "C"));
        tv.set_active(1);

        let key = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers { ctrl: true, shift: false, alt: false, super_key: false },
            text: None,
        };
        let event = tv.handle_key(&key);
        assert_eq!(event, Some(TabEvent::Selected(2)));
        assert_eq!(tv.active_id(), Some(2));
    }

    #[test]
    fn test_ctrl_shift_tab_cycles_backward() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "A"));
        tv.add_tab(Tab::new(2, "B"));
        tv.add_tab(Tab::new(3, "C"));
        tv.set_active(1);

        let key = KeyEvent {
            key: Key::Tab,
            pressed: true,
            modifiers: Modifiers { ctrl: true, shift: true, alt: false, super_key: false },
            text: None,
        };
        let event = tv.handle_key(&key);
        assert_eq!(event, Some(TabEvent::Selected(3)));
        assert_eq!(tv.active_id(), Some(3));
    }

    #[test]
    fn test_render_produces_commands() {
        let mut tv = TabView::new(TabPosition::Top);
        tv.add_tab(Tab::new(1, "Tab A"));
        tv.add_tab(Tab::new(2, "Tab B"));

        let (commands, content_y, content_height) = tv.render(0.0, 0.0, 400.0, 300.0);
        assert!(!commands.is_empty());
        // Content should start below the tab bar
        assert!((content_y - TAB_BAR_HEIGHT).abs() < f32::EPSILON);
        assert!((content_height - (300.0 - TAB_BAR_HEIGHT)).abs() < f32::EPSILON);
    }

    #[test]
    fn test_render_bottom_position() {
        let mut tv = TabView::new(TabPosition::Bottom);
        tv.add_tab(Tab::new(1, "Tab"));

        let (_, content_y, content_height) = tv.render(0.0, 0.0, 400.0, 300.0);
        // Content should start at y=0 with bottom tabs
        assert!(content_y.abs() < f32::EPSILON);
        assert!((content_height - (300.0 - TAB_BAR_HEIGHT)).abs() < f32::EPSILON);
    }
}
