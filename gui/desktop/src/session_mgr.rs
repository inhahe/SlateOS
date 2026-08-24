//! Session and workspace management.
//!
//! Saves and restores window layouts (positions, sizes, states) so the user
//! can quickly switch between work contexts (e.g., "Development" with editor
//! + terminal + browser, "Communication" with email + chat, etc.).
//!
//! Also handles session persistence across logouts/reboots — remembering
//! which apps were open and where they were placed.

use appearance::Palette;
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::step;
use guitk::style::CornerRadii;

// This module used to open with seven `const … : Color` holding Catppuccin
// Mocha values, so the workspace picker was a dark-mode overlay whatever the
// user had chosen. Every colour below is a role read from the [`Palette`] the
// renderer is handed. See known-issues.md
// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//
// Three of them did not survive the move unchanged, because reading a role is
// only half the job — the *right* role still has to be chosen:
//
// - The window count and the shortcut hint were `OVERLAY0`. `overlay0` is
//   documented in the palette as the rung *below* anything a user has to read,
//   and the measurement agrees: on the picker's own background it is 3.59:1 in
//   Mocha and 2.14:1 in Latte, under the 4.5:1 floor in both modes. They are
//   now `subtext1`. `subtext0`, the next rung down, is not available to them —
//   see the fill note below.
// - The empty-state line was `OVERLAY0` for the same reason and is now
//   `subtext1` as well.
// - The selected row's fill was `SURFACE0`. Latte's surface ladder sits too
//   close to its ink ladder for a quiet ink to survive on it: `subtext0` reads
//   3.40:1 there and `subtext1`, the next rung up, still only reaches 4.05 —
//   the same wall the calendar's event-detail card hit. So the fill moved
//   rather than the ink. The selected row is now `mantle`: one step *away*
//   from `base` in both modes, which makes the selection a shallow well rather
//   than a raised panel, and leaves the whole ink ladder legible on it.
//
// The consequence for the two per-row captions is that they land on `base`
// (an ordinary row) or on `mantle` (the selected one) depending on a condition
// they do not test, so they have to clear the floor on *both*. `subtext0` does
// not — 4.31:1 on `mantle` in Latte — and `subtext1` does, at 5.14. The
// unselected icon keeps `subtext0` because it is drawn only where `base` is,
// by construction: exactly one row is the selected one.

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
    /// Color tag for visual identification, if the user picked one.
    ///
    /// `None` is not "no colour" — the tag is always drawn — it is "this
    /// workspace has not been tagged, so use whatever the theme's default tag
    /// colour is". Storing a resolved [`Color`] here instead would bake one
    /// mode's blue into a saved workspace at the moment it was created, and
    /// that value would then outlive every theme change: a workspace made in
    /// dark mode would keep wearing a pastel blue in light mode forever.
    /// Resolve it at draw time with [`tag_color`](Self::tag_color).
    pub color: Option<Color>,
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
            color: None,
        }
    }

    /// The colour to draw this workspace's tag in.
    ///
    /// The user's choice if there is one, otherwise the theme's `blue`. The
    /// default is the named hue rather than [`Palette::accent`] on purpose: a
    /// tag whose default followed the accent would make every *untagged*
    /// workspace look like the themed one, which is the opposite of what a tag
    /// is for.
    #[must_use]
    pub fn tag_color(&self, p: &Palette) -> Color {
        self.color.unwrap_or(p.blue)
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
        // Unstable: equal &str are indistinguishable, so stability buys
        // nothing and costs a scratch allocation.
        ids.sort_unstable();
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
    /// Source of workspace IDs.
    ids: IdSeq<WorkspaceId>,
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
            ids: IdSeq::new(),
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
        let id = self.ids.issue_infallible();
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
        if self
            .workspaces
            .iter()
            .any(|w| w.name == new_name && w.id != id)
        {
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
    pub fn apply_workspace(
        &mut self,
        id: WorkspaceId,
        now_ms: u64,
    ) -> Option<Vec<SavedWindowState>> {
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
    pub fn save_session(
        &mut self,
        windows: Vec<SavedWindowState>,
        active_desktop: u32,
        now_ms: u64,
    ) {
        self.session.windows = windows;
        self.session.active_desktop = active_desktop;
        self.session.saved_at = now_ms;
    }

    /// Get session state for restore (at login).
    pub fn restore_session(&self) -> Option<&SessionState> {
        if self.session_restore_enabled
            && self.session.restore_on_login
            && !self.session.windows.is_empty()
        {
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
        self.workspaces
            .sort_by_key(|w| std::cmp::Reverse(w.last_used));
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
                    win.app_id,
                    win.title_hint,
                    win.x,
                    win.y,
                    win.width,
                    win.height,
                    win.state,
                    win.desktop
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
        self.selected_index = step::wrapping_after(count, self.selected_index);
    }

    pub fn select_prev(&mut self, count: usize) {
        self.selected_index = step::wrapping_before(count, self.selected_index);
    }

    /// Render the picker overlay.
    pub fn render(&self, p: &Palette, workspaces: &[Workspace]) -> Vec<RenderCommand> {
        if !self.visible {
            return Vec::new();
        }

        let picker_bg = p.base;
        let picker_border = p.surface1;
        let title_ink = p.text;
        let searching_ink = p.accent;
        let selected_row = p.mantle;
        let selected_icon_ink = p.text;
        let icon_ink = p.subtext0;
        let name_ink = p.text;
        let caption_ink = p.subtext1;

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
            color: picker_bg,
            corner_radii: CornerRadii::all(12.0),
        });

        // Border.
        commands.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: picker_border,
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
            color: if self.search_text.is_empty() {
                title_ink
            } else {
                searching_ink
            },
            font_weight: FontWeightHint::Bold,
            max_width: Some(picker_w - padding * 2.0),
            overflow: TextOverflow::Ellipsis,
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
                    color: selected_row,
                    corner_radii: CornerRadii::all(8.0),
                });
            }

            // Color tag.
            commands.push(RenderCommand::FillRect {
                x: px + padding,
                y: cy + 16.0,
                width: 4.0,
                height: 24.0,
                color: ws.tag_color(p),
                corner_radii: CornerRadii::all(2.0),
            });

            // Icon.
            commands.push(RenderCommand::Text {
                x: px + padding + 14.0,
                y: cy + 12.0,
                text: ws.icon.clone(),
                font_size: 20.0,
                color: if selected {
                    selected_icon_ink
                } else {
                    icon_ink
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Name.
            commands.push(RenderCommand::Text {
                x: px + padding + 44.0,
                y: cy + 10.0,
                text: ws.name.clone(),
                font_size: 14.0,
                color: name_ink,
                font_weight: FontWeightHint::Bold,
                max_width: Some(picker_w - padding * 2.0 - 100.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Window count and apps.
            let info = format!("{} windows", ws.window_count());
            commands.push(RenderCommand::Text {
                x: px + padding + 44.0,
                y: cy + 32.0,
                text: info,
                font_size: 11.0,
                color: caption_ink,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Shortcut hint.
            if let Some(sc) = &ws.shortcut {
                commands.push(RenderCommand::Text {
                    x: px + picker_w - padding - 60.0,
                    y: cy + 20.0,
                    text: sc.clone(),
                    font_size: 10.0,
                    color: caption_ink,
                    font_weight: FontWeightHint::Light,
                    max_width: None,
                    overflow: TextOverflow::Clip,
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
                color: caption_ink,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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

    fn make_mgr() -> SessionManager {
        SessionManager::new()
    }

    /// The stock dark palette.
    fn dark() -> Palette {
        Palette::for_mode(false)
    }

    /// A palette whose accent is in neither mode's ladder.
    ///
    /// Magenta is not a Catppuccin role in either mode, so a site that draws
    /// it can only have got it from `p.accent` — where the *stock* accent is
    /// `blue`, which is also a named role, so an accent site and a blue site
    /// would be indistinguishable.
    fn accented(light: bool) -> Palette {
        let mut p = Palette::for_mode(light);
        p.accent = Color::from_hex(0xFF00FF);
        p
    }

    /// A colour tag a user picked, off both palettes and off the instruments.
    const USER_PINK: Color = Color::from_hex(0xFF7F50);

    /// The drop shadow, after [`colors`] has flattened its alpha.
    const SHADOW: Color = Color::rgba(0, 0, 0, 255);

    /// Every colour a command list puts on the screen, in order, alpha flattened.
    ///
    /// Alpha is dropped because the picker draws its shadow at 120 and a role
    /// at an alpha is still that role; what this module's tests are about is
    /// *which* colour, not how much of it.
    fn colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. }
                | RenderCommand::BoxShadow { color, .. } => {
                    Some(Color::rgba(color.r, color.g, color.b, 255))
                }
                _ => None,
            })
            .collect()
    }

    /// Three workspaces and an open picker with the middle one selected.
    ///
    /// Deliberately mixed: one workspace carries a user-chosen tag and two do
    /// not, two have shortcut hints and one does not, and the selected row is
    /// neither the first nor the last. A fixture where every row looks the
    /// same cannot tell a per-row site from a per-picker one.
    fn scene() -> (WorkspacePicker, Vec<Workspace>) {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        picker.selected_index = 1;

        let mut dev = Workspace::new(1, "Dev");
        dev.color = Some(USER_PINK);
        dev.shortcut = Some("Super+1".to_string());

        let chat = Workspace::new(2, "Chat");

        let mut mail = Workspace::new(3, "Mail");
        mail.shortcut = Some("Super+3".to_string());

        (picker, vec![dev, chat, mail])
    }

    /// The colour every site in [`scene`] should draw, in render order.
    ///
    /// Written out by hand rather than derived from the command list: a
    /// locator taken from the renderer's own output cannot see the renderer
    /// permuting that output, and this list is the only thing in the module
    /// that pins the order.
    fn expected(p: &Palette) -> Vec<(&'static str, Color)> {
        vec![
            ("the drop shadow", SHADOW),
            ("the picker's background", p.base),
            ("the picker's border", p.surface1),
            ("the title", p.text),
            ("Dev's colour tag", USER_PINK),
            ("Dev's icon", p.subtext0),
            ("Dev's name", p.text),
            ("Dev's window count", p.subtext1),
            ("Dev's shortcut hint", p.subtext1),
            ("the selected row's fill", p.mantle),
            ("Chat's colour tag", p.blue),
            ("Chat's icon", p.text),
            ("Chat's name", p.text),
            ("Chat's window count", p.subtext1),
            ("Mail's colour tag", p.blue),
            ("Mail's icon", p.subtext0),
            ("Mail's name", p.text),
            ("Mail's window count", p.subtext1),
            ("Mail's shortcut hint", p.subtext1),
        ]
    }

    /// WCAG 2.1 relative-contrast ratio.
    fn contrast(a: Color, b: Color) -> f64 {
        fn lum(c: Color) -> f64 {
            fn ch(v: u8) -> f64 {
                let v = f64::from(v) / 255.0;
                if v <= 0.040_45 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        }
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
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
        mgr.get_mut(id)
            .unwrap()
            .add_window(sample_window("editor", 0, 0));
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
        mgr.get_mut(id)
            .unwrap()
            .add_window(sample_window("editor", 0, 0));
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
        mgr.get_mut(id)
            .unwrap()
            .add_window(sample_window("editor", 0, 0));
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
        let cmds = picker.render(&dark(), &[]);
        assert!(cmds.is_empty()); // not visible
    }

    #[test]
    fn picker_render_visible() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        let ws = vec![Workspace::new(1, "Dev"), Workspace::new(2, "Chat")];
        let cmds = picker.render(&dark(), &ws);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn picker_render_with_search() {
        let mut picker = WorkspacePicker::new(1920.0, 1080.0);
        picker.visible = true;
        picker.search_text = "dev".to_string();
        let ws = vec![Workspace::new(1, "Dev"), Workspace::new(2, "Chat")];
        let cmds = picker.render(&dark(), &ws);
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

    // ---- Palette conversion ----

    /// Every site the picker draws puts the role it claims where it claims it.
    ///
    /// The whole ordered vector, not a set: a set cannot see two sites
    /// swapping colours, and the icon/name/caption sites within a row are
    /// exactly the shape that would swap unnoticed. Run under an off-palette
    /// accent so the title's `accent` and the tag's `blue` stay distinguishable.
    #[test]
    fn every_picker_site_draws_the_role_it_claims() {
        for light in [false, true] {
            let p = accented(light);
            let (picker, ws) = scene();
            let drawn = colors(&picker.render(&p, &ws));
            let want = expected(&p);
            assert_eq!(
                drawn.len(),
                want.len(),
                "the picker drew {} colours, not {}, in {} mode",
                drawn.len(),
                want.len(),
                if light { "light" } else { "dark" }
            );
            for (got, (what, wanted)) in drawn.iter().zip(want) {
                assert_eq!(
                    format!("{got:?}"),
                    format!("{wanted:?}"),
                    "{what} is wrong in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// The title turns the accent colour only while the user is typing.
    #[test]
    fn the_title_wears_the_accent_only_while_searching() {
        let p = accented(false);
        let (mut picker, ws) = scene();

        let idle = colors(&picker.render(&p, &ws));
        assert_eq!(format!("{:?}", idle[3]), format!("{:?}", p.text));

        picker.search_text = "a".to_string();
        let typing = colors(&picker.render(&p, &ws));
        assert_eq!(format!("{:?}", typing[3]), format!("{:?}", p.accent));
    }

    /// Every colour the picker draws is one the palette it was handed contains.
    ///
    /// This is what catches a constant the conversion missed: a leftover
    /// Mocha value is not in the light palette and names itself.
    #[test]
    fn every_colour_the_picker_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = accented(light);
            let (picker, ws) = scene();
            crate::palette_check::assert_drawn_from(
                &p,
                &picker.render(&p, &ws),
                &[USER_PINK],
                "the workspace picker",
            );
        }
    }

    /// Every themed site the picker draws changes when the mode does.
    ///
    /// Membership is not enough: a site pinned to one role's *dark* value
    /// would still be a palette member in dark mode. Two sites are excluded
    /// and both are excluded on purpose — the drop shadow is an absence of
    /// light rather than a colour, and a tag the user picked is theirs, not
    /// the theme's. Run against the stock palette because a pinned accent
    /// legitimately does not move with the mode, and the stock accent does.
    #[test]
    fn every_themed_site_the_picker_draws_moves_with_the_mode() {
        let (picker, ws) = scene();
        let in_dark = colors(&picker.render(&Palette::for_mode(false), &ws));
        let in_light = colors(&picker.render(&Palette::for_mode(true), &ws));
        assert_eq!(in_dark.len(), in_light.len());

        for ((what, _), (d, l)) in expected(&dark())
            .into_iter()
            .zip(in_dark.iter().zip(&in_light))
        {
            if what == "the drop shadow" || what == "Dev's colour tag" {
                continue;
            }
            assert_ne!(
                format!("{d:?}"),
                format!("{l:?}"),
                "{what} draws the same colour in both modes"
            );
        }
    }

    /// Exactly one row is filled, and it is the selected one.
    ///
    /// This is the premise the ink choices rest on: the unselected icon is
    /// allowed to be `subtext0`, which is unreadable on `mantle` in light
    /// mode, only because an unselected row is never the filled one. If the
    /// renderer ever filled two rows — or the wrong one — that ink would be
    /// sitting on a background it was never checked against.
    #[test]
    fn only_the_selected_row_is_filled() {
        let p = dark();
        let (mut picker, ws) = scene();

        let fill_y = |picker: &WorkspacePicker| -> f32 {
            let ys: Vec<f32> = picker
                .render(&p, &ws)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { y, color, .. }
                        if format!("{color:?}") == format!("{:?}", p.mantle) =>
                    {
                        Some(*y)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(ys.len(), 1, "expected exactly one filled row, got {ys:?}");
            ys[0]
        };

        let first = fill_y(&picker);
        picker.selected_index = 0;
        let zeroth = fill_y(&picker);
        picker.selected_index = 2;
        let second = fill_y(&picker);

        let step = first - zeroth;
        assert!(step > 0.0, "the fill did not move down with the selection");
        assert!(
            (second - first - step).abs() < 0.001,
            "the fill moved {} for one step and {} for the next",
            step,
            second - first
        );
    }

    /// Every ink this module puts on a fill clears the 4.5:1 floor.
    ///
    /// Contrast is not a membership property — both halves of an unreadable
    /// pairing can be perfectly good palette members — so the sweep above is
    /// blind to this and always will be. Three pairings were below the floor
    /// before the conversion: the window count and the shortcut hint at
    /// 2.14:1 in light mode, and the empty-state line with them, all because
    /// `overlay0` was being read as an ink.
    ///
    /// Run against the stock palette: an arbitrary accent has arbitrary
    /// contrast, and what to do about that is the design question logged as
    /// `TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC`. The
    /// pairings are written out by hand because a pairing is a fact about
    /// which fill an ink lands on, and the command list does not record that.
    #[test]
    fn every_pairing_the_picker_draws_clears_the_contrast_floor() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let pairings = [
                ("the idle title", p.base, p.text),
                ("the search text", p.base, p.accent),
                ("an unselected workspace's icon", p.base, p.subtext0),
                ("the selected workspace's icon", p.mantle, p.text),
                ("an unselected workspace's name", p.base, p.text),
                ("the selected workspace's name", p.mantle, p.text),
                ("an unselected workspace's window count", p.base, p.subtext1),
                (
                    "the selected workspace's window count",
                    p.mantle,
                    p.subtext1,
                ),
                (
                    "an unselected workspace's shortcut hint",
                    p.base,
                    p.subtext1,
                ),
                (
                    "the selected workspace's shortcut hint",
                    p.mantle,
                    p.subtext1,
                ),
                ("the empty-state line", p.base, p.subtext1),
            ];
            for (what, bg, ink) in pairings {
                let ratio = contrast(bg, ink);
                assert!(
                    ratio >= 4.5,
                    "{what} reads at {ratio:.2}:1 in {} mode",
                    if light { "light" } else { "dark" }
                );
            }
        }
    }

    /// A new workspace is born untagged, and stays that way through a copy.
    ///
    /// The field holds the user's *choice*, so "not chosen" has to be
    /// representable. If `Workspace::new` resolved a colour, a workspace
    /// created in dark mode would carry a pastel blue into light mode forever
    /// and no theme change could reach it.
    #[test]
    fn a_new_workspace_is_born_untagged() {
        assert_eq!(Workspace::new(1, "Dev").color, None);

        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        assert_eq!(mgr.get(id).unwrap().color, None);

        let copy = mgr.duplicate_workspace(id).unwrap();
        assert_eq!(mgr.get(copy).unwrap().color, None);
    }

    /// A tagged workspace keeps its colour; the copy inherits it.
    #[test]
    fn a_tagged_workspace_keeps_the_users_colour() {
        let mut mgr = make_mgr();
        let id = mgr.create_workspace("Dev").unwrap();
        mgr.get_mut(id).unwrap().color = Some(USER_PINK);

        let copy = mgr.duplicate_workspace(id).unwrap();
        assert_eq!(mgr.get(copy).unwrap().color, Some(USER_PINK));
        assert_eq!(
            format!("{:?}", mgr.get(copy).unwrap().tag_color(&dark())),
            format!("{USER_PINK:?}")
        );
    }

    /// The tag the grid draws is the one `tag_color` resolves.
    ///
    /// Two answers to the same question in two places is how they drift
    /// apart; this pins the renderer to the resolver rather than to a value.
    #[test]
    fn the_tag_the_picker_draws_is_the_one_the_resolver_gives() {
        for light in [false, true] {
            let p = accented(light);
            let (picker, ws) = scene();
            let drawn = colors(&picker.render(&p, &ws));
            // Dev's tag is site 4, Chat's is site 10, Mail's is site 14.
            assert_eq!(
                format!("{:?}", drawn[4]),
                format!("{:?}", ws[0].tag_color(&p))
            );
            assert_eq!(
                format!("{:?}", drawn[10]),
                format!("{:?}", ws[1].tag_color(&p))
            );
            assert_eq!(
                format!("{:?}", drawn[14]),
                format!("{:?}", ws[2].tag_color(&p))
            );
        }
    }

    /// The empty state draws the caption ink, on the picker's own background.
    ///
    /// A site nothing renders is a site nothing checks, and [`scene`] always
    /// matches at least one workspace, so the empty branch needs its own
    /// fixture or it would be covered by the contrast table alone.
    #[test]
    fn the_empty_state_draws_the_caption_ink() {
        for light in [false, true] {
            let p = accented(light);
            let (mut picker, ws) = scene();
            picker.search_text = "nothing matches this".to_string();
            let drawn = colors(&picker.render(&p, &ws));
            assert_eq!(
                drawn.len(),
                5,
                "shadow, background, border, title and the empty line, got {drawn:?}"
            );
            assert_eq!(format!("{:?}", drawn[4]), format!("{:?}", p.subtext1));
        }
    }
}
