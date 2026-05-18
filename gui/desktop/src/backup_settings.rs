//! Backup and restore settings panel for the desktop shell.
//!
//! Configures system backup behavior including backup schedules,
//! target locations, file inclusion/exclusion rules, retention
//! policies, and backup history with restore capabilities.

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

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
    NetworkShare { host: String, share: String, path: String },
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
    pub fn color(self) -> Color {
        match self {
            Self::Success => GREEN,
            Self::PartialSuccess => YELLOW,
            Self::Failed => RED,
            Self::Cancelled => OVERLAY0,
            Self::InProgress => BLUE,
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
    pub fn duration_display(&self) -> String {
        if self.duration_secs < 60 {
            format!("{}s", self.duration_secs)
        } else if self.duration_secs < 3600 {
            format!("{}m {}s", self.duration_secs / 60, self.duration_secs % 60)
        } else {
            let hours = self.duration_secs / 3600;
            let mins = (self.duration_secs % 3600) / 60;
            format!("{hours}h {mins}m")
        }
    }

    /// Format the timestamp as a simple date string.
    pub fn date_display(&self) -> String {
        let days = self.timestamp / 86400;
        let time_of_day = self.timestamp % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        format!("Day {days} {hours:02}:{minutes:02}")
    }
}

/// Format bytes for display.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
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
    pub next_backup_id: u64,
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
            next_backup_id: 1,
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

    /// Record a completed backup.
    pub fn record_backup(&mut self, entry: BackupHistoryEntry) {
        if entry.status == BackupStatus::Success || entry.status == BackupStatus::PartialSuccess {
            self.last_backup_timestamp = Some(entry.timestamp);
            self.total_backup_size = self
                .total_backup_size
                .saturating_add(entry.total_bytes);
        }
        self.history.push(entry);
        self.next_backup_id += 1;
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
    pub fn render(&self, x: f32, y: f32, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: BASE,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 20.0,
            text: "Backup & Restore".to_string(),
            font_size: 22.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Tab bar
        let tab_y = y + 56.0;
        let mut tab_x = x + 16.0;
        for tab in BackupSettingsTab::all() {
            let label = tab.label();
            let tw = label.len() as f32 * 8.0 + 24.0;
            let is_active = *tab == self.active_tab;

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: tab_x,
                    y: tab_y,
                    width: tw,
                    height: 32.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: tab_x + 12.0,
                y: tab_y + 8.0,
                text: label.to_string(),
                font_size: 13.0,
                color: if is_active { BLUE } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
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
            color: CRUST,
            corner_radii: CornerRadii::all(6.0),
        });

        let cx = x + 24.0;
        let cy = content_y + 16.0;
        let cw = width - 48.0;

        match self.active_tab {
            BackupSettingsTab::Overview => self.render_overview(&mut cmds, cx, cy, cw),
            BackupSettingsTab::Schedule => self.render_schedule(&mut cmds, cx, cy, cw),
            BackupSettingsTab::Sources => self.render_sources(&mut cmds, cx, cy, cw),
            BackupSettingsTab::Exclusions => self.render_exclusions(&mut cmds, cx, cy, cw),
            BackupSettingsTab::History => self.render_history(&mut cmds, cx, cy, cw),
        }

        cmds
    }

    /// Render overview tab.
    fn render_overview(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Status card
        let status_color = if self.settings.enabled { GREEN } else { OVERLAY0 };
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 80.0,
            color: SURFACE0,
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
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: x + 36.0,
            y: row_y + 44.0,
            text: self.settings.schedule_description(),
            font_size: 12.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 52.0),
        });
        row_y += 96.0;

        // Stats cards
        let stats = [
            (
                "Total backups",
                format!("{}", self.settings.history.len()),
                BLUE,
            ),
            (
                "Successful",
                format!("{}", self.settings.successful_backup_count()),
                GREEN,
            ),
            (
                "Failed",
                format!("{}", self.settings.failed_backup_count()),
                RED,
            ),
            (
                "Total size",
                format_bytes(self.settings.total_backup_size),
                LAVENDER,
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
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: row_y + 28.0,
                text: value.clone(),
                font_size: 18.0,
                color: *color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
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
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 22.0;

        for (label, value) in &items {
            cmds.push(RenderCommand::Text {
                x: x + 8.0,
                y: row_y,
                text: format!("{label}:"),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 120.0,
                y: row_y,
                text: value.clone(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 140.0),
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
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 20.0,
            y: row_y + 10.0,
            text: "Backup now".to_string(),
            font_size: 13.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render schedule tab.
    fn render_schedule(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        // Enable toggle
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 36.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 10.0,
            text: "Enable automatic backups".to_string(),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        let toggle_bg = if self.settings.enabled { BLUE } else { SURFACE2 };
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
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });

            // Radio button
            cmds.push(RenderCommand::StrokeRect {
                x: x + 12.0,
                y: row_y + 8.0,
                width: 16.0,
                height: 16.0,
                color: if is_active { BLUE } else { SURFACE2 },
                corner_radii: CornerRadii::all(8.0),
                line_width: 2.0,
            });

            if is_active {
                cmds.push(RenderCommand::FillRect {
                    x: x + 16.0,
                    y: row_y + 12.0,
                    width: 8.0,
                    height: 8.0,
                    color: BLUE,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 36.0,
                y: row_y + 8.0,
                text: freq.label().to_string(),
                font_size: 13.0,
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
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
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
                color: if is_active { SURFACE1 } else { SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 6.0,
                text: bt.label().to_string(),
                font_size: 13.0,
                color: if is_active { TEXT } else { SUBTEXT0 },
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 24.0,
                text: bt.description().to_string(),
                font_size: 10.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
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
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 22.0;

        for (label, enabled) in &options {
            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 32.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: label.to_string(),
                font_size: 12.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            let toggle_color = if *enabled { BLUE } else { SURFACE2 };
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
    fn render_sources(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
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
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Add source button
        cmds.push(RenderCommand::FillRect {
            x: x + width - 100.0,
            y: row_y - 4.0,
            width: 100.0,
            height: 24.0,
            color: BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 88.0,
            y: row_y,
            text: "+ Add source".to_string(),
            font_size: 11.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        // Target
        cmds.push(RenderCommand::FillRect {
            x,
            y: row_y,
            width,
            height: 48.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 6.0,
            text: format!("Target: {}", self.settings.target.kind_label()),
            font_size: 12.0,
            color: SUBTEXT1,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: row_y + 26.0,
            text: self.settings.target.display_path(),
            font_size: 12.0,
            color: BLUE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 32.0),
        });
        row_y += 60.0;

        // Source list
        for source in &self.settings.sources {
            let bg = if source.enabled { SURFACE0 } else { Color::rgba(49, 50, 68, 128) };

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
                color: if source.enabled { BLUE } else { SURFACE2 },
                corner_radii: CornerRadii::all(3.0),
                line_width: 2.0,
            });

            if source.enabled {
                cmds.push(RenderCommand::Text {
                    x: x + 14.0,
                    y: row_y + 12.0,
                    text: "\u{2713}".to_string(),
                    font_size: 12.0,
                    color: BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            cmds.push(RenderCommand::Text {
                x: x + 40.0,
                y: row_y + 12.0,
                text: source.path.clone(),
                font_size: 13.0,
                color: if source.enabled { TEXT } else { OVERLAY0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 120.0),
            });

            if source.include_subdirs {
                cmds.push(RenderCommand::Text {
                    x: x + width - 80.0,
                    y: row_y + 14.0,
                    text: "Recursive".to_string(),
                    font_size: 10.0,
                    color: OVERLAY0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Remove button
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 12.0,
                text: "\u{2715}".to_string(),
                font_size: 12.0,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            row_y += 46.0;
        }
    }

    /// Render exclusions tab.
    fn render_exclusions(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!(
                "Exclusion rules ({} active)",
                self.settings.active_exclude_count()
            ),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::FillRect {
            x: x + width - 80.0,
            y: row_y - 4.0,
            width: 80.0,
            height: 24.0,
            color: BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + width - 68.0,
            y: row_y,
            text: "+ Add rule".to_string(),
            font_size: 11.0,
            color: CRUST,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        for rule in &self.settings.exclude_rules {
            let bg = if rule.enabled { SURFACE0 } else { Color::rgba(49, 50, 68, 128) };

            cmds.push(RenderCommand::FillRect {
                x,
                y: row_y,
                width,
                height: 44.0,
                color: bg,
                corner_radii: CornerRadii::all(4.0),
            });

            // Toggle
            let toggle_bg = if rule.enabled { BLUE } else { SURFACE2 };
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
                width: rule.pattern.len() as f32 * 7.0 + 16.0,
                height: 20.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(3.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 64.0,
                y: row_y + 10.0,
                text: rule.pattern.clone(),
                font_size: 11.0,
                color: LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Description
            cmds.push(RenderCommand::Text {
                x: x + 56.0,
                y: row_y + 28.0,
                text: rule.description.clone(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 100.0),
            });

            // Remove
            cmds.push(RenderCommand::Text {
                x: x + width - 24.0,
                y: row_y + 14.0,
                text: "\u{2715}".to_string(),
                font_size: 12.0,
                color: RED,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            row_y += 50.0;
        }
    }

    /// Render history tab.
    fn render_history(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32) {
        let mut row_y = y;

        cmds.push(RenderCommand::Text {
            x,
            y: row_y,
            text: format!("Backup history ({} entries)", self.settings.history.len()),
            font_size: 14.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        row_y += 28.0;

        if self.settings.history.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + 16.0,
                y: row_y + 8.0,
                text: "No backups yet. Click \"Backup now\" to create your first backup."
                    .to_string(),
                font_size: 13.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 32.0),
            });
        } else {
            for entry in self.settings.history.iter().rev() {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: row_y,
                    width,
                    height: 56.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });

                // Status badge
                cmds.push(RenderCommand::FillRect {
                    x: x + 8.0,
                    y: row_y + 8.0,
                    width: 8.0,
                    height: 8.0,
                    color: entry.status.color(),
                    corner_radii: CornerRadii::all(4.0),
                });

                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: row_y + 4.0,
                    text: format!(
                        "{} — {}",
                        entry.backup_type.label(),
                        entry.status.label()
                    ),
                    font_size: 13.0,
                    color: TEXT,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                cmds.push(RenderCommand::Text {
                    x: x + 24.0,
                    y: row_y + 22.0,
                    text: format!(
                        "{} — {} files — {} — {}",
                        entry.date_display(),
                        entry.files_count,
                        entry.size_display(),
                        entry.duration_display()
                    ),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 40.0),
                });

                if let Some(ref err) = entry.error_message {
                    cmds.push(RenderCommand::Text {
                        x: x + 24.0,
                        y: row_y + 38.0,
                        text: err.clone(),
                        font_size: 10.0,
                        color: RED,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width - 40.0),
                    });
                }

                row_y += 62.0;
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let _c1 = BackupStatus::Success.color();
        let _c2 = BackupStatus::Failed.color();
        let _c3 = BackupStatus::InProgress.color();
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
        assert_eq!(entry.size_display(), "4.7 GB");
        assert_eq!(entry.duration_display(), "1h 1m");
        assert!(entry.date_display().contains("01:01"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(1_500_000), "1.4 MB");
        assert_eq!(format_bytes(2_000_000_000), "1.9 GB");
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
        let cmds = ui.render(0.0, 0.0, 600.0, 800.0);
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
}
