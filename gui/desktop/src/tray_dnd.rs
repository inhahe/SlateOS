//! System tray drag-and-drop and icon arrangement.
//!
//! Provides drag-and-drop support for tray icons:
//! - Dragging icons out of the tray to remove/hide them
//! - Dragging icons into the tray from the taskbar to pin them
//! - Reordering icons within the tray via drag
//! - "Start minimized to tray" per-app configuration
//! - Enhanced context menu with pin/unpin/hide/show actions
//!
//! Uses the toolkit's DnD primitives ([`DataObject`], [`DataFormat`],
//! [`DragDropManager`]) for format negotiation and drop handling.

use guitk::dnd::{DataFormat, DataObject, DragDropManager, DropEffect, DropTarget};

use std::collections::HashMap;

// ============================================================================
// Constants
// ============================================================================

/// Minimum pixel movement before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 5.0;

/// Custom data format for tray icon drag data (icon ID + app name).
const TRAY_ICON_FORMAT: &str = "application/x-ouros-tray-icon";

/// Custom data format for taskbar app data (app ID being dragged in).
const TASKBAR_APP_FORMAT: &str = "application/x-ouros-taskbar-app";

/// Maximum number of visible icons before overflow kicks in.
const DEFAULT_MAX_VISIBLE: usize = 12;

// ============================================================================
// TrayDragSource
// ============================================================================

/// Tracks drag initiation from tray icons and provides visual feedback.
///
/// When the user presses and drags a tray icon beyond the threshold,
/// a drag operation begins carrying the icon's ID and app name.
/// Dropping outside the tray hides the icon; dropping on another
/// tray position reorders; pressing Escape cancels.
pub struct TrayDragSource {
    /// Whether a mouse press has started (potential drag).
    press_active: bool,
    /// X coordinate of the initial press.
    press_x: f32,
    /// Y coordinate of the initial press.
    press_y: f32,
    /// Icon ID that was pressed on.
    press_icon_id: Option<u64>,
    /// Whether the drag threshold has been exceeded.
    drag_active: bool,
    /// Whether the icon should appear semi-transparent (drag in progress).
    pub show_ghost: bool,
    /// The icon ID currently being dragged (if any).
    pub dragging_icon_id: Option<u64>,
    /// Whether the drag was cancelled via Escape.
    cancelled: bool,
}

impl TrayDragSource {
    /// Create a new drag source with no active drag.
    pub fn new() -> Self {
        Self {
            press_active: false,
            press_x: 0.0,
            press_y: 0.0,
            press_icon_id: None,
            drag_active: false,
            show_ghost: false,
            dragging_icon_id: None,
            cancelled: false,
        }
    }

    /// Called when the user presses on a tray icon. Records the position
    /// for threshold checking.
    pub fn on_press(&mut self, icon_id: u64, x: f32, y: f32) {
        self.press_active = true;
        self.press_x = x;
        self.press_y = y;
        self.press_icon_id = Some(icon_id);
        self.drag_active = false;
        self.show_ghost = false;
        self.dragging_icon_id = None;
        self.cancelled = false;
    }

    /// Called on mouse move. Returns `true` if the drag just became active
    /// (threshold exceeded for the first time).
    pub fn on_move(&mut self, x: f32, y: f32) -> bool {
        if !self.press_active || self.cancelled {
            return false;
        }
        if self.drag_active {
            // Already dragging -- nothing new to report.
            return false;
        }
        let dx = x - self.press_x;
        let dy = y - self.press_y;
        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
            self.drag_active = true;
            self.show_ghost = true;
            self.dragging_icon_id = self.press_icon_id;
            return true;
        }
        false
    }

    /// Build a [`DataObject`] carrying the tray icon's ID and app name
    /// for use with the toolkit's DnD manager.
    pub fn build_drag_data(&self, icon_id: u64, app_name: &str) -> DataObject {
        let mut data = DataObject::new();
        let payload = format!("{icon_id}:{app_name}");
        data.set_data(
            DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
            payload.into_bytes(),
        );
        data
    }

    /// Cancel the current drag (e.g., Escape pressed).
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.drag_active = false;
        self.show_ghost = false;
        self.dragging_icon_id = None;
        self.press_active = false;
    }

    /// Called on mouse release. Resets internal state.
    /// Returns `true` if a drag was active when released (i.e., a drop happened).
    pub fn on_release(&mut self) -> bool {
        let was_dragging = self.drag_active;
        self.press_active = false;
        self.drag_active = false;
        self.show_ghost = false;
        self.dragging_icon_id = None;
        self.cancelled = false;
        was_dragging
    }

    /// Whether a drag is currently in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag_active && !self.cancelled
    }

    /// Whether the press was just a click (released without exceeding threshold).
    pub fn was_click(&self) -> bool {
        self.press_active && !self.drag_active && !self.cancelled
    }
}

