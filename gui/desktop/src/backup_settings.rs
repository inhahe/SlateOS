//! Backup and restore settings panel for the desktop shell.
//!
//! Configures system backup behavior including backup schedules,
//! target locations, file inclusion/exclusion rules, retention
//! policies, and backup history with restore capabilities.

use appearance::Palette;
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::tzrules::Tz;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour below is a role read from the resolved [`Palette`], so the panel
// follows the user's light/dark mode and accent instead of the fourteen Mocha
// literals it used to hold.
//
// **This panel is mostly controls, so it has nine accent sites, not one.** Every
// switch, checkbox, radio and primary button here says either "this is on
// because you turned it on" or "press this and something happens" -- state you
// chose, and invitation, which is what the accent is for. They are: the tab you
// are on, "Backup now", the automatic-backup switch, the frequency radio (its
// ring and its dot), the three retention switches, "+ Add source", each source's
// checkbox (its box and its tick), "+ Add rule", and each exclusion rule's
// switch. Each gets its own negative assertion in the tests: over their union,
// one moving control would hide eight frozen ones.
//
// The three buttons take their label from [`Palette::on_accent`] rather than a
// fixed near-black, because a pale accent wants dark text on it and a deep one
// wants light.
//
// **Nothing that reports stays with them.** [`BackupStatus::color`] is an
// outcome — Completed, Partial, Failed, Cancelled, In progress — drawn as a dot
// down a scrollable list, so two outcomes sharing a hue makes a failed backup
// look like a finished one at a glance. `InProgress => blue` is the trap in it:
// blue is also the *default* accent, so `p.accent` there would look right on a
// fresh install and collapse onto Completed the moment someone picks Green. The
// four overview stats (total, successful, failed, size) are the same argument
// drawn four abreast, and the remove crosses are destructive rather than
// inviting.

// ============================================================================
// Backup types
// ============================================================================

/// Type of backup to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupType {
    /// Full backup of all selected files.
    Full,
    /// Only files changed since last full backup.
    Incremental,
    /// Only files changed since last backup of any type.
    Differential,
    /// Mirror current state (no versioning).
    Mirror,
}

impl BackupType {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full backup",
            Self::Incremental => "Incremental",
            Self::Differential => "Differential",
            Self::Mirror => "Mirror",
        }
    }

    /// Short description.
    pub fn description(self) -> &'static str {
        match self {
            Self::Full => "Complete copy of all selected files",
            Self::Incremental => "Only changes since last full backup",
            Self::Differential => "Only changes since any last backup",
            Self::Mirror => "Exact copy of current state, no history",
        }
    }

    /// Relative speed (1=fast, 3=slow).
    pub fn relative_speed(self) -> u8 {
        match self {
            Self::Incremental => 1,
            Self::Differential => 1,
            Self::Full => 2,
            Self::Mirror => 3,
        }
    }

    /// Relative storage usage (1=low, 3=high).
    pub fn storage_usage(self) -> u8 {
        match self {
            Self::Incremental => 1,
            Self::Differential => 2,
            Self::Full => 3,
            Self::Mirror => 1,
        }
    }
}

/// Backup schedule frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupFrequency {
    Manual,
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

impl BackupFrequency {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual only",
            Self::Hourly => "Every hour",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }

    /// Interval in seconds (0 for manual).
    pub fn interval_secs(self) -> u64 {
        match self {
            Self::Manual => 0,
            Self::Hourly => 3600,
            Self::Daily => 86400,
            Self::Weekly => 604800,
            Self::Monthly => 2592000,
        }
    }

    /// Whether this requires scheduling.
    pub fn is_scheduled(self) -> bool {
        self != Self::Manual
    }
}

/// Day of week for weekly backups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayOfWeek {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl DayOfWeek {
    /// Short label.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
            Self::Sunday => "Sun",
        }
    }
}

// ============================================================================
// Backup target
// ============================================================================

/// Where backups are stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupTarget {
    /// Local directory.
    LocalPath(String),
    /// External/removable drive.
    ExternalDrive { label: String, path: String },
    /// Network share.
    NetworkShare {
        host: String,
        share: String,
        path: String,
    },
}

impl BackupTarget {
    /// Display path.
    pub fn display_path(&self) -> String {
        match self {
            Self::LocalPath(p) => p.clone(),
            Self::ExternalDrive { label, path } => format!("{label} ({path})"),
            Self::NetworkShare { host, share, path } => {
                format!("//{host}/{share}{path}")
            }
        }
    }

    /// Short label.
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::LocalPath(_) => "Local",
            Self::ExternalDrive { .. } => "External",
            Self::NetworkShare { .. } => "Network",
        }
    }
}

// ============================================================================
// Inclusion / exclusion rules
// ============================================================================

/// A source directory to include in backups.
#[derive(Clone, Debug)]
pub struct BackupSource {
    pub path: String,
    pub include_subdirs: bool,
    pub enabled: bool,
}

/// A pattern for excluding files from backup.
#[derive(Clone, Debug)]
pub struct ExcludeRule {
    pub pattern: String,
    pub description: String,
    pub enabled: bool,
}

/// Default exclude rules for common non-essential files.
pub fn default_exclude_rules() -> Vec<ExcludeRule> {
    vec![
        ExcludeRule {
            pattern: "*.tmp".to_string(),
            description: "Temporary files".to_string(),
            enabled: true,
        },
        ExcludeRule {
            pattern: "*.cache".to_string(),
            description: "Cache files".to_string(),
            enabled: true,
        },
        ExcludeRule {
            pattern: "*.log".to_string(),
            description: "Log files".to_string(),
            enabled: false,
        },
        ExcludeRule {
            pattern: "target/".to_string(),
            description: "Build output directories".to_string(),
            enabled: true,
        },
        ExcludeRule {
            pattern: "node_modules/".to_string(),
            description: "Node.js dependencies".to_string(),
            enabled: true,
        },
        ExcludeRule {
            pattern: ".git/".to_string(),
            description: "Git repositories".to_string(),
            enabled: false,
        },
        ExcludeRule {
            pattern: "*.iso".to_string(),
            description: "Disc images".to_string(),
            enabled: true,
        },
        ExcludeRule {
            pattern: "*.vmdk".to_string(),
            description: "Virtual machine disks".to_string(),
            enabled: true,
        },
    ]
}

// ============================================================================
// Retention policy
// ============================================================================

/// How long to keep backup versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionPolicy {
    /// Keep all backups forever.
    KeepAll,
    /// Keep a fixed number of backups.
    KeepCount(u32),
    /// Keep backups for N days.
    KeepDays(u32),
    /// Tiered: keep daily for 7 days, weekly for 4 weeks, monthly for 12 months.
    Tiered,
}

impl RetentionPolicy {
    /// Display label.
    pub fn label(self) -> String {
        match self {
            Self::KeepAll => "Keep all".to_string(),
            Self::KeepCount(n) => format!("Keep last {n}"),
            Self::KeepDays(d) => format!("Keep {d} days"),
            Self::Tiered => "Tiered (7d/4w/12m)".to_string(),
        }
    }

    /// Estimated space multiplier relative to single backup.
    pub fn space_estimate(self) -> &'static str {
        match self {
            Self::KeepAll => "Unlimited",
            Self::KeepCount(n) if n <= 5 => "Low",
            Self::KeepCount(_) => "Moderate",
            Self::KeepDays(d) if d <= 7 => "Low",
            Self::KeepDays(d) if d <= 30 => "Moderate",
            Self::KeepDays(_) => "High",
            Self::Tiered => "Moderate",
        }
    }
}

