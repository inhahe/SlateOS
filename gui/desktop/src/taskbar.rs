//! Enhanced taskbar system with pinned apps, running apps, drag-to-reorder.
//!
//! Provides a Windows 11-style taskbar with:
//! - Pinned application shortcuts (persisted to config)
//! - Running application indicators with window grouping
//! - Drag-to-reorder pinned items
//! - Drag into/out of pinned section to pin/unpin
//! - Configurable position and appearance (icon-only or icon+name)
//!
//! Uses Catppuccin Mocha for theming.

use guitk::color::Color;
use guitk::event::{EventResult, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

/// Catppuccin Mocha: base (background)
const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
/// Catppuccin Mocha: surface0
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
/// Catppuccin Mocha: surface1
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
/// Catppuccin Mocha: surface2
const MOCHA_SURFACE2: Color = Color::from_hex(0x585B70);
/// Catppuccin Mocha: overlay0
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);
/// Catppuccin Mocha: text
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
/// Catppuccin Mocha: subtext0
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
/// Catppuccin Mocha: blue (accent)
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
/// Catppuccin Mocha: lavender
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);
/// Catppuccin Mocha: red
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
/// Catppuccin Mocha: mantle
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);

// ============================================================================
// Configuration
// ============================================================================

/// Taskbar position on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarPosition {
    Bottom,
    Top,
    Left,
    Right,
}

/// Taskbar configuration.
#[derive(Clone, Debug)]
pub struct TaskbarConfig {
    /// Position of the taskbar on screen.
    pub position: TaskbarPosition,
    /// Taskbar thickness in pixels (height for bottom/top, width for left/right).
    pub size: u32,
    /// Whether to show only icons (true) or icons + labels (false).
    pub icon_only: bool,
    /// Whether to auto-hide when no window is focused on the taskbar.
    pub auto_hide: bool,
    /// Width of each button in icon-only mode.
    pub button_icon_width: f32,
    /// Width of each button in icon+label mode.
    pub button_label_width: f32,
    /// Padding between buttons.
    pub button_gap: f32,
    /// Width reserved for the start button area.
    pub start_button_width: f32,
    /// Width reserved for the system tray area.
    pub system_tray_width: f32,
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            position: TaskbarPosition::Bottom,
            size: 48,
            icon_only: true,
            auto_hide: false,
            button_icon_width: 44.0,
            button_label_width: 160.0,
            button_gap: 4.0,
            start_button_width: 48.0,
            system_tray_width: 180.0,
        }
    }
}

// ============================================================================
// Pinned app data
// ============================================================================

/// A pinned application entry persisted in config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedApp {
    /// Unique application identifier.
    pub app_id: String,
    /// Display name shown in tooltip or label mode.
    pub display_name: String,
    /// Type of icon (for future icon registry lookup).
    pub icon_type: IconType,
    /// Executable path to launch the application.
    pub exec_path: String,
    /// Position in the pinned list (0-based, lower = more left).
    pub position: u32,
}

/// Icon type for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    /// System-provided icon by ID.
    System(u64),
    /// Generic placeholder icon.
    Generic,
}

// ============================================================================
// Running window info
// ============================================================================

/// Unique window identifier (mirrors the compositor's ID space).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// Information about a running window tracked in the taskbar.
#[derive(Clone, Debug)]
struct RunningWindow {
    window_id: WindowId,
    app_id: String,
    title: String,
}

// ============================================================================
// Button state and representation
// ============================================================================

/// Visual/interaction state of a taskbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    /// Pinned but not running — dim icon only.
    Idle,
    /// At least one window is running (not focused).
    Running,
    /// A window of this app is currently focused.
    Focused,
}

/// A single button on the taskbar (may represent pinned app, running app, or both).
#[derive(Clone, Debug)]
pub struct TaskbarButton {
    /// Application identifier.
    pub app_id: String,
    /// Display name.
    pub display_name: String,
    /// Icon type.
    pub icon_type: IconType,
    /// Whether this app is pinned.
    pub pinned: bool,
    /// Window IDs associated with this button (empty if pinned but not running).
    pub window_ids: Vec<WindowId>,
    /// Current visual state.
    pub state: ButtonState,
    /// Whether the mouse is hovering over this button.
    pub hovered: bool,
}

impl TaskbarButton {
    /// Number of windows grouped under this button.
    pub fn window_count(&self) -> usize {
        self.window_ids.len()
    }

    /// Whether this app is currently running (has at least one window).
    pub fn is_running(&self) -> bool {
        !self.window_ids.is_empty()
    }
}

// ============================================================================
// Drag state
// ============================================================================

/// Drag-and-drop state for reordering.
#[derive(Clone, Debug)]
struct DragState {
    /// Index of the button being dragged.
    source_index: usize,
    /// Current mouse X during drag.
    current_x: f32,
    /// Current mouse Y during drag.
    current_y: f32,
    /// Original X of the drag start.
    start_x: f32,
    /// Original Y of the drag start.
    start_y: f32,
    /// Whether the drag has moved enough to be considered active.
    active: bool,
}

/// Minimum pixel distance before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 5.0;

// ============================================================================
// Events emitted by the taskbar
// ============================================================================

/// Events emitted by the taskbar for the desktop shell to handle.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskbarEvent {
    /// User clicked a button to activate/focus a window.
    ActivateWindow { window_id: WindowId },
    /// User clicked a running+focused window to minimize it.
    MinimizeWindow { window_id: WindowId },
    /// User pinned an app to the taskbar.
    AppPinned { app_id: String, position: u32 },
    /// User unpinned an app from the taskbar.
    AppUnpinned { app_id: String },
    /// User reordered pinned apps.
    PinnedReordered { app_id: String, new_position: u32 },
    /// User requested to close a window.
    CloseWindow { window_id: WindowId },
    /// User requested to launch a pinned app.
    LaunchApp { app_id: String, exec_path: String },
}