impl Default for TrayDragSource {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TrayDropTarget
// ============================================================================

/// Accepts icons being dragged into the tray area.
///
/// Handles two kinds of inbound drops:
/// - **From taskbar**: pins the app as a tray icon.
/// - **From other tray icons**: reorders within the tray.
///
/// Provides visual feedback via an insertion indicator index.
pub struct TrayDropTarget {
    /// Unique target ID registered with the DnD manager.
    pub target_id: u64,
    /// Bounding rect of the tray area.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Index where a drop-insertion indicator should be drawn (between icons).
    pub insertion_index: Option<usize>,
    /// Number of icons currently visible (used for insertion calculation).
    icon_count: usize,
    /// Width of a single icon cell.
    icon_cell_width: f32,
}

impl TrayDropTarget {
    /// Create a new tray drop target.
    pub fn new(target_id: u64, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            target_id,
            x,
            y,
            width,
            height,
            insertion_index: None,
            icon_count: 0,
            icon_cell_width: 36.0,
        }
    }

    /// Update the icon count (call when icons change).
    pub fn set_icon_count(&mut self, count: usize) {
        self.icon_count = count;
    }

    /// Update the icon cell width.
    pub fn set_icon_cell_width(&mut self, width: f32) {
        self.icon_cell_width = width;
    }

    /// Register this target with a [`DragDropManager`].
    pub fn register(&self, manager: &mut DragDropManager) {
        let target = DropTarget {
            id: self.target_id,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            accepted_formats: vec![
                DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
                DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()),
            ],
            allowed_effects: vec![DropEffect::Move, DropEffect::Copy],
        };
        manager.register_target(target);
    }

    /// Calculate insertion index from a pointer X position within the tray.
    pub fn calc_insertion_index(&mut self, pointer_x: f32) {
        if self.icon_cell_width <= 0.0 || self.icon_count == 0 {
            self.insertion_index = Some(0);
            return;
        }
        let relative_x = pointer_x - self.x;
        // Round to nearest boundary between icons.
        let raw = (relative_x / self.icon_cell_width + 0.5) as usize;
        self.insertion_index = Some(raw.min(self.icon_count));
    }

    /// Clear the insertion indicator.
    pub fn clear_insertion(&mut self) {
        self.insertion_index = None;
    }

    /// Validate that drag data represents a recognized tray-compatible app.
    pub fn validate_drop(data: &DataObject) -> bool {
        // Accept either tray icon reorder data or taskbar app data.
        data.has_format(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string()))
            || data.has_format(&DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()))
    }

    /// Parse tray icon drag data into (icon_id, app_name).
    pub fn parse_tray_icon_data(data: &DataObject) -> Option<(u64, String)> {
        let bytes = data.get_data(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string()))?;
        let text = core::str::from_utf8(bytes).ok()?;
        let mut parts = text.splitn(2, ':');
        let id_str = parts.next()?;
        let name = parts.next().unwrap_or("");
        let id = id_str.parse::<u64>().ok()?;
        Some((id, name.to_string()))
    }

    /// Parse taskbar app drag data into an app_id string.
    pub fn parse_taskbar_app_data(data: &DataObject) -> Option<String> {
        let bytes = data.get_data(&DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()))?;
        let text = core::str::from_utf8(bytes).ok()?;
        if text.is_empty() {
            return None;
        }
        Some(text.to_string())
    }
}

// ============================================================================
// TrayIconSlot
// ============================================================================

/// A slot in the tray icon arrangement representing one icon's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayIconSlot {
    /// Unique identifier for the tray icon.
    pub icon_id: u64,
    /// Application name / identifier.
    pub app_name: String,
    /// Whether this icon is currently visible in the tray bar.
    pub visible: bool,
    /// Whether this icon is pinned (persists across restarts).
    pub pinned: bool,
}