// ============================================================================
// Backup status and history
// ============================================================================

/// Status of a backup operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupStatus {
    Success,
    PartialSuccess,
    Failed,
    Cancelled,
    InProgress,
}

impl BackupStatus {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Completed",
            Self::PartialSuccess => "Partial",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::InProgress => "In progress",
        }
    }

    /// Status color.
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Self::Success => p.green,
            Self::PartialSuccess => p.yellow,
            Self::Failed => p.red,
            Self::Cancelled => p.overlay0,
            Self::InProgress => p.blue,
        }
    }
}

/// A historical backup entry.
#[derive(Clone, Debug)]
pub struct BackupHistoryEntry {
    pub id: u64,
    pub timestamp: u64,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub files_count: u64,
    pub total_bytes: u64,
    pub duration_secs: u64,
    pub error_message: Option<String>,
    pub target_path: String,
}

impl BackupHistoryEntry {
    /// Format the size for display.
    pub fn size_display(&self) -> String {
        format_bytes(self.total_bytes)
    }

    /// Format the duration for display.
    ///
    /// [`guitk::duration::coarse`] rather than `coarse_minutes`: this is how
    /// long a backup *took*, a measurement, so a ninety-second run is `1m 30s`
    /// and dropping the seconds would discard something real. It also used to
    /// stop at hours, so a first full backup of a large disk read `26h 0m`.
    pub fn duration_display(&self) -> String {
        guitk::duration::coarse(self.duration_secs)
    }

    /// When the backup ran, as `2026-08-18 16:30`.
    ///
    /// This used to render `Day 20683 16:30` — the number of days since
    /// 1 January 1970, shown to the user because turning it into a date was
    /// work nobody had done. Meanwhile the backup *application* listed the
    /// same runs as `2026-08-18 16:30:45`, so the two surfaces that show a
    /// user their backup history disagreed about what a backup's date is.
    ///
    /// The zone is a parameter and is not defaulted, even though the only
    /// caller today can only supply UTC. This module lives in the desktop
    /// crate, which *does* have a real configured zone
    /// ([`crate::datetime_settings`]) — it is only that this panel is not yet
    /// wired to the shell. Making the zone an argument means that when it is
    /// wired, the compiler asks for the zone rather than the panel silently
    /// keeping a UTC clock nobody remembers choosing.
    pub fn date_display(&self, tz: &Tz) -> String {
        guitk::datetime::stamp(i64::try_from(self.timestamp).unwrap_or(i64::MAX), tz)
    }
}

/// Format bytes for display.
fn format_bytes(bytes: u64) -> String {
    guitk::bytes::iec(bytes)
}

// ============================================================================
// Backup settings aggregate
// ============================================================================

/// Complete backup configuration.
#[derive(Clone, Debug)]
pub struct BackupSettings {
    pub enabled: bool,
    pub backup_type: BackupType,
    pub frequency: BackupFrequency,
    pub schedule_time_hour: u8,
    pub schedule_time_minute: u8,
    pub schedule_day: DayOfWeek,
    pub target: BackupTarget,
    pub sources: Vec<BackupSource>,
    pub exclude_rules: Vec<ExcludeRule>,
    pub retention: RetentionPolicy,
    pub compression_enabled: bool,
    pub encryption_enabled: bool,
    pub verify_after_backup: bool,
    pub notify_on_complete: bool,
    pub notify_on_failure: bool,
    pub skip_if_on_battery: bool,
    pub skip_if_metered: bool,
    pub history: Vec<BackupHistoryEntry>,
    ids: IdSeq,
    pub last_backup_timestamp: Option<u64>,
    pub total_backup_size: u64,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            backup_type: BackupType::Incremental,
            frequency: BackupFrequency::Daily,
            schedule_time_hour: 2,
            schedule_time_minute: 0,
            schedule_day: DayOfWeek::Sunday,
            target: BackupTarget::LocalPath("/backup".to_string()),
            sources: vec![
                BackupSource {
                    path: "/home".to_string(),
                    include_subdirs: true,
                    enabled: true,
                },
                BackupSource {
                    path: "/etc".to_string(),
                    include_subdirs: true,
                    enabled: true,
                },
            ],
            exclude_rules: default_exclude_rules(),
            retention: RetentionPolicy::Tiered,
            compression_enabled: true,
            encryption_enabled: false,
            verify_after_backup: true,
            notify_on_complete: true,
            notify_on_failure: true,
            skip_if_on_battery: true,
            skip_if_metered: true,
            history: Vec::new(),
            ids: IdSeq::new(),
            last_backup_timestamp: None,
            total_backup_size: 0,
        }
    }
}

impl BackupSettings {
    /// Add a backup source directory.
    pub fn add_source(&mut self, path: &str) {
        if !self.sources.iter().any(|s| s.path == path) {
            self.sources.push(BackupSource {
                path: path.to_string(),
                include_subdirs: true,
                enabled: true,
            });
        }
    }

    /// Remove a backup source by path.
    pub fn remove_source(&mut self, path: &str) -> bool {
        let before = self.sources.len();
        self.sources.retain(|s| s.path != path);
        self.sources.len() < before
    }

    /// Toggle a source's enabled state.
    pub fn toggle_source(&mut self, path: &str) -> Option<bool> {
        if let Some(src) = self.sources.iter_mut().find(|s| s.path == path) {
            src.enabled = !src.enabled;
            Some(src.enabled)
        } else {
            None
        }
    }

    /// Add a custom exclude rule.
    pub fn add_exclude_rule(&mut self, pattern: &str, description: &str) {
        if !self.exclude_rules.iter().any(|r| r.pattern == pattern) {
            self.exclude_rules.push(ExcludeRule {
                pattern: pattern.to_string(),
                description: description.to_string(),
                enabled: true,
            });
        }
    }

    /// Remove an exclude rule by pattern.
    pub fn remove_exclude_rule(&mut self, pattern: &str) -> bool {
        let before = self.exclude_rules.len();
        self.exclude_rules.retain(|r| r.pattern != pattern);
        self.exclude_rules.len() < before
    }

    /// Toggle an exclude rule.
    pub fn toggle_exclude_rule(&mut self, pattern: &str) -> Option<bool> {
        if let Some(rule) = self.exclude_rules.iter_mut().find(|r| r.pattern == pattern) {
            rule.enabled = !rule.enabled;
            Some(rule.enabled)
        } else {
            None
        }
    }

    /// Record a completed backup, assigning it a history ID.
    ///
    /// The ID on `entry` is overwritten. Before this took its ID from the
    /// sequence, the counter here was advanced by every call and read by
    /// none: every caller invented its own ID, and nothing stopped two
    /// entries sharing one — which would have made `delete`/lookup by ID
    /// ambiguous the first time it mattered.
    pub fn record_backup(&mut self, mut entry: BackupHistoryEntry) -> u64 {
        let id = self.ids.issue_infallible();
        entry.id = id;
        if entry.status == BackupStatus::Success || entry.status == BackupStatus::PartialSuccess {
            self.last_backup_timestamp = Some(entry.timestamp);
            self.total_backup_size = self.total_backup_size.saturating_add(entry.total_bytes);
        }
        self.history.push(entry);
        id
    }

