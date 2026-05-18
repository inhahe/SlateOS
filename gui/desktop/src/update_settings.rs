//! System update settings panel.
//!
//! Manages OS and application updates: checking for updates, scheduling
//! automatic updates, update history, rollback, and active hours during
//! which the system should not restart.

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
// Update status
// ============================================================================

/// Current system update status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// System is up to date.
    UpToDate,
    /// Checking for updates.
    Checking,
    /// Updates available but not yet downloaded.
    Available,
    /// Downloading updates.
    Downloading,
    /// Downloaded, waiting for restart to install.
    PendingRestart,
    /// Error occurred during check/download.
    Error,
}

impl UpdateStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "Your system is up to date",
            Self::Checking => "Checking for updates…",
            Self::Available => "Updates available",
            Self::Downloading => "Downloading updates…",
            Self::PendingRestart => "Restart to finish installing updates",
            Self::Error => "Error checking for updates",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::UpToDate => GREEN,
            Self::Checking | Self::Downloading => BLUE,
            Self::Available => YELLOW,
            Self::PendingRestart => PEACH,
            Self::Error => RED,
        }
    }
}

// ============================================================================
// Update kind
// ============================================================================

/// Category of update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateKind {
    /// Operating system core update.
    System,
    /// Security patch.
    Security,
    /// Application update.
    Application,
    /// Driver update.
    Driver,
    /// Feature update (major version).
    Feature,
}

impl UpdateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Security => "Security",
            Self::Application => "Application",
            Self::Driver => "Driver",
            Self::Feature => "Feature update",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::System => "🔧",
            Self::Security => "🛡",
            Self::Application => "📦",
            Self::Driver => "🔌",
            Self::Feature => "🆕",
        }
    }
}

// ============================================================================
// Available update
// ============================================================================

/// An individual available update.
#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: UpdateKind,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Version string.
    pub version: String,
    /// Whether this update requires a restart.
    pub requires_restart: bool,
    /// Whether the user has selected this for installation.
    pub selected: bool,
}

impl AvailableUpdate {
    pub fn new(id: &str, title: &str, kind: UpdateKind, size: u64, version: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            kind,
            size_bytes: size,
            version: version.into(),
            requires_restart: kind == UpdateKind::System || kind == UpdateKind::Security || kind == UpdateKind::Feature,
            selected: true,
        }
    }
}

// ============================================================================
// Update history entry
// ============================================================================

/// Record of a past update.
#[derive(Clone, Debug)]
pub struct UpdateHistoryEntry {
    pub title: String,
    pub kind: UpdateKind,
    pub version: String,
    /// Timestamp (seconds since epoch) when installed.
    pub installed_at_secs: u64,
    /// Whether installation succeeded.
    pub success: bool,
    /// Error message if failed.
    pub error_msg: Option<String>,
    /// Whether this update can be rolled back.
    pub rollback_available: bool,
}

// ============================================================================
// Schedule
// ============================================================================

/// When to automatically install updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateSchedule {
    /// Install immediately when available.
    Automatic,
    /// Download automatically but ask before installing.
    DownloadOnly,
    /// Only check but don't download or install.
    NotifyOnly,
    /// Completely manual.
    Manual,
}

impl UpdateSchedule {
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic — install when available",
            Self::DownloadOnly => "Download automatically, ask before installing",
            Self::NotifyOnly => "Notify only — don't download",
            Self::Manual => "Manual — check only when I ask",
        }
    }

    pub const ALL: [Self; 4] = [Self::Automatic, Self::DownloadOnly, Self::NotifyOnly, Self::Manual];
}

// ============================================================================
// Update settings
// ============================================================================

