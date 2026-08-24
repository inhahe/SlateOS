//! System update settings panel.
//!
//! Manages OS and application updates: checking for updates, scheduling
//! automatic updates, update history, rollback, and active hours during
//! which the system should not restart.

use appearance::Palette;
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Thirteen Mocha constants used to live here. They are gone; every colour below
// is a role of the [`Palette`] the caller resolved.
//
// **Two accent sites, and they are the same site twice: "you are here".** The
// active tab's label and the selected schedule's label both mark the one
// option out of a row that the user is currently on, which is what the accent
// is for everywhere else in the shell (`notif_pane.rs` states the doctrine).
// Both were `BLUE`, and blue being the default accent is why they look like
// they were never converted; the test is what says otherwise.
//
// **`UpdateStatus::color` stays categorical, and it is the widest such row in
// the shell so far** — up-to-date green, checking/downloading blue, available
// yellow, pending-restart peach, error red. Five kinds of *fact about the
// system*, not five degrees of interactivity. The blue is the usual trap and
// its four siblings settle it: an accent that moved "Downloading updates…" and
// left "Error checking for updates" alone would be saying the two differ in
// importance rather than in kind. Same for the history ledger's green tick and
// red cross, and for the peach "some updates require a restart" warning.
//
// **The lavender captions are not a role decision so much as a faithful
// copy.** "Update schedule", "N updates can be rolled back" and the "↩
// rollback" tag were all lavender; lavender is a palette role in both modes,
// so they convert directly and keep their meaning as a quiet secondary accent
// the user did not choose.
//
// The "on" toggle stays green rather than becoming the accent — see
// `known-issues.md`; the shell disagrees with itself about which an on-switch
// uses, and a conversion is not the place to settle it.

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

    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::UpToDate => p.green,
            Self::Checking | Self::Downloading => p.blue,
            Self::Available => p.yellow,
            Self::PendingRestart => p.peach,
            Self::Error => p.red,
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
            requires_restart: kind == UpdateKind::System
                || kind == UpdateKind::Security
                || kind == UpdateKind::Feature,
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

    pub const ALL: [Self; 4] = [
        Self::Automatic,
        Self::DownloadOnly,
        Self::NotifyOnly,
        Self::Manual,
    ];
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
        self.available
            .iter()
            .filter(|u| u.selected)
            .map(|u| u.size_bytes)
            .sum()
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
        self.available
            .iter()
            .any(|u| u.selected && u.requires_restart)
    }
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// UI rendering
// ============================================================================

