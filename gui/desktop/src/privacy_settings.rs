//! Privacy settings panel.
//!
//! Manages app permissions for sensitive resources (camera, microphone,
//! location, contacts, calendar, notifications), activity history,
//! telemetry opt-out, and app background access controls.

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
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Permission type
// ============================================================================

/// Sensitive resource permission categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    Camera,
    Microphone,
    Location,
    Contacts,
    Calendar,
    Notifications,
    BackgroundApps,
    FileSystem,
    Clipboard,
    ScreenCapture,
}

impl PermissionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Location => "Location",
            Self::Contacts => "Contacts",
            Self::Calendar => "Calendar",
            Self::Notifications => "Notifications",
            Self::BackgroundApps => "Background apps",
            Self::FileSystem => "File system access",
            Self::Clipboard => "Clipboard",
            Self::ScreenCapture => "Screen capture",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Camera => "Allow apps to access camera hardware",
            Self::Microphone => "Allow apps to access the microphone",
            Self::Location => "Allow apps to determine your location",
            Self::Contacts => "Allow apps to read your contacts",
            Self::Calendar => "Allow apps to read your calendar events",
            Self::Notifications => "Allow apps to send you notifications",
            Self::BackgroundApps => "Allow apps to run in the background",
            Self::FileSystem => "Allow apps to access files outside their sandbox",
            Self::Clipboard => "Allow apps to read the clipboard",
            Self::ScreenCapture => "Allow apps to capture the screen",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Camera => "📷",
            Self::Microphone => "🎤",
            Self::Location => "📍",
            Self::Contacts => "👤",
            Self::Calendar => "📅",
            Self::Notifications => "🔔",
            Self::BackgroundApps => "⏳",
            Self::FileSystem => "📁",
            Self::Clipboard => "📋",
            Self::ScreenCapture => "🖥",
        }
    }

    pub const ALL: [Self; 10] = [
        Self::Camera, Self::Microphone, Self::Location, Self::Contacts,
        Self::Calendar, Self::Notifications, Self::BackgroundApps,
        Self::FileSystem, Self::Clipboard, Self::ScreenCapture,
    ];
}

// ============================================================================
// Per-app permission
// ============================================================================

/// Permission state for one app + one permission type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    /// App has been granted this permission.
    Allowed,
    /// App has been denied this permission.
    Denied,
    /// User hasn't been asked yet; will prompt on first access.
    NotDecided,
}

impl PermissionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allowed",
            Self::Denied => "Denied",
            Self::NotDecided => "Not decided",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::Allowed => GREEN,
            Self::Denied => RED,
            Self::NotDecided => OVERLAY0,
        }
    }
}

/// Per-app permission entry.
#[derive(Clone, Debug)]
pub struct AppPermission {
    pub app_id: String,
    pub app_name: String,
    pub kind: PermissionKind,
    pub state: PermissionState,
    /// How many times this permission was exercised.
    pub access_count: u32,
    /// Timestamp of last access (seconds since epoch), or 0 if never.
    pub last_access_secs: u64,
}

impl AppPermission {
    pub fn new(app_id: &str, app_name: &str, kind: PermissionKind) -> Self {
        Self {
            app_id: app_id.into(),
            app_name: app_name.into(),
            kind,
            state: PermissionState::NotDecided,
            access_count: 0,
            last_access_secs: 0,
        }
    }
}

// ============================================================================
// Activity history
// ============================================================================

/// An entry in the activity history log.
#[derive(Clone, Debug)]
pub struct ActivityEntry {
    pub app_id: String,
    pub app_name: String,
    pub permission: PermissionKind,
    pub timestamp_secs: u64,
    /// Whether the access was allowed or blocked.
    pub allowed: bool,
}

// ============================================================================
// Telemetry settings
// ============================================================================

/// Telemetry / data collection level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryLevel {
    /// No data collection at all.
    Off,
    /// Basic crash reports and error diagnostics only.
    Basic,
    /// Full usage data (feature usage, performance metrics).
    Full,
}