/// Full update configuration.
#[derive(Clone, Debug)]
pub struct UpdateConfig {
    /// Update schedule.
    pub schedule: UpdateSchedule,
    /// Active hours start (0–23).
    pub active_hours_start: u32,
    /// Active hours end (0–23).
    pub active_hours_end: u32,
    /// Whether to include driver updates.
    pub include_drivers: bool,
    /// Whether to include optional feature updates.
    pub include_features: bool,
    /// Whether to defer feature updates (weeks).
    pub defer_features_weeks: u32,
    /// Whether to defer security updates (days, max 7).
    pub defer_security_days: u32,
    /// Whether to auto-restart outside active hours.
    pub auto_restart: bool,
    /// Pause updates until a certain date (seconds since epoch, 0 = not paused).
    pub paused_until_secs: u64,
    /// Metered connection: defer downloads.
    pub defer_on_metered: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            schedule: UpdateSchedule::Automatic,
            active_hours_start: 8,
            active_hours_end: 23,
            include_drivers: true,
            include_features: true,
            defer_features_weeks: 0,
            defer_security_days: 0,
            auto_restart: true,
            paused_until_secs: 0,
            defer_on_metered: true,
        }
    }
}

impl UpdateConfig {
    pub fn set_active_hours(&mut self, start: u32, end: u32) {
        self.active_hours_start = start.min(23);
        self.active_hours_end = end.min(23);
    }

    pub fn set_defer_features(&mut self, weeks: u32) {
        self.defer_features_weeks = weeks.min(52);
    }

    pub fn set_defer_security(&mut self, days: u32) {
        self.defer_security_days = days.min(7);
    }

    pub fn is_paused(&self) -> bool {
        self.paused_until_secs > 0
    }
}

// ============================================================================
// Update manager
// ============================================================================

/// Central update settings state.
pub struct UpdateSettings {
    pub config: UpdateConfig,
    pub status: UpdateStatus,
    available: Vec<AvailableUpdate>,
    history: Vec<UpdateHistoryEntry>,
    /// Last check timestamp.
    pub last_check_secs: u64,
    /// Download progress percentage (0–100).
    pub download_progress: u32,
    /// Current OS version.
    pub os_version: String,
    /// Current OS build number.
    pub os_build: String,
}

impl UpdateSettings {
    pub fn new() -> Self {
        Self {
            config: UpdateConfig::default(),
            status: UpdateStatus::UpToDate,
            available: Vec::new(),
            history: Vec::new(),
            last_check_secs: 0,
            download_progress: 0,
            os_version: "0.1.0".into(),
            os_build: "2026.05.18".into(),
        }
    }

    pub fn add_available(&mut self, update: AvailableUpdate) {
        self.available.push(update);
        self.status = UpdateStatus::Available;
    }

    pub fn available_updates(&self) -> &[AvailableUpdate] {
        &self.available
    }

    pub fn selected_count(&self) -> usize {
        self.available.iter().filter(|u| u.selected).count()
    }

    pub fn selected_size(&self) -> u64 {
        self.available.iter().filter(|u| u.selected).map(|u| u.size_bytes).sum()
    }

    pub fn toggle_selection(&mut self, id: &str) {
        if let Some(u) = self.available.iter_mut().find(|u| u.id == id) {
            u.selected = !u.selected;
        }
    }

    pub fn select_all(&mut self) {
        for u in &mut self.available {
            u.selected = true;
        }
    }

    pub fn deselect_all(&mut self) {
        for u in &mut self.available {
            u.selected = false;
        }
    }

    pub fn clear_available(&mut self) {
        self.available.clear();
        self.status = UpdateStatus::UpToDate;
    }

    pub fn add_history(&mut self, entry: UpdateHistoryEntry) {
        self.history.push(entry);
    }

    pub fn history(&self) -> &[UpdateHistoryEntry] {
        &self.history
    }

    pub fn rollback_available_count(&self) -> usize {
        self.history.iter().filter(|h| h.rollback_available).count()
    }

    pub fn any_requires_restart(&self) -> bool {
        self.available.iter().any(|u| u.selected && u.requires_restart)
    }
}

// ============================================================================
// UI rendering
// ============================================================================

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// UI state for update settings.
pub struct UpdateSettingsUI {
    settings: UpdateSettings,
    /// 0=Status, 1=Schedule, 2=History.
    active_tab: usize,
}

impl UpdateSettingsUI {
    pub fn new() -> Self {
        Self {
            settings: UpdateSettings::new(),
            active_tab: 0,
        }
    }

