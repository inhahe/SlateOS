//! Storage management settings panel.
//!
//! Shows disk usage breakdown, configures default save locations, manages
//! temporary file cleanup, and provides storage sense — automatic space
//! reclamation policies.

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
// Storage category
// ============================================================================

/// Category of disk usage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StorageCategory {
    System,
    Apps,
    Documents,
    Media,
    Downloads,
    Trash,
    Temporary,
    PackageCache,
    Logs,
    Other,
}

impl StorageCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Apps => "Applications",
            Self::Documents => "Documents",
            Self::Media => "Photos & Media",
            Self::Downloads => "Downloads",
            Self::Trash => "Recycle Bin",
            Self::Temporary => "Temporary files",
            Self::PackageCache => "Package cache",
            Self::Logs => "Log files",
            Self::Other => "Other",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Self::System => BLUE,
            Self::Apps => LAVENDER,
            Self::Documents => GREEN,
            Self::Media => PEACH,
            Self::Downloads => YELLOW,
            Self::Trash => RED,
            Self::Temporary => OVERLAY0,
            Self::PackageCache => SUBTEXT0,
            Self::Logs => SURFACE1,
            Self::Other => SURFACE0,
        }
    }

    pub const ALL: [Self; 10] = [
        Self::System, Self::Apps, Self::Documents, Self::Media,
        Self::Downloads, Self::Trash, Self::Temporary, Self::PackageCache,
        Self::Logs, Self::Other,
    ];
}

/// Disk usage entry for one category.
#[derive(Clone, Debug)]
pub struct UsageEntry {
    pub category: StorageCategory,
    /// Bytes used.
    pub bytes: u64,
    /// Number of items (files+dirs).
    pub item_count: u64,
}

impl UsageEntry {
    pub fn new(category: StorageCategory, bytes: u64, items: u64) -> Self {
        Self { category, bytes, item_count: items }
    }
}

// ============================================================================
// Drive info
// ============================================================================

/// Information about a storage drive / partition.
#[derive(Clone, Debug)]
pub struct DriveInfo {
    pub mount_point: String,
    pub label: String,
    pub filesystem: String,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Used bytes.
    pub used_bytes: u64,
    /// Whether this is a removable drive.
    pub removable: bool,
    /// Per-category breakdown.
    pub categories: Vec<UsageEntry>,
}

impl DriveInfo {
    pub fn new(mount: &str, label: &str, fs: &str, total: u64, used: u64) -> Self {
        Self {
            mount_point: mount.into(),
            label: label.into(),
            filesystem: fs.into(),
            total_bytes: total,
            used_bytes: used,
            removable: false,
            categories: Vec::new(),
        }
    }

    pub fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    pub fn used_pct(&self) -> u32 {
        if self.total_bytes == 0 { return 0; }
        ((self.used_bytes as f64 / self.total_bytes as f64) * 100.0) as u32
    }

    pub fn is_low_space(&self) -> bool {
        self.used_pct() >= 90
    }
}

// ============================================================================
// Storage Sense configuration
// ============================================================================

/// How often Storage Sense runs automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenseFrequency {
    /// Only when disk space is low.
    WhenLow,
    Daily,
    Weekly,
    Monthly,
    Never,
}

impl SenseFrequency {
    pub fn label(self) -> &'static str {
        match self {
            Self::WhenLow => "When disk space is low",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Never => "Never (manual only)",
        }
    }

    pub const ALL: [Self; 5] = [Self::WhenLow, Self::Daily, Self::Weekly, Self::Monthly, Self::Never];
}

/// How long to keep files before auto-cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionPeriod {
    Never,
    OneDay,
    SevenDays,
    ThirtyDays,
    SixtyDays,
    NinetyDays,
}

impl RetentionPeriod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Never => "Never",
            Self::OneDay => "1 day",
            Self::SevenDays => "7 days",
            Self::ThirtyDays => "30 days",
            Self::SixtyDays => "60 days",
            Self::NinetyDays => "90 days",
        }
    }

    pub fn days(self) -> Option<u32> {
        match self {
            Self::Never => None,
            Self::OneDay => Some(1),
            Self::SevenDays => Some(7),
            Self::ThirtyDays => Some(30),
            Self::SixtyDays => Some(60),
            Self::NinetyDays => Some(90),
        }
    }

    pub const ALL: [Self; 6] = [Self::Never, Self::OneDay, Self::SevenDays, Self::ThirtyDays, Self::SixtyDays, Self::NinetyDays];
}

