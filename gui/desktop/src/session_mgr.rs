//! Session and workspace management.
//!
//! Saves and restores window layouts (positions, sizes, states) so the user
//! can quickly switch between work contexts (e.g., "Development" with editor
//! + terminal + browser, "Communication" with email + chat, etc.).
//!
//! Also handles session persistence across logouts/reboots — remembering
//! which apps were open and where they were placed.

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
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Types
// ============================================================================

/// Unique workspace ID.
pub type WorkspaceId = u64;

/// A saved window position within a workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedWindowState {
    /// Application identifier (executable name or app ID).
    pub app_id: String,
    /// Window title (for matching).
    pub title_hint: String,
    /// X position.
    pub x: i32,
    /// Y position.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// Window state.
    pub state: SavedWindowMode,
    /// Virtual desktop index.
    pub desktop: u32,
    /// Whether the window was focused.
    pub focused: bool,
    /// Z-order index (relative).
    pub z_index: u32,
}

/// Saved window mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedWindowMode {
    Normal,
    Maximized,
    Minimized,
    Fullscreen,
}

/// A named workspace (saved window layout).
#[derive(Clone, Debug)]
pub struct Workspace {
    /// Unique ID.
    pub id: WorkspaceId,
    /// User-visible name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// Icon character.
    pub icon: String,
    /// Saved window states.
    pub windows: Vec<SavedWindowState>,
    /// When this workspace was created (ms since epoch).
    pub created_at: u64,
    /// When this workspace was last applied (ms since epoch).
    pub last_used: u64,
    /// Whether to auto-launch apps that aren't running.
    pub auto_launch: bool,
    /// Keyboard shortcut to activate (e.g., "Super+1").
    pub shortcut: Option<String>,
    /// Associated virtual desktop (if workspace is tied to a specific desktop).
    pub pinned_desktop: Option<u32>,
    /// Color tag for visual identification.
    pub color: Color,
}

impl Workspace {
    pub fn new(id: WorkspaceId, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            description: String::new(),
            icon: "\u{1F4CB}".to_string(),
            windows: Vec::new(),
            created_at: 0,
            last_used: 0,
            auto_launch: false,
            shortcut: None,
            pinned_desktop: None,
            color: BLUE,
        }
    }

    /// Add a window state.
    pub fn add_window(&mut self, state: SavedWindowState) {
        self.windows.push(state);
    }

    /// Remove windows by app_id.
    pub fn remove_app_windows(&mut self, app_id: &str) {
        self.windows.retain(|w| w.app_id != app_id);
    }

    /// Number of windows in this workspace.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Unique app IDs in this workspace.
    pub fn app_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.windows.iter().map(|w| w.app_id.as_str()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Whether this workspace contains a window for the given app.
    pub fn has_app(&self, app_id: &str) -> bool {
        self.windows.iter().any(|w| w.app_id == app_id)
    }
}

// ============================================================================
// Session restore data
// ============================================================================