    pub fn settings(&self) -> &UpdateSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut UpdateSettings {
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

    const TAB_LABELS: [&'static str; 3] = ["Status", "Schedule", "History"];

    pub fn render(&self, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        cmds.push(RenderCommand::FillRect {
            x, y, width, height: 800.0,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: "System Updates".into(),
            font_size: 20.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 28.0;

        // OS version
        cmds.push(RenderCommand::Text {
            x: x + pad, y: cy,
            text: format!("Version {} (Build {})", self.settings.os_version, self.settings.os_build),
            font_size: 12.0, color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
        });
        cy += 22.0;

        // Status banner
        let status = self.settings.status;
        cmds.push(RenderCommand::FillRect {
            x: x + pad, y: cy, width: inner, height: 36.0,
            color: MANTLE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 12.0, y: cy + 10.0,
            text: status.label().into(),
            font_size: 14.0, color: status.color(),
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner - 24.0),
        });
        cy += 44.0;

        // Pause warning
        if self.settings.config.is_paused() {
            cmds.push(RenderCommand::Text {
                x: x + pad, y: cy,
                text: "⏸ Updates are paused".into(),
                font_size: 12.0, color: YELLOW,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner),
            });
            cy += 20.0;
        }

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
            0 => self.render_status_tab(&mut cmds, x + pad, cy, inner),
            1 => self.render_schedule_tab(&mut cmds, x + pad, cy, inner),
            2 => self.render_history_tab(&mut cmds, x + pad, cy, inner),
            _ => {}
        };

        cmds
    }

    fn render_status_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        if self.settings.available.is_empty() {
            cmds.push(RenderCommand::Text {
                x, y,
                text: "No updates available.".into(),
                font_size: 13.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x, y,
            text: format!("{} updates available ({} selected, {})",
                self.settings.available.len(),
                self.settings.selected_count(),
                format_size(self.settings.selected_size())),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        for upd in &self.settings.available {
            let bg = if upd.selected { SURFACE0 } else { MANTLE };
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 44.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            let check = if upd.selected { "☑" } else { "☐" };
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 4.0,
                text: format!("{} {} {} v{}", check, upd.kind.icon(), upd.title, upd.version),
                font_size: 13.0, color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
            });
            let restart_tag = if upd.requires_restart { " (restart required)" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + 28.0, y: y + 24.0,
                text: format!("{} — {}{}", upd.kind.label(), format_size(upd.size_bytes), restart_tag),
                font_size: 11.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 36.0),
            });
            y += 50.0;
        }