    /// Get the last successful backup.
    pub fn last_successful_backup(&self) -> Option<&BackupHistoryEntry> {
        self.history
            .iter()
            .rev()
            .find(|e| e.status == BackupStatus::Success)
    }

    /// Count successful backups.
    pub fn successful_backup_count(&self) -> usize {
        self.history
            .iter()
            .filter(|e| e.status == BackupStatus::Success)
            .count()
    }

    /// Count failed backups.
    pub fn failed_backup_count(&self) -> usize {
        self.history
            .iter()
            .filter(|e| e.status == BackupStatus::Failed)
            .count()
    }

    /// Count active (enabled) sources.
    pub fn active_source_count(&self) -> usize {
        self.sources.iter().filter(|s| s.enabled).count()
    }

    /// Count active exclude rules.
    pub fn active_exclude_count(&self) -> usize {
        self.exclude_rules.iter().filter(|r| r.enabled).count()
    }

    /// Get a schedule description string.
    pub fn schedule_description(&self) -> String {
        if !self.enabled {
            return "Backups disabled".to_string();
        }
        match self.frequency {
            BackupFrequency::Manual => "Manual backups only".to_string(),
            BackupFrequency::Hourly => "Every hour".to_string(),
            BackupFrequency::Daily => {
                format!(
                    "Daily at {:02}:{:02}",
                    self.schedule_time_hour, self.schedule_time_minute
                )
            }
            BackupFrequency::Weekly => {
                format!(
                    "Every {} at {:02}:{:02}",
                    self.schedule_day.short_label(),
                    self.schedule_time_hour,
                    self.schedule_time_minute
                )
            }
            BackupFrequency::Monthly => {
                format!(
                    "Monthly at {:02}:{:02}",
                    self.schedule_time_hour, self.schedule_time_minute
                )
            }
        }
    }
}

// ============================================================================
// Settings UI
// ============================================================================

/// Tabs in the backup settings panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupSettingsTab {
    Overview,
    Schedule,
    Sources,
    Exclusions,
    History,
}

impl BackupSettingsTab {
    /// All tabs.
    pub fn all() -> &'static [Self] {
        &[
            Self::Overview,
            Self::Schedule,
            Self::Sources,
            Self::Exclusions,
            Self::History,
        ]
    }

    /// Tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Schedule => "Schedule",
            Self::Sources => "Sources",
            Self::Exclusions => "Exclusions",
            Self::History => "History",
        }
    }
}

/// Backup settings UI state.
pub struct BackupSettingsUI {
    pub settings: BackupSettings,
    pub active_tab: BackupSettingsTab,
    pub scroll_offset: f32,
    pub dirty: bool,
}

