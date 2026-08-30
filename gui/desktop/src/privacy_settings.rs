//! Privacy settings panel.
//!
//! Manages app permissions for sensitive resources (camera, microphone,
//! location, contacts, calendar, notifications), activity history,
//! telemetry opt-out, and app background access controls.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// This panel had its own copy of Catppuccin Mocha; it now draws from the
// resolved `Palette` its caller passes in, so it follows the user's mode and
// accent. Four judgements decide which sites move with the accent and which
// are pinned to a fixed role.
//
// 1. **Two sites follow the accent, and both mean "you are here".** The
//    active tab's label and the selected telemetry level's label were both
//    Mocha `blue`. Selection is position, and marking position is the
//    accent's job, so both became `p.accent`. Nothing else in the panel is
//    allowed to.
//
// 2. **Allowed/Denied is a category, so green and red are frozen.** Three
//    separate sites report permission state in green-versus-red: the
//    `PermissionState::color` match, the overview's per-resource status, and
//    each row of the activity log. A user on a green desktop must still be
//    able to see that a resource is *denied*, and on a red desktop that one
//    is *allowed* — so these keep `p.green`/`p.red` regardless of accent.
//    This is a privacy panel; a status a user cannot trust at a glance is
//    worse than no status.
//
// 3. **The section headings stay `lavender`.** "Telemetry" and "Other" label
//    a category, and the accent never marks category (module 23's rule).
//
// 4. **The tab strip's own fill is position, but not accent.** Active tabs
//    take `p.surface0` against `p.mantle` — the shape says which tab is
//    selected, and the label already carries the accent; tinting the fill too
//    would leave the strip reading as one solid accent block.
//
// The switch knob stays `p.text` on the pill rather than becoming derived
// ink. That contrast problem is real and is tracked as
// `TD-C-SWITCH-KNOBS-ARE-LOW-CONTRAST-ON-THE-ON-PILL`, which is scheduled to
// fix every switch in the shell at once; fixing this one here would leave the
// desktop half-converted and make that sweep harder to verify.

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
        Self::Camera,
        Self::Microphone,
        Self::Location,
        Self::Contacts,
        Self::Calendar,
        Self::Notifications,
        Self::BackgroundApps,
        Self::FileSystem,
        Self::Clipboard,
        Self::ScreenCapture,
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

    /// The colour this state reports itself in.
    ///
    /// Categorical, not decorative: green *means* allowed and red *means*
    /// denied, so neither follows the accent. See judgement 2 in the module's
    /// colour notes.
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Allowed => p.green,
            Self::Denied => p.red,
            Self::NotDecided => p.overlay0,
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
            .is_none_or(|(_, e)| *e)
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
        self.app_permissions
            .iter()
            .filter(|p| p.kind == kind)
            .collect()
    }

    /// List all permission entries for an app.
    pub fn permissions_for_app(&self, app_id: &str) -> Vec<&AppPermission> {
        self.app_permissions
            .iter()
            .filter(|p| p.app_id == app_id)
            .collect()
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

impl Default for PrivacySettings {
    fn default() -> Self {
        Self::new()
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

    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 900.0,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "Privacy & Permissions".into(),
            font_size: 20.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 32.0;

        // Tab bar
        let tab_w = inner / Self::TAB_LABELS.len() as f32;
        for (i, label) in Self::TAB_LABELS.iter().enumerate() {
            let tx = x + pad + tab_w * i as f32;
            let active = self.active_tab == i;
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: cy,
                width: tab_w - 2.0,
                height: 30.0,
                color: if active { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: cy + 8.0,
                text: (*label).into(),
                font_size: 12.0,
                color: if active { p.accent } else { p.subtext0 },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        cy += 38.0;

        match self.active_tab {
            0 => self.render_permissions_tab(p, &mut cmds, x + pad, cy, inner),
            1 => self.render_activity_tab(p, &mut cmds, x + pad, cy, inner),
            2 => self.render_general_tab(p, &mut cmds, x + pad, cy, inner),
            _ => {}
        }

        cmds
    }

    fn render_permissions_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        // `select_permission` range-checks the index, but that check lives
        // ninety lines away and `ALL` is a list that grows as new sensitive
        // resources are added; resolving the index to the value here keeps the
        // proof in the same expression as the use.
        if let Some(kind) = self
            .selected_permission
            .and_then(|sel| PermissionKind::ALL.get(sel).copied())
        {
            // Detail view for selected permission.
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: format!("{} {}", kind.icon(), kind.label()),
                font_size: 16.0,
                color: p.lavender,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            y += 24.0;
            cmds.push(RenderCommand::Text {
                x: x + 4.0,
                y,
                text: kind.description().into(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 20.0;

            // Global toggle
            let enabled = self.settings.is_globally_enabled(kind);
            Self::render_toggle(
                p,
                cmds,
                x,
                y,
                width,
                "Allow access to this resource",
                enabled,
            );
            y += 32.0;

            // App list
            let apps = self.settings.apps_for_permission(kind);
            if apps.is_empty() {
                cmds.push(RenderCommand::Text {
                    x,
                    y,
                    text: "No apps have requested this permission.".into(),
                    font_size: 12.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
            } else {
                for app in &apps {
                    cmds.push(RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height: 32.0,
                        color: p.mantle,
                        corner_radii: CornerRadii::all(4.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + 8.0,
                        y: y + 8.0,
                        text: app.app_name.clone(),
                        font_size: 13.0,
                        color: p.text,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.5),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width * 0.55,
                        y: y + 8.0,
                        text: app.state.label().into(),
                        font_size: 13.0,
                        color: app.state.color(p),
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.2),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cmds.push(RenderCommand::Text {
                        x: x + width * 0.78,
                        y: y + 8.0,
                        text: format!("{}×", app.access_count),
                        font_size: 11.0,
                        color: p.overlay0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width * 0.2),
                        overflow: TextOverflow::Ellipsis,
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
                    x,
                    y,
                    width,
                    height: 40.0,
                    color: p.mantle,
                    corner_radii: CornerRadii::all(6.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: y + 4.0,
                    text: format!("{} {}", kind.icon(), kind.label()),
                    font_size: 14.0,
                    color: p.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width * 0.5),
                    overflow: TextOverflow::Ellipsis,
                });

                let status = if !enabled {
                    "Disabled".to_string()
                } else if count > 0 {
                    format!("{} apps allowed", count)
                } else {
                    "No apps".to_string()
                };
                cmds.push(RenderCommand::Text {
                    x: x + width * 0.55,
                    y: y + 4.0,
                    text: status,
                    font_size: 12.0,
                    color: if enabled { p.green } else { p.red },
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.4),
                    overflow: TextOverflow::Ellipsis,
                });
                cmds.push(RenderCommand::Text {
                    x: x + 8.0,
                    y: y + 22.0,
                    text: kind.description().into(),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });

                let _ = i; // used in hit-test
                y += 46.0;
            }
        }
    }

    fn render_activity_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        let log = self.settings.activity_log();
        if log.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No activity recorded yet.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: format!("{} recent access events", log.len()),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        // Show last 20 entries (newest first).
        let show = log.iter().rev().take(20);
        for entry in show {
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 28.0,
                color: p.mantle,
                corner_radii: CornerRadii::all(4.0),
            });
            let icon = entry.permission.icon();
            let status = if entry.allowed { "✓" } else { "✕" };
            let color = if entry.allowed { p.green } else { p.red };
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 6.0,
                text: format!(
                    "{} {} {} {}",
                    icon,
                    entry.app_name,
                    entry.permission.label(),
                    status
                ),
                font_size: 12.0,
                color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 32.0;
        }
    }

    fn render_general_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Telemetry".into(),
            font_size: 14.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        for level in TelemetryLevel::ALL {
            let active = self.settings.telemetry == level;
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 28.0,
                color: if active { p.surface0 } else { p.mantle },
                corner_radii: CornerRadii::all(4.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 6.0,
                text: format!("{}{}", indicator, level.label()),
                font_size: 13.0,
                color: if active { p.accent } else { p.text },
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 32.0;
        }

        y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Other".into(),
            font_size: 14.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Prompt on first access",
            self.settings.prompt_on_first_access,
        );
        y += 28.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Clear activity on logout",
            self.settings.clear_history_on_logout,
        );
        y += 28.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Location: while in use only",
            self.settings.location_while_in_use_only,
        );
    }

    fn render_toggle(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        label: &str,
        on: bool,
    ) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: label.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.65),
            overflow: TextOverflow::Ellipsis,
        });
        let tx = x + width - 48.0;
        let bg = if on { p.green } else { p.surface1 };
        cmds.extend(crate::switch::switch(tx, y, 40.0, 20.0, on, bg));
    }
}