// ============================================================================
// TrayIconArrangement
// ============================================================================

/// Manages the ordering, visibility, and pinning of tray icons.
///
/// Icons that don't fit in the visible area are placed into an overflow
/// set accessible via a chevron popup.
pub struct TrayIconArrangement {
    /// Ordered list of all icon slots.
    pub icons: Vec<TrayIconSlot>,
    /// Maximum number of icons visible before overflow.
    pub max_visible: usize,
}

impl TrayIconArrangement {
    /// Create a new arrangement with no icons and the default max visible count.
    pub fn new() -> Self {
        Self {
            icons: Vec::new(),
            max_visible: DEFAULT_MAX_VISIBLE,
        }
    }

    /// Create a new arrangement with a custom max visible count.
    pub fn with_max_visible(max_visible: usize) -> Self {
        Self {
            icons: Vec::new(),
            max_visible,
        }
    }

    /// Add an icon slot at the end of the arrangement.
    pub fn add_icon(&mut self, slot: TrayIconSlot) {
        self.icons.push(slot);
    }

    /// Reorder an icon from one index to another.
    ///
    /// If either index is out of bounds or they are equal, this is a no-op.
    pub fn reorder(&mut self, from_idx: usize, to_idx: usize) {
        let len = self.icons.len();
        if from_idx >= len || to_idx >= len || from_idx == to_idx {
            return;
        }
        let slot = self.icons.remove(from_idx);
        let insertion = to_idx.min(self.icons.len());
        self.icons.insert(insertion, slot);
    }

    /// Hide an icon by ID (moves it out of the visible area into overflow).
    pub fn hide_icon(&mut self, id: u64) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.icon_id == id) {
            slot.visible = false;
        }
    }

    /// Show a previously hidden icon by ID.
    pub fn show_icon(&mut self, id: u64) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.icon_id == id) {
            slot.visible = true;
        }
    }

    /// Pin an icon so it persists across restarts.
    pub fn pin_icon(&mut self, id: u64) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.icon_id == id) {
            slot.pinned = true;
        }
    }

    /// Unpin an icon (it becomes transient -- disappears when its app exits).
    pub fn unpin_icon(&mut self, id: u64) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.icon_id == id) {
            slot.pinned = false;
        }
    }

    /// Remove an icon from the arrangement entirely.
    pub fn remove_icon(&mut self, id: u64) {
        self.icons.retain(|s| s.icon_id != id);
    }

    /// Return the visible icons that fit in the tray bar (up to `max_visible`).
    pub fn visible_icons(&self) -> Vec<&TrayIconSlot> {
        self.icons
            .iter()
            .filter(|s| s.visible)
            .take(self.max_visible)
            .collect()
    }

    /// Return icons that are visible but don't fit in the tray bar (overflow).
    pub fn overflow_icons(&self) -> Vec<&TrayIconSlot> {
        self.icons
            .iter()
            .filter(|s| s.visible)
            .skip(self.max_visible)
            .collect()
    }

    /// Return all hidden icons.
    pub fn hidden_icons(&self) -> Vec<&TrayIconSlot> {
        self.icons.iter().filter(|s| !s.visible).collect()
    }

    /// Whether there are overflow icons (more visible icons than max_visible).
    pub fn has_overflow(&self) -> bool {
        self.icons.iter().filter(|s| s.visible).count() > self.max_visible
    }

    /// Find an icon by ID.
    pub fn find_icon(&self, id: u64) -> Option<&TrayIconSlot> {
        self.icons.iter().find(|s| s.icon_id == id)
    }

    /// Persist the arrangement to a serializable config.
    pub fn to_config(&self) -> TrayArrangementConfig {
        TrayArrangementConfig {
            slots: self
                .icons
                .iter()
                .map(|s| TraySlotConfig {
                    icon_id: s.icon_id,
                    app_name: s.app_name.clone(),
                    visible: s.visible,
                    pinned: s.pinned,
                })
                .collect(),
            max_visible: self.max_visible,
        }
    }

    /// Restore the arrangement from a config.
    pub fn from_config(config: &TrayArrangementConfig) -> Self {
        let icons = config
            .slots
            .iter()
            .map(|s| TrayIconSlot {
                icon_id: s.icon_id,
                app_name: s.app_name.clone(),
                visible: s.visible,
                pinned: s.pinned,
            })
            .collect();
        Self {
            icons,
            max_visible: config.max_visible,
        }
    }
}