// ============================================================================
// Context menu
// ============================================================================

/// Context menu state.
#[derive(Clone, Debug)]
struct ContextMenu {
    /// Index of the button that was right-clicked.
    button_index: usize,
    /// Screen position of the menu.
    x: f32,
    y: f32,
    /// Whether this menu is visible.
    visible: bool,
}

/// A context menu item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextMenuItem {
    Pin,
    Unpin,
    Close,
    CloseAll,
}

// ============================================================================
// Taskbar state
// ============================================================================

/// Complete taskbar state.
pub struct TaskbarState {
    /// Configuration.
    config: TaskbarConfig,
    /// Ordered list of pinned apps (by position).
    pinned_apps: Vec<PinnedApp>,
    /// Running windows (keyed by window ID).
    running_windows: HashMap<WindowId, RunningWindow>,
    /// Currently focused window, if any.
    focused_window: Option<WindowId>,
    /// Computed buttons (rebuilt when pinned/running state changes).
    buttons: Vec<TaskbarButton>,
    /// Drag state, if a drag is in progress.
    drag: Option<DragState>,
    /// Context menu state.
    context_menu: Option<ContextMenu>,
    /// Pending events to be drained by the desktop shell.
    events: Vec<TaskbarEvent>,
    /// Whether the button list needs rebuilding.
    dirty: bool,
    /// Index of the button the mouse is currently over (None if not hovering).
    hover_index: Option<usize>,
}