impl Default for PrivacySettingsUI {
    fn default() -> Self {
        Self::new()
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
    use crate::palette_check::assert_drawn_from;

    fn mocha() -> Palette {
        Palette::for_mode(false)
    }

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

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
        for s in [
            PermissionState::Allowed,
            PermissionState::Denied,
            PermissionState::NotDecided,
        ] {
            assert!(!s.label().is_empty());
            let _ = s.color(&mocha());
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
        s.set_app_permission(
            "cam_app",
            "Camera App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        assert_eq!(
            s.get_app_permission("cam_app", PermissionKind::Camera),
            PermissionState::Allowed
        );
    }

    #[test]
    fn update_app_permission() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Location,
            PermissionState::Allowed,
        );
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Location,
            PermissionState::Denied,
        );
        assert_eq!(
            s.get_app_permission("app", PermissionKind::Location),
            PermissionState::Denied
        );
    }

    #[test]
    fn get_undecided_by_default() {
        let s = PrivacySettings::new();
        assert_eq!(
            s.get_app_permission("any", PermissionKind::Camera),
            PermissionState::NotDecided
        );
    }

    #[test]
    fn is_allowed_respects_global() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        assert!(s.is_allowed("app", PermissionKind::Camera));
        s.set_globally_enabled(PermissionKind::Camera, false);
        assert!(!s.is_allowed("app", PermissionKind::Camera));
    }

    #[test]
    fn is_allowed_denied_app() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Microphone,
            PermissionState::Denied,
        );
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
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Microphone,
            PermissionState::Denied,
        );
        let perms = s.permissions_for_app("app");
        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn record_access() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        s.record_access("app", PermissionKind::Camera, true);
        assert_eq!(s.activity_log().len(), 1);
        assert!(s.activity_log()[0].allowed);
    }

    #[test]
    fn activity_log_ring_buffer() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        for _ in 0..600 {
            s.record_access("app", PermissionKind::Camera, true);
        }
        assert_eq!(s.activity_log().len(), 500);
    }

    #[test]
    fn clear_activity_log() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        s.record_access("app", PermissionKind::Camera, true);
        s.clear_activity_log();
        assert!(s.activity_log().is_empty());
    }

    #[test]
    fn revoke_all() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Microphone,
            PermissionState::Allowed,
        );
        s.revoke_all("app");
        assert_eq!(
            s.get_app_permission("app", PermissionKind::Camera),
            PermissionState::Denied
        );
        assert_eq!(
            s.get_app_permission("app", PermissionKind::Microphone),
            PermissionState::Denied
        );
    }

    #[test]
    fn remove_app() {
        let mut s = PrivacySettings::new();
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
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
        let cmds = ui.render(&mocha(), 0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = PrivacySettingsUI::new();
        for i in 0..3 {
            ui.set_active_tab(i);
            let cmds = ui.render(&mocha(), 0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_permission_detail() {
        let mut ui = PrivacySettingsUI::new();
        ui.settings_mut().set_app_permission(
            "cam",
            "Camera App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        ui.select_permission(Some(0)); // Camera
        let cmds = ui.render(&mocha(), 0.0, 0.0, 500.0);
        let has_cam = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Camera")));
        assert!(has_cam);
    }

    #[test]
    fn ui_render_activity_with_entries() {
        let mut ui = PrivacySettingsUI::new();
        ui.settings_mut().set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        ui.settings_mut()
            .record_access("app", PermissionKind::Camera, true);
        ui.set_active_tab(1);
        let cmds = ui.render(&mocha(), 0.0, 0.0, 500.0);
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
        s.set_app_permission(
            "app",
            "App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        s.record_access("app", PermissionKind::Camera, true);
        s.record_access("app", PermissionKind::Camera, true);
        let perms = s.apps_for_permission(PermissionKind::Camera);
        assert_eq!(perms[0].access_count, 2);
    }

    // ========================================================================
    // Palette conversion
    // ========================================================================

    const SAFE_ACCENTS: [Color; 4] = [
        appearance::MAUVE,
        appearance::TEAL,
        appearance::SAPPHIRE,
        appearance::PINK,
    ];

    /// Every state this panel can be rendered in, named.
    ///
    /// Built by enumerating the renderer's `if`s and `match`es rather than its
    /// colours: a leftover Mocha constant is only caught if the sweep renders
    /// the state that draws it, and two thirds of this module's colour sites
    /// sit behind the `match self.active_tab` in `render`.
    fn every_state() -> Vec<(String, PrivacySettingsUI)> {
        let mut out: Vec<(String, PrivacySettingsUI)> = Vec::new();

        // Tab 0, overview. The status line is three-way, so the fixture needs
        // a disabled resource, an enabled one with allowed apps, and an
        // enabled one with none.
        let mut st = PrivacySettings::new();
        st.set_globally_enabled(PermissionKind::Camera, false);
        st.set_app_permission(
            "a",
            "App A",
            PermissionKind::Microphone,
            PermissionState::Allowed,
        );
        let mut ui = PrivacySettingsUI::with_settings(st);
        ui.set_active_tab(0);
        ui.select_permission(None);
        out.push(("overview: disabled, allowed, empty".into(), ui));

        // Tab 0, detail, no app has asked yet.
        let mut ui = PrivacySettingsUI::new();
        ui.set_active_tab(0);
        ui.select_permission(Some(0));
        out.push(("detail: no apps".into(), ui));

        // Tab 0, detail, one app in each of the three states.
        let mut st = PrivacySettings::new();
        for (i, state) in [
            PermissionState::Allowed,
            PermissionState::Denied,
            PermissionState::NotDecided,
        ]
        .into_iter()
        .enumerate()
        {
            st.set_app_permission(
                &format!("app{i}"),
                &format!("App {i}"),
                PermissionKind::ALL[0],
                state,
            );
        }
        let mut ui = PrivacySettingsUI::with_settings(st);
        ui.set_active_tab(0);
        ui.select_permission(Some(0));
        out.push(("detail: one app per state, enabled".into(), ui));

        // Tab 0, detail, resource globally disabled -- flips the toggle.
        let mut st = PrivacySettings::new();
        st.set_globally_enabled(PermissionKind::ALL[0], false);
        st.set_app_permission(
            "app",
            "App",
            PermissionKind::ALL[0],
            PermissionState::Denied,
        );
        let mut ui = PrivacySettingsUI::with_settings(st);
        ui.set_active_tab(0);
        ui.select_permission(Some(0));
        out.push(("detail: resource disabled".into(), ui));

        // Tab 1, empty log.
        let mut ui = PrivacySettingsUI::new();
        ui.set_active_tab(1);
        out.push(("activity: empty".into(), ui));

        // Tab 1, log carrying both an allowed and a denied event.
        let mut st = PrivacySettings::new();
        st.set_app_permission(
            "ok",
            "Ok App",
            PermissionKind::Camera,
            PermissionState::Allowed,
        );
        st.record_access("ok", PermissionKind::Camera, true);
        st.record_access("no", PermissionKind::Microphone, false);
        let mut ui = PrivacySettingsUI::with_settings(st);
        ui.set_active_tab(1);
        out.push(("activity: allowed and denied".into(), ui));

        // Tab 2, one fixture per telemetry level so every radio row is
        // rendered both selected and unselected, crossed with the three
        // toggles set both ways.
        for (i, level) in TelemetryLevel::ALL.into_iter().enumerate() {
            for on in [false, true] {
                let mut st = PrivacySettings::new();
                st.telemetry = level;
                st.prompt_on_first_access = on;
                st.clear_history_on_logout = on;
                st.location_while_in_use_only = on;
                let mut ui = PrivacySettingsUI::with_settings(st);
                ui.set_active_tab(2);
                out.push((format!("general: level {i}, toggles {on}"), ui));
            }
        }

        out
    }

    fn texts_of(cmds: &[RenderCommand]) -> Vec<(String, f32, Color)> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    color,
                    ..
                } => Some((text.clone(), *font_size, *color)),
                _ => None,
            })
            .collect()
    }

    fn fills(cmds: &[RenderCommand], w: f32, h: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    ..
                } if (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01 => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn every_color(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { color, .. } | RenderCommand::FillRect { color, .. } => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_colour_this_panel_draws_comes_from_its_palette() {
        for dark in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut pal = Palette::for_mode(dark);
                pal.accent = accent;
                for (what, ui) in every_state() {
                    let cmds = ui.render(&pal, 0.0, 0.0, 500.0);
                    // A switch knob is `readable_on` its own track — one of the
                    // two extremes, not a role. The tracks are named rather
                    // than the extremes, so the exemption stays tied to the
                    // fill it sits on.
                    assert_drawn_from(
                        &pal,
                        &cmds,
                        &[
                            appearance::readable_on(pal.green),
                            appearance::readable_on(pal.surface1),
                        ],
                        &format!("{what} (dark={dark})"),
                    );
                }
            }
        }
    }

    #[test]
    fn the_fixtures_take_every_branch_this_panel_has() {
        let p = mocha();
        let all: Vec<Vec<RenderCommand>> = every_state()
            .into_iter()
            .map(|(_, ui)| ui.render(&p, 0.0, 0.0, 500.0))
            .collect();
        let says = |needle: &str| {
            all.iter()
                .any(|c| texts_of(c).iter().any(|(t, _, _)| t.contains(needle)))
        };

        // Permissions tab: both arms, and both sub-arms of the detail view.
        assert!(
            says("No apps have requested"),
            "detail-with-no-apps is never rendered"
        );
        assert!(says("Allowed"), "an allowed app row is never rendered");
        assert!(says("Denied"), "a denied app row is never rendered");
        assert!(
            says("Not decided"),
            "an undecided app row is never rendered"
        );
        // The overview status line is three-way.
        assert!(says("Disabled"), "the disabled status is never rendered");
        assert!(
            says("apps allowed"),
            "the allowed-count status is never rendered"
        );
        assert!(says("No apps"), "the no-apps status is never rendered");
        // Activity tab: both arms, and both event outcomes.
        assert!(
            says("No activity recorded"),
            "the empty log is never rendered"
        );
        assert!(
            says("recent access events"),
            "a populated log is never rendered"
        );
        assert!(
            says("\u{2713}"),
            "an allowed activity row is never rendered"
        );
        assert!(says("\u{2715}"), "a denied activity row is never rendered");
        // General tab: both radio arms.
        assert!(says("\u{25CF} "), "no telemetry level is ever selected");
        assert!(says("\u{25CB} "), "no telemetry level is ever unselected");
        assert!(says("Telemetry"), "the general tab is never rendered");
    }

    /// The fixture named `name`, or a panic naming what was asked for.
    fn state(name: &str) -> PrivacySettingsUI {
        every_state()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, ui)| ui)
            .unwrap_or_else(|| panic!("no fixture named {name:?}"))
    }

    /// Render the fixture named `name` at a fixed 500px width, so the sizes
    /// the role tables match on are stable: inner width is 468 and a tab is
    /// 154 wide.
    fn draw(name: &str, p: &Palette) -> Vec<RenderCommand> {
        state(name).render(p, 0.0, 0.0, 500.0)
    }

    /// The colour of every text whose content contains `want`. Same
    /// one-site-many-rows contract as `text_starting`.
    fn text_containing(cmds: &[RenderCommand], want: &str) -> Color {
        let hits: Vec<Color> = texts_of(cmds)
            .into_iter()
            .filter(|(t, _, _)| t.contains(want))
            .map(|(_, _, c)| c)
            .collect();
        assert!(!hits.is_empty(), "no text containing {want:?} is drawn");
        assert!(
            hits.iter().all(|c| rgb(*c) == rgb(hits[0])),
            "the {} texts containing {want:?} are not all one colour",
            hits.len()
        );
        hits[0]
    }

    /// The colour of every text whose content is exactly `want`.
    ///
    /// The tab labels need this rather than `text_containing`: "Permissions"
    /// is also a substring of the panel title, and a substring match would
    /// silently compare the wrong site.
    fn text_exact(cmds: &[RenderCommand], want: &str) -> Color {
        let hits: Vec<Color> = texts_of(cmds)
            .into_iter()
            .filter(|(t, _, _)| t == want)
            .map(|(_, _, c)| c)
            .collect();
        assert!(!hits.is_empty(), "no text exactly {want:?} is drawn");
        assert!(
            hits.iter().all(|c| rgb(*c) == rgb(hits[0])),
            "the {} texts exactly {want:?} are not all one colour",
            hits.len()
        );
        hits[0]
    }

    /// The palettes the role tables assert against.
    ///
    /// Both modes, and in each an accent that is deliberately **not** a member
    /// of either palette. Rendering only Mocha with the stock accent leaves a
    /// table blind to the two mistakes this conversion is most likely to make:
    /// a constant frozen back to its Mocha value is identical to the role that
    /// replaced it *when viewed in Mocha*, and a site naming `p.blue` instead
    /// of following the accent is identical to the stock accent, which is
    /// blue. Both then fall through to the membership sweep, which reports
    /// "some colour is not in the palette" rather than naming the site.
    fn table_palettes() -> Vec<(String, Palette)> {
        [false, true]
            .into_iter()
            .map(|light| {
                let mut p = Palette::for_mode(light);
                p.accent = Color::from_hex(0x00FF_8C1A);
                (format!("light={light}"), p)
            })
            .collect()
    }

    #[test]
    fn every_text_this_panel_draws_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            // ONE ENTRY PER SOURCE SITE. Not one per kind of site, and not one per
            // rendered row -- a loop that draws ten app rows from a single
            // `color:` expression is ONE site, but two expressions that happen to
            // name the same role are TWO. Shortening this table by grouping is how
            // modules 21 and 22 lost five defects between them; do not do it.
            let over = draw("overview: disabled, allowed, empty", &p);
            let bare = draw("detail: no apps", &p);
            let full = draw("detail: one app per state, enabled", &p);
            let none = draw("activity: empty", &p);
            let act = draw("activity: allowed and denied", &p);
            let tab2 = draw("general: level 0, toggles true", &p);

            let kind0 = PermissionKind::ALL[0];

            // render
            assert_eq!(
                rgb(text_containing(&over, "Privacy & Permissions")),
                rgb(p.text),
                "{mode}"
            );
            // render_permissions_tab, detail arm
            assert_eq!(
                rgb(text_containing(&bare, kind0.label())),
                rgb(p.lavender),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&bare, kind0.description())),
                rgb(p.subtext0),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&bare, "No apps have requested")),
                rgb(p.overlay0),
                "{mode}"
            );
            assert_eq!(rgb(text_containing(&full, "App 0")), rgb(p.text), "{mode}");
            assert_eq!(
                rgb(text_containing(&full, "0\u{d7}")),
                rgb(p.overlay0),
                "{mode}"
            );
            // The app-state label defers to `PermissionState::color`. Asserting
            // that method in isolation (as the choice table does) does not prove
            // this call site still asks it -- swapping `app.state.color(p)` for a
            // flat `p.text` leaves the method, and that table, untouched.
            //
            // But state the expected colour as the ROLE, never as
            // `PermissionState::X.color(&p)`. Writing the expectation in terms
            // of the code under test makes the assertion tautological: change
            // the `Allowed => p.green` arm and both sides of the comparison
            // move together, so the row that a user reads as "allowed" can turn
            // any colour at all and this table stays green. That is what let
            // two arm-swap defects through the first time. The role literal
            // pins the value; the three-state spread below still proves the
            // call site asks the method, because a flat literal at the call
            // site would give all three rows the same colour.
            assert_eq!(
                rgb(text_containing(&full, PermissionState::Allowed.label())),
                rgb(p.green),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&full, PermissionState::Denied.label())),
                rgb(p.red),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&full, PermissionState::NotDecided.label())),
                rgb(p.overlay0),
                "{mode}"
            );
            // render_permissions_tab, overview arm
            assert_eq!(
                rgb(text_containing(&over, PermissionKind::Camera.label())),
                rgb(p.text),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&over, PermissionKind::Camera.description())),
                rgb(p.overlay0),
                "{mode}"
            );
            // render_activity_tab
            assert_eq!(
                rgb(text_containing(&none, "No activity recorded")),
                rgb(p.overlay0),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&act, "recent access events")),
                rgb(p.text),
                "{mode}"
            );
            // render_general_tab
            assert_eq!(
                rgb(text_containing(&tab2, "Telemetry")),
                rgb(p.lavender),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&tab2, "Other")),
                rgb(p.lavender),
                "{mode}"
            );
            // render_toggle
            assert_eq!(
                rgb(text_containing(&tab2, "Prompt on first access")),
                rgb(p.subtext0),
                "{mode}"
            );
        }
    }

    #[test]
    fn every_rectangle_this_panel_draws_is_in_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            // ONE ENTRY PER SOURCE SITE -- see the note on the text table above.
            //
            // Note that the activity row and the telemetry row are both 468x28.
            // They are told apart by fixture, not by size: they live on different
            // tabs and are never in one render.
            let over = draw("overview: disabled, allowed, empty", &p);
            let full = draw("detail: one app per state, enabled", &p);
            let act = draw("activity: allowed and denied", &p);
            let tab2 = draw("general: level 0, toggles true", &p);

            // render: the panel background.
            assert_eq!(rgb(fills(&over, 500.0, 900.0)[0]), rgb(p.base), "{mode}");
            // render_permissions_tab: an app row.
            assert!(
                fills(&full, 468.0, 32.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(p.mantle)),
                "{mode}"
            );
            // render_permissions_tab: an overview row.
            assert!(
                fills(&over, 468.0, 40.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(p.mantle)),
                "{mode}"
            );
            // render_activity_tab: a log row.
            assert!(
                fills(&act, 468.0, 28.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(p.mantle)),
                "{mode}"
            );
            // render_toggle: the knob, which is `readable_on` its own pill
            // rather than a role. This fixture has every toggle *on*, so every
            // knob here rides `green`; the off ink is a different value and is
            // pinned by the switch module's own tests.
            assert!(
                fills(&tab2, 16.0, 16.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(appearance::readable_on(p.green))),
                "{mode}"
            );
        }
    }

    #[test]
    fn every_choice_this_panel_makes_hands_over_the_role_it_claims() {
        for (mode, p) in table_palettes() {
            // The sweep cannot see any of these and neither role table can reach
            // them: every arm names a role, and a role is a member of BOTH
            // palettes, so swapping one arm for another is invisible to a
            // membership check. A per-source-site table does not help either --
            // these sites choose a colour rather than drawing one, and the table
            // only ever sees whichever arm its fixture happened to take.
            //
            // So: one assertion per ARM, both sides of every choice.

            // PermissionState::color -- three arms.
            assert_eq!(
                rgb(PermissionState::Allowed.color(&p)),
                rgb(p.green),
                "{mode}"
            );
            assert_eq!(rgb(PermissionState::Denied.color(&p)), rgb(p.red), "{mode}");
            assert_eq!(
                rgb(PermissionState::NotDecided.color(&p)),
                rgb(p.overlay0),
                "{mode}"
            );

            // render: the tab strip, fill and label, selected and not.
            let over = draw("overview: disabled, allowed, empty", &p);
            let tabs = fills(&over, 154.0, 30.0);
            assert_eq!(
                rgb(tabs[0]),
                rgb(p.surface0),
                "the selected tab's fill ({mode})"
            );
            assert_eq!(
                rgb(tabs[1]),
                rgb(p.mantle),
                "an unselected tab's fill ({mode})"
            );
            assert_eq!(
                rgb(text_exact(&over, "Permissions")),
                rgb(p.accent),
                "{mode}"
            );
            assert_eq!(
                rgb(text_exact(&over, "Activity")),
                rgb(p.subtext0),
                "{mode}"
            );

            // render_permissions_tab: the overview status line, all three ways.
            assert_eq!(
                rgb(text_containing(&over, "Disabled")),
                rgb(p.red),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&over, "apps allowed")),
                rgb(p.green),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&over, "No apps")),
                rgb(p.green),
                "{mode}"
            );

            // render_activity_tab: an allowed event and a denied one.
            let act = draw("activity: allowed and denied", &p);
            assert_eq!(
                rgb(text_containing(&act, "\u{2713}")),
                rgb(p.green),
                "{mode}"
            );
            assert_eq!(rgb(text_containing(&act, "\u{2715}")), rgb(p.red), "{mode}");

            // render_general_tab: the radio row, fill and label, both ways.
            let tab2 = draw("general: level 0, toggles true", &p);
            let rows = fills(&tab2, 468.0, 28.0);
            assert_eq!(
                rgb(rows[0]),
                rgb(p.surface0),
                "the selected level's row ({mode})"
            );
            assert_eq!(
                rgb(rows[1]),
                rgb(p.mantle),
                "an unselected level's row ({mode})"
            );
            assert_eq!(
                rgb(text_containing(&tab2, "\u{25CF} ")),
                rgb(p.accent),
                "{mode}"
            );
            assert_eq!(
                rgb(text_containing(&tab2, "\u{25CB} ")),
                rgb(p.text),
                "{mode}"
            );

            // render_toggle: the pill, both ways.
            let off = draw("general: level 0, toggles false", &p);
            assert!(
                fills(&tab2, 40.0, 20.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(p.green)),
                "{mode}"
            );
            assert!(
                fills(&off, 40.0, 20.0)
                    .iter()
                    .all(|c| rgb(*c) == rgb(p.surface1)),
                "{mode}"
            );
        }
    }

    #[test]
    fn only_the_two_selection_labels_follow_the_accent() {
        // A COUNT, not a list. If a fourth site starts taking the accent this
        // fails rather than passing unnoticed, which is the whole point --
        // "the accent marks position and invitation" is a claim about the
        // sites that DON'T take it as much as the ones that do.
        //
        // Tab 2 is chosen because it draws both accent sites at once: its own
        // tab label, and the selected telemetry level.
        for accent in SAFE_ACCENTS {
            let mut p = mocha();
            p.accent = accent;
            let cmds = draw("general: level 0, toggles true", &p);
            let n = every_color(&cmds)
                .into_iter()
                .filter(|c| rgb(*c) == rgb(accent))
                .count();
            assert_eq!(
                n, 2,
                "the general tab should take the accent exactly twice \
                 (its tab label and the selected telemetry level), not {n} times"
            );
        }
    }

    #[test]
    fn nothing_but_the_selection_labels_moves_when_the_accent_does() {
        // Render each fixture under two accents, require every command to draw
        // the same colour in both, and require the commands that DO move to be
        // exactly as many as this panel is allowed to have.
        //
        // The count is the load-bearing half, and two earlier attempts died
        // without it. The first collected every colour equal to green, red or
        // overlay0 and compared *that* across accents -- filtering by role
        // before comparing throws away exactly the colour that moved, so a
        // site that stops drawing green and starts drawing the accent drops
        // out of both lists and they still match. The second compared every
        // command but skipped any site drawing accent A in the first render
        // and accent B in the second, "because that is a site meant to follow
        // the accent". That is the same hole: a toggle that wrongly reports
        // its state in the accent follows the accent too, so it was skipped
        // as well. Recognising accent sites by the fact that they follow the
        // accent can never tell a legitimate one from a new one. Counting can.
        //
        // Both accents are deliberately outside either palette, so a frozen
        // role can never be mistaken for an accent site.
        const A: Color = Color::from_hex(0x00FF_8C1A);
        const B: Color = Color::from_hex(0x0012_9E7D);

        for light in [false, true] {
            let (mut pa, mut pb) = (Palette::for_mode(light), Palette::for_mode(light));
            pa.accent = A;
            pb.accent = B;
            for (what, ui) in every_state() {
                let ca = ui.render(&pa, 0.0, 0.0, 500.0);
                let cb = ui.render(&pb, 0.0, 0.0, 500.0);
                assert_eq!(
                    ca.len(),
                    cb.len(),
                    "{what}: the accent changed how much is drawn"
                );
                // The tab strip always draws exactly one active tab label in
                // the accent. The general tab adds one more: the selected
                // telemetry level. Nothing else in this panel may move --
                // every permission state, every log row and every toggle
                // reports a fact, and a fact does not follow a preference.
                let want = if what.starts_with("general") { 2 } else { 1 };
                let mut moved = 0;
                let (xa, xb) = (every_color(&ca), every_color(&cb));
                for (i, (a, b)) in xa.iter().zip(xb.iter()).enumerate() {
                    if rgb(*a) == rgb(*b) {
                        continue;
                    }
                    assert_eq!(
                        (rgb(*a), rgb(*b)),
                        (rgb(A), rgb(B)),
                        "{what} (light={light}): command {i} changed with the \
                         accent without being the accent"
                    );
                    moved += 1;
                }
                assert_eq!(
                    moved, want,
                    "{what} (light={light}): {moved} commands follow the accent, \
                     but this state is allowed exactly {want}"
                );
            }
        }
    }

    #[test]
    fn allowed_and_denied_stay_apart_under_every_accent_and_mode() {
        // The pair is what carries the meaning, so neither half may collide
        // with the other or with the accent that happens to be set. A green
        // desktop must not make "denied" look allowed.
        for dark in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(dark);
                p.accent = accent;
                let allow = PermissionState::Allowed.color(&p);
                let deny = PermissionState::Denied.color(&p);
                let undecided = PermissionState::NotDecided.color(&p);
                assert_ne!(
                    rgb(allow),
                    rgb(deny),
                    "allowed and denied collide (dark={dark})"
                );
                assert_ne!(rgb(allow), rgb(undecided), "allowed and undecided collide");
                assert_ne!(rgb(deny), rgb(undecided), "denied and undecided collide");
                assert_ne!(
                    rgb(allow),
                    rgb(accent),
                    "allowed collides with the accent (dark={dark})"
                );
                assert_ne!(rgb(deny), rgb(accent), "denied collides with the accent");
            }
        }
    }
}