impl BackupSettingsUI {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            settings: BackupSettings::default(),
            active_tab: BackupSettingsTab::Overview,
            scroll_offset: 0.0,
            dirty: false,
        }
    }

    /// Switch tab.
    pub fn set_tab(&mut self, tab: BackupSettingsTab) {
        self.active_tab = tab;
        self.scroll_offset = 0.0;
    }

    /// Render the settings panel.
    ///
    /// `tz` is the zone the History tab dates its runs in — see
    /// [`BackupHistoryEntry::date_display`] for why it is asked for here
    /// rather than assumed.
    pub fn render(
        &self,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        tz: &Tz,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Backup & Restore".to_string(),
            font_size: 22.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in BackupSettingsTab::all() {
            let label = tab.label();
            let tw = text::padded_width_any_weight(label, 12.0, 13.0);
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { p.accent } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            tab_x += tw + 4.0;
        }

        let content_y = tab_y + 44.0;
        let content_h = height - (content_y - y) - 16.0;

        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: content_y,
            width: width - 16.0,
            height: content_h,
            color: p.crust,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            BackupSettingsTab::Overview => self.render_overview(p, &mut cmds, cx, cy, cw),
            BackupSettingsTab::Schedule => self.render_schedule(p, &mut cmds, cx, cy, cw),
            BackupSettingsTab::Sources => self.render_sources(p, &mut cmds, cx, cy, cw),
            BackupSettingsTab::Exclusions => self.render_exclusions(p, &mut cmds, cx, cy, cw),
            BackupSettingsTab::History => self.render_history(p, &mut cmds, cx, cy, cw, tz),
        }

        cmds
    }

    /// Render overview tab.
    fn render_overview(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Status card
        let status_color = if self.settings.enabled {
            p.green
        } else {
            p.overlay0
        };
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 80.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(8.0),
        });

        cmds.push(RenderCommand::FillRect {
            x: x + 16.0,
            y: row_y + 20.0,
            width: 12.0,
            height: 12.0,
            color: status_color,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 36.0,
            y: row_y + 16.0,
            text: if self.settings.enabled {
                "Backup is active"
            } else {
                "Backup is disabled"
            }
            .to_string(),
            font_size: 18.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: x + 36.0,
            y: row_y + 44.0,
            text: self.settings.schedule_description(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 52.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 96.0;

        // Stats cards
        let stats = [
            (
                "Total backups",
                format!("{}", self.settings.history.len()),
                p.blue,
            ),
            (
                "Successful",
                format!("{}", self.settings.successful_backup_count()),
                p.green,
            ),
            (
                "Failed",
                format!("{}", self.settings.failed_backup_count()),
                p.red,
            ),
            (
                "Total size",
                format_bytes(self.settings.total_backup_size),
                p.lavender,
            ),
        ];

        let card_w = (width - 24.0) / 4.0;
        for (i, (label, value, color)) in stats.iter().enumerate() {
            let cx = x + i as f32 * (card_w + 8.0);

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: row_y,
                width: card_w,
                height: 60.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 10.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 28.0,
                text: value.clone(),
                font_size: 18.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        row_y += 76.0;

        // Configuration summary
        let items = [
            ("Type", self.settings.backup_type.label().to_string()),
            ("Target", self.settings.target.display_path()),
            (
                "Sources",
                format!("{} active", self.settings.active_source_count()),
            ),
            (
                "Exclusions",
                format!("{} rules", self.settings.active_exclude_count()),
            ),
            ("Retention", self.settings.retention.label()),
            (
                "Compression",
                if self.settings.compression_enabled {
                    "On"
                } else {
                    "Off"
                }
                .to_string(),
            ),
            (
                "Encryption",
                if self.settings.encryption_enabled {
                    "On"
                } else {
                    "Off"
                }
                .to_string(),
            ),
        ];

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Configuration".to_string(),
            font_size: 14.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 22.0;

        for (label, value) in &items {
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y,
                text: format!("{label}:"),
                font_size: 12.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: x + 120.0,
                y: row_y,
                text: value.clone(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 140.0),
                overflow: TextOverflow::Ellipsis,
            });

            row_y += 20.0;
        }

        // Backup now button
        row_y += 16.0;
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width: 120.0,
            height: 36.0,
            color: p.accent,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 20.0,
            y: row_y + 10.0,
            text: "Backup now".to_string(),
            font_size: 13.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// Render schedule tab.
    fn render_schedule(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        // Enable toggle
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 36.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 10.0,
            text: "Enable automatic backups".to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let toggle_bg = if self.settings.enabled {
            p.accent
        } else {
            p.surface2
        };
        cmds.push(RenderCommand::FillRect {
            x: x + width - 56.0,
            y: row_y + 8.0,
            width: 40.0,
            height: 20.0,
            color: toggle_bg,
            corner_radii: CornerRadii::all(10.0),
        });
        row_y += 48.0;

        // Frequency selector
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Frequency".to_string(),
            font_size: 13.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 22.0;

        let freqs = [
            BackupFrequency::Manual,
            BackupFrequency::Hourly,
            BackupFrequency::Daily,
            BackupFrequency::Weekly,
            BackupFrequency::Monthly,
        ];

        for freq in &freqs {
            let is_active = *freq == self.settings.frequency;

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: if is_active { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });

            // Radio button
            cmds.push(RenderCommand::StrokeRect {
                x: x + 12.0,
                y: row_y + 8.0,
                width: 16.0,
                height: 16.0,
                color: if is_active { p.accent } else { p.surface2 },
                corner_radii: CornerRadii::all(8.0),
                line_width: 2.0,
            });

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 16.0,
                    y: row_y + 12.0,
                    width: 8.0,
                    height: 8.0,
                    color: p.accent,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 36.0,
                y: row_y + 8.0,
                text: freq.label().to_string(),
                font_size: 13.0,
                color: if is_active { p.text } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 38.0;
        }

        // Backup type
        row_y += 8.0;
        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Backup type".to_string(),
            font_size: 13.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 22.0;

        let types = [
            BackupType::Full,
            BackupType::Incremental,
            BackupType::Differential,
            BackupType::Mirror,
        ];

        for bt in &types {
            let is_active = *bt == self.settings.backup_type;

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 44.0,
                color: if is_active { p.surface1 } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 6.0,
                text: bt.label().to_string(),
                font_size: 13.0,
                color: if is_active { p.text } else { p.subtext0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 24.0,
                text: bt.description().to_string(),
                font_size: 10.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
                overflow: TextOverflow::Ellipsis,
            });

            row_y += 50.0;
        }

        // Options
        row_y += 8.0;
        let options = [
            ("Compression", self.settings.compression_enabled),
            ("Encryption", self.settings.encryption_enabled),
            ("Verify after backup", self.settings.verify_after_backup),
            ("Skip if on battery", self.settings.skip_if_on_battery),
            ("Skip if metered connection", self.settings.skip_if_metered),
        ];

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: "Options".to_string(),
            font_size: 13.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 22.0;

        for (label, enabled) in &options {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: p.surface0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 12.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let toggle_color = if *enabled { p.accent } else { p.surface2 };
            cmds.push(RenderCommand::FillRect {
                x: x + width - 56.0,
                y: row_y + 6.0,
                width: 40.0,
                height: 20.0,
                color: toggle_color,
                corner_radii: CornerRadii::all(10.0),
            });

            row_y += 38.0;
        }
    }

    /// Render sources tab.
    fn render_sources(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!(
                "Backup sources ({} active of {})",
                self.settings.active_source_count(),
                self.settings.sources.len()
            ),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Add source button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 100.0,
            y: row_y - 4.0,
            width: 100.0,
            height: 24.0,
            color: p.accent,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 88.0,
            y: row_y,
            text: "+ Add source".to_string(),
            font_size: 11.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        // Target
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 48.0,
            color: p.surface0,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 6.0,
            text: format!("Target: {}", self.settings.target.kind_label()),
            font_size: 12.0,
            color: p.subtext1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 26.0,
            text: self.settings.target.display_path(),
            font_size: 12.0,
            color: p.blue,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });
        row_y += 60.0;

        // Source list
        for source in &self.settings.sources {
            let bg = if source.enabled {
                p.surface0
            } else {
                Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)
            };

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 40.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            // Checkbox
            cmds.push(RenderCommand::StrokeRect {
                x: x + 12.0,
                y: row_y + 12.0,
                width: 16.0,
                height: 16.0,
                color: if source.enabled { p.accent } else { p.surface2 },
                corner_radii: CornerRadii::all(3.0),
                line_width: 2.0,
            });

            if source.enabled {
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 12.0,
                    text: "\u{2713}".to_string(),
                    font_size: 12.0,
                    color: p.accent,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 12.0,
                text: source.path.clone(),
                font_size: 13.0,
                color: if source.enabled { p.text } else { p.overlay0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 120.0),
                overflow: TextOverflow::Ellipsis,
            });

            if source.include_subdirs {
                cmds.push(RenderCommand::Text {
                    x: x + width - 80.0,
                    y: row_y + 14.0,
                    text: "Recursive".to_string(),
                    font_size: 10.0,
                    color: p.overlay0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Remove button
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 12.0,
                text: "\u{2715}".to_string(),
                font_size: 12.0,
                color: p.red,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 46.0;
        }
    }

    /// Render exclusions tab.
    fn render_exclusions(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!(
                "Exclusion rules ({} active)",
                self.settings.active_exclude_count()
            ),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        cmds.push(RenderCommand::FillRect {
            x: x + width - 80.0,
            y: row_y - 4.0,
            width: 80.0,
            height: 24.0,
            color: p.accent,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 68.0,
            y: row_y,
            text: "+ Add rule".to_string(),
            font_size: 11.0,
            color: p.on_accent(),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        for rule in &self.settings.exclude_rules {
            let bg = if rule.enabled {
                p.surface0
            } else {
                Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)
            };

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 44.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            // Toggle
            let toggle_bg = if rule.enabled { p.accent } else { p.surface2 };
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0,
                y: row_y + 12.0,
                width: 32.0,
                height: 16.0,
                color: toggle_bg,
                corner_radii: CornerRadii::all(8.0),
            });

            // Pattern
            cmds.push(RenderCommand::FillRect {
                x: x + 56.0,
                y: row_y + 8.0,
                width: text::padded_width(&rule.pattern, 8.0, 11.0, FontWeightHint::Bold),
                height: 20.0,
                color: p.surface1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 64.0,
                y: row_y + 10.0,
                text: rule.pattern.clone(),
                font_size: 11.0,
                color: p.lavender,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: x + 56.0,
                y: row_y + 28.0,
                text: rule.description.clone(),
                font_size: 10.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 100.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Remove
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 14.0,
                text: "\u{2715}".to_string(),
                font_size: 12.0,
                color: p.red,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            row_y += 50.0;
        }
    }

    /// Render history tab.
    fn render_history(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        width: f32,
        tz: &Tz,
    ) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Backup history ({} entries)", self.settings.history.len()),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        row_y += 28.0;

        if self.settings.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No backups yet. Click \"Backup now\" to create your first backup."
                    .to_string(),
                font_size: 13.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
                overflow: TextOverflow::Ellipsis,
            });
        } else {
            for entry in self.settings.history.iter().rev() {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 56.0,
                    color: p.surface0,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Status badge
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y + 8.0,
                    width: 8.0,
                    height: 8.0,
                    color: entry.status.color(p),
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: row_y + 4.0,
                    text: format!("{} — {}", entry.backup_type.label(), entry.status.label()),
                    font_size: 13.0,
                    color: p.text,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: row_y + 22.0,
                    text: format!(
                        "{} — {} files — {} — {}",
                        entry.date_display(tz),
                        entry.files_count,
                        entry.size_display(),
                        entry.duration_display()
                    ),
                    font_size: 11.0,
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 40.0),
                    overflow: TextOverflow::Ellipsis,
                });

                if let Some(ref err) = entry.error_message {
                    cmds.push(RenderCommand::Text {
                        x: x + 24.0,
                        y: row_y + 38.0,
                        text: err.clone(),
                        font_size: 10.0,
                        color: p.red,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 40.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                row_y += 62.0;
            }
        }
    }
}