impl TelemetryLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off — no data collected",
            Self::Basic => "Basic — crash reports only",
            Self::Full => "Full — usage and diagnostics",
        }
    }

    pub const ALL: [Self; 3] = [Self::Off, Self::Basic, Self::Full];
}

// ============================================================================
// Privacy settings manager
// ============================================================================

/// Central privacy settings state.
pub struct PrivacySettings {
    /// Global toggles per permission type (master switch).
    global_toggles: Vec<(PermissionKind, bool)>,
    /// Per-app permission entries.
    app_permissions: Vec<AppPermission>,
    /// Activity access history.
    activity_log: Vec<ActivityEntry>,
    /// Maximum activity log entries.
    max_log_entries: usize,
    /// Telemetry level.
    pub telemetry: TelemetryLevel,
    /// Whether to show permission prompts to the user.
    pub prompt_on_first_access: bool,
    /// Whether to clear activity history on logout.
    pub clear_history_on_logout: bool,
    /// Whether location access is limited to "while app is in use".
    pub location_while_in_use_only: bool,
}

impl PrivacySettings {
    pub fn new() -> Self {
        let global_toggles = PermissionKind::ALL.iter().map(|k| (*k, true)).collect();
        Self {
            global_toggles,
            app_permissions: Vec::new(),
            activity_log: Vec::new(),
            max_log_entries: 500,
            telemetry: TelemetryLevel::Basic,
            prompt_on_first_access: true,
            clear_history_on_logout: false,
            location_while_in_use_only: true,
        }
    }

    // ------------------------------------------------------------------
    // Global toggles
    // ------------------------------------------------------------------

    pub fn is_globally_enabled(&self, kind: PermissionKind) -> bool {
        self.global_toggles
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(true, |(_, e)| *e)
    }

    pub fn set_globally_enabled(&mut self, kind: PermissionKind, enabled: bool) {
        if let Some(entry) = self.global_toggles.iter_mut().find(|(k, _)| *k == kind) {
            entry.1 = enabled;
        }
    }

    // ------------------------------------------------------------------
    // Per-app permissions
    // ------------------------------------------------------------------

    pub fn set_app_permission(
        &mut self,
        app_id: &str,
        app_name: &str,
        kind: PermissionKind,
        state: PermissionState,
    ) {
        if let Some(entry) = self
            .app_permissions
            .iter_mut()
            .find(|p| p.app_id == app_id && p.kind == kind)
        {
            entry.state = state;
        } else {
            let mut p = AppPermission::new(app_id, app_name, kind);
            p.state = state;
            self.app_permissions.push(p);
        }
    }

    pub fn get_app_permission(&self, app_id: &str, kind: PermissionKind) -> PermissionState {
        self.app_permissions
            .iter()
            .find(|p| p.app_id == app_id && p.kind == kind)
            .map_or(PermissionState::NotDecided, |p| p.state)
    }

    /// Check whether an app should be allowed a permission, considering
    /// the global toggle and the per-app setting.
    pub fn is_allowed(&self, app_id: &str, kind: PermissionKind) -> bool {
        if !self.is_globally_enabled(kind) {
            return false;
        }
        self.get_app_permission(app_id, kind) == PermissionState::Allowed
    }

    /// List all apps that have any permission entry for a given kind.
    pub fn apps_for_permission(&self, kind: PermissionKind) -> Vec<&AppPermission> {
        self.app_permissions.iter().filter(|p| p.kind == kind).collect()
    }

    /// List all permission entries for an app.
    pub fn permissions_for_app(&self, app_id: &str) -> Vec<&AppPermission> {
        self.app_permissions.iter().filter(|p| p.app_id == app_id).collect()
    }