impl Default for TrayIconArrangement {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Config structs for persistence
// ============================================================================

/// Serializable configuration for a single tray icon slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraySlotConfig {
    pub icon_id: u64,
    pub app_name: String,
    pub visible: bool,
    pub pinned: bool,
}

/// Serializable configuration for the full tray arrangement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayArrangementConfig {
    pub slots: Vec<TraySlotConfig>,
    pub max_visible: usize,
}

// ============================================================================
// StartInTrayConfig
// ============================================================================

/// Tracks which apps should start minimized to the system tray.
///
/// When an app launches, it can check this config to decide whether to
/// show its main window or hide immediately to the tray.
pub struct StartInTrayConfig {
    /// Per-app "start in tray" setting. Key is the app ID string.
    pub start_in_tray: HashMap<String, bool>,
}

impl StartInTrayConfig {
    /// Create an empty config (no apps start in tray by default).
    pub fn new() -> Self {
        Self {
            start_in_tray: HashMap::new(),
        }
    }

    /// Set whether a specific app should start minimized to the tray.
    pub fn set_start_in_tray(&mut self, app_id: &str, enabled: bool) {
        self.start_in_tray.insert(app_id.to_string(), enabled);
    }

    /// Query whether a specific app should start minimized to the tray.
    /// Returns `false` if no setting exists for the app.
    pub fn should_start_in_tray(&self, app_id: &str) -> bool {
        self.start_in_tray.get(app_id).copied().unwrap_or(false)
    }

    /// Remove the setting for an app (reverts to default = false).
    pub fn clear(&mut self, app_id: &str) {
        self.start_in_tray.remove(app_id);
    }

    /// Return all app IDs that are configured to start in tray.
    pub fn enabled_apps(&self) -> Vec<&str> {
        self.start_in_tray
            .iter()
            .filter(|(_, v)| **v)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

impl Default for StartInTrayConfig {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TrayContextMenu
// ============================================================================

/// Entries in the enhanced tray icon context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayMenuEntry {
    /// Open the application's main window.
    Open,
    /// Open the application's settings (only if available).
    Settings,
    /// Separator line between groups.
    Separator,
    /// Toggle "Pin to tray" / "Unpin from tray".
    TogglePin,
    /// Toggle "Start minimized to tray" (checkmark).
    ToggleStartInTray,
    /// Hide this icon (moves to overflow).
    HideIcon,
    /// Show hidden icons popup.
    ShowHiddenIcons,
    /// Exit the application.
    Exit,
}

/// Built context menu for a tray icon, including state-dependent labels.
#[derive(Clone, Debug)]
pub struct TrayContextMenu {
    /// The icon ID this menu is for.
    pub icon_id: u64,
    /// Ordered list of menu entries.
    pub entries: Vec<TrayMenuEntry>,
    /// Whether the icon is currently pinned (affects TogglePin label).
    pub is_pinned: bool,
    /// Whether "start in tray" is enabled (affects ToggleStartInTray checkmark).
    pub start_in_tray_enabled: bool,
    /// Whether there are hidden overflow icons (controls ShowHiddenIcons visibility).
    pub has_hidden_icons: bool,
    /// Whether the app has a settings page.
    pub has_settings: bool,
}

impl TrayContextMenu {
    /// Build a context menu for the given icon state.
    pub fn build(
        icon_id: u64,
        is_pinned: bool,
        start_in_tray_enabled: bool,
        has_hidden_icons: bool,
        has_settings: bool,
    ) -> Self {
        let mut entries = Vec::new();

        // Group 1: Primary actions.
        entries.push(TrayMenuEntry::Open);
        if has_settings {
            entries.push(TrayMenuEntry::Settings);
        }

        entries.push(TrayMenuEntry::Separator);

        // Group 2: Pin/start-in-tray toggles.
        entries.push(TrayMenuEntry::TogglePin);
        entries.push(TrayMenuEntry::ToggleStartInTray);

        entries.push(TrayMenuEntry::Separator);

        // Group 3: Visibility.
        entries.push(TrayMenuEntry::HideIcon);
        if has_hidden_icons {
            entries.push(TrayMenuEntry::ShowHiddenIcons);
        }

        entries.push(TrayMenuEntry::Separator);

        // Group 4: Exit.
        entries.push(TrayMenuEntry::Exit);

        Self {
            icon_id,
            entries,
            is_pinned,
            start_in_tray_enabled,
            has_hidden_icons,
            has_settings,
        }
    }

    /// Get the display label for a menu entry, accounting for toggle state.
    pub fn label_for(&self, entry: &TrayMenuEntry) -> &'static str {
        match entry {
            TrayMenuEntry::Open => "Open",
            TrayMenuEntry::Settings => "Settings",
            TrayMenuEntry::Separator => "",
            TrayMenuEntry::TogglePin => {
                if self.is_pinned {
                    "Unpin from tray"
                } else {
                    "Pin to tray"
                }
            }
            TrayMenuEntry::ToggleStartInTray => "Start minimized to tray",
            TrayMenuEntry::HideIcon => "Hide icon",
            TrayMenuEntry::ShowHiddenIcons => "Show hidden icons",
            TrayMenuEntry::Exit => "Exit",
        }
    }

    /// Returns the number of non-separator entries (actionable items).
    pub fn actionable_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| **e != TrayMenuEntry::Separator)
            .count()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::dnd::DragEvent;