/// Storage Sense — automatic cleanup configuration.
#[derive(Clone, Debug)]
pub struct StorageSenseConfig {
    pub enabled: bool,
    pub frequency: SenseFrequency,
    /// Auto-delete temp files older than this.
    pub temp_retention: RetentionPeriod,
    /// Auto-empty recycle bin items older than this.
    pub trash_retention: RetentionPeriod,
    /// Auto-delete downloads older than this.
    pub downloads_retention: RetentionPeriod,
    /// Auto-clean package cache.
    pub clean_package_cache: bool,
    /// Auto-clean old log files.
    pub clean_logs: bool,
    /// Threshold percentage to trigger cleanup when frequency is WhenLow.
    pub low_space_threshold_pct: u32,
}

impl Default for StorageSenseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency: SenseFrequency::WhenLow,
            temp_retention: RetentionPeriod::SevenDays,
            trash_retention: RetentionPeriod::ThirtyDays,
            downloads_retention: RetentionPeriod::Never,
            clean_package_cache: true,
            clean_logs: true,
            low_space_threshold_pct: 90,
        }
    }
}

impl StorageSenseConfig {
    pub fn set_threshold(&mut self, pct: u32) {
        self.low_space_threshold_pct = pct.clamp(50, 99);
    }
}

// ============================================================================
// Default save locations
// ============================================================================

/// Default save locations for user content types.
#[derive(Clone, Debug)]
pub struct SaveLocations {
    pub documents: String,
    pub downloads: String,
    pub music: String,
    pub pictures: String,
    pub videos: String,
    pub desktop: String,
}

impl Default for SaveLocations {
    fn default() -> Self {
        Self {
            documents: "/home/user/Documents".into(),
            downloads: "/home/user/Downloads".into(),
            music: "/home/user/Music".into(),
            pictures: "/home/user/Pictures".into(),
            videos: "/home/user/Videos".into(),
            desktop: "/home/user/Desktop".into(),
        }
    }
}

// ============================================================================
// Storage settings manager
// ============================================================================

/// Full storage settings state.
pub struct StorageSettings {
    drives: Vec<DriveInfo>,
    sense: StorageSenseConfig,
    save_locations: SaveLocations,
}