    pub fn record_access(&mut self, app_id: &str, kind: PermissionKind, allowed: bool) {
        // Update access stats on the permission entry.
        if let Some(entry) = self
            .app_permissions
            .iter_mut()
            .find(|p| p.app_id == app_id && p.kind == kind)
        {
            entry.access_count = entry.access_count.saturating_add(1);
            // We'd use a real timestamp; use a placeholder for now.
            entry.last_access_secs = entry.last_access_secs.saturating_add(1);
        }

        // Add to activity log.
        if self.activity_log.len() >= self.max_log_entries {
            self.activity_log.remove(0);
        }
        self.activity_log.push(ActivityEntry {
            app_id: app_id.into(),
            app_name: self
                .app_permissions
                .iter()
                .find(|p| p.app_id == app_id)
                .map_or_else(|| app_id.to_string(), |p| p.app_name.clone()),
            permission: kind,
            timestamp_secs: 0,
            allowed,
        });
    }

    pub fn activity_log(&self) -> &[ActivityEntry] {
        &self.activity_log
    }

    pub fn clear_activity_log(&mut self) {
        self.activity_log.clear();
    }

    /// Revoke all permissions for a given app.
    pub fn revoke_all(&mut self, app_id: &str) {
        for p in &mut self.app_permissions {
            if p.app_id == app_id {
                p.state = PermissionState::Denied;
            }
        }
    }

    /// Remove all permission entries for a given app (e.g., on uninstall).
    pub fn remove_app(&mut self, app_id: &str) {
        self.app_permissions.retain(|p| p.app_id != app_id);
    }

    /// Count of apps with at least one Allowed permission of the given kind.
    pub fn allowed_count(&self, kind: PermissionKind) -> usize {
        self.app_permissions
            .iter()
            .filter(|p| p.kind == kind && p.state == PermissionState::Allowed)
            .count()
    }
}

// ============================================================================
// Settings panel rendering
// ============================================================================

/// UI state for the privacy settings panel.
pub struct PrivacySettingsUI {
    settings: PrivacySettings,
    /// Selected permission category index, or `None` for the overview.
    selected_permission: Option<usize>,
    /// Active tab: 0=Permissions, 1=Activity, 2=General.
    active_tab: usize,
}

impl PrivacySettingsUI {
    pub fn new() -> Self {
        Self {
            settings: PrivacySettings::new(),
            selected_permission: None,
            active_tab: 0,
        }
    }

    pub fn with_settings(settings: PrivacySettings) -> Self {
        Self {
            settings,
            selected_permission: None,
            active_tab: 0,
        }
    }

    pub fn settings(&self) -> &PrivacySettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut PrivacySettings {
        &mut self.settings
    }

    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    pub fn set_active_tab(&mut self, tab: usize) {
        if tab <= 2 {
            self.active_tab = tab;
        }
    }

    pub fn selected_permission(&self) -> Option<usize> {
        self.selected_permission
    }