impl Default for BackupSettingsUI {
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
    // These tests assert a float equals the exact literal the code under test was
    // handed. That is the assertion meant: a tolerance would let a value that has
    // drifted pass as one that has not.
    #![allow(clippy::float_cmp)]

    use super::*;
    use crate::draw_check::assert_nothing_is_drawn_and_never_seen;
    use crate::palette_check::assert_drawn_from;

    /// The palette the older tests were written against: dark mode, stock
    /// accent, which is what the deleted constants spelled out by hand.
    fn test_palette() -> Palette {
        Palette::for_mode(false)
    }

    #[test]
    fn test_backup_type_labels() {
        assert_eq!(BackupType::Full.label(), "Full backup");
        assert_eq!(BackupType::Incremental.label(), "Incremental");
    }

    #[test]
    fn test_backup_type_properties() {
        assert!(BackupType::Incremental.relative_speed() < BackupType::Full.relative_speed());
        assert!(BackupType::Incremental.storage_usage() < BackupType::Full.storage_usage());
    }

    #[test]
    fn test_frequency_interval() {
        assert_eq!(BackupFrequency::Manual.interval_secs(), 0);
        assert_eq!(BackupFrequency::Daily.interval_secs(), 86400);
        assert!(!BackupFrequency::Manual.is_scheduled());
        assert!(BackupFrequency::Daily.is_scheduled());
    }

    #[test]
    fn test_backup_target_display() {
        let local = BackupTarget::LocalPath("/backup".to_string());
        assert_eq!(local.display_path(), "/backup");
        assert_eq!(local.kind_label(), "Local");

        let net = BackupTarget::NetworkShare {
            host: "server".to_string(),
            share: "backups".to_string(),
            path: "/daily".to_string(),
        };
        assert_eq!(net.display_path(), "//server/backups/daily");
        assert_eq!(net.kind_label(), "Network");
    }