/// Data saved for session restore across reboot/logout.
#[derive(Clone, Debug)]
pub struct SessionState {
    /// Windows that were open.
    pub windows: Vec<SavedWindowState>,
    /// Active virtual desktop.
    pub active_desktop: u32,
    /// Timestamp when session was saved.
    pub saved_at: u64,
    /// Whether to restore this session on next login.
    pub restore_on_login: bool,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            active_desktop: 0,
            saved_at: 0,
            restore_on_login: true,
        }
    }

    pub fn add_window(&mut self, state: SavedWindowState) {
        self.windows.push(state);
    }

    pub fn clear(&mut self) {
        self.windows.clear();
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Session & workspace manager
// ============================================================================

/// Manages saved workspaces and session state.
pub struct SessionManager {
    /// Saved workspaces.
    workspaces: Vec<Workspace>,
    /// Current session state (for session restore).
    pub session: SessionState,
    /// Next workspace ID.
    next_id: WorkspaceId,
    /// Maximum number of workspaces.
    pub max_workspaces: usize,
    /// Whether session restore is enabled globally.
    pub session_restore_enabled: bool,
    /// Currently active workspace (if any).
    pub active_workspace: Option<WorkspaceId>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            session: SessionState::new(),
            next_id: 1,
            max_workspaces: 20,
            session_restore_enabled: true,
            active_workspace: None,
        }
    }

    /// Create a new empty workspace.
    pub fn create_workspace(&mut self, name: &str) -> Option<WorkspaceId> {
        if self.workspaces.len() >= self.max_workspaces {
            return None;
        }
        // Check for duplicate name.
        if self.workspaces.iter().any(|w| w.name == name) {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.workspaces.push(Workspace::new(id, name));
        Some(id)
    }

    /// Create a workspace from the current session state (snapshot).
    pub fn snapshot_to_workspace(&mut self, name: &str, now_ms: u64) -> Option<WorkspaceId> {
        let id = self.create_workspace(name)?;
        let session_windows = self.session.windows.clone();
        if let Some(ws) = self.get_mut(id) {
            ws.windows = session_windows;
            ws.created_at = now_ms;
        }
        Some(id)
    }

    /// Delete a workspace.
    pub fn delete_workspace(&mut self, id: WorkspaceId) -> bool {
        let len_before = self.workspaces.len();
        self.workspaces.retain(|w| w.id != id);
        if self.active_workspace == Some(id) {
            self.active_workspace = None;
        }
        self.workspaces.len() < len_before
    }

    /// Rename a workspace.
    pub fn rename_workspace(&mut self, id: WorkspaceId, new_name: &str) -> bool {
        // Check for duplicate name.
        if self.workspaces.iter().any(|w| w.name == new_name && w.id != id) {
            return false;
        }
        if let Some(ws) = self.get_mut(id) {
            ws.name = new_name.to_string();
            true
        } else {
            false
        }
    }

    /// Get a workspace by ID.
    pub fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// Get a mutable workspace by ID.
    pub fn get_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id)
    }

    /// Get a workspace by name.
    pub fn find_by_name(&self, name: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// List all workspaces.
    pub fn all_workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Count of workspaces.
    pub fn count(&self) -> usize {
        self.workspaces.len()
    }

    /// Apply a workspace (set it as active and return the window states to restore).
    pub fn apply_workspace(&mut self, id: WorkspaceId, now_ms: u64) -> Option<Vec<SavedWindowState>> {
        let ws = self.workspaces.iter_mut().find(|w| w.id == id)?;
        ws.last_used = now_ms;
        self.active_workspace = Some(id);
        Some(ws.windows.clone())
    }

    /// Update a workspace with the current session state.
    pub fn update_workspace_from_session(&mut self, id: WorkspaceId) -> bool {
        let session_windows = self.session.windows.clone();
        if let Some(ws) = self.get_mut(id) {
            ws.windows = session_windows;
            true
        } else {
            false
        }
    }

    /// Duplicate a workspace.
    pub fn duplicate_workspace(&mut self, id: WorkspaceId) -> Option<WorkspaceId> {
        let source = self.get(id)?.clone();
        let new_name = format!("{} (copy)", source.name);
        let new_id = self.create_workspace(&new_name)?;
        if let Some(new_ws) = self.get_mut(new_id) {
            new_ws.windows = source.windows;
            new_ws.icon = source.icon;
            new_ws.description = source.description;
            new_ws.auto_launch = source.auto_launch;
            new_ws.color = source.color;
        }
        Some(new_id)
    }

    /// Save current session state (called periodically or at logout).
    pub fn save_session(&mut self, windows: Vec<SavedWindowState>, active_desktop: u32, now_ms: u64) {
        self.session.windows = windows;
        self.session.active_desktop = active_desktop;
        self.session.saved_at = now_ms;
    }

    /// Get session state for restore (at login).
    pub fn restore_session(&self) -> Option<&SessionState> {
        if self.session_restore_enabled && self.session.restore_on_login && !self.session.windows.is_empty() {
            Some(&self.session)
        } else {
            None
        }
    }

    /// Find workspaces by shortcut key.
    pub fn find_by_shortcut(&self, shortcut: &str) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|w| w.shortcut.as_deref() == Some(shortcut))
    }

    /// Sort workspaces by last used (most recent first).
    pub fn sort_by_recent(&mut self) {
        self.workspaces.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    }

    /// Sort workspaces by name.
    pub fn sort_by_name(&mut self) {
        self.workspaces.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Serialize all workspaces to a text format (for config persistence).
    pub fn export_workspaces(&self) -> String {
        let mut output = String::new();
        for ws in &self.workspaces {
            output.push_str(&format!(
                "workspace:{}:{}:{}:{}\n",
                ws.id, ws.name, ws.icon, ws.auto_launch
            ));
            for win in &ws.windows {
                output.push_str(&format!(
                    "  window:{}:{}:{}:{}:{}:{}:{:?}:{}\n",
                    win.app_id, win.title_hint, win.x, win.y, win.width, win.height, win.state, win.desktop
                ));
            }
        }
        output
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Workspace picker / switcher UI
// ============================================================================

/// UI for the workspace picker (Super+W or similar).
pub struct WorkspacePicker {
    /// Whether the picker is visible.
    pub visible: bool,
    /// Selected index.
    pub selected_index: usize,
    /// Search text.
    pub search_text: String,
    /// Screen dimensions.
    pub screen_width: f32,
    pub screen_height: f32,
}

impl WorkspacePicker {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            visible: false,
            selected_index: 0,
            search_text: String::new(),
            screen_width,
            screen_height,
        }
    }

    /// Toggle visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.selected_index = 0;
            self.search_text.clear();
        }
    }

    /// Navigate selection.
    pub fn select_next(&mut self, count: usize) {
        if count > 0 {
            self.selected_index = (self.selected_index + 1) % count;
        }
    }

    pub fn select_prev(&mut self, count: usize) {
        if count > 0 {
            self.selected_index = if self.selected_index == 0 {
                count - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Render the picker overlay.
    pub fn render(&self, workspaces: &[Workspace]) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let mut commands = Vec::new();
        let picker_w = 400.0;
        let item_h = 56.0;
        let padding = 16.0;
        let header_h = 48.0;

        let filtered: Vec<&Workspace> = if self.search_text.is_empty() {
            workspaces.iter().collect()
        } else {
            let lower = self.search_text.to_lowercase();
            workspaces
                .iter()
                .filter(|w| w.name.to_lowercase().contains(&lower))
                .collect()
        };

        let picker_h = header_h + filtered.len() as f32 * item_h + padding * 2.0;
        let picker_h = picker_h.min(self.screen_height - 100.0);
        let px = (self.screen_width - picker_w) / 2.0;
        let py = (self.screen_height - picker_h) / 2.0;

        // Shadow.
        commands.push(RenderCommand::BoxShadow {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            offset_x: 0.0,
            offset_y: 8.0,
            blur: 24.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 120),
            corner_radii: CornerRadii::all(12.0),
        });

        // Background.
        commands.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: MANTLE,
            corner_radii: CornerRadii::all(12.0),
        });

        // Border.
        commands.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title / search.
        commands.push(RenderCommand::Text {
            x: px + padding,
            y: py + 14.0,
            text: if self.search_text.is_empty() {
                "Switch Workspace".to_string()
            } else {
                self.search_text.clone()
            },
            font_size: 16.0,
            color: if self.search_text.is_empty() { TEXT } else { BLUE },
            font_weight: FontWeightHint::Bold,
            max_width: Some(picker_w - padding * 2.0),
        });

        // Workspace entries.
        let mut cy = py + header_h;
        for (i, ws) in filtered.iter().enumerate() {
            let selected = i == self.selected_index;

            if selected {
                commands.push(RenderCommand::FillRect {
                    x: px + 4.0,
                    y: cy,
                    width: picker_w - 8.0,
                    height: item_h,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(8.0),
                });
            }

            // Color tag.
            commands.push(RenderCommand::FillRect {
                x: px + padding,
                y: cy + 16.0,
                width: 4.0,
                height: 24.0,
                color: ws.color,
                corner_radii: CornerRadii::all(2.0),
            });

            // Icon.
            commands.push(RenderCommand::Text {
                x: px + padding + 14.0,
                y: cy + 12.0,
                text: ws.icon.clone(),
                font_size: 20.0,
                color: if selected { TEXT } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Name.
            commands.push(RenderCommand::Text {
                x: px + padding + 44.0,
                y: cy + 10.0,
                text: ws.name.clone(),
                font_size: 14.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(picker_w - padding * 2.0 - 100.0),
            });

            // Window count and apps.
            let info = format!("{} windows", ws.window_count());
            commands.push(RenderCommand::Text {
                x: px + padding + 44.0,
                y: cy + 32.0,
                text: info,
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Shortcut hint.
            if let Some(sc) = &ws.shortcut {
                commands.push(RenderCommand::Text {
                    x: px + picker_w - padding - 60.0,
                    y: cy + 20.0,
                    text: sc.clone(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                });
            }

            cy += item_h;
            if cy > py + picker_h - padding {
                break;
            }
        }

        // Empty state.
        if filtered.is_empty() {
            commands.push(RenderCommand::Text {
                x: px + padding,
                y: py + header_h + 20.0,
                text: "No workspaces found".to_string(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        commands
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mgr() -> SessionManager {
        SessionManager::new()
    }

    fn sample_window(app_id: &str, x: i32, y: i32) -> SavedWindowState {
        SavedWindowState {
            app_id: app_id.to_string(),
            title_hint: format!("{app_id} Window"),
            x,
            y,
            width: 800,
            height: 600,
            state: SavedWindowMode::Normal,
            desktop: 0,
            focused: false,
            z_index: 0,
        }
    }

    // ---- Workspace creation ----

    #[test]
    fn create_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev");
        assert!(id.is_some());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn create_duplicate_name_rejected() {
        let mut mgr = make_mgr();
        mgr.create_workspace("Dev");
        assert!(mgr.create_workspace("Dev").is_none());
    }

    #[test]
    fn create_exceeds_max() {
        let mut mgr = make_mgr();
        mgr.max_workspaces = 2;
        mgr.create_workspace("A");
        mgr.create_workspace("B");
        assert!(mgr.create_workspace("C").is_none());
    }

    // ---- Workspace management ----

    #[test]
    fn delete_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Test").unwrap();
        assert!(mgr.delete_workspace(id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn delete_nonexistent() {
        let mut mgr = make_mgr();
        assert!(!mgr.delete_workspace(999));
    }

    #[test]
    fn rename_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Old").unwrap();
        assert!(mgr.rename_workspace(id, "New"));
        assert_eq!(mgr.get(id).unwrap().name, "New");
    }

    #[test]
    fn rename_to_existing_rejected() {
        let mut mgr = make_mgr();
        mgr.create_workspace("A");
        let id = mgr.create_workspace("B").unwrap();
        assert!(!mgr.rename_workspace(id, "A"));
    }

    #[test]
    fn find_by_name() {
        let mut mgr = make_mgr();
        mgr.create_workspace("Dev");
        assert!(mgr.find_by_name("Dev").is_some());
        assert!(mgr.find_by_name("Prod").is_none());
    }

    // ---- Workspace content ----

    #[test]
    fn add_window_to_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Test").unwrap();
        let ws = mgr.get_mut(id).unwrap();
        ws.add_window(sample_window("terminal", 100, 100));
        assert_eq!(ws.window_count(), 1);
    }

    #[test]
    fn remove_app_windows() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Test").unwrap();
        let ws = mgr.get_mut(id).unwrap();
        ws.add_window(sample_window("terminal", 100, 100));
        ws.add_window(sample_window("editor", 200, 200));
        ws.add_window(sample_window("terminal", 300, 100));
        ws.remove_app_windows("terminal");
        assert_eq!(ws.window_count(), 1);
    }

    #[test]
    fn app_ids() {
        let mut ws = Workspace::new(1, "Test");
        ws.add_window(sample_window("terminal", 0, 0));
        ws.add_window(sample_window("editor", 0, 0));
        ws.add_window(sample_window("terminal", 0, 0));
        let ids = ws.app_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"terminal"));
        assert!(ids.contains(&"editor"));
    }

    #[test]
    fn has_app() {
        let mut ws = Workspace::new(1, "Test");
        ws.add_window(sample_window("editor", 0, 0));
        assert!(ws.has_app("editor"));
        assert!(!ws.has_app("terminal"));
    }

    // ---- Apply workspace ----

    #[test]
    fn apply_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        mgr.get_mut(id).unwrap().add_window(sample_window("editor", 0, 0));
        let windows = mgr.apply_workspace(id, 5000);
        assert!(windows.is_some());
        assert_eq!(windows.unwrap().len(), 1);
        assert_eq!(mgr.active_workspace, Some(id));
        assert_eq!(mgr.get(id).unwrap().last_used, 5000);
    }

    #[test]
    fn apply_nonexistent() {
        let mut mgr = make_mgr();
        assert!(mgr.apply_workspace(999, 0).is_none());
    }

    // ---- Snapshot ----

    #[test]
    fn snapshot_to_workspace() {
        let mut mgr = make_mgr();
        mgr.session.add_window(sample_window("terminal", 100, 100));
        mgr.session.add_window(sample_window("editor", 200, 200));
        let id = mgr.snapshot_to_workspace("Snapshot", 1000).unwrap();
        assert_eq!(mgr.get(id).unwrap().window_count(), 2);
    }

    // ---- Update from session ----

    #[test]
    fn update_workspace_from_session() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Test").unwrap();
        mgr.session.add_window(sample_window("browser", 0, 0));
        assert!(mgr.update_workspace_from_session(id));
        assert_eq!(mgr.get(id).unwrap().window_count(), 1);
    }

    // ---- Duplicate ----

    #[test]
    fn duplicate_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        mgr.get_mut(id).unwrap().add_window(sample_window("editor", 0, 0));
        let new_id = mgr.duplicate_workspace(id).unwrap();
        assert_ne!(id, new_id);
        assert_eq!(mgr.get(new_id).unwrap().name, "Dev (copy)");
        assert_eq!(mgr.get(new_id).unwrap().window_count(), 1);
    }

    // ---- Session save/restore ----

    #[test]
    fn save_session() {
        let mut mgr = make_mgr();
        let windows = vec![sample_window("term", 0, 0)];
        mgr.save_session(windows, 2, 5000);
        assert_eq!(mgr.session.windows.len(), 1);
        assert_eq!(mgr.session.active_desktop, 2);
        assert_eq!(mgr.session.saved_at, 5000);
    }

    #[test]
    fn restore_session() {
        let mut mgr = make_mgr();
        mgr.save_session(vec![sample_window("term", 0, 0)], 0, 1000);
        let session = mgr.restore_session();
        assert!(session.is_some());
    }

    #[test]
    fn restore_disabled() {
        let mut mgr = make_mgr();
        mgr.session_restore_enabled = false;
        mgr.save_session(vec![sample_window("term", 0, 0)], 0, 1000);
        assert!(mgr.restore_session().is_none());
    }

    #[test]
    fn restore_empty_session() {
        let mgr = make_mgr();
        assert!(mgr.restore_session().is_none());
    }

    // ---- Shortcuts ----

    #[test]
    fn find_by_shortcut() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        mgr.get_mut(id).unwrap().shortcut = Some("Super+1".to_string());
        assert!(mgr.find_by_shortcut("Super+1").is_some());
        assert!(mgr.find_by_shortcut("Super+2").is_none());
    }

    // ---- Sorting ----

    #[test]
    fn sort_by_name() {
        let mut mgr = make_mgr();
        mgr.create_workspace("Zebra");
        mgr.create_workspace("Alpha");
        mgr.sort_by_name();
        assert_eq!(mgr.all_workspaces()[0].name, "Alpha");
    }

    #[test]
    fn sort_by_recent() {
        let mut mgr = make_mgr();
        let id1 = mgr.create_workspace("Old").unwrap();
        let id2 = mgr.create_workspace("New").unwrap();
        mgr.apply_workspace(id1, 1000);
        mgr.apply_workspace(id2, 2000);
        mgr.sort_by_recent();
        assert_eq!(mgr.all_workspaces()[0].name, "New");
    }

    // ---- Export ----

    #[test]
    fn export_workspaces() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        mgr.get_mut(id).unwrap().add_window(sample_window("editor", 0, 0));
        let exported = mgr.export_workspaces();
        assert!(exported.contains("Dev"));
        assert!(exported.contains("editor"));
    }

    // ---- Picker ----

    #[test]
    fn picker_toggle() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        assert!(!picker.visible);
        picker.toggle();
        assert!(picker.visible);
        picker.toggle();
        assert!(!picker.visible);
    }

    #[test]
    fn picker_navigation() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        picker.select_next(3);
        assert_eq!(picker.selected_index, 1);
        picker.select_next(3);
        assert_eq!(picker.selected_index, 2);
        picker.select_next(3);
        assert_eq!(picker.selected_index, 0); // wraps
    }

    #[test]
    fn picker_prev_wraps() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        picker.select_prev(3);
        assert_eq!(picker.selected_index, 2); // wraps to end
    }

    #[test]
    fn picker_render_empty() {
        let picker = WorkspacePicker::new(1920.0, 1080.0);
        let cmds = picker.render(&[]);
        assert!(cmds.is_empty()); // not visible
    }

    #[test]
    fn picker_render_visible() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        let ws = vec![Workspace::new(1, "Dev"), Workspace::new(2, "Chat")];
        let cmds = picker.render(&ws);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn picker_render_with_search() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        picker.search_text = "dev".to_string();
        let ws = vec![Workspace::new(1, "Dev"), Workspace::new(2, "Chat")];
        let cmds = picker.render(&ws);
        assert!(!cmds.is_empty());
    }

    // ---- SessionState ----

    #[test]
    fn session_state_clear() {
        let mut s = SessionState::new();
        s.add_window(sample_window("test", 0, 0));
        assert_eq!(s.windows.len(), 1);
        s.clear();
        assert!(s.windows.is_empty());
    }

    // ---- Delete active workspace clears active ----

    #[test]
    fn delete_active_workspace() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Test").unwrap();
        mgr.active_workspace = Some(id);
        mgr.delete_workspace(id);
        assert!(mgr.active_workspace.is_none());
    }
}