    // Helper to create a test slot.
    fn make_slot(id: u64, name: &str, visible: bool, pinned: bool) -> TrayIconSlot {
        TrayIconSlot {
            icon_id: id,
            app_name: name.to_string(),
            visible,
            pinned,
        }
    }

    // ======================================================================
    // TrayDragSource tests
    // ======================================================================

    #[test]
    fn drag_source_threshold_not_met_is_not_drag() {
        let mut src = TrayDragSource::new();
        src.on_press(42, 100.0, 200.0);

        // Move only 2 pixels (below the 5px threshold).
        let activated = src.on_move(101.0, 201.0);
        assert!(!activated);
        assert!(!src.is_dragging());
        assert!(src.dragging_icon_id.is_none());
    }

    #[test]
    fn drag_source_threshold_exceeded_activates_drag() {
        let mut src = TrayDragSource::new();
        src.on_press(42, 100.0, 200.0);

        // Move 6 pixels horizontally (exceeds 5px threshold).
        let activated = src.on_move(106.0, 200.0);
        assert!(activated);
        assert!(src.is_dragging());
        assert_eq!(src.dragging_icon_id, Some(42));
        assert!(src.show_ghost);
    }

    #[test]
    fn drag_source_diagonal_threshold() {
        let mut src = TrayDragSource::new();
        src.on_press(10, 50.0, 50.0);

        // Move 4 pixels diag (distance = sqrt(8) ~ 2.83, below 5).
        let activated = src.on_move(52.0, 52.0);
        assert!(!activated);

        // Move 4 pixels each axis (distance = sqrt(32) ~ 5.66, above 5).
        let activated = src.on_move(54.0, 54.0);
        assert!(activated);
    }

    #[test]
    fn drag_source_cancel_resets_state() {
        let mut src = TrayDragSource::new();
        src.on_press(7, 10.0, 10.0);
        src.on_move(20.0, 10.0); // activate drag

        assert!(src.is_dragging());
        src.cancel();
        assert!(!src.is_dragging());
        assert!(!src.show_ghost);
        assert!(src.dragging_icon_id.is_none());
    }

    #[test]
    fn drag_source_release_returns_was_dragging() {
        let mut src = TrayDragSource::new();
        src.on_press(1, 0.0, 0.0);
        src.on_move(10.0, 0.0); // activate

        let was_dragging = src.on_release();
        assert!(was_dragging);
        assert!(!src.is_dragging());
    }

    #[test]
    fn drag_source_release_without_drag_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(1, 0.0, 0.0);
        // Don't move enough to exceed threshold.