        if self.settings.any_requires_restart() {
            y += 4.0;
            cmds.push(RenderCommand::Text {
                x, y,
                text: "⚠ Some updates require a restart to complete".into(),
                font_size: 12.0, color: PEACH,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
        }
    }

    fn render_schedule_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        let cfg = &self.settings.config;

        cmds.push(RenderCommand::Text {
            x, y,
            text: "Update schedule".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        for sched in UpdateSchedule::ALL {
            let active = cfg.schedule == sched;
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 28.0,
                color: if active { SURFACE0 } else { MANTLE },
                corner_radii: CornerRadii::all(4.0),
            });
            let indicator = if active { "● " } else { "○ " };
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 6.0,
                text: format!("{}{}", indicator, sched.label()),
                font_size: 13.0,
                color: if active { BLUE } else { TEXT },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
            });
            y += 32.0;
        }

        y += 8.0;
        Self::render_kv(cmds, x, y, width, "Active hours",
            &format!("{:02}:00 — {:02}:00", cfg.active_hours_start, cfg.active_hours_end));
        y += 24.0;
        Self::render_toggle(cmds, x, y, width, "Auto-restart outside active hours", cfg.auto_restart);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Include driver updates", cfg.include_drivers);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Include feature updates", cfg.include_features);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Defer on metered connections", cfg.defer_on_metered);
        y += 28.0;

        if cfg.defer_features_weeks > 0 {
            Self::render_kv(cmds, x, y, width, "Feature update deferral",
                &format!("{} weeks", cfg.defer_features_weeks));
            y += 24.0;
        }
        if cfg.defer_security_days > 0 {
            Self::render_kv(cmds, x, y, width, "Security update deferral",
                &format!("{} days", cfg.defer_security_days));
        }
    }

    fn render_history_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        if self.settings.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x, y,
                text: "No update history.".into(),
                font_size: 13.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            return;
        }

        let rollbacks = self.settings.rollback_available_count();
        if rollbacks > 0 {
            cmds.push(RenderCommand::Text {
                x, y,
                text: format!("{} updates can be rolled back", rollbacks),
                font_size: 12.0, color: LAVENDER,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
            });
            y += 20.0;
        }

        for entry in self.settings.history.iter().rev().take(20) {
            let status_icon = if entry.success { "✓" } else { "✕" };
            let color = if entry.success { GREEN } else { RED };
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 32.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 8.0, y: y + 4.0,
                text: format!("{} {} {} v{}", status_icon, entry.kind.icon(), entry.title, entry.version),
                font_size: 12.0, color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.7),
            });
            let rollback_tag = if entry.rollback_available { "↩ rollback" } else { "" };
            cmds.push(RenderCommand::Text {
                x: x + width * 0.75, y: y + 4.0,
                text: rollback_tag.into(),
                font_size: 11.0, color: LAVENDER,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.25),
            });
            if let Some(err) = &entry.error_msg {
                cmds.push(RenderCommand::Text {
                    x: x + 28.0, y: y + 18.0,
                    text: err.clone(),
                    font_size: 10.0, color: RED,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 36.0),
                });
            }
            y += 38.0;
        }
    }

    fn render_kv(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, key: &str, val: &str) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y, text: key.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55, y, text: val.into(),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
    }

    fn render_toggle(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, label: &str, on: bool) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y, text: label.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.65),
        });
        let tx = x + width - 48.0;
        let bg = if on { GREEN } else { SURFACE1 };
        cmds.push(RenderCommand::FillRect {
            x: tx, y, width: 40.0, height: 20.0,
            color: bg, corner_radii: CornerRadii::all(10.0),
        });
        let knob_x = if on { tx + 22.0 } else { tx + 2.0 };
        cmds.push(RenderCommand::FillRect {
            x: knob_x, y: y + 2.0, width: 16.0, height: 16.0,
            color: TEXT, corner_radii: CornerRadii::all(8.0),
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
    fn update_status_labels() {
        for s in [UpdateStatus::UpToDate, UpdateStatus::Checking, UpdateStatus::Available,
                  UpdateStatus::Downloading, UpdateStatus::PendingRestart, UpdateStatus::Error] {
            assert!(!s.label().is_empty());
            let _ = s.color();
        }
    }

    #[test]
    fn update_kind_labels() {
        for k in [UpdateKind::System, UpdateKind::Security, UpdateKind::Application,
                  UpdateKind::Driver, UpdateKind::Feature] {
            assert!(!k.label().is_empty());
            assert!(!k.icon().is_empty());
        }
    }

    #[test]
    fn update_schedule_labels() {
        for s in UpdateSchedule::ALL {
            assert!(!s.label().is_empty());
        }
    }

    #[test]
    fn default_config() {
        let c = UpdateConfig::default();
        assert_eq!(c.schedule, UpdateSchedule::Automatic);
        assert!(c.auto_restart);
        assert!(c.include_drivers);
        assert!(!c.is_paused());
    }

    #[test]
    fn active_hours_clamped() {
        let mut c = UpdateConfig::default();
        c.set_active_hours(25, 30);
        assert_eq!(c.active_hours_start, 23);
        assert_eq!(c.active_hours_end, 23);
    }

    #[test]
    fn defer_features_clamped() {
        let mut c = UpdateConfig::default();
        c.set_defer_features(100);
        assert_eq!(c.defer_features_weeks, 52);
    }

    #[test]
    fn defer_security_clamped() {
        let mut c = UpdateConfig::default();
        c.set_defer_security(30);
        assert_eq!(c.defer_security_days, 7);
    }

    #[test]
    fn paused_state() {
        let mut c = UpdateConfig::default();
        assert!(!c.is_paused());
        c.paused_until_secs = 12345;
        assert!(c.is_paused());
    }

    #[test]
    fn add_available_update() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "Patch", UpdateKind::Security, 5_000_000, "1.0.1"));
        assert_eq!(s.available_updates().len(), 1);
        assert_eq!(s.status, UpdateStatus::Available);
    }

    #[test]
    fn selected_count_and_size() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "A", UpdateKind::System, 1000, "1.0"));
        s.add_available(AvailableUpdate::new("u2", "B", UpdateKind::Application, 2000, "2.0"));
        assert_eq!(s.selected_count(), 2);
        assert_eq!(s.selected_size(), 3000);
    }

    #[test]
    fn toggle_selection() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "A", UpdateKind::System, 1000, "1.0"));
        s.toggle_selection("u1");
        assert_eq!(s.selected_count(), 0);
        s.toggle_selection("u1");
        assert_eq!(s.selected_count(), 1);
    }

    #[test]
    fn select_deselect_all() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "A", UpdateKind::System, 1000, "1.0"));
        s.add_available(AvailableUpdate::new("u2", "B", UpdateKind::Driver, 2000, "1.0"));
        s.deselect_all();
        assert_eq!(s.selected_count(), 0);
        s.select_all();
        assert_eq!(s.selected_count(), 2);
    }

    #[test]
    fn clear_available() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "A", UpdateKind::System, 1000, "1.0"));
        s.clear_available();
        assert!(s.available_updates().is_empty());
        assert_eq!(s.status, UpdateStatus::UpToDate);
    }

    #[test]
    fn history() {
        let mut s = UpdateSettings::new();
        s.add_history(UpdateHistoryEntry {
            title: "Patch".into(),
            kind: UpdateKind::Security,
            version: "1.0.1".into(),
            installed_at_secs: 1000,
            success: true,
            error_msg: None,
            rollback_available: true,
        });
        assert_eq!(s.history().len(), 1);
        assert_eq!(s.rollback_available_count(), 1);
    }

    #[test]
    fn any_requires_restart() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new("u1", "App", UpdateKind::Application, 1000, "1.0"));
        assert!(!s.any_requires_restart()); // Application doesn't require restart
        s.add_available(AvailableUpdate::new("u2", "Sys", UpdateKind::System, 1000, "1.0"));
        assert!(s.any_requires_restart());
    }

    #[test]
    fn ui_new() {
        let ui = UpdateSettingsUI::new();
        assert_eq!(ui.active_tab(), 0);
    }

    #[test]
    fn ui_set_tab() {
        let mut ui = UpdateSettingsUI::new();
        ui.set_active_tab(2);
        assert_eq!(ui.active_tab(), 2);
        ui.set_active_tab(99);
        assert_eq!(ui.active_tab(), 2);
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = UpdateSettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = UpdateSettingsUI::new();
        for i in 0..3 {
            ui.set_active_tab(i);
            let cmds = ui.render(0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_with_updates() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().add_available(AvailableUpdate::new("u1", "Security Patch", UpdateKind::Security, 5_000_000, "1.0.1"));
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_update = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Security Patch")));
        assert!(has_update);
    }

    #[test]
    fn ui_render_paused() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().config.paused_until_secs = 99999;
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_paused = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("paused")));
        assert!(has_paused);
    }

    #[test]
    fn ui_render_history() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().add_history(UpdateHistoryEntry {
            title: "Old Patch".into(), kind: UpdateKind::System,
            version: "0.9.1".into(), installed_at_secs: 500, success: true,
            error_msg: None, rollback_available: false,
        });
        ui.set_active_tab(2);
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_hist = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Old Patch")));
        assert!(has_hist);
    }

    #[test]
    fn format_size_units() {
        assert!(format_size(500).contains("B"));
        assert!(format_size(5_000_000).contains("MB"));
    }

    #[test]
    fn available_update_requires_restart() {
        let u = AvailableUpdate::new("u1", "Test", UpdateKind::System, 100, "1.0");
        assert!(u.requires_restart);
        let u2 = AvailableUpdate::new("u2", "App", UpdateKind::Application, 100, "1.0");
        assert!(!u2.requires_restart);
    }
}