    pub fn select_permission(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            if i < PermissionKind::ALL.len() {
                self.selected_permission = Some(i);
            }
        } else {
            self.selected_permission = None;
        }
    }

    const TAB_LABELS: [&'static str; 3] = ["Permissions", "Activity", "General"];

    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        // Background
        cmds.push(RenderCommand::FillRect {
            x, y, width, height: 900.0,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: "Privacy & Permissions".into(),
            font_size: 20.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 32.0;

        // Tab bar
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        for (i, label) in Self::TAB_LABELS.iter().enumerate() {
            let tx = x + pad + tab_w * i as f32;
            let active = self.active_tab == i;
            cmds.push(RenderCommand::FillRect {
                x: tx, y: cy, width: tab_w - 2.0, height: 30.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0, y: cy + 8.0,
                text: (*label).into(),
                font_size: 12.0,
                color: if active { BLUE } else { SUBTEXT0 },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(tab_w - 16.0),
            });
        }
        cy += 38.0;

        match self.active_tab {
            0 => self.render_permissions_tab(&mut cmds, x + pad, cy, inner),
            1 => self.render_activity_tab(&mut cmds, x + pad, cy, inner),
            2 => self.render_general_tab(&mut cmds, x + pad, cy, inner),
            _ => {}
        };

        cmds
    }

    fn render_permissions_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        if let Some(sel) = self.selected_permission {
            let kind = PermissionKind::ALL[sel];
            // Detail view for selected permission.
            cmds.push(RenderCommand::Text {
                x, y,
                text: format!("{} {}", kind.icon(), kind.label()),
                font_size: 16.0, color: LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });
            y += 24.0;
            cmds.push(RenderCommand::Text {
                x: x + 4.0, y,
                text: kind.description().into(),
                font_size: 12.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 8.0),
            });
            y += 20.0;

            // Global toggle
            let enabled = self.settings.is_globally_enabled(kind);
            Self::render_toggle(cmds, x, y, width, "Allow access to this resource", enabled);
            y += 32.0;

            // App list
            let apps = self.settings.apps_for_permission(kind);
            if apps.is_empty() {
                cmds.push(RenderCommand::Text {
                    x, y,
                    text: "No apps have requested this permission.".into(),
                    font_size: 12.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                });
            } else {
                for app in &apps {
                    cmds.push(RenderCommand::FillRect {
                        x, y, width, height: 32.0,
                        color: MANTLE,
                        corner_radii: CornerRadii::all(4.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + 8.0, y: y + 8.0,
                        text: app.app_name.clone(),
                        font_size: 13.0, color: TEXT,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.5),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width * 0.55, y: y + 8.0,
                        text: app.state.label().into(),
                        font_size: 13.0, color: app.state.color(),
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.2),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width * 0.78, y: y + 8.0,
                        text: format!("{}×", app.access_count),
                        font_size: 11.0, color: OVERLAY0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.2),
                    });
                    y += 36.0;
                }
            }
        } else {
            // Overview: list all permission categories.
            for (i, kind) in PermissionKind::ALL.iter().enumerate() {
                let enabled = self.settings.is_globally_enabled(*kind);
                let count = self.settings.allowed_count(*kind);
                cmds.push(RenderCommand::FillRect {
                    x, y, width, height: 40.0,
                    color: MANTLE,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 8.0, y: y + 4.0,
                    text: format!("{} {}", kind.icon(), kind.label()),
                    font_size: 14.0, color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.5),
                });

                let status = if !enabled {
                    "Disabled".to_string()
                } else if count > 0 {
                    format!("{} apps allowed", count)
                } else {
                    "No apps".to_string()
                };
                cmds.push(RenderCommand::Text {
                    x: x + width * 0.55, y: y + 4.0,
                    text: status,
                    font_size: 12.0,
                    color: if enabled { GREEN } else { RED },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.4),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 8.0, y: y + 22.0,
                    text: kind.description().into(),
                    font_size: 10.0, color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 16.0),
                });

                let _ = i; // used in hit-test
                y += 46.0;
            }
        }
    }

    fn render_activity_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        let log = self.settings.activity_log();
        if log.is_empty() {
            cmds.push(RenderCommand::Text {
                x, y,
                text: "No activity recorded yet.".into(),
                font_size: 13.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x, y,
            text: format!("{} recent access events", log.len()),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        // Show last 20 entries (newest first).
        let show = log.iter().rev().take(20);
        for entry in show {
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 28.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            let icon = entry.permission.icon();
            let status = if entry.allowed { "✓" } else { "✕" };
            let color = if entry.allowed { GREEN } else { RED };
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 6.0,
                text: format!("{} {} {} {}", icon, entry.app_name, entry.permission.label(), status),
                font_size: 12.0, color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
            });
            y += 32.0;
        }
    }

    fn render_general_tab(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x, y,
            text: "Telemetry".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        for level in TelemetryLevel::ALL {
            let active = self.settings.telemetry == level;
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 28.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(4.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 6.0,
                text: format!("{}{}", indicator, level.label()),
                font_size: 13.0,
                color: if active { BLUE } else { TEXT },
                font_weight: if active { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(width - 16.0),
            });
            y += 32.0;
        }

        y += 8.0;
        cmds.push(RenderCommand::Text {
            x, y,
            text: "Other".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        Self::render_toggle(cmds, x, y, width, "Prompt on first access", self.settings.prompt_on_first_access);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Clear activity on logout", self.settings.clear_history_on_logout);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Location: while in use only", self.settings.location_while_in_use_only);
    }

    fn render_toggle(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, label: &str, on: bool) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y,
            text: label.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.65),
        });
        let tx = x + width - 48.0;
        let bg = if on { GREEN } else { SURFACE1 };
        cmds.push(RenderCommand::FillRect {
            x: tx, y, width: 40.0, height: 20.0,
            color: bg,
            corner_radii: CornerRadii::all(10.0),
        });
        let knob_x = if on { tx + 22.0 } else { tx + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x, y: y + 2.0, width: 16.0, height: 16.0,
            color: TEXT,
            corner_radii: CornerRadii::all(8.0),
        });
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_kind_labels() {
        for k in PermissionKind::ALL {
            assert!(!k.label().is_empty());
            assert!(!k.description().is_empty());
            assert!(!k.icon().is_empty());
        }
    }

    #[test]
    fn permission_state_labels() {
        for s in [PermissionState::Allowed, PermissionState::Denied, PermissionState::NotDecided] {
            assert!(!s.label().is_empty());
            let _ = s.color();
        }
    }

    #[test]
    fn telemetry_labels() {
        for l in TelemetryLevel::ALL {
            assert!(!l.label().is_empty());
        }
    }

    #[test]
    fn global_toggle_default_enabled() {
        let s = PrivacySettings::new();
        for k in PermissionKind::ALL {
            assert!(s.is_globally_enabled(k));
        }
    }

    #[test]
    fn global_toggle_disable() {
        let mut s = PrivacySettings::new();
        s.set_globally_enabled(PermissionKind::Camera, false);
        assert!(!s.is_globally_enabled(PermissionKind::Camera));
        assert!(s.is_globally_enabled(PermissionKind::Microphone));
    }

    #[test]
    fn set_app_permission() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("cam_app", "Camera App", PermissionKind::Camera, PermissionState::Allowed);
        assert_eq!(s.get_app_permission("cam_app", PermissionKind::Camera), PermissionState::Allowed);
    }

    #[test]
    fn update_app_permission() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Location, PermissionState::Allowed);
        s.set_app_permission("app", "App", PermissionKind::Location, PermissionState::Denied);
        assert_eq!(s.get_app_permission("app", PermissionKind::Location), PermissionState::Denied);
    }

    #[test]
    fn get_undecided_by_default() {
        let s = PrivacySettings::new();
        assert_eq!(s.get_app_permission("any", PermissionKind::Camera), PermissionState::NotDecided);
    }

    #[test]
    fn is_allowed_respects_global() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        assert!(s.is_allowed("app", PermissionKind::Camera));
        s.set_globally_enabled(PermissionKind::Camera, false);
        assert!(!s.is_allowed("app", PermissionKind::Camera));
    }

    #[test]
    fn is_allowed_denied_app() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Microphone, PermissionState::Denied);
        assert!(!s.is_allowed("app", PermissionKind::Microphone));
    }

    #[test]
    fn apps_for_permission() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("a", "A", PermissionKind::Camera, PermissionState::Allowed);
        s.set_app_permission("b", "B", PermissionKind::Camera, PermissionState::Denied);
        s.set_app_permission("c", "C", PermissionKind::Location, PermissionState::Allowed);
        let cam_apps = s.apps_for_permission(PermissionKind::Camera);
        assert_eq!(cam_apps.len(), 2);
    }

    #[test]
    fn permissions_for_app() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.set_app_permission("app", "App", PermissionKind::Microphone, PermissionState::Denied);
        let perms = s.permissions_for_app("app");
        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn record_access() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.record_access("app", PermissionKind::Camera, true);
        assert_eq!(s.activity_log().len(), 1);
        assert!(s.activity_log()[0].allowed);
    }

    #[test]
    fn activity_log_ring_buffer() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        for _ in 0..600 {
            s.record_access("app", PermissionKind::Camera, true);
        }
        assert_eq!(s.activity_log().len(), 500);
    }

    #[test]
    fn clear_activity_log() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.record_access("app", PermissionKind::Camera, true);
        s.clear_activity_log();
        assert!(s.activity_log().is_empty());
    }

    #[test]
    fn revoke_all() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.set_app_permission("app", "App", PermissionKind::Microphone, PermissionState::Allowed);
        s.revoke_all("app");
        assert_eq!(s.get_app_permission("app", PermissionKind::Camera), PermissionState::Denied);
        assert_eq!(s.get_app_permission("app", PermissionKind::Microphone), PermissionState::Denied);
    }

    #[test]
    fn remove_app() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.remove_app("app");
        assert!(s.permissions_for_app("app").is_empty());
    }

    #[test]
    fn allowed_count() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("a", "A", PermissionKind::Camera, PermissionState::Allowed);
        s.set_app_permission("b", "B", PermissionKind::Camera, PermissionState::Allowed);
        s.set_app_permission("c", "C", PermissionKind::Camera, PermissionState::Denied);
        assert_eq!(s.allowed_count(PermissionKind::Camera), 2);
    }

    #[test]
    fn ui_new() {
        let ui = PrivacySettingsUI::new();
        assert_eq!(ui.active_tab(), 0);
        assert!(ui.selected_permission().is_none());
    }

    #[test]
    fn ui_set_tab() {
        let mut ui = PrivacySettingsUI::new();
        ui.set_active_tab(2);
        assert_eq!(ui.active_tab(), 2);
        ui.set_active_tab(99);
        assert_eq!(ui.active_tab(), 2);
    }

    #[test]
    fn ui_select_permission() {
        let mut ui = PrivacySettingsUI::new();
        ui.select_permission(Some(3));
        assert_eq!(ui.selected_permission(), Some(3));
        ui.select_permission(None);
        assert!(ui.selected_permission().is_none());
    }

    #[test]
    fn ui_select_permission_out_of_range() {
        let mut ui = PrivacySettingsUI::new();
        ui.select_permission(Some(99));
        assert!(ui.selected_permission().is_none());
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = PrivacySettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = PrivacySettingsUI::new();
        for i in 0..3 {
            ui.set_active_tab(i);
            let cmds = ui.render(0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_permission_detail() {
        let mut ui = PrivacySettingsUI::new();
        ui.settings_mut().set_app_permission("cam", "Camera App", PermissionKind::Camera, PermissionState::Allowed);
        ui.select_permission(Some(0)); // Camera
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_cam = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Camera")));
        assert!(has_cam);
    }

    #[test]
    fn ui_render_activity_with_entries() {
        let mut ui = PrivacySettingsUI::new();
        ui.settings_mut().set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        ui.settings_mut().record_access("app", PermissionKind::Camera, true);
        ui.set_active_tab(1);
        let cmds = ui.render(0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn default_telemetry() {
        let s = PrivacySettings::new();
        assert_eq!(s.telemetry, TelemetryLevel::Basic);
    }

    #[test]
    fn default_privacy_booleans() {
        let s = PrivacySettings::new();
        assert!(s.prompt_on_first_access);
        assert!(!s.clear_history_on_logout);
        assert!(s.location_while_in_use_only);
    }

    #[test]
    fn access_count_increments() {
        let mut s = PrivacySettings::new();
        s.set_app_permission("app", "App", PermissionKind::Camera, PermissionState::Allowed);
        s.record_access("app", PermissionKind::Camera, true);
        s.record_access("app", PermissionKind::Camera, true);
        let perms = s.apps_for_permission(PermissionKind::Camera);
        assert_eq!(perms[0].access_count, 2);
    }
}