        let was_dragging = src.on_release();
        assert!(!was_dragging);
    }

    #[test]
    fn drag_source_build_data_contains_format() {
        let src = TrayDragSource::new();
        let data = src.build_drag_data(42, "my_app");

        assert!(data.has_format(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string())));

        let raw = data
            .get_data(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string()))
            .expect("should have tray icon data");
        let text = core::str::from_utf8(raw).expect("should be valid utf8");
        assert_eq!(text, "42:my_app");
    }

    #[test]
    fn drag_source_on_move_after_cancel_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(1, 0.0, 0.0);
        src.cancel();

        let activated = src.on_move(100.0, 100.0);
        assert!(!activated);
    }

    #[test]
    fn drag_source_on_move_already_dragging_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(1, 0.0, 0.0);
        let first = src.on_move(10.0, 0.0);
        assert!(first);

        // Second move while already dragging should return false.
        let second = src.on_move(20.0, 0.0);
        assert!(!second);
    }

    // ======================================================================
    // TrayDropTarget tests
    // ======================================================================

    #[test]
    fn drop_target_validate_tray_icon_data() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
            b"10:volume".to_vec(),
        );
        assert!(TrayDropTarget::validate_drop(&data));
    }

    #[test]
    fn drop_target_validate_taskbar_data() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()),
            b"com.example.app".to_vec(),
        );
        assert!(TrayDropTarget::validate_drop(&data));
    }

    #[test]
    fn drop_target_validate_rejects_unknown_data() {
        let data = DataObject::with_text("random text");
        assert!(!TrayDropTarget::validate_drop(&data));
    }

    #[test]
    fn drop_target_parse_tray_icon_data() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
            b"99:NetworkManager".to_vec(),
        );
        let result = TrayDropTarget::parse_tray_icon_data(&data);
        assert_eq!(result, Some((99, "NetworkManager".to_string())));
    }

    #[test]
    fn drop_target_parse_taskbar_app_data() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()),
            b"org.ouros.settings".to_vec(),
        );
        let result = TrayDropTarget::parse_taskbar_app_data(&data);
        assert_eq!(result, Some("org.ouros.settings".to_string()));
    }

    #[test]
    fn drop_target_parse_empty_taskbar_data_returns_none() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()),
            Vec::new(),
        );
        let result = TrayDropTarget::parse_taskbar_app_data(&data);
        assert!(result.is_none());
    }

    #[test]
    fn drop_target_insertion_index_calculation() {
        let mut target = TrayDropTarget::new(1, 100.0, 500.0, 432.0, 40.0);
        target.set_icon_count(12);
        target.set_icon_cell_width(36.0);

        // Pointer at start of tray: should insert at 0.
        target.calc_insertion_index(100.0);
        assert_eq!(target.insertion_index, Some(0));

        // Pointer near the middle of the first icon: round to 0 or 1 based on rounding.
        target.calc_insertion_index(118.0); // 18px into tray, 18/36=0.5 → rounds to 1.
        assert_eq!(target.insertion_index, Some(1));

        // Pointer past all icons: clamp to icon_count.
        target.calc_insertion_index(700.0);
        assert_eq!(target.insertion_index, Some(12));
    }

    #[test]
    fn drop_target_register_with_manager() {
        let target = TrayDropTarget::new(42, 10.0, 20.0, 300.0, 40.0);
        let mut manager = DragDropManager::new();

        target.register(&mut manager);
        // After registering, we can verify by starting a drag with a compatible format.
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
            b"1:test".to_vec(),
        );
        manager.begin_drag(1, 0.0, 0.0, data, vec![DropEffect::Move]);
        // Move over the target area.
        let event = manager.update_position(15.0, 25.0);
        // Should get a DragEnter since we're over the registered target.
        assert!(matches!(event, Some(DragEvent::DragEnter { .. })));
    }

    // ======================================================================
    // TrayIconArrangement tests
    // ======================================================================

    #[test]
    fn arrangement_reorder_forward() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "a", true, false));
        arr.add_icon(make_slot(2, "b", true, false));
        arr.add_icon(make_slot(3, "c", true, false));

        arr.reorder(0, 2);

        assert_eq!(arr.icons[0].icon_id, 2);
        assert_eq!(arr.icons[1].icon_id, 3);
        assert_eq!(arr.icons[2].icon_id, 1);
    }

    #[test]
    fn arrangement_reorder_backward() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "a", true, false));
        arr.add_icon(make_slot(2, "b", true, false));
        arr.add_icon(make_slot(3, "c", true, false));

        arr.reorder(2, 0);

        assert_eq!(arr.icons[0].icon_id, 3);
        assert_eq!(arr.icons[1].icon_id, 1);
        assert_eq!(arr.icons[2].icon_id, 2);
    }

    #[test]
    fn arrangement_reorder_same_index_noop() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "a", true, false));
        arr.add_icon(make_slot(2, "b", true, false));

        arr.reorder(1, 1);

        assert_eq!(arr.icons[0].icon_id, 1);
        assert_eq!(arr.icons[1].icon_id, 2);
    }

    #[test]
    fn arrangement_reorder_out_of_bounds_noop() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "a", true, false));

        arr.reorder(0, 5);
        assert_eq!(arr.icons[0].icon_id, 1);

        arr.reorder(5, 0);
        assert_eq!(arr.icons[0].icon_id, 1);
    }

    #[test]
    fn arrangement_pin_unpin() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "app", true, false));

        assert!(!arr.icons[0].pinned);

        arr.pin_icon(1);
        assert!(arr.icons[0].pinned);

        arr.unpin_icon(1);
        assert!(!arr.icons[0].pinned);
    }

    #[test]
    fn arrangement_hide_show() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "app", true, false));

        assert!(arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 1);

        arr.hide_icon(1);
        assert!(!arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 0);
        assert_eq!(arr.hidden_icons().len(), 1);

        arr.show_icon(1);
        assert!(arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 1);
    }

    #[test]
    fn arrangement_overflow() {
        let mut arr = TrayIconArrangement::with_max_visible(3);
        for i in 0..5 {
            arr.add_icon(make_slot(i, &format!("app_{i}"), true, false));
        }

        assert_eq!(arr.visible_icons().len(), 3);
        assert_eq!(arr.overflow_icons().len(), 2);
        assert!(arr.has_overflow());
    }

    #[test]
    fn arrangement_no_overflow_when_within_limit() {
        let mut arr = TrayIconArrangement::with_max_visible(10);
        for i in 0..5 {
            arr.add_icon(make_slot(i, &format!("app_{i}"), true, false));
        }

        assert_eq!(arr.visible_icons().len(), 5);
        assert!(arr.overflow_icons().is_empty());
        assert!(!arr.has_overflow());
    }

    #[test]
    fn arrangement_hidden_icons_not_in_visible_or_overflow() {
        let mut arr = TrayIconArrangement::with_max_visible(5);
        arr.add_icon(make_slot(1, "a", true, false));
        arr.add_icon(make_slot(2, "b", false, false)); // hidden
        arr.add_icon(make_slot(3, "c", true, false));

        assert_eq!(arr.visible_icons().len(), 2);
        assert_eq!(arr.hidden_icons().len(), 1);
        assert_eq!(arr.hidden_icons()[0].icon_id, 2);
    }

    #[test]
    fn arrangement_remove_icon() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, "a", true, false));
        arr.add_icon(make_slot(2, "b", true, false));

        arr.remove_icon(1);
        assert_eq!(arr.icons.len(), 1);
        assert_eq!(arr.icons[0].icon_id, 2);
    }

    #[test]
    fn arrangement_find_icon() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(42, "special", true, true));

        let found = arr.find_icon(42);
        assert!(found.is_some());
        assert_eq!(found.unwrap().app_name, "special");

        assert!(arr.find_icon(999).is_none());
    }

    #[test]
    fn arrangement_persistence_round_trip() {
        let mut arr = TrayIconArrangement::with_max_visible(8);
        arr.add_icon(make_slot(1, "volume", true, true));
        arr.add_icon(make_slot(2, "network", true, true));
        arr.add_icon(make_slot(3, "hidden_app", false, false));

        let config = arr.to_config();
        let restored = TrayIconArrangement::from_config(&config);

        assert_eq!(restored.icons.len(), 3);
        assert_eq!(restored.max_visible, 8);
        assert_eq!(restored.icons[0].icon_id, 1);
        assert_eq!(restored.icons[0].app_name, "volume");
        assert!(restored.icons[0].visible);
        assert!(restored.icons[0].pinned);
        assert_eq!(restored.icons[2].icon_id, 3);
        assert!(!restored.icons[2].visible);
        assert!(!restored.icons[2].pinned);
    }

    // ======================================================================
    // StartInTrayConfig tests
    // ======================================================================

    #[test]
    fn start_in_tray_default_is_false() {
        let config = StartInTrayConfig::new();
        assert!(!config.should_start_in_tray("any.app"));
    }

    #[test]
    fn start_in_tray_set_and_get() {
        let mut config = StartInTrayConfig::new();
        config.set_start_in_tray("com.example.chat", true);

        assert!(config.should_start_in_tray("com.example.chat"));
        assert!(!config.should_start_in_tray("com.example.other"));
    }

    #[test]
    fn start_in_tray_disable_after_enable() {
        let mut config = StartInTrayConfig::new();
        config.set_start_in_tray("app_a", true);
        assert!(config.should_start_in_tray("app_a"));

        config.set_start_in_tray("app_a", false);
        assert!(!config.should_start_in_tray("app_a"));
    }

    #[test]
    fn start_in_tray_clear_reverts_to_default() {
        let mut config = StartInTrayConfig::new();
        config.set_start_in_tray("app_a", true);
        config.clear("app_a");

        assert!(!config.should_start_in_tray("app_a"));
    }

    #[test]
    fn start_in_tray_enabled_apps() {
        let mut config = StartInTrayConfig::new();
        config.set_start_in_tray("app_a", true);
        config.set_start_in_tray("app_b", false);
        config.set_start_in_tray("app_c", true);

        let mut enabled = config.enabled_apps();
        enabled.sort();
        assert_eq!(enabled, vec!["app_a", "app_c"]);
    }

    // ======================================================================
    // TrayContextMenu tests
    // ======================================================================

    #[test]
    fn context_menu_basic_entries() {
        let menu = TrayContextMenu::build(1, false, false, false, false);

        // Should have: Open, Sep, TogglePin, ToggleStartInTray, Sep, HideIcon, Sep, Exit
        assert!(menu.entries.contains(&TrayMenuEntry::Open));
        assert!(menu.entries.contains(&TrayMenuEntry::TogglePin));
        assert!(menu.entries.contains(&TrayMenuEntry::ToggleStartInTray));
        assert!(menu.entries.contains(&TrayMenuEntry::HideIcon));
        assert!(menu.entries.contains(&TrayMenuEntry::Exit));
    }

    #[test]
    fn context_menu_with_settings() {
        let menu = TrayContextMenu::build(1, false, false, false, true);

        assert!(menu.entries.contains(&TrayMenuEntry::Settings));
        assert!(menu.has_settings);
    }

    #[test]
    fn context_menu_without_settings() {
        let menu = TrayContextMenu::build(1, false, false, false, false);

        assert!(!menu.entries.contains(&TrayMenuEntry::Settings));
        assert!(!menu.has_settings);
    }

    #[test]
    fn context_menu_with_hidden_icons() {
        let menu = TrayContextMenu::build(1, false, false, true, false);

        assert!(menu.entries.contains(&TrayMenuEntry::ShowHiddenIcons));
        assert!(menu.has_hidden_icons);
    }

    #[test]
    fn context_menu_without_hidden_icons() {
        let menu = TrayContextMenu::build(1, false, false, false, false);

        assert!(!menu.entries.contains(&TrayMenuEntry::ShowHiddenIcons));
    }

    #[test]
    fn context_menu_pin_label_when_unpinned() {
        let menu = TrayContextMenu::build(1, false, false, false, false);
        assert_eq!(menu.label_for(&TrayMenuEntry::TogglePin), "Pin to tray");
    }

    #[test]
    fn context_menu_unpin_label_when_pinned() {
        let menu = TrayContextMenu::build(1, true, false, false, false);
        assert_eq!(menu.label_for(&TrayMenuEntry::TogglePin), "Unpin from tray");
    }

    #[test]
    fn context_menu_separator_count() {
        let menu = TrayContextMenu::build(1, false, false, true, true);
        let sep_count = menu
            .entries
            .iter()
            .filter(|e| **e == TrayMenuEntry::Separator)
            .count();
        // Should have 3 separators (between 4 groups).
        assert_eq!(sep_count, 3);
    }

    #[test]
    fn context_menu_actionable_count() {
        let menu = TrayContextMenu::build(1, false, false, false, false);
        // Open, TogglePin, ToggleStartInTray, HideIcon, Exit = 5
        assert_eq!(menu.actionable_count(), 5);

        let menu_full = TrayContextMenu::build(1, false, false, true, true);
        // Open, Settings, TogglePin, ToggleStartInTray, HideIcon, ShowHiddenIcons, Exit = 7
        assert_eq!(menu_full.actionable_count(), 7);
    }
}