fn format_size(bytes: u64) -> String {
    guitk::bytes::si(bytes)
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

    pub fn render(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let pad = 16.0_f32;
        let inner = width - 2.0 * pad;
        let mut cy = y;

        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: 800.0,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        cy += pad;
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: "System Updates".into(),
            font_size: 20.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 28.0;

        // OS version
        cmds.push(RenderCommand::Text {
            x: x + pad,
            y: cy,
            text: format!(
                "Version {} (Build {})",
                self.settings.os_version, self.settings.os_build
            ),
            font_size: 12.0,
            color: p.overlay0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(inner),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 22.0;

        // Status banner
        let status = self.settings.status;
        cmds.push(RenderCommand::FillRect {
            x: x + pad,
            y: cy,
            width: inner,
            height: 36.0,
            color: p.mantle,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + pad + 12.0,
            y: cy + 10.0,
            text: status.label().into(),
            font_size: 14.0,
            color: status.color(p),
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        cy += 44.0;

        // Pause warning
        if self.settings.config.is_paused() {
            cmds.push(RenderCommand::Text {
                x: x + pad,
                y: cy,
                text: "⏸ Updates are paused".into(),
                font_size: 12.0,
                color: p.yellow,
                font_weight: FontWeightHint::Regular,
                max_width: Some(inner),
                overflow: TextOverflow::Ellipsis,
            });
            cy += 20.0;
        }

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
            0 => self.render_status_tab(p, &mut cmds, x + pad, cy, inner),
            1 => self.render_schedule_tab(p, &mut cmds, x + pad, cy, inner),
            2 => self.render_history_tab(p, &mut cmds, x + pad, cy, inner),
            _ => {}
        }

        cmds
    }

    fn render_status_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        if self.settings.available.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No updates available.".into(),
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
            text: format!(
                "{} updates available ({} selected, {})",
                self.settings.available.len(),
                self.settings.selected_count(),
                format_size(self.settings.selected_size())
            ),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        for upd in &self.settings.available {
            let bg = if upd.selected { p.surface0 } else { p.mantle };
            cmds.push(RenderCommand::FillRect {
                x,
                y,
                width,
                height: 44.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });
            let check = if upd.selected { "☑" } else { "☐" };
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: y + 4.0,
                text: format!(
                    "{} {} {} v{}",
                    check,
                    upd.kind.icon(),
                    upd.title,
                    upd.version
                ),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            let restart_tag = if upd.requires_restart {
                " (restart required)"
            } else {
                ""
            };
            cmds.push(RenderCommand::Text {
                x: x + 28.0,
                y: y + 24.0,
                text: format!(
                    "{} — {}{}",
                    upd.kind.label(),
                    format_size(upd.size_bytes),
                    restart_tag
                ),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 36.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 50.0;
        }

        if self.settings.any_requires_restart() {
            y += 4.0;
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "⚠ Some updates require a restart to complete".into(),
                font_size: 12.0,
                color: p.peach,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_schedule_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        let cfg = &self.settings.config;

        cmds.push(RenderCommand::Text {
            x,
            y,
            text: "Update schedule".into(),
            font_size: 14.0,
            color: p.lavender,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
        y += 24.0;

        for sched in UpdateSchedule::ALL {
            let active = cfg.schedule == sched;
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
                text: format!("{}{}", indicator, sched.label()),
                font_size: 13.0,
                color: if active { p.accent } else { p.text },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            y += 32.0;
        }

        y += 8.0;
        Self::render_kv(
            p,
            cmds,
            x,
            y,
            width,
            "Active hours",
            &format!(
                "{:02}:00 — {:02}:00",
                cfg.active_hours_start, cfg.active_hours_end
            ),
        );
        y += 24.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Auto-restart outside active hours",
            cfg.auto_restart,
        );
        y += 28.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Include driver updates",
            cfg.include_drivers,
        );
        y += 28.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Include feature updates",
            cfg.include_features,
        );
        y += 28.0;
        Self::render_toggle(
            p,
            cmds,
            x,
            y,
            width,
            "Defer on metered connections",
            cfg.defer_on_metered,
        );
        y += 28.0;

        if cfg.defer_features_weeks > 0 {
            Self::render_kv(
                p,
                cmds,
                x,
                y,
                width,
                "Feature update deferral",
                &format!("{} weeks", cfg.defer_features_weeks),
            );
            y += 24.0;
        }
        if cfg.defer_security_days > 0 {
            Self::render_kv(
                p,
                cmds,
                x,
                y,
                width,
                "Security update deferral",
                &format!("{} days", cfg.defer_security_days),
            );
        }
    }

    fn render_history_tab(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        mut y: f32,
        width: f32,
    ) {
        if self.settings.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: "No update history.".into(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            return;
        }

        let rollbacks = self.settings.rollback_available_count();
        if rollbacks > 0 {
            cmds.push(RenderCommand::Text {
                x,
                y,
                text: format!("{} updates can be rolled back", rollbacks),
                font_size: 12.0,
                color: p.lavender,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
            y += 20.0;
        }

        for entry in self.settings.history.iter().rev().take(20) {
            let status_icon = if entry.success { "✓" } else { "✕" };
            let color = if entry.success { p.green } else { p.red };
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
                y: y + 4.0,
                text: format!(
                    "{} {} {} v{}",
                    status_icon,
                    entry.kind.icon(),
                    entry.title,
                    entry.version
                ),
                font_size: 12.0,
                color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.7),
                overflow: TextOverflow::Ellipsis,
            });
            let rollback_tag = if entry.rollback_available {
                "↩ rollback"
            } else {
                ""
            };
            cmds.push(RenderCommand::Text {
                x: x + width * 0.75,
                y: y + 4.0,
                text: rollback_tag.into(),
                font_size: 11.0,
                color: p.lavender,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.25),
                overflow: TextOverflow::Ellipsis,
            });
            if let Some(err) = &entry.error_msg {
                cmds.push(RenderCommand::Text {
                    x: x + 28.0,
                    y: y + 18.0,
                    text: err.clone(),
                    font_size: 10.0,
                    color: p.red,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 36.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            y += 38.0;
        }
    }

    fn render_kv(
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        key: &str,
        val: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y,
            text: key.into(),
            font_size: 13.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.5),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.55,
            y,
            text: val.into(),
            font_size: 13.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
            overflow: TextOverflow::Ellipsis,
        });
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

impl Default for UpdateSettingsUI {
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
    use crate::palette_check;

    /// The dark palette, which is what every deleted constant used to hold.
    fn test_palette() -> Palette {
        Palette::for_mode(false)
    }

    #[test]
    fn update_status_labels() {
        for s in [
            UpdateStatus::UpToDate,
            UpdateStatus::Checking,
            UpdateStatus::Available,
            UpdateStatus::Downloading,
            UpdateStatus::PendingRestart,
            UpdateStatus::Error,
        ] {
            assert!(!s.label().is_empty());
            let _ = s.color(&test_palette());
        }
    }

    #[test]
    fn update_kind_labels() {
        for k in [
            UpdateKind::System,
            UpdateKind::Security,
            UpdateKind::Application,
            UpdateKind::Driver,
            UpdateKind::Feature,
        ] {
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
        s.add_available(AvailableUpdate::new(
            "u1",
            "Patch",
            UpdateKind::Security,
            5_000_000,
            "1.0.1",
        ));
        assert_eq!(s.available_updates().len(), 1);
        assert_eq!(s.status, UpdateStatus::Available);
    }

    #[test]
    fn selected_count_and_size() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new(
            "u1",
            "A",
            UpdateKind::System,
            1000,
            "1.0",
        ));
        s.add_available(AvailableUpdate::new(
            "u2",
            "B",
            UpdateKind::Application,
            2000,
            "2.0",
        ));
        assert_eq!(s.selected_count(), 2);
        assert_eq!(s.selected_size(), 3000);
    }

    #[test]
    fn toggle_selection() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new(
            "u1",
            "A",
            UpdateKind::System,
            1000,
            "1.0",
        ));
        s.toggle_selection("u1");
        assert_eq!(s.selected_count(), 0);
        s.toggle_selection("u1");
        assert_eq!(s.selected_count(), 1);
    }

    #[test]
    fn select_deselect_all() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new(
            "u1",
            "A",
            UpdateKind::System,
            1000,
            "1.0",
        ));
        s.add_available(AvailableUpdate::new(
            "u2",
            "B",
            UpdateKind::Driver,
            2000,
            "1.0",
        ));
        s.deselect_all();
        assert_eq!(s.selected_count(), 0);
        s.select_all();
        assert_eq!(s.selected_count(), 2);
    }

    #[test]
    fn clear_available() {
        let mut s = UpdateSettings::new();
        s.add_available(AvailableUpdate::new(
            "u1",
            "A",
            UpdateKind::System,
            1000,
            "1.0",
        ));
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
        s.add_available(AvailableUpdate::new(
            "u1",
            "App",
            UpdateKind::Application,
            1000,
            "1.0",
        ));
        assert!(!s.any_requires_restart()); // Application doesn't require restart
        s.add_available(AvailableUpdate::new(
            "u2",
            "Sys",
            UpdateKind::System,
            1000,
            "1.0",
        ));
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
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = UpdateSettingsUI::new();
        for i in 0..3 {
            ui.set_active_tab(i);
            let cmds = ui.render(&test_palette(), 0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_with_updates() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().add_available(AvailableUpdate::new(
            "u1",
            "Security Patch",
            UpdateKind::Security,
            5_000_000,
            "1.0.1",
        ));
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 500.0);
        let has_update = cmds.iter().any(
            |c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Security Patch")),
        );
        assert!(has_update);
    }

    #[test]
    fn ui_render_paused() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().config.paused_until_secs = 99999;
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 500.0);
        let has_paused = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("paused")));
        assert!(has_paused);
    }

    #[test]
    fn ui_render_history() {
        let mut ui = UpdateSettingsUI::new();
        ui.settings_mut().add_history(UpdateHistoryEntry {
            title: "Old Patch".into(),
            kind: UpdateKind::System,
            version: "0.9.1".into(),
            installed_at_secs: 500,
            success: true,
            error_msg: None,
            rollback_available: false,
        });
        ui.set_active_tab(2);
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 500.0);
        let has_hist = cmds
            .iter()
            .any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Old Patch")));
        assert!(has_hist);
    }

    #[test]
    fn format_size_units() {
        assert!(format_size(500).contains('B'));
        assert!(format_size(5_000_000).contains("MB"));
    }

    #[test]
    fn available_update_requires_restart() {
        let u = AvailableUpdate::new("u1", "Test", UpdateKind::System, 100, "1.0");
        assert!(u.requires_restart);
        let u2 = AvailableUpdate::new("u2", "App", UpdateKind::Application, 100, "1.0");
        assert!(!u2.requires_restart);
    }

    // --- Palette conversion --------------------------------------------------

    const ALL_STATUSES: [UpdateStatus; 6] = [
        UpdateStatus::UpToDate,
        UpdateStatus::Checking,
        UpdateStatus::Available,
        UpdateStatus::Downloading,
        UpdateStatus::PendingRestart,
        UpdateStatus::Error,
    ];

    /// A panel wound into a state that reaches every colour branch at once.
    ///
    /// The three tabs draw disjoint sets of colours and only one is rendered
    /// per call, so the sweep below iterates tabs rather than this fixture
    /// covering them. What the fixture covers is everything *within* a tab:
    /// five update kinds with the selection alternating (so both the selected
    /// and unselected row backgrounds appear), a selected update that requires
    /// a restart (the peach warning), and a history holding both a success and
    /// a failure, the failure carrying an error message and the success
    /// offering a rollback (the red caption and the two lavender captions).
    fn wound_ui(
        status: UpdateStatus,
        tab: usize,
        paused: bool,
        populated: bool,
    ) -> UpdateSettingsUI {
        let mut ui = UpdateSettingsUI::new();
        ui.set_active_tab(tab);
        {
            let s = ui.settings_mut();
            s.status = status;
            s.config.paused_until_secs = u64::from(paused);
            s.config.defer_features_weeks = 2;
            s.config.defer_security_days = 3;
            s.config.auto_restart = true;
            s.config.include_drivers = false;
            s.config.include_features = true;
            s.config.defer_on_metered = false;
        }
        if !populated {
            return ui;
        }
        let kinds = [
            UpdateKind::System,
            UpdateKind::Security,
            UpdateKind::Application,
            UpdateKind::Driver,
            UpdateKind::Feature,
        ];
        for (i, kind) in kinds.iter().enumerate() {
            let mut u = AvailableUpdate::new(
                &format!("u{i}"),
                &format!("Update {i}"),
                *kind,
                1024 * (i as u64 + 1),
                "1.0",
            );
            u.selected = i % 2 == 0;
            u.requires_restart = i == 0;
            ui.settings_mut().add_available(u);
        }
        // `add_available` forces the status to `Available`; put back the one the
        // caller asked for, or five of the six statuses would go untested on the
        // Status tab.
        ui.settings_mut().status = status;
        for (i, success) in [true, false, true].iter().enumerate() {
            ui.settings_mut().add_history(UpdateHistoryEntry {
                title: format!("Installed {i}"),
                kind: kinds[i],
                version: "1.0".into(),
                installed_at_secs: 1000 + i as u64,
                success: *success,
                error_msg: if *success {
                    None
                } else {
                    Some("disk full".into())
                },
                rollback_available: i == 0,
            });
        }
        ui
    }

    /// Every colour this panel draws is a role of the palette it was handed.
    ///
    /// Thirteen Mocha constants used to live at the top of this file. The way
    /// their deletion fails is that one substitution is missed, which still
    /// compiles and still draws the colour it always drew. Rendering under the
    /// *light* palette is what makes that visible: a leftover constant is a
    /// dark value Latte does not contain, so it names itself.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for status in ALL_STATUSES {
                for tab in 0..3_usize {
                    for paused in [false, true] {
                        // The empty branches are separate colours: each tab
                        // draws an `overlay0` "nothing here" caption instead of
                        // its list, and a fixture that always has data would
                        // never render one.
                        for populated in [false, true] {
                            let ui = wound_ui(status, tab, paused, populated);
                            let cmds = ui.render(&p, 0.0, 0.0, 500.0);
                            // A switch knob is `readable_on` its own track —
                            // one of the two extremes, not a role. The tracks
                            // are named rather than the extremes, so the
                            // exemption stays tied to the fill it sits on.
                            palette_check::assert_drawn_from(
                                &p,
                                &cmds,
                                &[
                                    appearance::readable_on(p.green),
                                    appearance::readable_on(p.surface1),
                                ],
                                "update_settings",
                            );
                        }
                    }
                }
            }
        }
    }

    /// Every colour that says *what state the machine or an update is in*.
    ///
    /// The status banner's caption (14pt bold), the history ledger's tick and
    /// cross rows (12pt) and a failure's error message (10pt) — all
    /// categorical, none of them the accent's to move.
    fn status_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size: 14.0,
                    font_weight: FontWeightHint::Bold,
                    text,
                    color,
                    ..
                } if ALL_STATUSES.iter().any(|s| s.label() == text) => Some(*color),
                RenderCommand::Text {
                    font_size: 10.0,
                    color,
                    ..
                } => Some(*color),
                RenderCommand::Text {
                    font_size: 12.0,
                    text,
                    color,
                    ..
                } if text.starts_with('\u{2713}') || text.starts_with('\u{2715}') => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The active tab's label — the only bold 12pt text carrying a tab name.
    fn active_tab_label_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    text,
                    color,
                    ..
                } if UpdateSettingsUI::TAB_LABELS.contains(&text.as_str()) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The chosen schedule's label — the 13pt row whose bullet is filled.
    fn selected_schedule_label_colors(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    font_size: 13.0,
                    text,
                    color,
                    ..
                } if text.starts_with('\u{25CF}') => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// What an update is doing is not the accent's to repaint.
    ///
    /// The membership sweep above cannot see this. A wrong *role* is a member
    /// of both palettes, so writing `p.accent` where `p.blue` belongs passes in
    /// light mode exactly as in dark; only a second render under a different
    /// accent separates the two.
    ///
    /// **There is one `assert_ne!` per accent site, and that is not a stylistic
    /// choice.** An `assert_ne!` over a *combined* vector of accent sites
    /// proves only that *at least one* of them moved, so a still-moving site
    /// masks a frozen one — which is exactly how `bluetooth.rs`'s first draft
    /// missed harness defect FFF. This panel has two accent sites and therefore
    /// two negative assertions.
    #[test]
    fn an_updates_status_colours_do_not_follow_the_accent() {
        let mut blue = Palette::for_mode(false);
        blue.accent = appearance::BLUE;
        let mut mauve = Palette::for_mode(false);
        mauve.accent = appearance::MAUVE;

        // Negative half, site 1: the active tab label, drawn on every tab.
        for tab in 0..3_usize {
            let ui = wound_ui(UpdateStatus::Available, tab, false, true);
            let label_blue = active_tab_label_colors(&ui.render(&blue, 0.0, 0.0, 500.0));
            assert_eq!(label_blue.len(), 1, "tab {tab} has no single active label");
            assert_ne!(
                label_blue,
                active_tab_label_colors(&ui.render(&mauve, 0.0, 0.0, 500.0)),
                "the active tab's label did not move with the accent, so the \
                 rest of this test would pass on a panel that ignored the accent"
            );
        }

        // Negative half, site 2: the chosen schedule's label, Schedule tab only.
        let sched = wound_ui(UpdateStatus::Available, 1, false, true);
        let sched_blue = selected_schedule_label_colors(&sched.render(&blue, 0.0, 0.0, 500.0));
        assert_eq!(
            sched_blue.len(),
            1,
            "exactly one schedule should be marked chosen"
        );
        assert_ne!(
            sched_blue,
            selected_schedule_label_colors(&sched.render(&mauve, 0.0, 0.0, 500.0)),
            "the chosen schedule's label did not move with the accent"
        );

        // Positive half: nothing categorical moved, on any tab, in any status.
        for status in ALL_STATUSES {
            for tab in 0..3_usize {
                let ui = wound_ui(status, tab, false, true);
                let under_blue = status_colors(&ui.render(&blue, 0.0, 0.0, 500.0));
                let under_mauve = status_colors(&ui.render(&mauve, 0.0, 0.0, 500.0));
                assert!(
                    !under_blue.is_empty(),
                    "tab {tab} drew no status colours, so nothing was checked"
                );
                assert_eq!(
                    under_blue, under_mauve,
                    "an update status, a history outcome or an error message \
                     moved with the accent on tab {tab}. Those say what the \
                     machine is doing, the way a risk level does; a mauve \
                     accent says nothing about what \"downloading\" means."
                );
            }
        }
    }

    /// The six update statuses have to stay tellable apart under every accent.
    ///
    /// Same argument as `bluetooth.rs`'s scan button: were any of these written
    /// as `p.accent` it would collide with whichever sibling the user's accent
    /// happens to equal — and green, blue, yellow, peach and red are all among
    /// the fourteen accents that can be picked.
    #[test]
    fn the_update_statuses_stay_distinct_in_both_modes() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::PEACH,
                appearance::MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let mut seen: Vec<Color> = Vec::new();
                for status in ALL_STATUSES {
                    // Checking and Downloading share a colour by design; every
                    // other pair must differ.
                    if status == UpdateStatus::Downloading {
                        continue;
                    }
                    let c = status.color(&p);
                    assert!(
                        !seen.iter().any(|s| s.r == c.r && s.g == c.g && s.b == c.b),
                        "{status:?} repeats a colour already used by another \
                         status (light={light})"
                    );
                    seen.push(c);
                }
            }
        }
    }
}