impl StorageSettings {
    pub fn new() -> Self {
        Self {
            drives: Vec::new(),
            sense: StorageSenseConfig::default(),
            save_locations: SaveLocations::default(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut s = Self::new();
        let mut root = DriveInfo::new("/", "System", "ext4", 256_000_000_000, 85_000_000_000);
        root.categories = vec![
            UsageEntry::new(StorageCategory::System, 20_000_000_000, 50_000),
            UsageEntry::new(StorageCategory::Apps, 15_000_000_000, 2_000),
            UsageEntry::new(StorageCategory::Documents, 8_000_000_000, 12_000),
            UsageEntry::new(StorageCategory::Media, 25_000_000_000, 5_000),
            UsageEntry::new(StorageCategory::Downloads, 10_000_000_000, 800),
            UsageEntry::new(StorageCategory::Trash, 2_000_000_000, 150),
            UsageEntry::new(StorageCategory::Temporary, 3_000_000_000, 20_000),
            UsageEntry::new(StorageCategory::PackageCache, 1_500_000_000, 300),
            UsageEntry::new(StorageCategory::Logs, 500_000_000, 100),
        ];
        s.drives.push(root);
        s
    }

    pub fn add_drive(&mut self, drive: DriveInfo) {
        self.drives.push(drive);
    }

    pub fn remove_drive(&mut self, mount: &str) -> bool {
        let before = self.drives.len();
        self.drives.retain(|d| d.mount_point != mount);
        self.drives.len() < before
    }

    pub fn drives(&self) -> &[DriveInfo] {
        &self.drives
    }

    pub fn get_drive(&self, mount: &str) -> Option<&DriveInfo> {
        self.drives.iter().find(|d| d.mount_point == mount)
    }

    pub fn sense(&self) -> &StorageSenseConfig {
        &self.sense
    }

    pub fn sense_mut(&mut self) -> &mut StorageSenseConfig {
        &mut self.sense
    }

    pub fn save_locations(&self) -> &SaveLocations {
        &self.save_locations
    }

    pub fn save_locations_mut(&mut self) -> &mut SaveLocations {
        &mut self.save_locations
    }

    /// Total used across all drives.
    pub fn total_used(&self) -> u64 {
        self.drives.iter().map(|d| d.used_bytes).sum()
    }

    /// Total capacity across all drives.
    pub fn total_capacity(&self) -> u64 {
        self.drives.iter().map(|d| d.total_bytes).sum()
    }

    /// Whether any drive is in low-space condition.
    pub fn any_low_space(&self) -> bool {
        self.drives.iter().any(|d| d.is_low_space())
    }

    /// Estimate how much space can be reclaimed.
    pub fn estimated_reclaimable(&self) -> u64 {
        let mut total = 0_u64;
        for drive in &self.drives {
            for cat in &drive.categories {
                match cat.category {
                    StorageCategory::Trash | StorageCategory::Temporary => {
                        total = total.saturating_add(cat.bytes);
                    }
                    StorageCategory::PackageCache => {
                        if self.sense.clean_package_cache {
                            total = total.saturating_add(cat.bytes / 2);
                        }
                    }
                    StorageCategory::Logs => {
                        if self.sense.clean_logs {
                            total = total.saturating_add(cat.bytes);
                        }
                    }
                    _ => {}
                }
            }
        }
        total
    }
}

// ============================================================================
// Settings panel rendering
// ============================================================================

/// Format bytes as human-readable.
fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1} TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// UI state for the storage settings panel.
pub struct StorageSettingsUI {
    settings: StorageSettings,
    /// 0=Overview, 1=Storage Sense, 2=Save Locations.
    active_tab: usize,
    /// Which drive index is selected for detailed view.
    selected_drive: usize,
}

impl StorageSettingsUI {
    pub fn new() -> Self {
        Self {
            settings: StorageSettings::with_defaults(),
            active_tab: 0,
            selected_drive: 0,
        }
    }

    pub fn with_settings(settings: StorageSettings) -> Self {
        Self {
            settings,
            active_tab: 0,
            selected_drive: 0,
        }
    }

    pub fn settings(&self) -> &StorageSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut StorageSettings {
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

    pub fn selected_drive(&self) -> usize {
        self.selected_drive
    }

    pub fn select_drive(&mut self, idx: usize) {
        if idx < self.settings.drives().len() {
            self.selected_drive = idx;
        }
    }

    const TAB_LABELS: [&'static str; 3] = ["Overview", "Storage Sense", "Save Locations"];

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
            text: "Storage Settings".into(),
            font_size: 20.0, color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(inner),
        });
        cy += 32.0;

        // Warning if low space
        if self.settings.any_low_space() {
            cmds.push(RenderCommand::FillRect {
                x: x + pad, y: cy, width: inner, height: 28.0,
                color: Color::rgba(RED.r, RED.g, RED.b, 40),
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + pad + 10.0, y: cy + 6.0,
                text: "⚠ Low disk space — consider running cleanup".into(),
                font_size: 12.0, color: RED,
                font_weight: FontWeightHint::Bold,
                max_width: Some(inner - 20.0),
            });
            cy += 34.0;
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
            0 => self.render_overview(&mut cmds, x + pad, cy, inner),
            1 => self.render_sense_tab(&mut cmds, x + pad, cy, inner),
            2 => self.render_locations_tab(&mut cmds, x + pad, cy, inner),
            _ => {}
        };

        cmds
    }

