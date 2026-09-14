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
const TRAY_ICON_FORMAT: &str = "application/x-slateos-tray-icon";

/// Custom data format for taskbar app data (app ID being dragged in).
const TASKBAR_APP_FORMAT: &str = "application/x-slateos-taskbar-app";

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
    /// Which icon was pressed on.
    press_icon: Option<TrayIconKey>,
    /// Whether the drag threshold has been exceeded.
    drag_active: bool,
    /// Whether the icon should appear semi-transparent (drag in progress).
    pub show_ghost: bool,
    /// The icon currently being dragged, if any.
    pub dragging_icon: Option<TrayIconKey>,
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
            press_icon: None,
            drag_active: false,
            show_ghost: false,
            dragging_icon: None,
            cancelled: false,
        }
    }

    /// Called when the user presses on a tray icon. Records the position
    /// for threshold checking.
    pub fn on_press(&mut self, icon: TrayIconKey, x: f32, y: f32) {
        self.press_active = true;
        self.press_x = x;
        self.press_y = y;
        self.press_icon = Some(icon);
        self.drag_active = false;
        self.show_ghost = false;
        self.dragging_icon = None;
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
            self.dragging_icon = self.press_icon;
            return true;
        }
        false
    }

    /// Build a [`DataObject`] naming the icon being dragged.
    ///
    /// Carries the key and nothing else. An earlier version also carried the
    /// program's name, which is the compositor's fact and would have been a
    /// snapshot taken at press time -- stale by the drop if the program
    /// relabelled its icon mid-drag. The receiver has the list; it can look
    /// the name up.
    #[must_use]
    pub fn build_drag_data(&self, icon: TrayIconKey) -> DataObject {
        let mut data = DataObject::new();
        let payload = format!("{}:{}", icon.owner, icon.id);
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
        self.dragging_icon = None;
        self.press_active = false;
    }

    /// Called on mouse release. Resets internal state.
    /// Returns `true` if a drag was active when released (i.e., a drop happened).
    pub fn on_release(&mut self) -> bool {
        let was_dragging = self.drag_active;
        self.press_active = false;
        self.drag_active = false;
        self.show_ghost = false;
        self.dragging_icon = None;
        self.cancelled = false;
        was_dragging
    }

    /// The icon the press landed on, whether or not it became a drag.
    ///
    /// A press that stays a click still needs to name its icon -- that is the
    /// click the owning program is told about -- so this is readable before
    /// the threshold is crossed, unlike
    /// [`dragging_icon`](Self::dragging_icon).
    #[must_use]
    pub fn pressed_icon(&self) -> Option<TrayIconKey> {
        self.press_icon
    }

    /// Whether a drag is currently in progress.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag_active && !self.cancelled
    }

    /// Whether the press was just a click (released without exceeding threshold).
    #[must_use]
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

    /// Parse tray icon drag data back into the key it names.
    ///
    /// Returns `None` for anything that is not exactly two numbers. The
    /// earlier version accepted a missing second field and substituted an
    /// empty string, so a payload carrying only half an identity parsed
    /// successfully and named an icon that was not the one dragged.
    #[must_use]
    pub fn parse_tray_icon_data(data: &DataObject) -> Option<TrayIconKey> {
        let bytes = data.get_data(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string()))?;
        let text = core::str::from_utf8(bytes).ok()?;
        let (owner, id) = text.split_once(':')?;
        Some(TrayIconKey {
            owner: owner.parse().ok()?,
            id: id.parse().ok()?,
        })
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

/// Which icon, across the whole tray.
///
/// **A pair, not a number, and the pair is not optional.** `id` is the
/// owning program's own name for its icon and is unique only within that
/// program: `guiremote::tray` says in so many words that two programs may
/// both use 1, and the compositor's tests prove it. So an arrangement keyed
/// on `id` alone would treat one program's battery icon and another's volume
/// icon as the same icon -- hiding one would hide both, and dragging one
/// would move the other.
///
/// **`owner` is a process id, so this identifies an icon for as long as the
/// program runs and no longer.** It is deliberately not a persistence key:
/// the same program started tomorrow has a different pid, so an order saved
/// under these keys would restore onto nothing. See the note on
/// [`TrayIconArrangement`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrayIconKey {
    /// The process that registered the icon. Filled in by the compositor from
    /// the connection, never by the program, so it cannot be spoofed.
    pub owner: u64,
    /// The owning program's own name for this icon.
    pub id: u32,
}