impl TaskbarState {
    /// Create a new taskbar with the given configuration.
    pub fn new(config: TaskbarConfig) -> Self {
        Self {
            config,
            pinned_apps: Vec::new(),
            running_windows: HashMap::new(),
            focused_window: None,
            buttons: Vec::new(),
            drag: None,
            context_menu: None,
            events: Vec::new(),
            dirty: true,
            hover_index: None,
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &TaskbarConfig {
        &self.config
    }

    /// Get the current list of buttons (rebuilds if dirty).
    pub fn buttons(&mut self) -> &[TaskbarButton] {
        if self.dirty {
            self.rebuild_buttons();
        }
        &self.buttons
    }

    // ======================================================================
    // Pinned app management
    // ======================================================================

    /// Add a pinned app to the taskbar.
    pub fn add_pinned(&mut self, app: PinnedApp) {
        // Avoid duplicate pinning.
        if self.pinned_apps.iter().any(|p| p.app_id == app.app_id) {
            return;
        }
        self.pinned_apps.push(app);
        self.pinned_apps.sort_by_key(|p| p.position);
        self.dirty = true;
    }

    /// Remove a pinned app by its app_id.
    pub fn remove_pinned(&mut self, app_id: &str) {
        self.pinned_apps.retain(|p| p.app_id != app_id);
        // Re-normalize positions.
        for (i, app) in self.pinned_apps.iter_mut().enumerate() {
            app.position = i as u32;
        }
        self.dirty = true;
    }

    /// Reorder a pinned app from one position to another.
    /// Both `from` and `to` are indices in the pinned list.
    pub fn reorder_pinned(&mut self, from: usize, to: usize) {
        let len = self.pinned_apps.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let app = self.pinned_apps.remove(from);
        self.reinsert_pinned(app, to);
    }

    /// Get the list of pinned apps (for serialization).
    pub fn pinned_apps(&self) -> &[PinnedApp] {
        &self.pinned_apps
    }

    fn reinsert_pinned(&mut self, app: PinnedApp, to: usize) {
        let to = to.min(self.pinned_apps.len());
        self.pinned_apps.insert(to, app);
        // Re-normalize positions.
        for (i, a) in self.pinned_apps.iter_mut().enumerate() {
            a.position = i as u32;
        }
        self.dirty = true;
    }

    // ======================================================================
    // Running window management
    // ======================================================================

    /// Register a new running window.
    pub fn add_running_window(&mut self, window_id: WindowId, app_id: &str, title: &str) {
        self.running_windows.insert(
            window_id,
            RunningWindow {
                window_id,
                app_id: app_id.to_string(),
                title: title.to_string(),
            },
        );
        self.dirty = true;
    }

    /// Remove a running window (closed).
    pub fn remove_running_window(&mut self, window_id: WindowId) {
        self.running_windows.remove(&window_id);
        if self.focused_window == Some(window_id) {
            self.focused_window = None;
        }
        self.dirty = true;
    }

    /// Update which window is currently focused.
    pub fn set_focused_window(&mut self, window_id: Option<WindowId>) {
        if self.focused_window != window_id {
            self.focused_window = window_id;
            self.dirty = true;
        }
    }

    // ======================================================================
    // Event handling
    // ======================================================================

    /// Handle a mouse event on the taskbar.
    /// Coordinates should be relative to the taskbar's top-left corner.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> EventResult {
        // If context menu is visible, handle it first.
        if let Some(ref menu) = self.context_menu.clone()
            && menu.visible {
                return self.handle_context_menu_event(event);
            }

        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_left_press(event.x, event.y),
            MouseEventKind::Release(MouseButton::Left) => {
                self.handle_left_release(event.x, event.y)
            }
            MouseEventKind::Press(MouseButton::Right) => self.handle_right_press(event.x, event.y),
            MouseEventKind::Move => self.handle_mouse_move(event.x, event.y),
            MouseEventKind::Leave => {
                self.hover_index = None;
                self.update_hover_states();
                if let Some(ref mut drag) = self.drag {
                    // Dragging out of the taskbar — unpin if it was pinned.
                    if drag.active {
                        let idx = drag.source_index;
                        if idx < self.buttons.len() && self.buttons[idx].pinned {
                            let app_id = self.buttons[idx].app_id.clone();
                            self.remove_pinned(&app_id);
                            self.events.push(TaskbarEvent::AppUnpinned { app_id });
                        }
                    }
                    self.drag = None;
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key event on the taskbar.
    /// Currently a placeholder for potential keyboard navigation (Super+1..9).
    pub fn handle_key_event(&mut self, _event: &guitk::event::KeyEvent) -> EventResult {
        // Future: Super+number to activate Nth pinned app.
        EventResult::Ignored
    }

    /// Drain all pending events produced by user interactions.
    pub fn drain_events(&mut self) -> Vec<TaskbarEvent> {
        std::mem::take(&mut self.events)
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Render the taskbar into a list of render commands.
    /// `bar_width` and `bar_height` are the dimensions of the taskbar area.
    pub fn render(&mut self, bar_width: f32, bar_height: f32) -> Vec<RenderCommand> {
        if self.dirty {
            self.rebuild_buttons();
        }

        let mut cmds = Vec::new();

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: bar_width,
            height: bar_height,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border line (subtle separator from content area).
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: 0.0,
            x2: bar_width,
            y2: 0.0,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        let btn_width = if self.config.icon_only {
            self.config.button_icon_width
        } else {
            self.config.button_label_width
        };
        let gap = self.config.button_gap;

        // Find the divider position (between pinned and running-only sections).
        let pinned_count = self.buttons.iter().filter(|b| b.pinned).count();

        let mut x = self.config.start_button_width + gap;
        let btn_y = 2.0;
        let btn_h = bar_height - 4.0;

        for (i, button) in self.buttons.iter().enumerate() {
            // Draw divider before the first non-pinned button.
            if i == pinned_count && pinned_count > 0 && i < self.buttons.len() {
                let div_x = x - gap / 2.0;
                cmds.push(RenderCommand::Line {
                    x1: div_x,
                    y1: btn_y + 8.0,
                    x2: div_x,
                    y2: btn_y + btn_h - 8.0,
                    color: MOCHA_SURFACE2,
                    width: 1.0,
                });
            }

            // If this button is being dragged and the drag is active,
            // render a ghost at the drag position instead.
            let is_dragged =
                self.drag.as_ref().is_some_and(|d| d.active && d.source_index == i);

            if is_dragged {
                // Render insertion indicator at the drop position.
                if let Some(ref drag) = self.drag {
                    let drop_idx = self.drop_target_index(drag.current_x);
                    let indicator_x = self.config.start_button_width
                        + gap
                        + drop_idx as f32 * (btn_width + gap)
                        - gap / 2.0;
                    cmds.push(RenderCommand::FillRect {
                        x: indicator_x - 1.0,
                        y: btn_y + 4.0,
                        width: 2.0,
                        height: btn_h - 8.0,
                        color: MOCHA_BLUE,
                        corner_radii: CornerRadii::all(1.0),
                    });
                }

                // Ghost button at drag position.
                if let Some(ref drag) = self.drag {
                    let ghost_x = drag.current_x - btn_width / 2.0;
                    cmds.push(RenderCommand::FillRect {
                        x: ghost_x,
                        y: btn_y,
                        width: btn_width,
                        height: btn_h,
                        color: Color::rgba(69, 71, 90, 180), // MOCHA_SURFACE1 with transparency
                        corner_radii: CornerRadii::all(6.0),
                    });
                    self.render_button_content(
                        &mut cmds, button, ghost_x, btn_y, btn_width, btn_h, true,
                    );
                }

                x += btn_width + gap;
                continue;
            }

            // Background based on state.
            let bg_color = match (button.state, button.hovered) {
                (_, true) => MOCHA_SURFACE1,
                (ButtonState::Focused, false) => MOCHA_SURFACE0,
                (ButtonState::Running, false) => Color::rgba(49, 50, 68, 128), // subtle bg
                (ButtonState::Idle, false) => Color::TRANSPARENT,
            };

            if bg_color != Color::TRANSPARENT {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: btn_y,
                    width: btn_width,
                    height: btn_h,
                    color: bg_color,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Button content (icon placeholder + optional label).
            self.render_button_content(&mut cmds, button, x, btn_y, btn_width, btn_h, false);

            // Underline indicator for running/focused apps.
            if button.is_running() {
                let indicator_color = if button.state == ButtonState::Focused {
                    MOCHA_BLUE
                } else {
                    MOCHA_LAVENDER
                };
                let indicator_w = if button.state == ButtonState::Focused {
                    16.0
                } else {
                    8.0
                };
                let indicator_x = x + (btn_width - indicator_w) / 2.0;
                let indicator_y = btn_y + btn_h - 4.0;
                cmds.push(RenderCommand::FillRect {
                    x: indicator_x,
                    y: indicator_y,
                    width: indicator_w,
                    height: 3.0,
                    color: indicator_color,
                    corner_radii: CornerRadii::all(1.5),
                });
            }

            // Badge for multiple windows.
            if button.window_count() > 1 {
                let badge_x = x + btn_width - 14.0;
                let badge_y = btn_y + 4.0;
                cmds.push(RenderCommand::FillRect {
                    x: badge_x,
                    y: badge_y,
                    width: 12.0,
                    height: 12.0,
                    color: MOCHA_RED,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: badge_x + 3.0,
                    y: badge_y + 1.0,
                    text: format!("{}", button.window_count()),
                    color: MOCHA_MANTLE,
                    font_size: 9.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            x += btn_width + gap;
        }

        // Context menu rendering.
        if let Some(ref menu) = self.context_menu
            && menu.visible {
                self.render_context_menu(&mut cmds, menu.button_index, menu.x, menu.y);
            }

        cmds
    }

    // ======================================================================
    // Internal: button rebuilding
    // ======================================================================

    fn rebuild_buttons(&mut self) {
        self.buttons.clear();

        // First: all pinned apps (in position order).
        let mut pinned_sorted = self.pinned_apps.clone();
        pinned_sorted.sort_by_key(|p| p.position);

        for pinned in &pinned_sorted {
            let windows: Vec<WindowId> = self
                .running_windows
                .values()
                .filter(|rw| rw.app_id == pinned.app_id)
                .map(|rw| rw.window_id)
                .collect();

            let state = self.compute_button_state(&windows);

            self.buttons.push(TaskbarButton {
                app_id: pinned.app_id.clone(),
                display_name: pinned.display_name.clone(),
                icon_type: pinned.icon_type,
                pinned: true,
                window_ids: windows,
                state,
                hovered: false,
            });
        }

        // Second: running apps that are NOT pinned.
        let pinned_ids: Vec<&str> = self.pinned_apps.iter().map(|p| p.app_id.as_str()).collect();

        // Group running windows by app_id.
        let mut running_groups: HashMap<String, Vec<WindowId>> = HashMap::new();
        let mut running_names: HashMap<String, String> = HashMap::new();
        for rw in self.running_windows.values() {
            if !pinned_ids.contains(&rw.app_id.as_str()) {
                running_groups
                    .entry(rw.app_id.clone())
                    .or_default()
                    .push(rw.window_id);
                running_names
                    .entry(rw.app_id.clone())
                    .or_insert_with(|| rw.title.clone());
            }
        }

        // Sort by app_id for stable ordering.
        let mut running_app_ids: Vec<String> = running_groups.keys().cloned().collect();
        running_app_ids.sort();

        for app_id in &running_app_ids {
            let windows = running_groups.get(app_id).cloned().unwrap_or_default();
            let name = running_names
                .get(app_id)
                .cloned()
                .unwrap_or_default();
            let state = self.compute_button_state(&windows);

            self.buttons.push(TaskbarButton {
                app_id: app_id.clone(),
                display_name: name,
                icon_type: IconType::Generic,
                pinned: false,
                window_ids: windows,
                state,
                hovered: false,
            });
        }

        self.dirty = false;
        self.update_hover_states();
    }

    fn compute_button_state(&self, windows: &[WindowId]) -> ButtonState {
        if windows.is_empty() {
            return ButtonState::Idle;
        }
        if let Some(focused) = self.focused_window
            && windows.contains(&focused) {
                return ButtonState::Focused;
            }
        ButtonState::Running
    }

    fn update_hover_states(&mut self) {
        for (i, button) in self.buttons.iter_mut().enumerate() {
            button.hovered = self.hover_index == Some(i);
        }
    }

    // ======================================================================
    // Internal: hit testing
    // ======================================================================

    /// Determine which button index is at the given x coordinate.
    fn button_at_x(&self, x: f32) -> Option<usize> {
        let btn_width = if self.config.icon_only {
            self.config.button_icon_width
        } else {
            self.config.button_label_width
        };
        let gap = self.config.button_gap;
        let start_x = self.config.start_button_width + gap;

        if x < start_x {
            return None;
        }

        let relative_x = x - start_x;
        let slot_width = btn_width + gap;
        if slot_width <= 0.0 {
            return None;
        }
        let idx = (relative_x / slot_width) as usize;

        // Verify it's within the button bounds (not in the gap).
        let button_start = start_x + idx as f32 * slot_width;
        if x >= button_start && x <= button_start + btn_width && idx < self.buttons.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Determine the drop target index for a drag at the given x coordinate.
    fn drop_target_index(&self, x: f32) -> usize {
        let btn_width = if self.config.icon_only {
            self.config.button_icon_width
        } else {
            self.config.button_label_width
        };
        let gap = self.config.button_gap;
        let start_x = self.config.start_button_width + gap;

        if x < start_x {
            return 0;
        }

        let relative_x = x - start_x;
        let slot_width = btn_width + gap;
        if slot_width <= 0.0 {
            return 0;
        }
        let idx = (relative_x / slot_width + 0.5) as usize;
        idx.min(self.pinned_apps.len())
    }

    // ======================================================================
    // Internal: mouse event handlers
    // ======================================================================

    fn handle_left_press(&mut self, x: f32, y: f32) -> EventResult {
        self.context_menu = None;

        if let Some(idx) = self.button_at_x(x) {
            // Start potential drag.
            self.drag = Some(DragState {
                source_index: idx,
                current_x: x,
                current_y: y,
                start_x: x,
                start_y: y,
                active: false,
            });
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn handle_left_release(&mut self, x: f32, _y: f32) -> EventResult {
        let drag = self.drag.take();

        match drag {
            Some(d) if d.active => {
                // Complete the drag operation.
                let drop_idx = self.drop_target_index(x);
                let src_idx = d.source_index;

                if src_idx < self.buttons.len() {
                    let button = &self.buttons[src_idx];
                    if button.pinned {
                        // Reorder pinned apps.
                        let pinned_idx = self
                            .pinned_apps
                            .iter()
                            .position(|p| p.app_id == button.app_id);
                        if let Some(from) = pinned_idx {
                            let to = drop_idx.min(self.pinned_apps.len().saturating_sub(1));
                            if from != to {
                                let app_id = self.pinned_apps[from].app_id.clone();
                                self.reorder_pinned(from, to);
                                self.events.push(TaskbarEvent::PinnedReordered {
                                    app_id,
                                    new_position: to as u32,
                                });
                            }
                        }
                    } else {
                        // Dragging an unpinned running app into the pinned section — pin it.
                        let app_id = button.app_id.clone();
                        let display_name = button.display_name.clone();
                        let position = drop_idx as u32;
                        self.add_pinned(PinnedApp {
                            app_id: app_id.clone(),
                            display_name,
                            icon_type: IconType::Generic,
                            exec_path: String::new(),
                            position,
                        });
                        self.events.push(TaskbarEvent::AppPinned {
                            app_id,
                            position,
                        });
                    }
                }
                EventResult::Consumed
            }
            Some(d) => {
                // Click (no drag).
                let idx = d.source_index;
                self.handle_button_click(idx)
            }
            None => EventResult::Ignored,
        }
    }

    fn handle_right_press(&mut self, x: f32, y: f32) -> EventResult {
        if let Some(idx) = self.button_at_x(x) {
            self.context_menu = Some(ContextMenu {
                button_index: idx,
                x,
                y,
                visible: true,
            });
            return EventResult::Consumed;
        }
        self.context_menu = None;
        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, x: f32, y: f32) -> EventResult {
        // Update hover.
        let new_hover = self.button_at_x(x);
        if new_hover != self.hover_index {
            self.hover_index = new_hover;
            self.update_hover_states();
        }

        // Update drag.
        if let Some(ref mut drag) = self.drag {
            drag.current_x = x;
            drag.current_y = y;
            if !drag.active {
                let dx = x - drag.start_x;
                let dy = y - drag.start_y;
                if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
                    drag.active = true;
                }
            }
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    fn handle_button_click(&mut self, idx: usize) -> EventResult {
        if idx >= self.buttons.len() {
            return EventResult::Ignored;
        }

        let button = &self.buttons[idx];
        if button.is_running() {
            if button.state == ButtonState::Focused {
                // Already focused — minimize.
                if let Some(&wid) = button.window_ids.first() {
                    self.events.push(TaskbarEvent::MinimizeWindow { window_id: wid });
                }
            } else {
                // Bring to front.
                if let Some(&wid) = button.window_ids.first() {
                    self.events.push(TaskbarEvent::ActivateWindow { window_id: wid });
                }
            }
        } else if button.pinned {
            // Launch the app.
            let pinned = self.pinned_apps.iter().find(|p| p.app_id == button.app_id);
            if let Some(p) = pinned {
                self.events.push(TaskbarEvent::LaunchApp {
                    app_id: p.app_id.clone(),
                    exec_path: p.exec_path.clone(),
                });
            }
        }

        EventResult::Consumed
    }

    // ======================================================================
    // Internal: context menu
    // ======================================================================

    fn handle_context_menu_event(&mut self, event: &MouseEvent) -> EventResult {
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                // Check if click is on a menu item.
                if let Some(item) = self.context_menu_item_at(event.x, event.y) {
                    self.execute_context_menu_item(item);
                }
                self.context_menu = None;
                EventResult::Consumed
            }
            MouseEventKind::Press(_) => {
                self.context_menu = None;
                EventResult::Consumed
            }
            _ => EventResult::Consumed,
        }
    }

    fn context_menu_items(&self, button_index: usize) -> Vec<ContextMenuItem> {
        if button_index >= self.buttons.len() {
            return Vec::new();
        }
        let button = &self.buttons[button_index];
        let mut items = Vec::new();

        if button.pinned {
            items.push(ContextMenuItem::Unpin);
        } else {
            items.push(ContextMenuItem::Pin);
        }
        if button.is_running() {
            items.push(ContextMenuItem::Close);
            if button.window_count() > 1 {
                items.push(ContextMenuItem::CloseAll);
            }
        }
        items
    }

    fn context_menu_item_at(&self, x: f32, y: f32) -> Option<ContextMenuItem> {
        let menu = self.context_menu.as_ref()?;
        let items = self.context_menu_items(menu.button_index);

        let menu_x = menu.x;
        let menu_y = menu.y - (items.len() as f32 * 28.0 + 8.0);
        let menu_w = 140.0;
        let item_h = 28.0;
        let padding = 4.0;

        if x < menu_x || x > menu_x + menu_w {
            return None;
        }

        for (i, item) in items.iter().enumerate() {
            let iy = menu_y + padding + i as f32 * item_h;
            if y >= iy && y < iy + item_h {
                return Some(*item);
            }
        }
        None
    }

    fn execute_context_menu_item(&mut self, item: ContextMenuItem) {
        let menu = match &self.context_menu {
            Some(m) => m.clone(),
            None => return,
        };
        let idx = menu.button_index;
        if idx >= self.buttons.len() {
            return;
        }

        let button = &self.buttons[idx];
        match item {
            ContextMenuItem::Pin => {
                let app_id = button.app_id.clone();
                let display_name = button.display_name.clone();
                let position = self.pinned_apps.len() as u32;
                self.add_pinned(PinnedApp {
                    app_id: app_id.clone(),
                    display_name,
                    icon_type: IconType::Generic,
                    exec_path: String::new(),
                    position,
                });
                self.events.push(TaskbarEvent::AppPinned {
                    app_id,
                    position,
                });
            }
            ContextMenuItem::Unpin => {
                let app_id = button.app_id.clone();
                self.remove_pinned(&app_id);
                self.events.push(TaskbarEvent::AppUnpinned { app_id });
            }
            ContextMenuItem::Close => {
                if let Some(&wid) = button.window_ids.first() {
                    self.events.push(TaskbarEvent::CloseWindow { window_id: wid });
                }
            }
            ContextMenuItem::CloseAll => {
                for &wid in &button.window_ids {
                    self.events.push(TaskbarEvent::CloseWindow { window_id: wid });
                }
            }
        }
    }

    // ======================================================================
    // Internal: rendering helpers
    // ======================================================================

    fn render_button_content(
        &self,
        cmds: &mut Vec<RenderCommand>,
        button: &TaskbarButton,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        ghost: bool,
    ) {
        let icon_color = if ghost {
            Color::rgba(205, 214, 244, 140) // MOCHA_TEXT with alpha
        } else {
            match button.state {
                ButtonState::Idle => MOCHA_SUBTEXT0,
                ButtonState::Running | ButtonState::Focused => MOCHA_TEXT,
            }
        };

        // Icon placeholder — render a small square or circle as icon stand-in.
        let icon_size = 20.0;
        let icon_x = if self.config.icon_only {
            x + (width - icon_size) / 2.0
        } else {
            x + 8.0
        };
        let icon_y = y + (height - icon_size) / 2.0;

        cmds.push(RenderCommand::FillRect {
            x: icon_x,
            y: icon_y,
            width: icon_size,
            height: icon_size,
            color: icon_color,
            corner_radii: CornerRadii::all(4.0),
        });

        // If in label mode, render truncated name.
        if !self.config.icon_only {
            let label_x = x + 32.0;
            let label_y = y + (height - 12.0) / 2.0;
            let max_chars = ((width - 40.0) / 7.0) as usize;
            let label: String = if button.display_name.len() > max_chars && max_chars > 3 {
                let truncated: String =
                    button.display_name.chars().take(max_chars - 1).collect();
                format!("{truncated}\u{2026}")
            } else {
                button.display_name.clone()
            };

            cmds.push(RenderCommand::Text {
                x: label_x,
                y: label_y,
                text: label,
                color: icon_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 40.0),
            });
        }
    }

    fn render_context_menu(
        &self,
        cmds: &mut Vec<RenderCommand>,
        button_index: usize,
        menu_x: f32,
        menu_y: f32,
    ) {
        let items = self.context_menu_items(button_index);
        if items.is_empty() {
            return;
        }

        let menu_w = 140.0;
        let item_h = 28.0;
        let padding = 4.0;
        let menu_h = items.len() as f32 * item_h + padding * 2.0;
        let actual_y = menu_y - menu_h;

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: menu_x,
            y: actual_y,
            width: menu_w,
            height: menu_h,
            offset_x: 0.0,
            offset_y: 2.0,
            blur: 8.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 80),
            corner_radii: CornerRadii::all(6.0),
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: menu_x,
            y: actual_y,
            width: menu_w,
            height: menu_h,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: menu_x,
            y: actual_y,
            width: menu_w,
            height: menu_h,
            color: MOCHA_SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Items.
        for (i, item) in items.iter().enumerate() {
            let iy = actual_y + padding + i as f32 * item_h;
            let label = match item {
                ContextMenuItem::Pin => "Pin to taskbar",
                ContextMenuItem::Unpin => "Unpin",
                ContextMenuItem::Close => "Close",
                ContextMenuItem::CloseAll => "Close all",
            };
            cmds.push(RenderCommand::Text {
                x: menu_x + 12.0,
                y: iy + 7.0,
                text: label.to_string(),
                color: MOCHA_TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(menu_w - 24.0),
            });
        }
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> TaskbarState {
        TaskbarState::new(TaskbarConfig::default())
    }

    fn make_pinned(id: &str, name: &str, pos: u32) -> PinnedApp {
        PinnedApp {
            app_id: id.to_string(),
            display_name: name.to_string(),
            icon_type: IconType::Generic,
            exec_path: format!("/usr/bin/{id}"),
            position: pos,
        }
    }

    // ==========================================================================
    // Pinning / Unpinning tests
    // ==========================================================================

    #[test]
    fn test_add_pinned_app() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_pinned(make_pinned("files", "Files", 1));

        assert_eq!(state.pinned_apps().len(), 2);
        assert_eq!(state.pinned_apps()[0].app_id, "terminal");
        assert_eq!(state.pinned_apps()[1].app_id, "files");
    }

    #[test]
    fn test_add_duplicate_pinned_ignored() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_pinned(make_pinned("terminal", "Terminal 2", 1));

        assert_eq!(state.pinned_apps().len(), 1);
        assert_eq!(state.pinned_apps()[0].display_name, "Terminal");
    }

    #[test]
    fn test_remove_pinned_app() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_pinned(make_pinned("files", "Files", 1));
        state.add_pinned(make_pinned("editor", "Editor", 2));

        state.remove_pinned("files");

        assert_eq!(state.pinned_apps().len(), 2);
        assert_eq!(state.pinned_apps()[0].app_id, "terminal");
        assert_eq!(state.pinned_apps()[1].app_id, "editor");
        // Positions normalized.
        assert_eq!(state.pinned_apps()[0].position, 0);
        assert_eq!(state.pinned_apps()[1].position, 1);
    }

    #[test]
    fn test_remove_nonexistent_pinned() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.remove_pinned("nonexistent");
        assert_eq!(state.pinned_apps().len(), 1);
    }

    #[test]
    fn test_unpin_via_context_menu_emits_event() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.rebuild_buttons();

        // Simulate right-click to open context menu.
        state.context_menu = Some(ContextMenu {
            button_index: 0,
            x: 60.0,
            y: 20.0,
            visible: true,
        });
        state.execute_context_menu_item(ContextMenuItem::Unpin);

        let events = state.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TaskbarEvent::AppUnpinned {
                app_id: "terminal".to_string()
            }
        );
        assert_eq!(state.pinned_apps().len(), 0);
    }

    // ==========================================================================
    // Reordering tests
    // ==========================================================================

    #[test]
    fn test_reorder_pinned_forward() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));
        state.add_pinned(make_pinned("c", "C", 2));

        state.reorder_pinned(0, 2);

        assert_eq!(state.pinned_apps()[0].app_id, "b");
        assert_eq!(state.pinned_apps()[1].app_id, "c");
        assert_eq!(state.pinned_apps()[2].app_id, "a");
    }

    #[test]
    fn test_reorder_pinned_backward() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));
        state.add_pinned(make_pinned("c", "C", 2));

        state.reorder_pinned(2, 0);

        assert_eq!(state.pinned_apps()[0].app_id, "c");
        assert_eq!(state.pinned_apps()[1].app_id, "a");
        assert_eq!(state.pinned_apps()[2].app_id, "b");
    }

    #[test]
    fn test_reorder_same_position_noop() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));

        state.reorder_pinned(1, 1);

        assert_eq!(state.pinned_apps()[0].app_id, "a");
        assert_eq!(state.pinned_apps()[1].app_id, "b");
    }

    #[test]
    fn test_reorder_out_of_bounds_noop() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));

        state.reorder_pinned(0, 5);
        assert_eq!(state.pinned_apps()[0].app_id, "a");

        state.reorder_pinned(5, 0);
        assert_eq!(state.pinned_apps()[0].app_id, "a");
    }

    // ==========================================================================
    // Window grouping tests
    // ==========================================================================

    #[test]
    fn test_running_windows_grouped_by_app() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));

        state.add_running_window(WindowId(1), "terminal", "Terminal 1");
        state.add_running_window(WindowId(2), "terminal", "Terminal 2");
        state.add_running_window(WindowId(3), "browser", "Browser");

        let buttons = state.buttons();

        // Should have 2 buttons: terminal (pinned+running), browser (running only).
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].app_id, "terminal");
        assert_eq!(buttons[0].window_count(), 2);
        assert!(buttons[0].pinned);
        assert_eq!(buttons[1].app_id, "browser");
        assert_eq!(buttons[1].window_count(), 1);
        assert!(!buttons[1].pinned);
    }

    #[test]
    fn test_remove_window_updates_group() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "editor", "Editor 1");
        state.add_running_window(WindowId(2), "editor", "Editor 2");

        let buttons = state.buttons();
        assert_eq!(buttons[0].window_count(), 2);

        state.remove_running_window(WindowId(1));
        let buttons = state.buttons();
        assert_eq!(buttons[0].window_count(), 1);
    }

    #[test]
    fn test_remove_all_windows_removes_unpinned_button() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "editor", "Editor");

        assert_eq!(state.buttons().len(), 1);

        state.remove_running_window(WindowId(1));
        assert_eq!(state.buttons().len(), 0);
    }

    #[test]
    fn test_pinned_app_stays_when_windows_close() {
        let mut state = default_state();
        state.add_pinned(make_pinned("editor", "Editor", 0));
        state.add_running_window(WindowId(1), "editor", "Editor");

        assert_eq!(state.buttons().len(), 1);
        assert!(state.buttons()[0].is_running());

        state.remove_running_window(WindowId(1));
        let buttons = state.buttons();
        assert_eq!(buttons.len(), 1);
        assert!(!buttons[0].is_running());
        assert_eq!(buttons[0].state, ButtonState::Idle);
    }

    // ==========================================================================
    // Button state transition tests
    // ==========================================================================

    #[test]
    fn test_button_state_idle() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));

        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Idle);
    }

    #[test]
    fn test_button_state_running() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");

        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Running);
    }

    #[test]
    fn test_button_state_focused() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.set_focused_window(Some(WindowId(1)));

        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Focused);
    }

    #[test]
    fn test_button_state_transitions_on_focus_change() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.add_running_window(WindowId(2), "browser", "Browser");

        // Unpinned running apps are sorted alphabetically by app_id, so
        // `browser` (WindowId 2) appears at index 0, `terminal`
        // (WindowId 1) at index 1.

        state.set_focused_window(Some(WindowId(1)));
        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Running); // browser
        assert_eq!(buttons[1].state, ButtonState::Focused); // terminal

        state.set_focused_window(Some(WindowId(2)));
        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Focused); // browser
        assert_eq!(buttons[1].state, ButtonState::Running); // terminal

        state.set_focused_window(None);
        let buttons = state.buttons();
        assert_eq!(buttons[0].state, ButtonState::Running);
        assert_eq!(buttons[1].state, ButtonState::Running);
    }

    // ==========================================================================
    // Click behavior tests
    // ==========================================================================

    #[test]
    fn test_click_focused_window_minimizes() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.set_focused_window(Some(WindowId(1)));
        state.rebuild_buttons();

        state.handle_button_click(0);

        let events = state.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TaskbarEvent::MinimizeWindow {
                window_id: WindowId(1)
            }
        );
    }

    #[test]
    fn test_click_unfocused_window_activates() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.add_running_window(WindowId(2), "browser", "Browser");
        state.set_focused_window(Some(WindowId(2)));
        state.rebuild_buttons();

        // Click the terminal button (index depends on sort, but terminal < browser).
        let term_idx = state
            .buttons
            .iter()
            .position(|b| b.app_id == "terminal")
            .unwrap();
        state.handle_button_click(term_idx);

        let events = state.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TaskbarEvent::ActivateWindow {
                window_id: WindowId(1)
            }
        );
    }

    #[test]
    fn test_click_idle_pinned_launches() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.rebuild_buttons();

        state.handle_button_click(0);

        let events = state.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TaskbarEvent::LaunchApp {
                app_id: "terminal".to_string(),
                exec_path: "/usr/bin/terminal".to_string(),
            }
        );
    }

    // ==========================================================================
    // Rendering tests
    // ==========================================================================

    #[test]
    fn test_render_empty_taskbar() {
        let mut state = default_state();
        let cmds = state.render(1920.0, 48.0);

        // Should have at least background + top border line.
        assert!(cmds.len() >= 2);

        // First command is the background fill.
        match &cmds[0] {
            RenderCommand::FillRect {
                width, height, color, ..
            } => {
                assert_eq!(*width, 1920.0);
                assert_eq!(*height, 48.0);
                assert_eq!(*color, MOCHA_BASE);
            }
            _ => panic!("Expected FillRect as first command"),
        }
    }

    #[test]
    fn test_render_with_pinned_apps() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_pinned(make_pinned("files", "Files", 1));

        let cmds = state.render(1920.0, 48.0);
        // Should render background + top line + icon placeholders for 2 buttons.
        // Each button gets at least an icon rect.
        assert!(cmds.len() >= 4);
    }

    #[test]
    fn test_render_shows_indicator_for_running() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");

        let cmds = state.render(1920.0, 48.0);

        // Should have an indicator (small FillRect with MOCHA_LAVENDER or MOCHA_BLUE color).
        let has_indicator = cmds.iter().any(|cmd| match cmd {
            RenderCommand::FillRect { color, height, .. } => {
                (*color == MOCHA_LAVENDER || *color == MOCHA_BLUE) && *height == 3.0
            }
            _ => false,
        });
        assert!(has_indicator, "Expected an underline indicator for running app");
    }

    #[test]
    fn test_render_shows_badge_for_multiple_windows() {
        let mut state = default_state();
        state.add_running_window(WindowId(1), "editor", "Editor 1");
        state.add_running_window(WindowId(2), "editor", "Editor 2");
        state.add_running_window(WindowId(3), "editor", "Editor 3");

        let cmds = state.render(1920.0, 48.0);

        // Should have a badge circle (FillRect with MOCHA_RED) and text "3".
        let has_badge_bg = cmds.iter().any(|cmd| match cmd {
            RenderCommand::FillRect { color, width, height, .. } => {
                *color == MOCHA_RED && *width == 12.0 && *height == 12.0
            }
            _ => false,
        });
        let has_badge_text = cmds.iter().any(|cmd| match cmd {
            RenderCommand::Text { text, .. } => text == "3",
            _ => false,
        });
        assert!(has_badge_bg, "Expected badge background");
        assert!(has_badge_text, "Expected badge text '3'");
    }

    #[test]
    fn test_render_divider_between_pinned_and_running() {
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.add_running_window(WindowId(2), "browser", "Browser");

        let cmds = state.render(1920.0, 48.0);

        // Should have a vertical divider line between pinned and unpinned sections.
        let has_divider = cmds.iter().any(|cmd| match cmd {
            RenderCommand::Line { color, .. } => *color == MOCHA_SURFACE2,
            _ => false,
        });
        assert!(has_divider, "Expected divider line between sections");
    }

    #[test]
    fn test_render_label_mode() {
        let mut state = TaskbarState::new(TaskbarConfig {
            icon_only: false,
            ..Default::default()
        });
        state.add_pinned(make_pinned("terminal", "Terminal", 0));

        let cmds = state.render(1920.0, 48.0);

        // Should have a text command with the app name.
        let has_label = cmds.iter().any(|cmd| match cmd {
            RenderCommand::Text { text, .. } => text == "Terminal",
            _ => false,
        });
        assert!(has_label, "Expected label text in non-icon-only mode");
    }

    // ==========================================================================
    // Hit testing tests
    // ==========================================================================

    #[test]
    fn test_button_at_x_first_button() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));
        state.rebuild_buttons();

        // First button starts at start_button_width + gap = 48 + 4 = 52.
        let idx = state.button_at_x(60.0);
        assert_eq!(idx, Some(0));
    }

    #[test]
    fn test_button_at_x_second_button() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));
        state.rebuild_buttons();

        // Second button starts at 52 + 44 + 4 = 100.
        let idx = state.button_at_x(105.0);
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn test_button_at_x_before_buttons() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.rebuild_buttons();

        // Click in the start button area.
        let idx = state.button_at_x(30.0);
        assert_eq!(idx, None);
    }

    #[test]
    fn test_button_at_x_beyond_buttons() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.rebuild_buttons();

        // Click way past the last button.
        let idx = state.button_at_x(500.0);
        assert_eq!(idx, None);
    }

    // ==========================================================================
    // Drag tests
    // ==========================================================================

    #[test]
    fn test_drag_threshold_not_met_is_click() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_running_window(WindowId(1), "a", "A Window");
        state.set_focused_window(Some(WindowId(1)));
        state.rebuild_buttons();

        // Press at the first button.
        let press = MouseEvent {
            x: 60.0,
            y: 20.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        state.handle_mouse_event(&press);

        // Move only 2 pixels (below threshold).
        let mv = MouseEvent {
            x: 62.0,
            y: 20.0,
            kind: MouseEventKind::Move,
        };
        state.handle_mouse_event(&mv);

        // Release.
        let release = MouseEvent {
            x: 62.0,
            y: 20.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        };
        state.handle_mouse_event(&release);

        // Should have generated a click event (minimize since focused).
        let events = state.drain_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            TaskbarEvent::MinimizeWindow {
                window_id: WindowId(1)
            }
        );
    }

    #[test]
    fn test_drag_beyond_threshold_activates_drag() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_pinned(make_pinned("b", "B", 1));
        state.rebuild_buttons();

        // Press on first button.
        let press = MouseEvent {
            x: 60.0,
            y: 20.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        };
        state.handle_mouse_event(&press);

        // Move far enough to activate drag.
        let mv = MouseEvent {
            x: 120.0,
            y: 20.0,
            kind: MouseEventKind::Move,
        };
        state.handle_mouse_event(&mv);

        assert!(state.drag.as_ref().is_some_and(|d| d.active));
    }
}