    #[test]
    fn test_default_exclude_rules() {
        let rules = default_exclude_rules();
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.pattern == "*.tmp"));
    }

    #[test]
    fn test_retention_labels() {
        assert_eq!(RetentionPolicy::KeepAll.label(), "Keep all");
        assert_eq!(RetentionPolicy::KeepCount(5).label(), "Keep last 5");
        assert_eq!(RetentionPolicy::KeepDays(30).label(), "Keep 30 days");
    }

    #[test]
    fn test_backup_status_colors() {
        // Just verify they don't panic
        let p = test_palette();
        let _c1 = BackupStatus::Success.color(&p);
        let _c2 = BackupStatus::Failed.color(&p);
        let _c3 = BackupStatus::InProgress.color(&p);
    }

    #[test]
    fn test_history_entry_display() {
        let entry = BackupHistoryEntry {
            id: 1,
            timestamp: 86400 + 3661,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 1234,
            total_bytes: 5_000_000_000,
            duration_secs: 3661,
            error_message: None,
            target_path: "/backup".to_string(),
        };
        assert_eq!(entry.size_display(), "4.7 GiB");
        assert_eq!(entry.duration_display(), "1h 1m");
        // Asserted by value, not by `contains`. The assertion this replaces
        // was `date_display().contains("01:01")`, which was satisfied by
        // "Day 1 01:01" — so it proved the *time* and never looked at the
        // part that was a day counter rather than a date.
        assert_eq!(entry.date_display(&Tz::utc()), "1970-01-02 01:01");
    }

    /// The panel and the backup application date the same run alike.
    ///
    /// They are two surfaces onto one object — a backup run and when it
    /// happened — and they disagreed: the application said `2026-08-18
    /// 16:30:45` while this panel said `Day 20683 16:30`. Both now render
    /// through `guitk::datetime`, so the only remaining difference is whether
    /// seconds are shown, which is a deliberate choice per surface.
    #[test]
    fn the_history_panel_dates_a_run_the_way_the_backup_application_does() {
        let entry = BackupHistoryEntry {
            id: 1,
            // 2026-08-18 16:30:45 UTC.
            timestamp: 1_787_070_645,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 1,
            total_bytes: 1,
            duration_secs: 1,
            error_message: None,
            target_path: "/backup".to_string(),
        };
        let utc = Tz::utc();
        assert_eq!(entry.date_display(&utc), "2026-08-18 16:30");
        assert_eq!(
            guitk::datetime::stamp_secs(1_787_070_645, &utc),
            "2026-08-18 16:30:45"
        );
    }

    /// The zone reaches the dates, which is the whole reason it is a
    /// parameter rather than an assumption.
    #[test]
    fn the_zone_reaches_the_history_dates() {
        let entry = BackupHistoryEntry {
            id: 1,
            // 2026-08-18 02:00:00 UTC — the previous evening in New York.
            timestamp: 1_787_018_400,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 1,
            total_bytes: 1,
            duration_secs: 1,
            error_message: None,
            target_path: "/backup".to_string(),
        };
        let ny = Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").expect("a valid POSIX TZ string");
        assert_eq!(entry.date_display(&Tz::utc()), "2026-08-18 02:00");
        assert_eq!(entry.date_display(&ny), "2026-08-17 22:00");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(1_500_000), "1.4 MiB");
        assert_eq!(format_bytes(2_000_000_000), "1.9 GiB");
    }

    #[test]
    fn test_default_settings() {
        let settings = BackupSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.backup_type, BackupType::Incremental);
        assert_eq!(settings.frequency, BackupFrequency::Daily);
        assert!(!settings.sources.is_empty());
    }

    #[test]
    fn test_add_source() {
        let mut settings = BackupSettings::default();
        let count = settings.sources.len();
        settings.add_source("/data");
        assert_eq!(settings.sources.len(), count + 1);

        // Duplicate add ignored
        settings.add_source("/data");
        assert_eq!(settings.sources.len(), count + 1);
    }

    #[test]
    fn test_remove_source() {
        let mut settings = BackupSettings::default();
        settings.add_source("/data");
        assert!(settings.remove_source("/data"));
        assert!(!settings.remove_source("/nonexistent"));
    }

    #[test]
    fn test_toggle_source() {
        let mut settings = BackupSettings::default();
        let path = settings.sources[0].path.clone();
        let was_enabled = settings.sources[0].enabled;

        let result = settings.toggle_source(&path);
        assert_eq!(result, Some(!was_enabled));

        assert!(settings.toggle_source("nonexistent").is_none());
    }

    #[test]
    fn test_exclude_rules() {
        let mut settings = BackupSettings::default();
        let count = settings.exclude_rules.len();

        settings.add_exclude_rule("*.bak", "Backup files");
        assert_eq!(settings.exclude_rules.len(), count + 1);

        // Duplicate ignored
        settings.add_exclude_rule("*.bak", "Backup files");
        assert_eq!(settings.exclude_rules.len(), count + 1);

        assert!(settings.remove_exclude_rule("*.bak"));
        assert_eq!(settings.exclude_rules.len(), count);
    }

    #[test]
    fn test_toggle_exclude() {
        let mut settings = BackupSettings::default();
        let pattern = settings.exclude_rules[0].pattern.clone();
        let was = settings.exclude_rules[0].enabled;

        assert_eq!(settings.toggle_exclude_rule(&pattern), Some(!was));
        assert!(settings.toggle_exclude_rule("nonexistent").is_none());
    }

    /// Before `record_backup` took the ID from the sequence, the counter it
    /// kept was advanced by every call and read by none — so the ID on a
    /// history entry was whatever the caller happened to put there, and two
    /// entries could share one. Every entry now gets an ID nothing else has.
    #[test]
    fn a_recorded_backup_is_given_an_id_rather_than_trusting_the_callers() {
        let mut settings = BackupSettings::default();
        let entry = |id| BackupHistoryEntry {
            id,
            timestamp: 100,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 1,
            total_bytes: 1,
            duration_secs: 1,
            error_message: None,
            target_path: "/b".to_string(),
        };

        // Three callers all claiming ID 7.
        let ids: Vec<u64> = (0..3).map(|_| settings.record_backup(entry(7))).collect();

        assert_eq!(ids, vec![1, 2, 3]);
        let stored: Vec<u64> = settings.history.iter().map(|e| e.id).collect();
        assert_eq!(
            stored, ids,
            "the stored IDs are the issued ones, not the 7s"
        );
    }

    #[test]
    fn test_record_backup() {
        let mut settings = BackupSettings::default();
        settings.record_backup(BackupHistoryEntry {
            id: 1,
            timestamp: 100000,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 500,
            total_bytes: 1_000_000,
            duration_secs: 60,
            error_message: None,
            target_path: "/backup".to_string(),
        });

        assert_eq!(settings.history.len(), 1);
        assert_eq!(settings.successful_backup_count(), 1);
        assert_eq!(settings.last_backup_timestamp, Some(100000));
        assert_eq!(settings.total_backup_size, 1_000_000);
    }

    #[test]
    fn test_failed_backup_not_tracked() {
        let mut settings = BackupSettings::default();
        settings.record_backup(BackupHistoryEntry {
            id: 1,
            timestamp: 200000,
            backup_type: BackupType::Full,
            status: BackupStatus::Failed,
            files_count: 0,
            total_bytes: 0,
            duration_secs: 5,
            error_message: Some("Disk full".to_string()),
            target_path: "/backup".to_string(),
        });

        assert_eq!(settings.history.len(), 1);
        assert_eq!(settings.failed_backup_count(), 1);
        assert_eq!(settings.successful_backup_count(), 0);
        assert!(settings.last_backup_timestamp.is_none());
        assert_eq!(settings.total_backup_size, 0);
    }

    #[test]
    fn test_last_successful() {
        let mut settings = BackupSettings::default();
        settings.record_backup(BackupHistoryEntry {
            id: 1,
            timestamp: 100,
            backup_type: BackupType::Full,
            status: BackupStatus::Success,
            files_count: 10,
            total_bytes: 1000,
            duration_secs: 5,
            error_message: None,
            target_path: "/b".to_string(),
        });
        settings.record_backup(BackupHistoryEntry {
            id: 2,
            timestamp: 200,
            backup_type: BackupType::Incremental,
            status: BackupStatus::Failed,
            files_count: 0,
            total_bytes: 0,
            duration_secs: 1,
            error_message: Some("err".to_string()),
            target_path: "/b".to_string(),
        });

        let last = settings.last_successful_backup().unwrap();
        assert_eq!(last.id, 1);
    }

    #[test]
    fn test_schedule_description() {
        let mut settings = BackupSettings::default();
        settings.enabled = false;
        assert_eq!(settings.schedule_description(), "Backups disabled");

        settings.enabled = true;
        settings.frequency = BackupFrequency::Daily;
        assert!(settings.schedule_description().contains("Daily"));

        settings.frequency = BackupFrequency::Weekly;
        assert!(settings.schedule_description().contains("Sun"));
    }

    #[test]
    fn test_active_counts() {
        let settings = BackupSettings::default();
        assert!(settings.active_source_count() > 0);
        assert!(settings.active_exclude_count() > 0);
    }

    // UI tests
    #[test]
    fn test_ui_new() {
        let ui = BackupSettingsUI::new();
        assert_eq!(ui.active_tab, BackupSettingsTab::Overview);
        assert!(!ui.dirty);
    }

    #[test]
    fn test_ui_set_tab() {
        let mut ui = BackupSettingsUI::new();
        ui.scroll_offset = 100.0;
        ui.set_tab(BackupSettingsTab::History);
        assert_eq!(ui.active_tab, BackupSettingsTab::History);
        assert_eq!(ui.scroll_offset, 0.0);
    }

    #[test]
    fn test_ui_render_produces_commands() {
        let ui = BackupSettingsUI::new();
        let cmds = ui.render(&test_palette(), 0.0, 0.0, 600.0, 800.0, &Tz::utc());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_tab_all() {
        assert_eq!(BackupSettingsTab::all().len(), 5);
    }

    #[test]
    fn test_retention_space_estimate() {
        assert_eq!(RetentionPolicy::KeepAll.space_estimate(), "Unlimited");
        assert_eq!(RetentionPolicy::KeepCount(3).space_estimate(), "Low");
        assert_eq!(RetentionPolicy::Tiered.space_estimate(), "Moderate");
    }

    #[test]
    fn test_day_of_week_labels() {
        assert_eq!(DayOfWeek::Monday.short_label(), "Mon");
        assert_eq!(DayOfWeek::Sunday.short_label(), "Sun");
    }

    // ---- Colour ----

    /// A panel wound up so that every colour-bearing branch actually draws.
    ///
    /// Both halves of every switch and checkbox are on screen at once — two
    /// sources with one of them off, two rules with one of them off, and the
    /// five retention options split — because a control is coloured by a
    /// boolean, and a fixture that only ever renders the `true` arm proves
    /// nothing about the `false` one. Every backup outcome appears in the
    /// history, including the one that carries an error line.
    fn wound(tab: BackupSettingsTab, enabled: bool, freq: BackupFrequency) -> BackupSettingsUI {
        let mut ui = BackupSettingsUI::new();
        ui.settings.enabled = enabled;
        ui.settings.frequency = freq;
        ui.settings.sources.clear();
        ui.settings.add_source("/home/u/documents");
        ui.settings.add_source("/home/u/pictures");
        ui.settings.sources[1].enabled = false;
        ui.settings.sources[1].include_subdirs = false;
        ui.settings.exclude_rules.clear();
        ui.settings.add_exclude_rule("*.tmp", "Temporary files");
        ui.settings
            .add_exclude_rule("node_modules", "Dependency trees");
        ui.settings.exclude_rules[1].enabled = false;
        ui.settings.compression_enabled = true;
        ui.settings.encryption_enabled = false;
        ui.settings.verify_after_backup = true;
        ui.settings.skip_if_on_battery = false;
        ui.settings.skip_if_metered = true;
        ui.settings.history.clear();
        for (i, status) in [
            BackupStatus::Success,
            BackupStatus::PartialSuccess,
            BackupStatus::Failed,
            BackupStatus::Cancelled,
            BackupStatus::InProgress,
        ]
        .iter()
        .enumerate()
        {
            ui.settings.record_backup(BackupHistoryEntry {
                id: 0,
                timestamp: 1_700_000_000 + i as u64 * 3600,
                backup_type: BackupType::Full,
                status: *status,
                files_count: 1234,
                total_bytes: 5_000_000,
                duration_secs: 90,
                // Only the failed run draws the error line.
                error_message: if *status == BackupStatus::Failed {
                    Some("target unreachable".to_string())
                } else {
                    None
                },
                target_path: "/mnt/backup".to_string(),
            });
        }
        ui.active_tab = tab;
        ui
    }

    /// Every state the panel can be in, so no branch escapes the sweep below.
    fn every_state() -> Vec<(BackupSettingsUI, String)> {
        let mut out = Vec::new();
        for tab in BackupSettingsTab::all() {
            for enabled in [false, true] {
                for freq in [
                    BackupFrequency::Manual,
                    BackupFrequency::Hourly,
                    BackupFrequency::Daily,
                    BackupFrequency::Weekly,
                    BackupFrequency::Monthly,
                ] {
                    out.push((
                        wound(*tab, enabled, freq),
                        format!("backup panel (tab={tab:?}, enabled={enabled}, freq={freq:?})"),
                    ));
                }
            }
        }
        // The empty history draws a caption no populated one ever does.
        let mut bare = BackupSettingsUI::new();
        bare.active_tab = BackupSettingsTab::History;
        bare.settings.history.clear();
        out.push((bare, "backup panel (History, no backups yet)".to_string()));
        out
    }

    fn render(ui: &BackupSettingsUI, p: &Palette) -> Vec<RenderCommand> {
        ui.render(p, 0.0, 0.0, 700.0, 800.0, &Tz::utc())
    }

    /// The membership sweep: nothing the panel draws is outside its palette.
    ///
    /// Every constant this module used to hold was a Catppuccin *Mocha* value,
    /// so the light render is where a survivor gives itself away — Latte does
    /// not contain it, and the failure names the colour back.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                assert_drawn_from(&p, &render(&ui, &p), &[], &format!("{what}, light={light}"));
            }
        }
    }

    /// Nothing is painted and then erased before anyone could see it.
    #[test]
    fn the_panel_draws_nothing_that_is_immediately_erased() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for (ui, what) in every_state() {
                assert_nothing_is_drawn_and_never_seen(
                    &render(&ui, &p),
                    &format!("{what}, light={light}"),
                );
            }
        }
    }

    // -- Extractors, one per class of control the accent is supposed to reach --

    /// The tab strip's labels, in the order they are drawn.
    fn tab_labels(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y: 64.0,
                    font_size: 13.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The fill of the button labelled `label`, and that label's own colour.
    ///
    /// A button here is a `FillRect` immediately followed by its `Text`, so the
    /// pairing is "the last fill before this word".
    fn button(cmds: &[RenderCommand], label: &str) -> (Color, Color) {
        let mut fill = None;
        for c in cmds {
            match c {
                RenderCommand::FillRect { color, .. } => fill = Some(*color),
                RenderCommand::Text { text, color, .. } if text == label => {
                    return (fill.expect("a button's fill precedes its label"), *color);
                }
                _ => {}
            }
        }
        panic!("no button labelled {label:?} was drawn");
    }

    /// Every switch: a fully-rounded pill wider than it is tall.
    fn switch_fills(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    width,
                    height,
                    color,
                    corner_radii,
                    ..
                } if *width > *height && corner_radii.top_left * 2.0 == *height => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every radio ring and checkbox outline.
    fn outlines(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The chosen radio's dot — the 8x8 fill at x 16, *not* every 8x8 fill.
    ///
    /// The history tab's status badge is also 8x8, so a size-only pattern
    /// claims both. That is not a theoretical worry: it silently pulled the
    /// status badges out of [`colors_apart_from_the_controls`] below, which
    /// left the frozen-union check blind to a run's outcome following the
    /// accent. The x offset separates them (the dot lands at 40 and the
    /// badge at 32, both tabs being inset by the same cx = x + 24).
    fn radio_dots(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x: 40.0,
                    width: 8.0,
                    height: 8.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every ticked checkbox's tick.
    fn ticks(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, color, .. } if text == "\u{2713}" => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every colour the panel draws that no control above claimed.
    ///
    /// The radio dot is excluded *by position as well as size*. Dropping every
    /// 8x8 fill would also drop the history tab's status badges, and those
    /// belong in this vector: they are a category, and this check exists to
    /// prove a category does not follow the accent.
    fn colors_apart_from_the_controls(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter(|c| {
                !matches!(
                    c,
                    RenderCommand::Text {
                        y: 64.0,
                        font_size: 13.0,
                        ..
                    } | RenderCommand::StrokeRect { .. }
                        | RenderCommand::FillRect {
                            x: 40.0,
                            width: 8.0,
                            height: 8.0,
                            ..
                        }
                ) && !matches!(c, RenderCommand::Text { text, .. } if text == "\u{2713}")
                    && !matches!(
                        c,
                        RenderCommand::FillRect { width, height, corner_radii, .. }
                            if *width > *height && corner_radii.top_left * 2.0 == *height
                    )
                    // The three primary buttons and their labels.
                    && !matches!(
                        c,
                        RenderCommand::FillRect { width: 120.0, height: 36.0, .. }
                            | RenderCommand::FillRect { width: 100.0, height: 24.0, .. }
                            | RenderCommand::FillRect { width: 80.0, height: 24.0, .. }
                    )
                    && !matches!(
                        c,
                        RenderCommand::Text { text, .. }
                            if text == "Backup now" || text == "+ Add source" || text == "+ Add rule"
                    )
            })
            .filter_map(|c| match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Text { color, .. } => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every control that offers something follows the accent — each proved
    /// separately.
    ///
    /// Nine sites means nine `assert_ne!`s. Over their union one moving control
    /// would hide eight frozen ones, which is the failure FFF/NNN/WWW/EEEE
    /// established and this module has the most room for.
    ///
    /// The frozen half stays a single equality over everything else: an
    /// `assert_eq!` fails if *any* member moves, so it loses nothing and covers
    /// sites nobody thought to name.
    ///
    /// The three button *labels* are deliberately not among the nine, and an
    /// `assert_ne!` on them would be a bug in the test rather than a check:
    /// a label is `p.on_accent()`, which is `readable_on(accent)`, and every
    /// accent on offer is pale enough that all fourteen resolve to the same
    /// near-black. Correct code therefore draws the same label under any two
    /// accents. What actually distinguishes `p.on_accent()` from a frozen
    /// `p.crust` is the *mode*, so the labels are proved by
    /// [`each_buttons_label_is_legible_on_it`] instead, which asserts equality
    /// with `readable_on` across both modes.
    #[test]
    fn every_control_that_offers_something_follows_the_accent() {
        let mut a = Palette::for_mode(false);
        a.accent = appearance::MAUVE;
        let mut b = Palette::for_mode(false);
        b.accent = appearance::TEAL;

        // Walk every tab as the active one: the tab label's colour is chosen by
        // a boolean, so a fixture pinned to one tab leaves four unproven.
        for tab in BackupSettingsTab::all() {
            let ui = wound(*tab, true, BackupFrequency::Daily);
            let x = render(&ui, &a);
            let y = render(&ui, &b);

            assert_eq!(tab_labels(&x).len(), 5, "five tabs are labelled");
            assert_ne!(
                tab_labels(&x),
                tab_labels(&y),
                "the {tab:?} tab's label did not move with the accent"
            );

            match tab {
                BackupSettingsTab::Overview => {
                    let (fa, _) = button(&x, "Backup now");
                    let (fb, _) = button(&y, "Backup now");
                    assert_ne!(
                        (fa.r, fa.g, fa.b),
                        (fb.r, fb.g, fb.b),
                        "\"Backup now\" did not move with the accent"
                    );
                }
                BackupSettingsTab::Schedule => {
                    // Six pills are drawn here, but they are only *two* places
                    // in the source: the automatic-backup master switch, and
                    // the five retention switches in their loop. They are the
                    // same 40x20 shape at the same x, so nothing about a pill
                    // says which site emitted it — the master is simply the
                    // first, its row being the top of the tab. Splitting them
                    // is the point: one `assert_ne!` over all six would still
                    // pass with the retention loop frozen, because the master
                    // alone moving makes the vectors differ.
                    assert_eq!(
                        switch_fills(&x).len(),
                        6,
                        "one master + five retention switches"
                    );
                    assert_ne!(
                        switch_fills(&x).first(),
                        switch_fills(&y).first(),
                        "the automatic-backup switch did not move with the accent"
                    );
                    assert_ne!(
                        switch_fills(&x).get(1..),
                        switch_fills(&y).get(1..),
                        "no retention switch moved with the accent"
                    );
                    assert_eq!(outlines(&x).len(), 5, "five frequency radios are drawn");
                    assert_ne!(
                        outlines(&x),
                        outlines(&y),
                        "the frequency radio's ring did not move with the accent"
                    );
                    assert_eq!(radio_dots(&x).len(), 1, "one radio is chosen");
                    assert_ne!(
                        radio_dots(&x),
                        radio_dots(&y),
                        "the chosen frequency's dot did not move with the accent"
                    );
                }
                BackupSettingsTab::Sources => {
                    let (fa, _) = button(&x, "+ Add source");
                    let (fb, _) = button(&y, "+ Add source");
                    assert_ne!(
                        (fa.r, fa.g, fa.b),
                        (fb.r, fb.g, fb.b),
                        "\"+ Add source\" did not move with the accent"
                    );
                    assert_eq!(outlines(&x).len(), 2, "one checkbox per source");
                    assert_ne!(
                        outlines(&x),
                        outlines(&y),
                        "a source's checkbox did not move with the accent"
                    );
                    assert_eq!(ticks(&x).len(), 1, "one source is ticked");
                    assert_ne!(
                        ticks(&x),
                        ticks(&y),
                        "the ticked source's tick did not move with the accent"
                    );
                }
                BackupSettingsTab::Exclusions => {
                    let (fa, _) = button(&x, "+ Add rule");
                    let (fb, _) = button(&y, "+ Add rule");
                    assert_ne!(
                        (fa.r, fa.g, fa.b),
                        (fb.r, fb.g, fb.b),
                        "\"+ Add rule\" did not move with the accent"
                    );
                    assert_eq!(switch_fills(&x).len(), 2, "one switch per rule");
                    assert_ne!(
                        switch_fills(&x),
                        switch_fills(&y),
                        "an exclusion rule's switch did not move with the accent"
                    );
                }
                BackupSettingsTab::History => {}
            }

            assert_eq!(
                colors_apart_from_the_controls(&x),
                colors_apart_from_the_controls(&y),
                "something that is not a control moved with the accent \
                 (tab={tab:?}) — the outcome dots are a category, the four \
                 overview stats are a category, and the remove crosses are \
                 destructive rather than inviting"
            );
        }
    }

    /// The panel's own two surfaces are the palette's, in both modes.
    ///
    /// This is the one thing the membership sweep structurally cannot check.
    /// `assert_drawn_from` allows `0x11111B` and `0xEFF1F5` at any alpha,
    /// because those are the two answers [`appearance::readable_on`] can give
    /// and a legitimately-converted foreground will be one of them. But
    /// `0x11111B` is *also* Mocha's `crust` — so putting the literal back where
    /// `p.crust` belongs produces a render the sweep is obliged to accept. The
    /// same goes for `base`, one step lighter.
    ///
    /// Membership is the wrong question for these two. They are not "some
    /// palette colour", they are one specific role each, so the test names the
    /// role and asserts equality — which also fails in *dark* mode if the role
    /// is wrong, where a membership check could only ever fail in light.
    #[test]
    fn the_panels_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = render(
                &wound(BackupSettingsTab::Overview, true, BackupFrequency::Daily),
                &p,
            );

            let backdrop = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x: 0.0,
                    y: 0.0,
                    width: 700.0,
                    height: 800.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                backdrop,
                Some(p.base),
                "the panel's backdrop is not p.base (light={light})"
            );

            // The well the tab content sits in: the full-width inset at x 8.
            let well = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect {
                    x: 8.0,
                    width: 684.0,
                    color,
                    ..
                } => Some(*color),
                _ => None,
            });
            assert_eq!(
                well,
                Some(p.crust),
                "the content well is not p.crust (light={light})"
            );
        }
    }

    /// Each primary button's label is picked for its own fill, not fixed.
    ///
    /// The accent test above cannot reach this. Every accent on offer is pale,
    /// so [`appearance::readable_on`] answers the same near-black for all of
    /// them and an `assert_ne!` between two accents would fail on correct code.
    /// What separates `p.on_accent()` from a hard-coded `p.crust` is the
    /// *mode*: Latte's `crust` is near-white, which on a pale accent is
    /// illegible.
    #[test]
    fn each_buttons_label_is_legible_on_it() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                for (tab, label) in [
                    (BackupSettingsTab::Overview, "Backup now"),
                    (BackupSettingsTab::Sources, "+ Add source"),
                    (BackupSettingsTab::Exclusions, "+ Add rule"),
                ] {
                    let ui = wound(tab, true, BackupFrequency::Daily);
                    let (fill, text) = button(&render(&ui, &p), label);
                    assert_eq!(
                        (fill.r, fill.g, fill.b),
                        (accent.r, accent.g, accent.b),
                        "{label:?} is not filled with the accent (light={light})"
                    );
                    let want = appearance::readable_on(accent);
                    assert_eq!(
                        (text.r, text.g, text.b),
                        (want.r, want.g, want.b),
                        "{label:?}'s label is not chosen for its own fill \
                         (light={light}); a fixed colour is legible on one \
                         mode's accents and not the other's"
                    );
                }
            }
        }
    }

    /// The five backup outcomes stay tellable apart, under every accent and in
    /// both modes.
    ///
    /// They are drawn as dots down one scrollable list, so two outcomes sharing
    /// a colour do not merely confuse a learnt code — they make a failed backup
    /// look like a finished one in the same glance. Three of the five are hues
    /// that are also selectable accents, which is why the accent has to be
    /// varied and not merely defaulted.
    #[test]
    fn the_backup_outcomes_stay_distinct_under_every_accent() {
        for light in [false, true] {
            for accent in [
                appearance::BLUE,
                appearance::GREEN,
                appearance::RED,
                appearance::YELLOW,
                appearance::MAUVE,
            ] {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let mut seen: Vec<(BackupStatus, Color)> = Vec::new();
                for status in [
                    BackupStatus::Success,
                    BackupStatus::PartialSuccess,
                    BackupStatus::Failed,
                    BackupStatus::Cancelled,
                    BackupStatus::InProgress,
                ] {
                    let c = status.color(&p);
                    if let Some((other, _)) = seen
                        .iter()
                        .find(|(_, o)| o.r == c.r && o.g == c.g && o.b == c.b)
                    {
                        panic!(
                            "a {status:?} backup is marked exactly like a {other:?} \
                             one (light={light}, accent={accent:?}), so a failure \
                             reads as a success in the same list"
                        );
                    }
                    seen.push((status, c));
                }
            }
        }
    }
}