impl TrayIconKey {
    /// The key of an icon the compositor reported.
    #[must_use]
    pub fn of(icon: &guiremote::tray::TrayIcon) -> Self {
        Self {
            owner: icon.owner,
            id: icon.id,
        }
    }
}

/// One icon's place in the tray, as the *shell* sees it.
///
/// Carries only what the shell decides. The glyph and the tooltip are the
/// owning program's to change at any moment and are read from the
/// compositor's list at draw time rather than copied here -- a copy would go
/// stale the first time a program relabelled its icon, and two records of one
/// fact is the defect that produced four tray models in this tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayIconSlot {
    /// Which icon this slot is for.
    pub key: TrayIconKey,
    /// Whether this icon is currently visible in the tray bar.
    pub visible: bool,
    /// Whether this icon is pinned (persists across restarts).
    pub pinned: bool,
}

impl TrayIconSlot {
    /// A newly-seen icon: shown, unpinned.
    #[must_use]
    pub fn showing(key: TrayIconKey) -> Self {
        Self {
            key,
            visible: true,
            pinned: false,
        }
    }
}

// ============================================================================
// TrayIconArrangement
// ============================================================================

/// The shell's ordering, visibility and pinning of the tray's icons.
///
/// **Why the shell holds this and not the compositor.** The compositor owns
/// *which* icons exist, because it is the thing that outlives both the
/// programs and the shell. It deliberately carries no position, so that the
/// order can be the user's -- and the user is who the shell answers to.
///
/// **This is session-scoped, and cannot yet be otherwise.** Keys are
/// `(pid, id)` pairs, so an order written to disk would restore onto pids
/// that no longer exist. Persisting it needs the compositor to report a
/// *stable* name for the program behind a connection, which it does not have:
/// `ClientLink` carries `client_pid` and nothing else. Until it does, saving
/// this would produce a settings file that silently does nothing, which is
/// worse than not saving it.
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

    /// Fold the compositor's list into this arrangement, and say whether the
    /// result differs from what was there.
    ///
    /// **Keeping an icon's place is the whole point.** The compositor sends
    /// the entire list on every change, so a program that merely relabelled
    /// its icon arrives looking exactly like a program that just registered
    /// one. Matching on key first means a relabel does not move anything;
    /// only registering and departing do.
    ///
    /// New icons append in the order the compositor reported, departed icons
    /// drop out, and every icon the user placed stays where the user put it.
    pub fn sync(&mut self, icons: &[guiremote::tray::TrayIcon]) -> bool {
        let before = self.icons.clone();
        // Departed first, so that a program which exited and re-registered
        // within one frame is treated as new rather than silently keeping a
        // slot whose `pinned`/`visible` the user set for the old instance.
        let live: Vec<TrayIconKey> = icons.iter().map(TrayIconKey::of).collect();
        self.icons.retain(|slot| live.contains(&slot.key));
        for key in live {
            if !self.icons.iter().any(|slot| slot.key == key) {
                self.icons.push(TrayIconSlot::showing(key));
            }
        }
        self.icons != before
    }

    /// Move `key` to a drop boundary, and say whether anything moved.
    ///
    /// **A boundary is not a slot, and conflating them is an off-by-one that
    /// only shows up dragging rightwards.** With three icons there are four
    /// places a drop can land -- before the first, between each pair, after
    /// the last -- so a boundary runs `0..=len` while a slot runs `0..len`.
    /// Dropping at boundary `b` means "end up with `b` icons to your left",
    /// and once the dragged icon is lifted out, every boundary to its right
    /// has shifted one place closer. That subtraction lives here rather than
    /// in the caller because it is the kind of thing that is right in the test
    /// that was written for it and wrong in the second caller.
    ///
    /// Dropping an icon immediately either side of where it already sits is
    /// not a move, and answers `false` -- so a drag that wanders and comes
    /// home does not mark the tray changed.
    pub fn move_to_boundary(&mut self, key: TrayIconKey, boundary: usize) -> bool {
        let Some(from) = self.icons.iter().position(|slot| slot.key == key) else {
            return false;
        };
        let target = if boundary > from {
            // Lifting the icon out closes the gap it left, so every boundary
            // beyond it names a slot one lower than it did.
            boundary.saturating_sub(1)
        } else {
            boundary
        };
        if target == from || target >= self.icons.len() {
            return false;
        }
        self.reorder(from, target);
        true
    }

    /// The icons in the shell's order, whatever their visibility.
    ///
    /// This, not the compositor's list, is what the tray draws.
    #[must_use]
    pub fn ordered_keys(&self) -> Vec<TrayIconKey> {
        self.icons.iter().map(|slot| slot.key).collect()
    }

    /// Reorder an icon from one slot index to another.
    ///
    /// `to_idx` is where the icon ends up, not where it is inserted before --
    /// see [`move_to_boundary`](Self::move_to_boundary), which converts.
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
    pub fn hide_icon(&mut self, key: TrayIconKey) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.key == key) {
            slot.visible = false;
        }
    }

    /// Show a previously hidden icon by ID.
    pub fn show_icon(&mut self, key: TrayIconKey) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.key == key) {
            slot.visible = true;
        }
    }

    /// Pin an icon so it persists across restarts.
    pub fn pin_icon(&mut self, key: TrayIconKey) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.key == key) {
            slot.pinned = true;
        }
    }

    /// Unpin an icon (it becomes transient -- disappears when its app exits).
    pub fn unpin_icon(&mut self, key: TrayIconKey) {
        if let Some(slot) = self.icons.iter_mut().find(|s| s.key == key) {
            slot.pinned = false;
        }
    }

    /// Remove an icon from the arrangement entirely.
    pub fn remove_icon(&mut self, key: TrayIconKey) {
        self.icons.retain(|s| s.key != key);
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

    /// Find an icon by key.
    pub fn find_icon(&self, key: TrayIconKey) -> Option<&TrayIconSlot> {
        self.icons.iter().find(|s| s.key == key)
    }
}

