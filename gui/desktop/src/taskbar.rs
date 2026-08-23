//! Enhanced taskbar system with pinned apps, running apps, drag-to-reorder.
//!
//! Provides a Windows 11-style taskbar with:
//! - Pinned application shortcuts (persisted to config)
//! - Running application indicators with window grouping
//! - Drag-to-reorder pinned items
//! - Drag into/out of pinned section to pin/unpin
//! - Configurable position and appearance (icon-only or icon+name)
//!
//! Every colour comes from the [`Palette`] the caller resolved — see the
//! "Colour" section below.

use appearance::{Palette, readable_on};
use guitk::color::Color;
use guitk::event::{EventResult, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::theme::with_alpha;

use std::collections::HashMap;

// ============================================================================
// Colour
// ============================================================================
//
// This module used to keep ten `const MOCHA_*: Color` of its own, and four
// more colours written inline as raw `Color::rgba(...)` triples with the role
// they were derived from noted only in a comment. All fourteen now come from
// the `Palette` the caller resolved. The taskbar is the shell's one
// permanently-visible surface, so it is also the one where a frozen theme is
// least deniable: with the light mode selected, everything else on screen
// turned pale and a black bar stayed nailed across the bottom.
//
// Six judgements are worth stating, because a reader will otherwise assume the
// module either forgot the theme or changed the design for no reason:
//
// 1. *The focus underline is the accent; the running underline is not.* The
//    accent marks position — "this is the window you are in" — which is what
//    the wide underline under the focused button says. The narrow underline
//    under a running-but-unfocused button says something different: "this app
//    is open." That is presence, not position, so it is `subtext0`: legible,
//    clearly secondary, and unmistakable against the accent beside it. The
//    widths (16 vs 8) already differ; colour is the second channel, and a
//    module that spent both channels on the same distinction would be using
//    one.
//
// 2. *The drag insertion caret is green, not the accent.* This is the one
//    place the taskbar contradicts "position takes the accent", and
//    deliberately: during a reorder drag the caret ("release here") and the
//    focus underline ("you are here") are on screen at the same moment, a few
//    pixels apart, both small bars near the bottom edge. Two marks that must
//    be told apart at a glance cannot be the same hue. `Palette::drop_target`
//    exists for exactly this reason and says so; the caret takes that helper's
//    *hue* but not its alpha, because a 2-pixel-wide bar has no area in which
//    to be translucent.
//
// 3. *The window-count badge is a measurement, so it is frozen to a named
//    hue.* "3 windows" is a count the user reads a number off; a count that
//    changed colour with the accent would say something different on every
//    machine. It stays `red` — the conventional badge hue — and that is
//    `p.red` and emphatically *not* `p.accent`, even on the stock theme where
//    the two happen to sit side by side in the same palette.
//
// 4. *The digit on the badge is derived, never named.* It is ink on a coloured
//    fill, so it is `readable_on(p.red)`, which answers near-black on Mocha's
//    pale red and near-white on Latte's deep one. The old code named
//    `MOCHA_MANTLE`, which is legible on one of those two and invisible on the
//    other.
//
// 5. *The button-background ladder was re-seated on the roles that describe
//    it.* The old ladder ran surface0 (focused) < surface1 (hovered) with a
//    half-transparent surface0 for merely-running; the palette's own docs
//    reserve `surface1` for "a button at rest, a selected row" and `surface2`
//    for "a hovered button". The ladder is therefore surface0-at-alpha
//    (running) < surface1 (focused) < surface2 (hovered), which keeps the
//    original ordering — quieter to louder — while making each step the role
//    that names it.
//
// 6. *The section divider is `overlay0`, not `surface2`.* `overlay0` is the
//    palette's separator role; `surface2` is a raised surface, and the context
//    menu's outline still uses it. The two were the same value in this file
//    only because Mocha's happened to look acceptable for both.

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
            && menu.visible
        {
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
                if let Some(drag) = self.drag.take() {
                    // Dragging out of the taskbar — unpin if it was pinned.
                    // The index can be stale for the same reason it can be on
                    // release: the button list is rebuilt whenever a window
                    // opens or closes.
                    let unpin = drag
                        .active
                        .then(|| self.buttons.get(drag.source_index))
                        .flatten()
                        .filter(|button| button.pinned)
                        .map(|button| button.app_id.clone());
                    if let Some(app_id) = unpin {
                        self.remove_pinned(&app_id);
                        self.events.push(TaskbarEvent::AppUnpinned { app_id });
                    }
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
    ///
    /// `p` is the palette the caller resolved for the user's mode and accent;
    /// this module holds no colours of its own. `bar_width` and `bar_height`
    /// are the dimensions of the taskbar area.
    pub fn render(&mut self, p: &Palette, bar_width: f32, bar_height: f32) -> Vec<RenderCommand> {
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
            color: p.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border line (subtle separator from content area).
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: 0.0,
            x2: bar_width,
            y2: 0.0,
            color: p.surface0,
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
                    // Judgement 6: a separator, which is `overlay0`'s job.
                    color: p.overlay0,
                    width: 1.0,
                });
            }

            // If this button is being dragged and the drag is active,
            // render a ghost at the drag position instead.
            let is_dragged = self
                .drag
                .as_ref()
                .is_some_and(|d| d.active && d.source_index == i);

            if is_dragged {
                // Render insertion indicator at the drop position.
                if let Some(ref drag) = self.drag {
                    let drop_idx = self.drop_target_index(drag.current_x);
                    let indicator_x =
                        self.config.start_button_width + gap + drop_idx as f32 * (btn_width + gap)
                            - gap / 2.0;
                    cmds.push(RenderCommand::FillRect {
                        x: indicator_x - 1.0,
                        y: btn_y + 4.0,
                        width: 2.0,
                        height: btn_h - 8.0,
                        // Judgement 2: green, not the accent, because the
                        // focus underline is on screen at the same moment and
                        // has to be a different hue. `drop_target`'s hue at
                        // full opacity — a 2px bar cannot be translucent.
                        color: p.green,
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
                        // The button as it is being carried: its resting
                        // background, half-there.
                        color: with_alpha(p.surface1, 180),
                        corner_radii: CornerRadii::all(6.0),
                    });
                    self.render_button_content(
                        &mut cmds, p, button, ghost_x, btn_y, btn_width, btn_h, true,
                    );
                }

                x += btn_width + gap;
                continue;
            }

            // Background based on state. Judgement 5: the ladder runs quiet to
            // loud, and each rung is the role whose documentation describes it.
            let bg_color = match (button.state, button.hovered) {
                (_, true) => p.surface2,
                (ButtonState::Focused, false) => p.surface1,
                (ButtonState::Running, false) => with_alpha(p.surface0, 128),
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
            self.render_button_content(&mut cmds, p, button, x, btn_y, btn_width, btn_h, false);

            // Underline indicator for running/focused apps.
            if button.is_running() {
                // Judgement 1: the accent means "you are here", so only the
                // focused button gets it. A running app that is not focused is
                // reporting presence, not position.
                let indicator_color = if button.state == ButtonState::Focused {
                    p.accent
                } else {
                    p.subtext0
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
                    // Judgement 3: a count is a measurement, so its hue is
                    // named and frozen. Not `p.accent`.
                    color: p.red,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: badge_x + 3.0,
                    y: badge_y + 1.0,
                    text: format!("{}", button.window_count()),
                    // Judgement 4: ink on a coloured fill is derived from that
                    // fill, never named alongside it.
                    color: readable_on(p.red),
                    font_size: 9.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            x += btn_width + gap;
        }

        // Context menu rendering.
        if let Some(ref menu) = self.context_menu
            && menu.visible
        {
            self.render_context_menu(&mut cmds, p, menu.button_index, menu.x, menu.y);
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
            let name = running_names.get(app_id).cloned().unwrap_or_default();
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
            && windows.contains(&focused)
        {
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

                // The button list is rebuilt whenever a window opens or
                // closes, so between the press that started the drag and this
                // release the source index can go stale. `get` makes that a
                // dropped drag rather than a panic in the shell.
                let Some(button) = self.buttons.get(src_idx) else {
                    return EventResult::Consumed;
                };
                let app_id = button.app_id.clone();
                let display_name = button.display_name.clone();

                if button.pinned {
                    // Reorder pinned apps. The search below matches on this
                    // very `app_id`, so the slot it finds necessarily carries
                    // it — reading the id back out of the slot would be a
                    // second lookup answering a question we already answered.
                    let pinned_idx = self.pinned_apps.iter().position(|p| p.app_id == app_id);
                    if let Some(from) = pinned_idx {
                        let to = drop_idx.min(self.pinned_apps.len().saturating_sub(1));
                        if from != to {
                            self.reorder_pinned(from, to);
                            self.events.push(TaskbarEvent::PinnedReordered {
                                app_id,
                                new_position: to as u32,
                            });
                        }
                    }
                } else {
                    // Dragging an unpinned running app into the pinned section — pin it.
                    let position = drop_idx as u32;
                    self.add_pinned(PinnedApp {
                        app_id: app_id.clone(),
                        display_name,
                        icon_type: IconType::Generic,
                        exec_path: String::new(),
                        position,
                    });
                    self.events
                        .push(TaskbarEvent::AppPinned { app_id, position });
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
        let Some(button) = self.buttons.get(idx) else {
            return EventResult::Ignored;
        };
        if button.is_running() {
            if button.state == ButtonState::Focused {
                // Already focused — minimize.
                if let Some(&wid) = button.window_ids.first() {
                    self.events
                        .push(TaskbarEvent::MinimizeWindow { window_id: wid });
                }
            } else {
                // Bring to front.
                if let Some(&wid) = button.window_ids.first() {
                    self.events
                        .push(TaskbarEvent::ActivateWindow { window_id: wid });
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
        let Some(button) = self.buttons.get(button_index) else {
            return Vec::new();
        };
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
        // The menu carries the index it was opened for, and the button list is
        // rebuilt whenever a window opens or closes — so between the right
        // click and the menu click the index can go stale. `get` is what makes
        // that a no-op rather than a panic in the shell.
        let Some(button) = self.buttons.get(idx) else {
            return;
        };
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
                self.events
                    .push(TaskbarEvent::AppPinned { app_id, position });
            }
            ContextMenuItem::Unpin => {
                let app_id = button.app_id.clone();
                self.remove_pinned(&app_id);
                self.events.push(TaskbarEvent::AppUnpinned { app_id });
            }
            ContextMenuItem::Close => {
                if let Some(&wid) = button.window_ids.first() {
                    self.events
                        .push(TaskbarEvent::CloseWindow { window_id: wid });
                }
            }
            ContextMenuItem::CloseAll => {
                for &wid in &button.window_ids {
                    self.events
                        .push(TaskbarEvent::CloseWindow { window_id: wid });
                }
            }
        }
    }

    // ======================================================================
    // Internal: rendering helpers
    // ======================================================================

    #[allow(
        clippy::too_many_arguments,
        reason = "geometry plus the palette; splitting it into a struct would \
                  hide which of the six numbers a caller got wrong"
    )]
    fn render_button_content(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
        button: &TaskbarButton,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        ghost: bool,
    ) {
        let icon_color = if ghost {
            // Being carried: the primary ink, half-there, to match the ghost
            // background behind it.
            with_alpha(p.text, 140)
        } else {
            match button.state {
                ButtonState::Idle => p.subtext0,
                ButtonState::Running | ButtonState::Focused => p.text,
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

        // If in label mode, render the name, fitted to the button by the
        // renderer.
        if !self.config.icon_only {
            let label_x = x + 32.0;
            let label_y = y + (height - 12.0) / 2.0;
            // The name goes to the renderer whole. `max_width` +
            // `TextOverflow::Ellipsis` below already say "fit this to the
            // button and mark the cut", and the renderer does it by measuring
            // the face it is about to draw in. The pre-truncation this replaces
            // did the same job by guessing — `(width - 40) / 7` characters,
            // compared against `display_name.len()`, which is a count of
            // *bytes*, so a name with one accented letter was elided a
            // character early and a CJK one several early. Two elisions in
            // series is also strictly worse than one: the guess cut first, so
            // the measured pass never saw the text it was meant to fit.
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: label_y,
                text: button.display_name.clone(),
                color: icon_color,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 40.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_context_menu(
        &self,
        cmds: &mut Vec<RenderCommand>,
        p: &Palette,
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
            // Black in both modes: a shadow is an absence of light, not a
            // colour. The shell had three different alphas for this one thing;
            // `Palette::shadow` is the one it settled on.
            color: p.shadow(),
            corner_radii: CornerRadii::all(6.0),
        });

        // Background. A menu floats, so it takes the transparency the user set
        // for floating surfaces rather than a flat surface colour.
        cmds.push(RenderCommand::FillRect {
            x: menu_x,
            y: actual_y,
            width: menu_w,
            height: menu_h,
            color: p.panel_bg(),
            corner_radii: CornerRadii::all(6.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: menu_x,
            y: actual_y,
            width: menu_w,
            height: menu_h,
            color: p.surface2,
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
                color: p.text,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(menu_w - 24.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]
    // These tests search the render output for a rectangle of an exact size —
    // a literal the renderer writes and the test reads back with no arithmetic
    // in between. Exact equality is the assertion meant; a tolerance here would
    // let a 3.0-tall indicator pass as a 3.4-tall one.
    #![allow(clippy::float_cmp)]

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

    /// A palette on which two questions this module asks have distinct answers.
    ///
    /// Both changes exist because the *stock* palette cannot tell two things
    /// apart, and a test run only against it would therefore never fail:
    ///
    /// - The stock accent **is** `blue`, so "the focus underline is
    ///   `p.accent`" would pass just as happily against a module that still
    ///   said `blue` — which is precisely the confusion this conversion exists
    ///   to remove. The probe accent is a hue nothing else in either palette
    ///   supplies, and that is asserted rather than assumed.
    /// - The stock `panel_alpha` is 255, so `panel_bg()` and `base` are the
    ///   same colour and "the menu is a panel" is unfalsifiable. Setting a
    ///   real transparency makes the difference observable.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        p.panel_alpha = 200;
        assert!(
            p.roles()
                .iter()
                .filter(|(name, c)| *name != "accent" && *c == p.accent)
                .count()
                == 0,
            "the probe accent collides with another role, so accent \
             assertions below would not distinguish them"
        );
        assert_ne!(
            p.panel_bg(),
            p.base,
            "a panel that is indistinguishable from a flat surface makes the \
             menu's role assertion vacuous"
        );
        p
    }

    /// A taskbar in which every branch of `render` is taken at once.
    ///
    /// Five buttons: one being dragged (ghost + insertion caret), one focused,
    /// one idle, one hovered, and one unpinned-and-running with three windows
    /// (the count badge). Four pinned before one unpinned forces the section
    /// divider, and a visible context menu forces the popup.
    fn full_fixture() -> TaskbarState {
        let mut s = default_state();
        s.add_pinned(make_pinned("dragged", "Dragged", 0));
        s.add_pinned(make_pinned("focused", "Focused", 1));
        s.add_pinned(make_pinned("idle", "Idle", 2));
        s.add_pinned(make_pinned("hovered", "Hovered", 3));
        s.add_running_window(WindowId(1), "focused", "Focused");
        s.add_running_window(WindowId(2), "hovered", "Hovered");
        s.add_running_window(WindowId(10), "many", "Many 1");
        s.add_running_window(WindowId(11), "many", "Many 2");
        s.add_running_window(WindowId(12), "many", "Many 3");
        s.set_focused_window(Some(WindowId(1)));
        s.hover_index = Some(3);
        s.drag = Some(DragState {
            source_index: 0,
            current_x: 400.0,
            current_y: 20.0,
            start_x: 60.0,
            start_y: 20.0,
            active: true,
        });
        s.context_menu = Some(ContextMenu {
            button_index: 1,
            x: 200.0,
            y: 300.0,
            visible: true,
        });
        s.rebuild_buttons();
        s
    }

    /// The same taskbar in label mode, which is the one branch `full_fixture`
    /// cannot also be in.
    fn label_fixture() -> TaskbarState {
        let mut s = TaskbarState::new(TaskbarConfig {
            icon_only: false,
            ..Default::default()
        });
        s.add_pinned(make_pinned("terminal", "Terminal", 0));
        s.add_running_window(WindowId(1), "terminal", "Terminal");
        s.set_focused_window(Some(WindowId(1)));
        s.rebuild_buttons();
        s
    }

    /// Every `FillRect` as `(x, width, height, colour)`.
    ///
    /// `x` is carried because several fills share a size and differ only in
    /// which button they belong to — and a table that cannot say *which* site
    /// a colour came from cannot see two sites swapped.
    fn fills(cmds: &[RenderCommand]) -> Vec<(f32, f32, f32, Color)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x,
                    width,
                    height,
                    color,
                    ..
                } => Some((*x, *width, *height, *color)),
                _ => None,
            })
            .collect()
    }

    /// Where button `i` is drawn, given the default config.
    ///
    /// `start_button_width + gap + i * (button_icon_width + gap)`, spelled out
    /// so the table below is indexed by a number the test computed and not by
    /// one it read back out of the render.
    fn button_x(i: usize) -> f32 {
        48.0 + 4.0 + i as f32 * (44.0 + 4.0)
    }

    /// Every colour the render puts on screen, whatever command carries it.
    fn all_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::BoxShadow { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn rgb_eq(a: Color, b: Color) -> bool {
        a.r == b.r && a.g == b.g && a.b == b.b
    }

    /// The one fill of a given size, or a failure naming what was there.
    fn only_fill(cmds: &[RenderCommand], w: f32, h: f32, what: &str) -> Color {
        let hits: Vec<Color> = fills(cmds)
            .into_iter()
            .filter(|(_, fw, fh, _)| *fw == w && *fh == h)
            .map(|(_, _, _, c)| c)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "expected exactly one {what} ({w}x{h}), found {}",
            hits.len()
        );
        hits[0]
    }

    #[test]
    fn test_render_empty_taskbar() {
        let p = Palette::for_mode(false);
        let mut state = default_state();
        let cmds = state.render(&p, 1920.0, 48.0);

        // Should have at least background + top border line.
        assert!(cmds.len() >= 2);

        // First command is the background fill.
        match &cmds[0] {
            RenderCommand::FillRect {
                width,
                height,
                color,
                ..
            } => {
                assert_eq!(*width, 1920.0);
                assert_eq!(*height, 48.0);
                assert_eq!(*color, p.base);
            }
            _ => panic!("Expected FillRect as first command"),
        }
    }

    #[test]
    fn test_render_with_pinned_apps() {
        let p = Palette::for_mode(false);
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_pinned(make_pinned("files", "Files", 1));

        let cmds = state.render(&p, 1920.0, 48.0);
        // Should render background + top line + icon placeholders for 2 buttons.
        // Each button gets at least an icon rect.
        assert!(cmds.len() >= 4);
    }

    #[test]
    fn test_render_shows_indicator_for_running() {
        let p = accented(false);
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");

        let cmds = state.render(&p, 1920.0, 48.0);

        // A running-but-unfocused app reports presence, not position, so its
        // underline is `subtext0` and explicitly not the accent.
        let has_indicator = cmds.iter().any(|cmd| match cmd {
            RenderCommand::FillRect { color, height, .. } => *color == p.subtext0 && *height == 3.0,
            _ => false,
        });
        assert!(
            has_indicator,
            "Expected an underline indicator for running app"
        );
    }

    #[test]
    fn test_render_shows_badge_for_multiple_windows() {
        let p = Palette::for_mode(false);
        let mut state = default_state();
        state.add_running_window(WindowId(1), "editor", "Editor 1");
        state.add_running_window(WindowId(2), "editor", "Editor 2");
        state.add_running_window(WindowId(3), "editor", "Editor 3");

        let cmds = state.render(&p, 1920.0, 48.0);

        // Should have a badge circle (a 12x12 fill in `red`) and text "3".
        let has_badge_bg = cmds.iter().any(|cmd| match cmd {
            RenderCommand::FillRect {
                color,
                width,
                height,
                ..
            } => *color == p.red && *width == 12.0 && *height == 12.0,
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
        let p = Palette::for_mode(false);
        let mut state = default_state();
        state.add_pinned(make_pinned("terminal", "Terminal", 0));
        state.add_running_window(WindowId(1), "terminal", "Terminal");
        state.add_running_window(WindowId(2), "browser", "Browser");

        let cmds = state.render(&p, 1920.0, 48.0);

        // Should have a vertical divider line between pinned and unpinned
        // sections, in the palette's separator role.
        let has_divider = cmds.iter().any(|cmd| match cmd {
            RenderCommand::Line { color, x1, x2, .. } => *color == p.overlay0 && x1 == x2,
            _ => false,
        });
        assert!(has_divider, "Expected divider line between sections");
    }

    #[test]
    fn test_render_label_mode() {
        let p = Palette::for_mode(false);
        let mut state = label_fixture();

        let cmds = state.render(&p, 1920.0, 48.0);

        // Should have a text command with the app name.
        let has_label = cmds.iter().any(|cmd| match cmd {
            RenderCommand::Text { text, .. } => text == "Terminal",
            _ => false,
        });
        assert!(has_label, "Expected label text in non-icon-only mode");
    }

    // ==========================================================================
    // Colour: the conversion off this module's own ten constants
    // ==========================================================================

    /// The membership sweep, in both modes, over a render that takes every
    /// branch — see `crate::palette_check` for why the *light* pass is the one
    /// that finds a leftover Mocha constant.
    ///
    /// `derived` is empty on purpose. Everything this module computes is
    /// either a role at an alpha (which the sweep compares on RGB alone), a
    /// `readable_on` endpoint, or black — so a colour that needs declaring
    /// here would be a colour that came from nowhere.
    #[test]
    fn every_colour_the_taskbar_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            crate::palette_check::assert_drawn_from(&p, &cmds, &[], "taskbar (full)");

            let cmds = label_fixture().render(&p, 1920.0, 48.0);
            crate::palette_check::assert_drawn_from(&p, &cmds, &[], "taskbar (labels)");

            let cmds = default_state().render(&p, 1920.0, 48.0);
            crate::palette_check::assert_drawn_from(&p, &cmds, &[], "taskbar (empty)");
        }
    }

    /// A sweep is only as wide as the render it is handed, so this pins the
    /// fixture's coverage rather than trusting it.
    ///
    /// Every `if` in `render` and its two helpers is represented here. If a
    /// future edit makes one of these branches unreachable from the fixture,
    /// this fails loudly instead of the sweep quietly checking less.
    #[test]
    fn the_fixture_takes_every_branch_this_module_has() {
        let p = accented(false);
        let cmds = full_fixture().render(&p, 1920.0, 48.0);

        // Bar background and its top border.
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, _)| *w == 1920.0 && *h == 48.0)
        );
        assert!(cmds.iter().any(|c| matches!(
            c,
            RenderCommand::Line { x1, x2, .. } if *x1 == 0.0 && *x2 == 1920.0
        )));
        // The pinned/running divider is the vertical line.
        assert!(cmds.iter().any(|c| matches!(
            c,
            RenderCommand::Line { x1, x2, .. } if x1 == x2
        )));
        // Drag: the 2px insertion caret and the full-size ghost.
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, _)| *w == 2.0 && *h == 36.0)
        );
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, c)| *w == 44.0 && *h == 44.0 && c.a == 180)
        );
        // Three button backgrounds: hovered, focused, running-at-alpha. The
        // idle one is `TRANSPARENT` and is therefore *not* pushed, which is
        // asserted by counting below rather than by looking for it.
        let bgs: Vec<Color> = fills(&cmds)
            .into_iter()
            .filter(|(_, w, h, _)| *w == 44.0 && *h == 44.0)
            .map(|(_, _, _, c)| c)
            .collect();
        assert_eq!(bgs.len(), 4, "ghost + three drawn button backgrounds");
        // …and each rung is actually *taken*. Counting four backgrounds does
        // not distinguish "hovered, focused, running" from "running, running,
        // running" — which is exactly what this fixture degrades into if it
        // stops hovering anything, since the hovered app is also running and
        // so keeps drawing a background, just a different one. Found by
        // harness defect Hx38, which cleared `hover_index` and left the count
        // at four; only the positional table noticed.
        for (rung, what) in [
            (p.surface2, "hovered"),
            (p.surface1, "focused"),
            (with_alpha(p.surface0, 128), "running-but-unfocused"),
        ] {
            assert!(
                bgs.contains(&rung),
                "the fixture no longer reaches the {what} rung of the button \
                 background ladder, so nothing renders it and every test that \
                 checks it passes vacuously"
            );
        }
        // Both underlines.
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, _)| *w == 16.0 && *h == 3.0)
        );
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, _)| *w == 8.0 && *h == 3.0)
        );
        // Badge and its digit.
        assert!(
            fills(&cmds)
                .iter()
                .any(|(_, w, h, _)| *w == 12.0 && *h == 12.0)
        );
        assert!(cmds.iter().any(|c| matches!(
            c,
            RenderCommand::Text { text, .. } if text == "3"
        )));
        // Icon placeholders: one per button, dragged one included.
        assert_eq!(
            fills(&cmds)
                .iter()
                .filter(|(_, w, h, _)| *w == 20.0 && *h == 20.0)
                .count(),
            5
        );
        // The context menu: shadow, panel, outline, and at least one label.
        assert!(
            cmds.iter()
                .any(|c| matches!(c, RenderCommand::BoxShadow { .. }))
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, RenderCommand::StrokeRect { .. }))
        );
        assert!(fills(&cmds).iter().any(|(_, w, _, _)| *w == 140.0));

        // And the branch no `full_fixture` render can reach.
        let cmds = label_fixture().render(&p, 1920.0, 48.0);
        assert!(cmds.iter().any(|c| matches!(
            c,
            RenderCommand::Text { text, .. } if text == "Terminal"
        )));
    }

    /// Every colour on the bar itself, named against the role literal.
    ///
    /// Written as role literals rather than as "whatever `render` produced",
    /// because an expectation phrased in terms of the code under test cannot
    /// fail. Run in both modes: a mapping that is right in the palette it was
    /// converted *from* proves nothing.
    #[test]
    fn every_colour_on_the_bar_is_in_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);

            assert_eq!(only_fill(&cmds, 1920.0, 48.0, "bar background"), p.base);
            assert_eq!(only_fill(&cmds, 16.0, 3.0, "focus underline"), p.accent);
            // Two apps are running-but-unfocused, and *both* underlines are
            // checked: a per-site table with one representative per site is
            // not a per-site table.
            let running: Vec<Color> = fills(&cmds)
                .into_iter()
                .filter(|(_, w, h, _)| *w == 8.0 && *h == 3.0)
                .map(|(_, _, _, c)| c)
                .collect();
            assert_eq!(running, vec![p.subtext0; 2], "running underlines");
            assert_eq!(only_fill(&cmds, 12.0, 12.0, "count badge"), p.red);
            assert_eq!(only_fill(&cmds, 2.0, 36.0, "insertion caret"), p.green);

            // The top border and the section divider are both `Line`s, told
            // apart by orientation.
            let horizontal: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { x1, x2, color, .. } if x1 != x2 => Some(*color),
                    _ => None,
                })
                .collect();
            let vertical: Vec<Color> = cmds
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::Line { x1, x2, color, .. } if x1 == x2 => Some(*color),
                    _ => None,
                })
                .collect();
            assert_eq!(horizontal, vec![p.surface0], "the bar's top border");
            assert_eq!(vertical, vec![p.overlay0], "the pinned/running divider");

            // The digit on the badge is chosen *from* the badge, so it is one
            // of the two `readable_on` answers and not a named role.
            let digit = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, color, .. } if text == "3" => Some(*color),
                    _ => None,
                })
                .expect("badge digit");
            assert_eq!(digit, readable_on(p.red));
        }
    }

    /// The four button states, each in the role that describes it — and each
    /// checked at the button it belongs to, not merely somewhere in the render.
    ///
    /// The site matters because these four colours are drawn as one shape at
    /// one size and differ only in `x`. A table that asked "is `surface1`
    /// present" would be satisfied by a render that had swapped the hovered
    /// and focused rungs of the ladder, which is a permutation of the same
    /// set. Indexing by `button_x` is what makes the ladder's *order*
    /// falsifiable.
    #[test]
    fn each_button_state_draws_the_background_its_role_names() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            let bgs: Vec<(f32, Color)> = fills(&cmds)
                .into_iter()
                .filter(|(_, w, h, _)| *w == 44.0 && *h == 44.0)
                .map(|(x, _, _, c)| (x, c))
                .collect();

            let at = |x: f32| -> Color {
                bgs.iter()
                    .find(|(bx, _)| *bx == x)
                    .unwrap_or_else(|| panic!("no button background at x={x}"))
                    .1
            };
            // Button 1 is focused, 3 is hovered, 4 is running-but-unfocused.
            assert_eq!(at(button_x(1)), p.surface1, "focused is surface1");
            assert_eq!(at(button_x(3)), p.surface2, "hovered is surface2");
            assert_eq!(
                at(button_x(4)),
                with_alpha(p.surface0, 128),
                "running-but-unfocused is surface0 at half"
            );
            // The ghost follows the pointer, so it is the one background not
            // at a button's x: `current_x - btn_width / 2`.
            assert_eq!(
                at(400.0 - 22.0),
                with_alpha(p.surface1, 180),
                "the dragged ghost is surface1 at 180"
            );
            // Four backgrounds for five buttons: button 2 is idle, and idle is
            // `TRANSPARENT`, which is not drawn at all. Counting is the only
            // way to assert that — there is no colour to go looking for.
            assert_eq!(bgs.len(), 4);
            assert!(
                !bgs.iter().any(|(x, _)| *x == button_x(2)),
                "an idle button drew a background"
            );
        }
    }

    /// Button ink: idle is secondary, running and focused are primary, and a
    /// button being carried is the primary ink made half-there.
    #[test]
    fn button_ink_follows_the_button_state() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            let icons: Vec<Color> = fills(&cmds)
                .into_iter()
                .filter(|(_, w, h, _)| *w == 20.0 && *h == 20.0)
                .map(|(_, _, _, c)| c)
                .collect();

            assert_eq!(icons.len(), 5);
            assert_eq!(
                icons
                    .iter()
                    .filter(|c| **c == with_alpha(p.text, 140))
                    .count(),
                1,
                "exactly one button is being dragged"
            );
            assert_eq!(
                icons.iter().filter(|c| **c == p.subtext0).count(),
                1,
                "exactly one button is idle"
            );
            assert_eq!(
                icons.iter().filter(|c| **c == p.text).count(),
                3,
                "the other three are running or focused"
            );

            // In label mode the label shares the icon's colour, which is the
            // point of computing it once.
            let cmds = label_fixture().render(&p, 1920.0, 48.0);
            let label = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::Text { text, color, .. } if text == "Terminal" => Some(*color),
                    _ => None,
                })
                .expect("label");
            assert_eq!(label, p.text);
        }
    }

    /// The context menu floats, so it is a panel and a shadow, not a surface.
    #[test]
    fn every_colour_in_the_context_menu_is_in_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);

            let shadow = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::BoxShadow { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("menu shadow");
            assert_eq!(shadow, p.shadow());
            assert_eq!(
                (shadow.r, shadow.g, shadow.b),
                (0, 0, 0),
                "a shadow is an absence of light, so it does not flip with the mode"
            );

            let panel = fills(&cmds)
                .into_iter()
                .find(|(_, w, _, _)| *w == 140.0)
                .expect("menu panel")
                .3;
            assert_eq!(panel, p.panel_bg());
            assert_eq!(
                panel.a, p.panel_alpha,
                "a menu takes the user's transparency setting"
            );

            let outline = cmds
                .iter()
                .find_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("menu outline");
            assert_eq!(outline, p.surface2);

            for item in ["Unpin", "Close", "Close all", "Pin to taskbar"] {
                if let Some(color) = cmds.iter().find_map(|c| match c {
                    RenderCommand::Text { text, color, .. } if text == item => Some(*color),
                    _ => None,
                }) {
                    assert_eq!(color, p.text, "menu item {item}");
                }
            }
        }
    }

    /// Judgement 1: exactly one mark on the taskbar says "you are here".
    ///
    /// Counting rather than inspecting, because the module draws several small
    /// bars and the question is not "is this one the accent" but "how many
    /// things claim to be the focus". A second accent-coloured mark is the
    /// failure; a test that only looked at the focus underline could not see
    /// it.
    #[test]
    fn exactly_one_thing_on_the_taskbar_carries_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            let n = all_colors(&cmds)
                .into_iter()
                .filter(|c| rgb_eq(*c, p.accent))
                .count();
            assert_eq!(
                n,
                1,
                "the focused button's underline is the only accent on the bar \
                 ({} in {} mode)",
                n,
                if light { "light" } else { "dark" }
            );
        }
    }

    /// Judgement 2: the "release here" caret and the "you are here" underline
    /// are on screen together, so they must not be the same hue.
    ///
    /// The check is a hue comparison and not "the caret is green", because
    /// what matters is that the two are told apart — a future edit that moved
    /// the focus underline to green would satisfy a literal test and break the
    /// thing the literal was standing in for.
    #[test]
    fn the_drop_caret_is_never_the_same_hue_as_the_focus_underline() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            let caret = only_fill(&cmds, 2.0, 36.0, "insertion caret");
            let focus = only_fill(&cmds, 16.0, 3.0, "focus underline");
            assert!(
                !rgb_eq(caret, focus),
                "the drop caret and the focus underline are both on screen \
                 during a reorder and both are small bars; identical hues make \
                 them one mark"
            );
        }
    }

    /// Judgement 3: a count is a measurement, so the accent must not reach it.
    #[test]
    fn the_count_badge_does_not_follow_the_accent() {
        for light in [false, true] {
            let p = accented(light);
            let cmds = full_fixture().render(&p, 1920.0, 48.0);
            let badge = only_fill(&cmds, 12.0, 12.0, "count badge");
            assert!(
                !rgb_eq(badge, p.accent),
                "a window count would say something different on every machine \
                 if it followed the accent"
            );
            assert_eq!(badge, p.red);
        }
    }

    /// Judgement 4: the digit is *derived* from the badge, not named beside it.
    ///
    /// Proved by the two modes disagreeing. `readable_on` answers near-black on
    /// Mocha's pale red and near-white on Latte's deep one, so a module that
    /// froze the digit to any single value fails one of the two.
    #[test]
    fn the_badge_digit_is_computed_from_the_badge_it_sits_on() {
        let digits: Vec<Color> = [false, true]
            .into_iter()
            .map(|light| {
                let p = accented(light);
                let cmds = full_fixture().render(&p, 1920.0, 48.0);
                let badge = only_fill(&cmds, 12.0, 12.0, "count badge");
                let digit = cmds
                    .iter()
                    .find_map(|c| match c {
                        RenderCommand::Text { text, color, .. } if text == "3" => Some(*color),
                        _ => None,
                    })
                    .expect("badge digit");
                assert_eq!(digit, readable_on(badge));
                digit
            })
            .collect();
        assert_ne!(
            digits[0], digits[1],
            "the two modes' reds are on opposite sides of the legibility \
             threshold, so a digit that is the same in both was not derived \
             from the fill"
        );
    }

    /// The underlines differ in width as well as in colour.
    ///
    /// Two channels for one distinction is redundancy, not waste: the focus
    /// mark has to survive a user who cannot tell the accent from `subtext0`.
    #[test]
    fn the_focus_underline_is_wider_than_the_running_one() {
        let p = accented(false);
        let cmds = full_fixture().render(&p, 1920.0, 48.0);
        let underlines: Vec<f32> = fills(&cmds)
            .into_iter()
            .filter(|(_, _, h, _)| *h == 3.0)
            .map(|(_, w, _, _)| w)
            .collect();
        assert_eq!(underlines.len(), 3, "one focused, two running");
        assert_eq!(underlines.iter().filter(|w| **w == 16.0).count(), 1);
        assert_eq!(underlines.iter().filter(|w| **w == 8.0).count(), 2);
    }

    /// The whole point, stated as one assertion: the bar is not the same in
    /// both modes.
    #[test]
    fn the_render_is_not_the_same_in_both_modes() {
        let dark = full_fixture().render(&accented(false), 1920.0, 48.0);
        let light = full_fixture().render(&accented(true), 1920.0, 48.0);
        assert_eq!(dark.len(), light.len(), "same shapes, different colours");
        assert_ne!(
            all_colors(&dark),
            all_colors(&light),
            "a module that ignored its palette would draw the same bar twice"
        );
    }

    /// None of the ten deleted constants survives anywhere in a light render.
    ///
    /// The sweep above already rejects any colour outside the palette, but it
    /// cannot name the offender in the terms the conversion was written in.
    /// This one does, and it covers the case the sweep is blind to: a value
    /// that happens to also be a `readable_on` endpoint.
    #[test]
    fn none_of_the_ten_deleted_constants_is_still_drawn() {
        const DELETED: [(&str, u32); 10] = [
            ("MOCHA_BASE", 0x001E_1E2E),
            ("MOCHA_SURFACE0", 0x0031_3244),
            ("MOCHA_SURFACE1", 0x0045_475A),
            ("MOCHA_SURFACE2", 0x0058_5B70),
            ("MOCHA_TEXT", 0x00CD_D6F4),
            ("MOCHA_SUBTEXT0", 0x00A6_ADC8),
            ("MOCHA_BLUE", 0x0089_B4FA),
            ("MOCHA_LAVENDER", 0x00B4_BEFE),
            ("MOCHA_RED", 0x00F3_8BA8),
            ("MOCHA_MANTLE", 0x0018_1825),
        ];
        let p = accented(true);
        let mut cmds = full_fixture().render(&p, 1920.0, 48.0);
        cmds.extend(label_fixture().render(&p, 1920.0, 48.0));

        for c in all_colors(&cmds) {
            let rgb = (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
            for (name, value) in DELETED {
                assert_ne!(
                    rgb, value,
                    "the light taskbar still draws {name} (#{value:06X}), so \
                     that constant's substitution was missed"
                );
            }
        }
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

    /// The button list is rebuilt whenever a window opens or closes, which can
    /// happen between the press that starts a drag and the release that ends
    /// it — the dragged window closing on its own is the ordinary case. The
    /// index the drag carries is then stale, and a taskbar is the last process
    /// in the system that may abort over it.
    #[test]
    fn a_drag_whose_button_disappeared_mid_gesture_is_dropped_not_fatal() {
        let mut state = default_state();
        state.add_pinned(make_pinned("a", "A", 0));
        state.add_running_window(WindowId(1), "b", "B Window");
        state.add_running_window(WindowId(2), "c", "C Window");
        state.rebuild_buttons();
        let last = state.buttons.len() - 1;
        let x = (0..2000)
            .map(|i| i as f32)
            .find(|&x| state.button_at_x(x) == Some(last))
            .expect("the last button has to be somewhere on the bar");

        state.handle_mouse_event(&MouseEvent {
            x,
            y: 20.0,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
        state.handle_mouse_event(&MouseEvent {
            x: x + 40.0,
            y: 20.0,
            kind: MouseEventKind::Move,
        });

        // Both unpinned windows close while the button is still held.
        state.remove_running_window(WindowId(1));
        state.remove_running_window(WindowId(2));
        state.rebuild_buttons();
        state.drain_events();

        let result = state.handle_mouse_event(&MouseEvent {
            x: x + 40.0,
            y: 20.0,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        assert_eq!(result, EventResult::Consumed);
        assert!(
            state.drain_events().is_empty(),
            "a drag of a button that no longer exists must not pin or reorder anything",
        );
    }

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