    fn render_overview(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        for (i, drive) in self.settings.drives().iter().enumerate() {
            let selected = i == self.selected_drive;
            let bg = if selected { SURFACE0 } else { MANTLE };

            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 56.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            // Drive label + mount
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 6.0,
                text: format!("{} ({})", drive.label, drive.mount_point),
                font_size: 14.0, color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width * 0.55),
            });

            // Used / total
            cmds.push(RenderCommand::Text {
                x: x + width * 0.6, y: y + 6.0,
                text: format!("{} / {} ({}%)",
                    format_bytes(drive.used_bytes),
                    format_bytes(drive.total_bytes),
                    drive.used_pct()),
                font_size: 12.0,
                color: if drive.is_low_space() { RED } else { SUBTEXT0 },
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.38),
            });

            // Usage bar
            let bar_y = y + 30.0;
            cmds.push(RenderCommand::FillRect {
                x: x + 12.0, y: bar_y, width: width - 24.0, height: 12.0,
                color: SURFACE1,
                corner_radii: CornerRadii::all(6.0),
            });

            // Stacked category bars
            let bar_w = width - 24.0;
            let mut bx = x + 12.0;
            let total = drive.total_bytes.max(1) as f64;
            for cat in &drive.categories {
                let seg_w = (cat.bytes as f64 / total * bar_w as f64) as f32;
                if seg_w > 0.5 {
                    cmds.push(RenderCommand::FillRect {
                        x: bx, y: bar_y, width: seg_w, height: 12.0,
                        color: cat.category.color(),
                        corner_radii: CornerRadii::ZERO,
                    });
                    bx += seg_w;
                }
            }

            // FS type
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 44.0,
                text: format!("{}{}", drive.filesystem,
                    if drive.removable { " (removable)" } else { "" }),
                font_size: 10.0, color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width - 24.0),
            });

            y += 62.0;
        }

        // Category legend for selected drive
        if let Some(drive) = self.settings.drives().get(self.selected_drive) {
            y += 8.0;
            cmds.push(RenderCommand::Text {
                x, y,
                text: "Breakdown".into(),
                font_size: 14.0, color: LAVENDER,
                font_weight: FontWeightHint::Bold,
                max_width: Some(width),
            });
            y += 24.0;

            for cat in &drive.categories {
                // Color swatch
                cmds.push(RenderCommand::FillRect {
                    x, y: y + 2.0, width: 12.0, height: 12.0,
                    color: cat.category.color(),
                    corner_radii: CornerRadii::all(2.0),
                });
                cmds.push(RenderCommand::Text {
                    x: x + 18.0, y,
                    text: cat.category.label().into(),
                    font_size: 12.0, color: TEXT,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.4),
                });
                cmds.push(RenderCommand::Text {
                    x: x + width * 0.45, y,
                    text: format!("{}  ({} items)", format_bytes(cat.bytes), cat.item_count),
                    font_size: 12.0, color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width * 0.5),
                });
                y += 20.0;
            }

            // Reclaimable estimate
            y += 8.0;
            let reclaimable = self.settings.estimated_reclaimable();
            if reclaimable > 0 {
                cmds.push(RenderCommand::Text {
                    x, y,
                    text: format!("Estimated reclaimable: {}", format_bytes(reclaimable)),
                    font_size: 13.0, color: GREEN,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width),
                });
            }
        }
    }

    fn render_sense_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        let s = &self.settings.sense;

        Self::render_toggle(cmds, x, y, width, "Storage Sense enabled", s.enabled);
        y += 28.0;

        Self::render_kv(cmds, x, y, width, "Run frequency", s.frequency.label());
        y += 24.0;

        if s.frequency == SenseFrequency::WhenLow {
            Self::render_kv(cmds, x, y, width, "Trigger at", &format!("{}% used", s.low_space_threshold_pct));
            y += 24.0;
        }

        y += 8.0;
        cmds.push(RenderCommand::Text {
            x, y,
            text: "Auto-cleanup rules".into(),
            font_size: 14.0, color: LAVENDER,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
        });
        y += 24.0;

        Self::render_kv(cmds, x, y, width, "Delete temp files older than", s.temp_retention.label());
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Empty trash items older than", s.trash_retention.label());
        y += 24.0;
        Self::render_kv(cmds, x, y, width, "Delete downloads older than", s.downloads_retention.label());
        y += 24.0;
        Self::render_toggle(cmds, x, y, width, "Clean package cache", s.clean_package_cache);
        y += 28.0;
        Self::render_toggle(cmds, x, y, width, "Clean old log files", s.clean_logs);
    }

    fn render_locations_tab(&self, cmds: &mut Vec<RenderCommand>, x: f32, mut y: f32, width: f32) {
        let locs = &self.settings.save_locations;

        let entries: &[(&str, &str)] = &[
            ("Documents", &locs.documents),
            ("Downloads", &locs.downloads),
            ("Music", &locs.music),
            ("Pictures", &locs.pictures),
            ("Videos", &locs.videos),
            ("Desktop", &locs.desktop),
        ];

        for (label, path) in entries {
            cmds.push(RenderCommand::FillRect {
                x, y, width, height: 36.0,
                color: MANTLE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + 12.0, y: y + 4.0,
                text: (*label).into(),
                font_size: 13.0, color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.3),
            });
            cmds.push(RenderCommand::Text {
                x: x + width * 0.35, y: y + 4.0,
                text: (*path).into(),
                font_size: 13.0, color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width * 0.6),
            });
            // Change button placeholder
            cmds.push(RenderCommand::FillRect {
                x: x + width - 70.0, y: y + 6.0, width: 56.0, height: 22.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: x + width - 62.0, y: y + 10.0,
                text: "Change".into(),
                font_size: 11.0, color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(48.0),
            });
            y += 42.0;
        }
    }

    fn render_kv(cmds: &mut Vec<RenderCommand>, x: f32, y: f32, width: f32, key: &str, val: &str) {
        cmds.push(RenderCommand::Text {
            x: x + 8.0, y,
            text: key.into(),
            font_size: 13.0, color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.55),
        });
        cmds.push(RenderCommand::Text {
            x: x + width * 0.58, y,
            text: val.into(),
            font_size: 13.0, color: TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width * 0.4),
        });
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
    fn storage_category_labels() {
        for c in StorageCategory::ALL {
            assert!(!c.label().is_empty());
            let _ = c.color();
        }
    }

    #[test]
    fn drive_info_basics() {
        let d = DriveInfo::new("/", "Root", "ext4", 100_000, 60_000);
        assert_eq!(d.free_bytes(), 40_000);
        assert_eq!(d.used_pct(), 60);
        assert!(!d.is_low_space());
    }

    #[test]
    fn drive_low_space() {
        let d = DriveInfo::new("/", "Root", "ext4", 100_000, 95_000);
        assert!(d.is_low_space());
    }

    #[test]
    fn drive_zero_total() {
        let d = DriveInfo::new("/", "Empty", "ext4", 0, 0);
        assert_eq!(d.used_pct(), 0);
    }

    #[test]
    fn format_bytes_units() {
        assert!(format_bytes(500).contains("B"));
        assert!(format_bytes(1_500).contains("KB"));
        assert!(format_bytes(1_500_000).contains("MB"));
        assert!(format_bytes(1_500_000_000).contains("GB"));
        assert!(format_bytes(1_500_000_000_000).contains("TB"));
    }

    #[test]
    fn sense_frequency_labels() {
        for f in SenseFrequency::ALL {
            assert!(!f.label().is_empty());
        }
    }

    #[test]
    fn retention_labels_and_days() {
        assert_eq!(RetentionPeriod::Never.days(), None);
        assert_eq!(RetentionPeriod::SevenDays.days(), Some(7));
        assert_eq!(RetentionPeriod::ThirtyDays.days(), Some(30));
        for r in RetentionPeriod::ALL {
            assert!(!r.label().is_empty());
        }
    }

    #[test]
    fn sense_config_defaults() {
        let s = StorageSenseConfig::default();
        assert!(s.enabled);
        assert_eq!(s.frequency, SenseFrequency::WhenLow);
        assert_eq!(s.low_space_threshold_pct, 90);
    }

    #[test]
    fn sense_threshold_clamped() {
        let mut s = StorageSenseConfig::default();
        s.set_threshold(10);
        assert_eq!(s.low_space_threshold_pct, 50);
        s.set_threshold(100);
        assert_eq!(s.low_space_threshold_pct, 99);
    }

    #[test]
    fn save_locations_defaults() {
        let l = SaveLocations::default();
        assert!(l.documents.contains("Documents"));
        assert!(l.downloads.contains("Downloads"));
    }

    #[test]
    fn storage_settings_with_defaults() {
        let s = StorageSettings::with_defaults();
        assert_eq!(s.drives().len(), 1);
        assert!(s.total_capacity() > 0);
        assert!(s.total_used() > 0);
    }

    #[test]
    fn storage_add_remove_drive() {
        let mut s = StorageSettings::new();
        let d = DriveInfo::new("/data", "Data", "ext4", 500_000, 100_000);
        s.add_drive(d);
        assert_eq!(s.drives().len(), 1);
        assert!(s.remove_drive("/data"));
        assert!(s.drives().is_empty());
    }

    #[test]
    fn storage_get_drive() {
        let mut s = StorageSettings::new();
        s.add_drive(DriveInfo::new("/", "Root", "ext4", 100, 50));
        assert!(s.get_drive("/").is_some());
        assert!(s.get_drive("/nope").is_none());
    }

    #[test]
    fn storage_any_low_space() {
        let mut s = StorageSettings::new();
        s.add_drive(DriveInfo::new("/", "R", "ext4", 100, 50));
        assert!(!s.any_low_space());
        s.add_drive(DriveInfo::new("/d", "D", "ext4", 100, 95));
        assert!(s.any_low_space());
    }

    #[test]
    fn estimated_reclaimable() {
        let s = StorageSettings::with_defaults();
        let reclaimable = s.estimated_reclaimable();
        // Should be trash + temp + some pkg cache + logs
        assert!(reclaimable > 0);
    }

    #[test]
    fn estimated_reclaimable_sense_disabled_pkg() {
        let mut s = StorageSettings::with_defaults();
        s.sense_mut().clean_package_cache = false;
        s.sense_mut().clean_logs = false;
        let r = s.estimated_reclaimable();
        // Still has trash + temp
        assert!(r > 0);
    }

    #[test]
    fn ui_new() {
        let ui = StorageSettingsUI::new();
        assert_eq!(ui.active_tab(), 0);
        assert_eq!(ui.selected_drive(), 0);
    }

    #[test]
    fn ui_set_tab() {
        let mut ui = StorageSettingsUI::new();
        ui.set_active_tab(2);
        assert_eq!(ui.active_tab(), 2);
        ui.set_active_tab(99);
        assert_eq!(ui.active_tab(), 2);
    }

    #[test]
    fn ui_select_drive() {
        let mut ui = StorageSettingsUI::new();
        // Default has 1 drive at index 0.
        ui.select_drive(0);
        assert_eq!(ui.selected_drive(), 0);
        ui.select_drive(99); // out of range
        assert_eq!(ui.selected_drive(), 0);
    }

    #[test]
    fn ui_render_produces_commands() {
        let ui = StorageSettingsUI::new();
        let cmds = ui.render(0.0, 0.0, 500.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn ui_render_each_tab() {
        let mut ui = StorageSettingsUI::new();
        for i in 0..3 {
            ui.set_active_tab(i);
            let cmds = ui.render(0.0, 0.0, 500.0);
            assert!(!cmds.is_empty());
        }
    }

    #[test]
    fn ui_render_low_space_warning() {
        let mut s = StorageSettings::new();
        s.add_drive(DriveInfo::new("/", "Root", "ext4", 100_000, 95_000));
        let ui = StorageSettingsUI::with_settings(s);
        let cmds = ui.render(0.0, 0.0, 500.0);
        let has_warning = cmds.iter().any(|c| matches!(c, RenderCommand::Text { text, .. } if text.contains("Low disk")));
        assert!(has_warning);
    }

    #[test]
    fn usage_entry_new() {
        let e = UsageEntry::new(StorageCategory::System, 1000, 50);
        assert_eq!(e.bytes, 1000);
        assert_eq!(e.item_count, 50);
    }

    #[test]
    fn settings_mut_access() {
        let mut ui = StorageSettingsUI::new();
        ui.settings_mut().sense_mut().enabled = false;
        assert!(!ui.settings().sense().enabled);
    }
}