impl Default for TrayIconArrangement {
    fn default() -> Self {
        Self::new()
    }
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
    /// Which icon this menu is for.
    pub icon: TrayIconKey,
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
    #[must_use]
    pub fn build(
        icon: TrayIconKey,
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
            icon,
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

    use super::*;
    use guitk::dnd::DragEvent;

    /// One owner for the tests that are about order rather than ownership.
    ///
    /// The owner is the compositor's to fill in, and most of what is tested
    /// here is what the shell does with a run of icons. The tests that are
    /// specifically about two programs say so by using `OTHER`.
    const OWNER: u64 = 900;
    const OTHER: u64 = 901;

    fn key(id: u32) -> TrayIconKey {
        TrayIconKey { owner: OWNER, id }
    }

    // Helper to create a test slot.
    fn make_slot(id: u32, visible: bool, pinned: bool) -> TrayIconSlot {
        TrayIconSlot {
            key: key(id),
            visible,
            pinned,
        }
    }

    /// An icon as the compositor would report it.
    fn reported(owner: u64, id: u32, glyph: &str) -> guiremote::tray::TrayIcon {
        guiremote::tray::TrayIcon {
            owner,
            id,
            glyph: glyph.to_string(),
            tooltip: String::new(),
        }
    }

    // ======================================================================
    // TrayDragSource tests
    // ======================================================================

    #[test]
    fn drag_source_threshold_not_met_is_not_drag() {
        let mut src = TrayDragSource::new();
        src.on_press(key(42), 100.0, 200.0);

        // Move only 2 pixels (below the 5px threshold).
        let activated = src.on_move(101.0, 201.0);
        assert!(!activated);
        assert!(!src.is_dragging());
        assert!(src.dragging_icon.is_none());
    }

    #[test]
    fn drag_source_threshold_exceeded_activates_drag() {
        let mut src = TrayDragSource::new();
        src.on_press(key(42), 100.0, 200.0);

        // Move 6 pixels horizontally (exceeds 5px threshold).
        let activated = src.on_move(106.0, 200.0);
        assert!(activated);
        assert!(src.is_dragging());
        assert_eq!(src.dragging_icon, Some(key(42)));
        assert!(src.show_ghost);
    }

    #[test]
    fn drag_source_diagonal_threshold() {
        let mut src = TrayDragSource::new();
        src.on_press(key(10), 50.0, 50.0);

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
        src.on_press(key(7), 10.0, 10.0);
        src.on_move(20.0, 10.0); // activate drag

        assert!(src.is_dragging());
        src.cancel();
        assert!(!src.is_dragging());
        assert!(!src.show_ghost);
        assert!(src.dragging_icon.is_none());
    }

    #[test]
    fn drag_source_release_returns_was_dragging() {
        let mut src = TrayDragSource::new();
        src.on_press(key(1), 0.0, 0.0);
        src.on_move(10.0, 0.0); // activate

        let was_dragging = src.on_release();
        assert!(was_dragging);
        assert!(!src.is_dragging());
    }

    #[test]
    fn drag_source_release_without_drag_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(key(1), 0.0, 0.0);
        // Don't move enough to exceed threshold.

        let was_dragging = src.on_release();
        assert!(!was_dragging);
    }

    #[test]
    fn drag_source_build_data_contains_format() {
        let src = TrayDragSource::new();
        let data = src.build_drag_data(key(42));

        assert!(data.has_format(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string())));

        let raw = data
            .get_data(&DataFormat::Custom(TRAY_ICON_FORMAT.to_string()))
            .expect("should have tray icon data");
        let text = core::str::from_utf8(raw).expect("should be valid utf8");
        assert_eq!(text, "900:42", "both halves of the identity, owner first");
    }

    #[test]
    fn drag_source_on_move_after_cancel_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(key(1), 0.0, 0.0);
        src.cancel();

        let activated = src.on_move(100.0, 100.0);
        assert!(!activated);
    }

    #[test]
    fn drag_source_on_move_already_dragging_returns_false() {
        let mut src = TrayDragSource::new();
        src.on_press(key(1), 0.0, 0.0);
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
            b"99:4".to_vec(),
        );
        let result = TrayDropTarget::parse_tray_icon_data(&data);
        assert_eq!(result, Some(TrayIconKey { owner: 99, id: 4 }));
    }

    #[test]
    fn drop_target_refuses_half_an_identity() {
        // The shape the old parser accepted: one number, no separator. It
        // returned Some with an empty name, so a drop carrying half an
        // identity named an icon that was not the one dragged.
        for payload in [&b"99"[..], b"99:", b":4", b"99:4:5", b"a:4", b"99:b"] {
            let mut data = DataObject::new();
            data.set_data(
                DataFormat::Custom(TRAY_ICON_FORMAT.to_string()),
                payload.to_vec(),
            );
            assert_eq!(
                TrayDropTarget::parse_tray_icon_data(&data),
                None,
                "{} should not parse",
                String::from_utf8_lossy(payload),
            );
        }
    }

    #[test]
    fn drop_target_parse_taskbar_app_data() {
        let mut data = DataObject::new();
        data.set_data(
            DataFormat::Custom(TASKBAR_APP_FORMAT.to_string()),
            b"org.slateos.settings".to_vec(),
        );
        let result = TrayDropTarget::parse_taskbar_app_data(&data);
        assert_eq!(result, Some("org.slateos.settings".to_string()));
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
        arr.add_icon(make_slot(1, true, false));
        arr.add_icon(make_slot(2, true, false));
        arr.add_icon(make_slot(3, true, false));

        arr.reorder(0, 2);

        assert_eq!(arr.icons[0].key.id, 2);
        assert_eq!(arr.icons[1].key.id, 3);
        assert_eq!(arr.icons[2].key.id, 1);
    }

    #[test]
    fn arrangement_reorder_backward() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));
        arr.add_icon(make_slot(2, true, false));
        arr.add_icon(make_slot(3, true, false));

        arr.reorder(2, 0);

        assert_eq!(arr.icons[0].key.id, 3);
        assert_eq!(arr.icons[1].key.id, 1);
        assert_eq!(arr.icons[2].key.id, 2);
    }

    #[test]
    fn arrangement_reorder_same_index_noop() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));
        arr.add_icon(make_slot(2, true, false));

        arr.reorder(1, 1);

        assert_eq!(arr.icons[0].key.id, 1);
        assert_eq!(arr.icons[1].key.id, 2);
    }

    #[test]
    fn arrangement_reorder_out_of_bounds_noop() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));

        arr.reorder(0, 5);
        assert_eq!(arr.icons[0].key.id, 1);

        arr.reorder(5, 0);
        assert_eq!(arr.icons[0].key.id, 1);
    }

    #[test]
    fn arrangement_pin_unpin() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));

        assert!(!arr.icons[0].pinned);

        arr.pin_icon(key(1));
        assert!(arr.icons[0].pinned);

        arr.unpin_icon(key(1));
        assert!(!arr.icons[0].pinned);
    }

    #[test]
    fn arrangement_hide_show() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));

        assert!(arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 1);

        arr.hide_icon(key(1));
        assert!(!arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 0);
        assert_eq!(arr.hidden_icons().len(), 1);

        arr.show_icon(key(1));
        assert!(arr.icons[0].visible);
        assert_eq!(arr.visible_icons().len(), 1);
    }

    #[test]
    fn arrangement_overflow() {
        let mut arr = TrayIconArrangement::with_max_visible(3);
        for i in 0..5 {
            arr.add_icon(make_slot(i, true, false));
        }

        assert_eq!(arr.visible_icons().len(), 3);
        assert_eq!(arr.overflow_icons().len(), 2);
        assert!(arr.has_overflow());
    }

    #[test]
    fn arrangement_no_overflow_when_within_limit() {
        let mut arr = TrayIconArrangement::with_max_visible(10);
        for i in 0..5 {
            arr.add_icon(make_slot(i, true, false));
        }

        assert_eq!(arr.visible_icons().len(), 5);
        assert!(arr.overflow_icons().is_empty());
        assert!(!arr.has_overflow());
    }

    #[test]
    fn arrangement_hidden_icons_not_in_visible_or_overflow() {
        let mut arr = TrayIconArrangement::with_max_visible(5);
        arr.add_icon(make_slot(1, true, false));
        arr.add_icon(make_slot(2, false, false)); // hidden
        arr.add_icon(make_slot(3, true, false));

        assert_eq!(arr.visible_icons().len(), 2);
        assert_eq!(arr.hidden_icons().len(), 1);
        assert_eq!(arr.hidden_icons()[0].key.id, 2);
    }

    #[test]
    fn arrangement_remove_icon() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));
        arr.add_icon(make_slot(2, true, false));

        arr.remove_icon(key(1));
        assert_eq!(arr.icons.len(), 1);
        assert_eq!(arr.icons[0].key.id, 2);
    }

    #[test]
    fn arrangement_find_icon() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(42, true, true));

        let found = arr.find_icon(key(42));
        assert!(found.is_some());
        assert!(found.unwrap().pinned);

        assert!(arr.find_icon(key(999)).is_none());

        // The half that a single number could not express: same id, other
        // program. An arrangement keyed on `id` alone would find this.
        assert!(
            arr.find_icon(TrayIconKey {
                owner: OTHER,
                id: 42
            })
            .is_none(),
            "another program's icon 42 is not this one"
        );
    }

    // ======================================================================
    // sync -- folding the compositor's list into the shell's order
    // ======================================================================

    #[test]
    fn sync_adopts_a_list_it_has_never_seen() {
        let mut arr = TrayIconArrangement::new();
        assert!(arr.sync(&[reported(OWNER, 1, "A"), reported(OWNER, 2, "B")]));
        assert_eq!(arr.ordered_keys(), vec![key(1), key(2)]);
        assert!(arr.icons.iter().all(|s| s.visible && !s.pinned));
    }

    #[test]
    fn sync_of_an_unchanged_list_changes_nothing() {
        let mut arr = TrayIconArrangement::new();
        let list = [reported(OWNER, 1, "A"), reported(OWNER, 2, "B")];
        assert!(arr.sync(&list));
        assert!(!arr.sync(&list), "the same list again is not a change");
    }

    #[test]
    fn a_relabelled_icon_does_not_move() {
        // The case the whole fold exists for. The compositor sends the entire
        // list whenever any of it changes, so a program that only swapped its
        // glyph arrives looking exactly like one that just registered.
        let mut arr = TrayIconArrangement::new();
        arr.sync(&[reported(OWNER, 1, "A"), reported(OWNER, 2, "B")]);
        arr.reorder(0, 1);
        assert_eq!(arr.ordered_keys(), vec![key(2), key(1)]);

        let moved = arr.sync(&[reported(OWNER, 1, "!"), reported(OWNER, 2, "B")]);

        assert!(!moved, "a new glyph is not a change to the order");
        assert_eq!(
            arr.ordered_keys(),
            vec![key(2), key(1)],
            "the user's order survived the program relabelling its icon"
        );
    }

    #[test]
    fn a_new_icon_appends_rather_than_resetting_the_order() {
        let mut arr = TrayIconArrangement::new();
        arr.sync(&[reported(OWNER, 1, "A"), reported(OWNER, 2, "B")]);
        arr.reorder(0, 1);

        assert!(arr.sync(&[
            reported(OWNER, 1, "A"),
            reported(OWNER, 2, "B"),
            reported(OTHER, 1, "C"),
        ]));

        assert_eq!(
            arr.ordered_keys(),
            vec![
                key(2),
                key(1),
                TrayIconKey {
                    owner: OTHER,
                    id: 1
                }
            ],
            "the new icon went to the end, and did not displace the user's order"
        );
    }

    #[test]
    fn a_departed_program_leaves_the_order() {
        let mut arr = TrayIconArrangement::new();
        arr.sync(&[reported(OWNER, 1, "A"), reported(OTHER, 1, "B")]);

        assert!(arr.sync(&[reported(OTHER, 1, "B")]));

        assert_eq!(
            arr.ordered_keys(),
            vec![TrayIconKey {
                owner: OTHER,
                id: 1
            }],
            "and the other program's icon 1 was not mistaken for the departed one"
        );
    }

    #[test]
    fn two_programs_using_id_one_are_two_icons() {
        // `guiremote::tray` promises this and the compositor's tests prove it;
        // this is the shell keeping the same promise. An arrangement keyed on
        // `id` alone would hold one slot here, and hiding one program's icon
        // would hide the other's.
        let mut arr = TrayIconArrangement::new();
        arr.sync(&[reported(OWNER, 1, "A"), reported(OTHER, 1, "B")]);
        assert_eq!(arr.icons.len(), 2);

        arr.hide_icon(key(1));

        assert_eq!(arr.visible_icons().len(), 1);
        assert_eq!(arr.visible_icons()[0].key.owner, OTHER);
    }

    #[test]
    fn a_program_that_exits_and_returns_starts_fresh() {
        // Same pid is not reused within a session, so a slot whose visibility
        // the user set for the departed instance must not be inherited by
        // whoever comes next.
        let mut arr = TrayIconArrangement::new();
        arr.sync(&[reported(OWNER, 1, "A")]);
        arr.hide_icon(key(1));
        assert_eq!(arr.visible_icons().len(), 0);

        arr.sync(&[]);
        arr.sync(&[reported(OWNER, 1, "A")]);

        assert_eq!(
            arr.visible_icons().len(),
            1,
            "the returning icon is shown, not still hidden"
        );
    }

    // ======================================================================
    // move_to_boundary -- where a dropped icon lands
    // ======================================================================

    /// Every boundary, for every icon, against a hand-written expectation.
    ///
    /// Written as a table because the interesting cases are the ones nobody
    /// thinks to write by hand: dropping just left of yourself and just right
    /// of yourself are both no-moves, and they are no-moves for different
    /// reasons.
    #[test]
    fn a_drop_lands_where_the_boundary_says() {
        // (icon moved, boundary dropped at, resulting order)
        let cases: &[(u32, usize, [u32; 3])] = &[
            (1, 0, [1, 2, 3]), // already leftmost
            (1, 1, [1, 2, 3]), // just right of itself is not a move
            (1, 2, [2, 1, 3]),
            (1, 3, [2, 3, 1]),
            (2, 0, [2, 1, 3]),
            (2, 1, [1, 2, 3]), // just left of itself
            (2, 2, [1, 2, 3]), // just right of itself
            (2, 3, [1, 3, 2]),
            (3, 0, [3, 1, 2]),
            (3, 1, [1, 3, 2]),
            (3, 2, [1, 2, 3]), // just left of itself
            (3, 3, [1, 2, 3]), // already rightmost
        ];
        for &(moved, boundary, expected) in cases {
            let mut arr = TrayIconArrangement::new();
            for id in 1..=3 {
                arr.add_icon(make_slot(id, true, false));
            }
            let changed = arr.move_to_boundary(key(moved), boundary);
            let got: Vec<u32> = arr.ordered_keys().iter().map(|k| k.id).collect();
            assert_eq!(
                got,
                expected.to_vec(),
                "moving {moved} to boundary {boundary}"
            );
            assert_eq!(
                changed,
                got != vec![1, 2, 3],
                "moving {moved} to boundary {boundary} reported the wrong answer"
            );
        }
    }

    #[test]
    fn a_drop_naming_no_icon_moves_nothing() {
        let mut arr = TrayIconArrangement::new();
        arr.add_icon(make_slot(1, true, false));
        // The program exited between the press and the release.
        assert!(!arr.move_to_boundary(key(9), 0));
        assert_eq!(arr.ordered_keys(), vec![key(1)]);
    }

    #[test]
    fn a_drop_past_the_end_of_the_tray_lands_at_the_end() {
        let mut arr = TrayIconArrangement::new();
        for id in 1..=3 {
            arr.add_icon(make_slot(id, true, false));
        }
        // The pointer was well right of the last icon; the caller clamps to
        // `len`, and anything beyond must not panic or silently do nothing.
        assert!(arr.move_to_boundary(key(1), 3));
        assert_eq!(
            arr.ordered_keys().iter().map(|k| k.id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
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
        enabled.sort_unstable();
        assert_eq!(enabled, vec!["app_a", "app_c"]);
    }

    // ======================================================================
    // TrayContextMenu tests
    // ======================================================================

    #[test]
    fn context_menu_basic_entries() {
        let menu = TrayContextMenu::build(key(1), false, false, false, false);

        // Should have: Open, Sep, TogglePin, ToggleStartInTray, Sep, HideIcon, Sep, Exit
        assert!(menu.entries.contains(&TrayMenuEntry::Open));
        assert!(menu.entries.contains(&TrayMenuEntry::TogglePin));
        assert!(menu.entries.contains(&TrayMenuEntry::ToggleStartInTray));
        assert!(menu.entries.contains(&TrayMenuEntry::HideIcon));
        assert!(menu.entries.contains(&TrayMenuEntry::Exit));
    }

    #[test]
    fn context_menu_with_settings() {
        let menu = TrayContextMenu::build(key(1), false, false, false, true);

        assert!(menu.entries.contains(&TrayMenuEntry::Settings));
        assert!(menu.has_settings);
    }

    #[test]
    fn context_menu_without_settings() {
        let menu = TrayContextMenu::build(key(1), false, false, false, false);

        assert!(!menu.entries.contains(&TrayMenuEntry::Settings));
        assert!(!menu.has_settings);
    }

    #[test]
    fn context_menu_with_hidden_icons() {
        let menu = TrayContextMenu::build(key(1), false, false, true, false);

        assert!(menu.entries.contains(&TrayMenuEntry::ShowHiddenIcons));
        assert!(menu.has_hidden_icons);
    }

    #[test]
    fn context_menu_without_hidden_icons() {
        let menu = TrayContextMenu::build(key(1), false, false, false, false);

        assert!(!menu.entries.contains(&TrayMenuEntry::ShowHiddenIcons));
    }

    #[test]
    fn context_menu_pin_label_when_unpinned() {
        let menu = TrayContextMenu::build(key(1), false, false, false, false);
        assert_eq!(menu.label_for(&TrayMenuEntry::TogglePin), "Pin to tray");
    }

    #[test]
    fn context_menu_unpin_label_when_pinned() {
        let menu = TrayContextMenu::build(key(1), true, false, false, false);
        assert_eq!(menu.label_for(&TrayMenuEntry::TogglePin), "Unpin from tray");
    }

    #[test]
    fn context_menu_separator_count() {
        let menu = TrayContextMenu::build(key(1), false, false, true, true);
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
        let menu = TrayContextMenu::build(key(1), false, false, false, false);
        // Open, TogglePin, ToggleStartInTray, HideIcon, Exit = 5
        assert_eq!(menu.actionable_count(), 5);

        let menu_full = TrayContextMenu::build(key(1), false, false, true, true);
        // Open, Settings, TogglePin, ToggleStartInTray, HideIcon, ShowHiddenIcons, Exit = 7
        assert_eq!(menu_full.actionable_count(), 7);
    }
}
