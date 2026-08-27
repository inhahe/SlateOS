//! startupmanager -- Slate OS Startup Apps Manager
//!
//! A graphical application for managing programs that launch automatically
//! at system startup. Supports four startup categories (Login, Service,
//! Scheduled, Driver), boot-time impact estimation, enable/disable toggling,
//! and import/export of configurations in a simple line-based text format.
//!
//! # Architecture
//!
//! ```text
//! StartupEntry    -- a single startup item with metadata and status
//!       |
//!       v
//! StartupManager  -- collection of entries with CRUD, sort, search, stats
//!       |
//!       v
//! StartupConfig   -- import/export in line-based text format
//!       |
//!       v
//! StartupUI       -- guitk-based GUI with table, toolbar, details panel
//! ```

use std::collections::BTreeMap;
use std::process::ExitCode;

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::wheel;
use oswindow::app::{self, App, Response};

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
#[allow(dead_code)]
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT: Color = Color::from_hex(0xA6ADC8);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 650.0;
const HEADER_HEIGHT: f32 = 48.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const SEARCH_BAR_HEIGHT: f32 = 36.0;
const TABLE_HEADER_HEIGHT: f32 = 32.0;
const ROW_HEIGHT: f32 = 30.0;
const DETAILS_PANEL_HEIGHT: f32 = 120.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const PADDING: f32 = 12.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const BUTTON_WIDTH: f32 = 90.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;

// Column widths for the table
const COL_NAME_WIDTH: f32 = 180.0;
const COL_PUBLISHER_WIDTH: f32 = 140.0;
const COL_STATUS_WIDTH: f32 = 90.0;
const COL_IMPACT_WIDTH: f32 = 80.0;
const COL_TYPE_WIDTH: f32 = 90.0;
const COL_PATH_WIDTH: f32 = 260.0;

/// One toolbar button per `ToolbarAction`, and one table column per
/// `SortColumn`. Named so the layout's arrays are sized by the thing they
/// describe rather than by a literal that drifts when a variant is added.
const TOOLBAR_BUTTON_COUNT: usize = 5;
const COLUMN_COUNT: usize = 6;

/// Gap between adjacent buttons.
const BUTTON_GAP: f32 = 8.0;
/// Inset of the search field inside its strip, on all four sides.
const SEARCH_FIELD_INSET: f32 = 4.0;
/// Baseline-to-baseline distance in the details panel.
const DETAIL_LINE_SPACING: f32 = 18.0;

// Dialog geometry.
const DIALOG_WIDTH: f32 = 440.0;
const DIALOG_HEIGHT: f32 = 380.0;
const CONFIRM_WIDTH: f32 = 360.0;
const CONFIRM_HEIGHT: f32 = 160.0;
/// Top of the first form field, measured from the top of the dialog box.
const DIALOG_FIELD_TOP: f32 = 44.0;
/// Distance between the tops of consecutive form fields.
const FIELD_GAP: f32 = 42.0;
/// Height of a field's caption, above its input box.
const FIELD_LABEL_HEIGHT: f32 = 14.0;
const FIELD_INPUT_HEIGHT: f32 = 24.0;

// The type/impact cyclers on the row below the form fields, as offsets from
// the dialog's left content edge.
const SELECTOR_TYPE_VALUE_X: f32 = 80.0;
const SELECTOR_IMPACT_LABEL_X: f32 = 200.0;
const SELECTOR_IMPACT_VALUE_X: f32 = 280.0;
const SELECTOR_WIDTH: f32 = 110.0;
const SELECTOR_HEIGHT: f32 = 22.0;
/// How far a cycler's click box reaches above its text baseline.
const SELECTOR_PAD: f32 = 4.0;

// ============================================================================
// StartupType
// ============================================================================

/// Category of startup entry determining when/how it launches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StartupType {
    /// Runs when the user logs in.
    Login,
    /// Runs as a system service during boot.
    Service,
    /// Runs on a schedule (e.g., at first login of the day).
    Scheduled,
    /// Loaded as a driver during early boot.
    Driver,
}

impl StartupType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Service => "Service",
            Self::Scheduled => "Scheduled",
            Self::Driver => "Driver",
        }
    }

    /// Parse from a string label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "login" => Some(Self::Login),
            "service" => Some(Self::Service),
            "scheduled" => Some(Self::Scheduled),
            "driver" => Some(Self::Driver),
            _ => None,
        }
    }

    /// All startup type variants.
    pub fn all() -> &'static [Self] {
        &[Self::Login, Self::Service, Self::Scheduled, Self::Driver]
    }
}

// ============================================================================
// StartupImpact
// ============================================================================

/// Estimated impact on boot time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StartupImpact {
    /// No measurable impact.
    None,
    /// Minimal impact (< 0.5s).
    Low,
    /// Moderate impact (0.5s - 2s).
    Medium,
    /// Significant impact (> 2s).
    High,
}

impl StartupImpact {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// Parse from a string label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// Numeric weight for sorting and aggregate estimation.
    pub fn weight(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Low => 1,
            Self::Medium => 3,
            Self::High => 6,
        }
    }

    /// Color associated with this impact level.
    pub fn color(self) -> Color {
        match self {
            Self::None => COLOR_SUBTEXT,
            Self::Low => COLOR_GREEN,
            Self::Medium => COLOR_YELLOW,
            Self::High => COLOR_RED,
        }
    }

    /// All impact variants.
    pub fn all() -> &'static [Self] {
        &[Self::None, Self::Low, Self::Medium, Self::High]
    }
}

// ============================================================================
// StartupEntry
// ============================================================================

/// A single startup entry with all associated metadata.
#[derive(Clone, Debug)]
pub struct StartupEntry {
    /// Unique identifier.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Path to the executable.
    pub path: String,
    /// Command-line arguments.
    pub args: String,
    /// Category of startup.
    pub startup_type: StartupType,
    /// Whether the entry is enabled.
    pub enabled: bool,
    /// Estimated boot-time impact.
    pub impact: StartupImpact,
    /// Publisher / vendor name.
    pub publisher: String,
    /// Description of what this entry does.
    pub description: String,
    /// Timestamp (seconds since epoch) when this entry was added.
    pub added_timestamp: u64,
}

impl StartupEntry {
    /// Create a new startup entry with the given fields.
    pub fn new(
        id: u64,
        name: &str,
        path: &str,
        args: &str,
        startup_type: StartupType,
        impact: StartupImpact,
        publisher: &str,
        description: &str,
        added_timestamp: u64,
    ) -> Self {
        Self {
            id,
            name: name.to_string(),
            path: path.to_string(),
            args: args.to_string(),
            startup_type,
            enabled: true,
            impact,
            publisher: publisher.to_string(),
            description: description.to_string(),
            added_timestamp,
        }
    }

    /// Status label for display.
    pub fn status_label(&self) -> &'static str {
        if self.enabled { "Enabled" } else { "Disabled" }
    }

    /// Status color for display.
    pub fn status_color(&self) -> Color {
        if self.enabled {
            COLOR_GREEN
        } else {
            COLOR_OVERLAY0
        }
    }
}

// ============================================================================
// SortColumn / SortOrder
// ============================================================================

/// Which column to sort the table by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Publisher,
    Status,
    Impact,
    Type,
    Path,
}

impl SortColumn {
    /// Header text for the column.
    pub fn header(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Publisher => "Publisher",
            Self::Status => "Status",
            Self::Impact => "Impact",
            Self::Type => "Type",
            Self::Path => "Path",
        }
    }

    /// Column width in pixels.
    pub fn width(self) -> f32 {
        match self {
            Self::Name => COL_NAME_WIDTH,
            Self::Publisher => COL_PUBLISHER_WIDTH,
            Self::Status => COL_STATUS_WIDTH,
            Self::Impact => COL_IMPACT_WIDTH,
            Self::Type => COL_TYPE_WIDTH,
            Self::Path => COL_PATH_WIDTH,
        }
    }

    /// All columns in display order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Name,
            Self::Publisher,
            Self::Status,
            Self::Impact,
            Self::Type,
            Self::Path,
        ]
    }
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    /// Toggle the sort order.
    pub fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

// ============================================================================
// StartupStats
// ============================================================================

/// Aggregate statistics about startup entries.
#[derive(Clone, Debug, Default)]
pub struct StartupStats {
    pub total: usize,
    pub enabled: usize,
    pub disabled: usize,
    pub login_count: usize,
    pub service_count: usize,
    pub scheduled_count: usize,
    pub driver_count: usize,
    /// Estimated total impact weight (sum of enabled entry weights).
    pub total_impact_weight: u32,
}

impl StartupStats {
    /// Human-readable summary of overall boot impact.
    pub fn impact_summary(&self) -> &'static str {
        match self.total_impact_weight {
            0 => "Minimal",
            1..=5 => "Low",
            6..=15 => "Medium",
            16..=30 => "High",
            _ => "Very High",
        }
    }

    /// Color for the overall impact.
    pub fn impact_color(&self) -> Color {
        match self.total_impact_weight {
            0 => COLOR_SUBTEXT,
            1..=5 => COLOR_GREEN,
            6..=15 => COLOR_YELLOW,
            16..=30 => COLOR_PEACH,
            _ => COLOR_RED,
        }
    }
}

// ============================================================================
// StartupManager — core data model
// ============================================================================

/// Manages the collection of startup entries with CRUD, sort, search, stats.
pub struct StartupManager {
    entries: BTreeMap<u64, StartupEntry>,
    next_id: u64,
}

impl StartupManager {
    /// Create a new empty startup manager.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Add a new startup entry and return its assigned ID.
    pub fn add_entry(
        &mut self,
        name: &str,
        path: &str,
        args: &str,
        startup_type: StartupType,
        impact: StartupImpact,
        publisher: &str,
        description: &str,
        added_timestamp: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let entry = StartupEntry::new(
            id,
            name,
            path,
            args,
            startup_type,
            impact,
            publisher,
            description,
            added_timestamp,
        );
        self.entries.insert(id, entry);
        id
    }

    /// Remove an entry by ID. Returns `true` if the entry existed.
    pub fn remove_entry(&mut self, id: u64) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// Enable an entry by ID. Returns `true` if the entry existed.
    pub fn enable_entry(&mut self, id: u64) -> bool {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable an entry by ID. Returns `true` if the entry existed.
    pub fn disable_entry(&mut self, id: u64) -> bool {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.enabled = false;
            true
        } else {
            false
        }
    }

    /// Toggle the enabled state of an entry. Returns the new state, or `None`
    /// if the entry was not found.
    pub fn toggle_entry(&mut self, id: u64) -> Option<bool> {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.enabled = !entry.enabled;
            Some(entry.enabled)
        } else {
            Option::None
        }
    }

    /// Get an entry by ID.
    pub fn get_entry(&self, id: u64) -> Option<&StartupEntry> {
        self.entries.get(&id)
    }

    /// Get a mutable entry by ID.
    pub fn get_entry_mut(&mut self, id: u64) -> Option<&mut StartupEntry> {
        self.entries.get_mut(&id)
    }

    /// Update an existing entry's fields. Returns `true` if the entry existed.
    pub fn update_entry(
        &mut self,
        id: u64,
        name: &str,
        path: &str,
        args: &str,
        startup_type: StartupType,
        impact: StartupImpact,
        publisher: &str,
        description: &str,
    ) -> bool {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.name = name.to_string();
            entry.path = path.to_string();
            entry.args = args.to_string();
            entry.startup_type = startup_type;
            entry.impact = impact;
            entry.publisher = publisher.to_string();
            entry.description = description.to_string();
            true
        } else {
            false
        }
    }

    /// Number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get all entry IDs.
    pub fn entry_ids(&self) -> Vec<u64> {
        self.entries.keys().copied().collect()
    }

    /// Get sorted entries according to the given column and order.
    pub fn sorted_entries(&self, column: SortColumn, order: SortOrder) -> Vec<&StartupEntry> {
        let mut entries: Vec<&StartupEntry> = self.entries.values().collect();
        entries.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::Name => a
                    .name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Publisher => a
                    .publisher
                    .to_ascii_lowercase()
                    .cmp(&b.publisher.to_ascii_lowercase()),
                SortColumn::Status => a.enabled.cmp(&b.enabled),
                SortColumn::Impact => a.impact.cmp(&b.impact),
                SortColumn::Type => a.startup_type.cmp(&b.startup_type),
                SortColumn::Path => a
                    .path
                    .to_ascii_lowercase()
                    .cmp(&b.path.to_ascii_lowercase()),
            };
            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
        entries
    }

    /// Get entries filtered by a search query (case-insensitive name match).
    pub fn search_entries(&self, query: &str) -> Vec<&StartupEntry> {
        if query.is_empty() {
            return self.entries.values().collect();
        }
        let lower_query = query.to_ascii_lowercase();
        self.entries
            .values()
            .filter(|e| {
                e.name.to_ascii_lowercase().contains(&lower_query)
                    || e.publisher.to_ascii_lowercase().contains(&lower_query)
                    || e.path.to_ascii_lowercase().contains(&lower_query)
            })
            .collect()
    }

    /// Get entries filtered and sorted.
    pub fn filtered_sorted(
        &self,
        query: &str,
        column: SortColumn,
        order: SortOrder,
    ) -> Vec<&StartupEntry> {
        let mut entries = self.search_entries(query);
        entries.sort_by(|a, b| {
            let cmp = match column {
                SortColumn::Name => a
                    .name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Publisher => a
                    .publisher
                    .to_ascii_lowercase()
                    .cmp(&b.publisher.to_ascii_lowercase()),
                SortColumn::Status => a.enabled.cmp(&b.enabled),
                SortColumn::Impact => a.impact.cmp(&b.impact),
                SortColumn::Type => a.startup_type.cmp(&b.startup_type),
                SortColumn::Path => a
                    .path
                    .to_ascii_lowercase()
                    .cmp(&b.path.to_ascii_lowercase()),
            };
            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
        entries
    }

    /// Compute aggregate statistics.
    pub fn stats(&self) -> StartupStats {
        let mut s = StartupStats {
            total: self.entries.len(),
            ..StartupStats::default()
        };
        for entry in self.entries.values() {
            if entry.enabled {
                s.enabled = s.enabled.saturating_add(1);
                s.total_impact_weight = s.total_impact_weight.saturating_add(entry.impact.weight());
            } else {
                s.disabled = s.disabled.saturating_add(1);
            }
            let bucket = match entry.startup_type {
                StartupType::Login => &mut s.login_count,
                StartupType::Service => &mut s.service_count,
                StartupType::Scheduled => &mut s.scheduled_count,
                StartupType::Driver => &mut s.driver_count,
            };
            *bucket = bucket.saturating_add(1);
        }
        s
    }

    /// Populate with sample entries for demonstration.
    pub fn populate_sample_data(&mut self) {
        self.add_entry(
            "System Tray",
            "/usr/bin/systray",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "Slate OS",
            "System tray notification area",
            1700000000,
        );
        self.add_entry(
            "Network Manager",
            "/usr/sbin/networkd",
            "--daemon",
            StartupType::Service,
            StartupImpact::Medium,
            "Slate OS",
            "Manages network connections and interfaces",
            1700000100,
        );
        self.add_entry(
            "Audio Service",
            "/usr/sbin/audiod",
            "",
            StartupType::Service,
            StartupImpact::Low,
            "Slate OS",
            "Audio mixing and output service",
            1700000200,
        );
        self.add_entry(
            "Cloud Sync",
            "/opt/cloudsync/sync",
            "--background",
            StartupType::Login,
            StartupImpact::High,
            "CloudCorp",
            "Synchronizes files with cloud storage",
            1700000300,
        );
        self.add_entry(
            "Disk Monitor",
            "/usr/sbin/diskmond",
            "",
            StartupType::Scheduled,
            StartupImpact::None,
            "Slate OS",
            "Monitors disk health via SMART",
            1700000400,
        );
        self.add_entry(
            "GPU Driver",
            "/usr/lib/gpu/driver",
            "",
            StartupType::Driver,
            StartupImpact::Medium,
            "GPU Vendor",
            "Graphics processing unit kernel driver",
            1700000500,
        );
        self.add_entry(
            "Chat App",
            "/opt/chatapp/chat",
            "--minimize",
            StartupType::Login,
            StartupImpact::Medium,
            "ChatCo",
            "Instant messaging application",
            1700000600,
        );
        self.add_entry(
            "Bluetooth Service",
            "/usr/sbin/bluetoothd",
            "",
            StartupType::Service,
            StartupImpact::Low,
            "Slate OS",
            "Bluetooth device management service",
            1700000700,
        );
    }
}

impl Default for StartupManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// StartupConfig — import/export in line-based text format
// ============================================================================

/// Line-based text serialization for startup entries.
///
/// Format: one entry per line, fields separated by `|`:
/// ```text
/// VERSION|1
/// ENTRY|id|name|path|args|type|enabled|impact|publisher|description|timestamp
/// ```
///
/// Lines starting with `#` are comments and are ignored on import.
pub struct StartupConfig;

impl StartupConfig {
    /// Serialize a `StartupManager` to a line-based text format.
    pub fn serialize(manager: &StartupManager) -> String {
        let mut out = String::new();
        out.push_str("# Slate OS Startup Manager Configuration\n");
        out.push_str("VERSION|1\n");

        for entry in manager.entries.values() {
            out.push_str("ENTRY|");
            out.push_str(&entry.id.to_string());
            out.push('|');
            out.push_str(&Self::escape_field(&entry.name));
            out.push('|');
            out.push_str(&Self::escape_field(&entry.path));
            out.push('|');
            out.push_str(&Self::escape_field(&entry.args));
            out.push('|');
            out.push_str(entry.startup_type.label());
            out.push('|');
            out.push_str(if entry.enabled { "1" } else { "0" });
            out.push('|');
            out.push_str(entry.impact.label());
            out.push('|');
            out.push_str(&Self::escape_field(&entry.publisher));
            out.push('|');
            out.push_str(&Self::escape_field(&entry.description));
            out.push('|');
            out.push_str(&entry.added_timestamp.to_string());
            out.push('\n');
        }

        out
    }

    /// Deserialize a `StartupManager` from line-based text.
    pub fn deserialize(text: &str) -> Result<StartupManager, ConfigError> {
        let mut manager = StartupManager::new();
        let mut max_id: u64 = 0;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(ver_str) = trimmed.strip_prefix("VERSION|") {
                // Version check — we only support version 1.
                if ver_str.trim() != "1" {
                    return Err(ConfigError::UnsupportedVersion(ver_str.trim().to_string()));
                }
                continue;
            }

            if let Some(rest) = trimmed.strip_prefix("ENTRY|") {
                let fields = Self::split_escaped(rest, 10);
                let [
                    id_f,
                    name_f,
                    path_f,
                    args_f,
                    type_f,
                    enabled_f,
                    impact_f,
                    pub_f,
                    desc_f,
                    ts_f,
                ] = fields.as_slice()
                else {
                    return Err(ConfigError::MalformedEntry(trimmed.to_string()));
                };

                let id: u64 = id_f
                    .parse()
                    .map_err(|_| ConfigError::InvalidField("id".to_string()))?;
                let name = Self::unescape_field(name_f);
                let path = Self::unescape_field(path_f);
                let args = Self::unescape_field(args_f);
                let startup_type = StartupType::from_label(type_f)
                    .ok_or_else(|| ConfigError::InvalidField("type".to_string()))?;
                let enabled = enabled_f.as_str() == "1";
                let impact = StartupImpact::from_label(impact_f)
                    .ok_or_else(|| ConfigError::InvalidField("impact".to_string()))?;
                let publisher = Self::unescape_field(pub_f);
                let description = Self::unescape_field(desc_f);
                let added_timestamp: u64 = ts_f
                    .parse()
                    .map_err(|_| ConfigError::InvalidField("timestamp".to_string()))?;

                let entry = StartupEntry {
                    id,
                    name,
                    path,
                    args,
                    startup_type,
                    enabled,
                    impact,
                    publisher,
                    description,
                    added_timestamp,
                };
                manager.entries.insert(id, entry);

                if id >= max_id {
                    max_id = id;
                }
            }

            // Unknown line types are silently skipped for forward compatibility.
        }

        // Ensure next_id is beyond any imported entry.
        manager.next_id = max_id.saturating_add(1);
        Ok(manager)
    }

    /// Split a serialized ENTRY payload into at most `max` fields on `|`,
    /// honoring backslash escaping so an escaped pipe (`\|`) inside a field
    /// value is not treated as a separator. The escape sequences are left
    /// intact for [`Self::unescape_field`] to interpret. Mirrors `splitn`
    /// semantics: once `max - 1` separators have been consumed, the remainder
    /// (including any further pipes) becomes the final field.
    fn split_escaped(s: &str, max: usize) -> Vec<String> {
        let mut fields = Vec::new();
        let mut cur = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                // A backslash escapes the next char; keep both together so the
                // escaped pipe/backslash is preserved for later unescaping.
                cur.push('\\');
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            } else if ch == '|' && fields.len().saturating_add(1) < max {
                fields.push(core::mem::take(&mut cur));
            } else {
                cur.push(ch);
            }
        }
        fields.push(cur);
        fields
    }

    /// Escape pipe characters and backslashes within a field value.
    fn escape_field(s: &str) -> String {
        s.replace('\\', "\\\\").replace('|', "\\|")
    }

    /// Unescape pipe characters and backslashes within a field value.
    fn unescape_field(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    match next {
                        '|' => out.push('|'),
                        '\\' => out.push('\\'),
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                } else {
                    out.push('\\');
                }
            } else {
                out.push(ch);
            }
        }
        out
    }
}

/// Errors from config parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The version in the config is not supported.
    UnsupportedVersion(String),
    /// An ENTRY line has the wrong number of fields.
    MalformedEntry(String),
    /// A field could not be parsed.
    InvalidField(String),
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedVersion(v) => write!(f, "unsupported config version: {v}"),
            Self::MalformedEntry(line) => write!(f, "malformed entry line: {line}"),
            Self::InvalidField(field) => write!(f, "invalid field: {field}"),
        }
    }
}

// ============================================================================
// Dialog state types
// ============================================================================

/// Which dialog is currently open, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogState {
    /// No dialog is open.
    Closed,
    /// Add/edit entry dialog.
    AddEdit(AddEditDialog),
    /// Confirm delete dialog.
    ConfirmDelete(u64),
}

/// State for the add/edit entry dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddEditDialog {
    /// `None` for add, `Some(id)` for edit.
    pub editing_id: Option<u64>,
    pub name: String,
    pub path: String,
    pub args: String,
    pub startup_type_index: usize,
    pub impact_index: usize,
    pub publisher: String,
    pub description: String,
    /// Which field is currently focused (0=name, 1=path, 2=args, 3=publisher, 4=description).
    pub focused_field: usize,
}

impl AddEditDialog {
    /// Create a blank dialog for adding a new entry.
    pub fn new_add() -> Self {
        Self {
            editing_id: Option::None,
            name: String::new(),
            path: String::new(),
            args: String::new(),
            startup_type_index: 0,
            impact_index: 1, // Default to Low
            publisher: String::new(),
            description: String::new(),
            focused_field: 0,
        }
    }

    /// Create a dialog pre-filled for editing an existing entry.
    pub fn new_edit(entry: &StartupEntry) -> Self {
        let startup_type_index = StartupType::all()
            .iter()
            .position(|&t| t == entry.startup_type)
            .unwrap_or(0);
        let impact_index = StartupImpact::all()
            .iter()
            .position(|&i| i == entry.impact)
            .unwrap_or(0);
        Self {
            editing_id: Some(entry.id),
            name: entry.name.clone(),
            path: entry.path.clone(),
            args: entry.args.clone(),
            startup_type_index,
            impact_index,
            publisher: entry.publisher.clone(),
            description: entry.description.clone(),
            focused_field: 0,
        }
    }

    /// Get the selected startup type.
    pub fn selected_type(&self) -> StartupType {
        StartupType::all()
            .get(self.startup_type_index)
            .copied()
            .unwrap_or(StartupType::Login)
    }

    /// Get the selected impact level.
    pub fn selected_impact(&self) -> StartupImpact {
        StartupImpact::all()
            .get(self.impact_index)
            .copied()
            .unwrap_or(StartupImpact::Low)
    }

    /// Validate the dialog fields. Returns an error message if invalid.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() {
            return Err("Name is required");
        }
        if self.path.trim().is_empty() {
            return Err("Path is required");
        }
        Ok(())
    }

    /// Cycle the startup type forward.
    pub fn next_type(&mut self) {
        self.startup_type_index = self
            .startup_type_index
            .saturating_add(1)
            .checked_rem(StartupType::all().len())
            .unwrap_or(0);
    }

    /// Cycle the impact level forward.
    pub fn next_impact(&mut self) {
        self.impact_index = self
            .impact_index
            .saturating_add(1)
            .checked_rem(StartupImpact::all().len())
            .unwrap_or(0);
    }

    /// Number of text fields that can be focused.
    pub const FIELD_COUNT: usize = 5;

    /// The text fields in focus order, as `(caption, value)`.
    ///
    /// The renderer, the hit test and `focused_text_mut` all walk this one
    /// list, so a sixth field cannot be drawn without also being typeable.
    pub fn fields(&self) -> [(&'static str, &str); Self::FIELD_COUNT] {
        [
            ("Name", &self.name),
            ("Path", &self.path),
            ("Arguments", &self.args),
            ("Publisher", &self.publisher),
            ("Description", &self.description),
        ]
    }

    /// The focused field's text, to type into.
    pub fn focused_text_mut(&mut self) -> &mut String {
        match self.focused_field {
            1 => &mut self.path,
            2 => &mut self.args,
            3 => &mut self.publisher,
            4 => &mut self.description,
            // 0 -- and anything a stale index could hold. Name is the field the
            // dialog opens on, so it is where a stray keystroke does least harm.
            _ => &mut self.name,
        }
    }

    /// Number of text fields that can be focused.
    pub fn field_count(&self) -> usize {
        Self::FIELD_COUNT
    }

    /// Move focus to the next field.
    pub fn focus_next(&mut self) {
        self.focused_field = self
            .focused_field
            .saturating_add(1)
            .checked_rem(Self::FIELD_COUNT)
            .unwrap_or(0);
    }

    /// Move focus to the previous field.
    pub fn focus_prev(&mut self) {
        self.focused_field = if self.focused_field == 0 {
            Self::FIELD_COUNT.saturating_sub(1)
        } else {
            self.focused_field.saturating_sub(1)
        };
    }
}

// ============================================================================
// StartupUI — GUI state, layout, events and rendering
// ============================================================================

/// One of the five toolbar actions.
///
/// A named action rather than a button index, because the keyboard reaches the
/// same five things (`Delete`, `F5`, `Ctrl+N`) and an index would make the two
/// routes agree only by counting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Open the add dialog.
    Add,
    /// Open the confirm-delete dialog for the selected entry.
    Remove,
    /// Enable the selected entry.
    Enable,
    /// Disable the selected entry.
    Disable,
    /// Re-read the list: drop a stale selection and pull the viewport back in.
    Refresh,
}

impl ToolbarAction {
    /// Button text.
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Remove => "Remove",
            Self::Enable => "Enable",
            Self::Disable => "Disable",
            Self::Refresh => "Refresh",
        }
    }

    /// Accent colour for the button.
    fn color(self) -> Color {
        match self {
            Self::Add => COLOR_BLUE,
            Self::Remove => COLOR_RED,
            Self::Enable => COLOR_GREEN,
            Self::Disable => COLOR_PEACH,
            Self::Refresh => COLOR_SUBTEXT,
        }
    }

    /// All actions, in toolbar order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Add,
            Self::Remove,
            Self::Enable,
            Self::Disable,
            Self::Refresh,
        ]
    }
}

/// Every control the window draws, and the whole vocabulary the event handlers
/// speak.
///
/// The renderer records a hit box for each of these *as it draws it*, so a
/// test can ask "where is Save?" rather than recomputing the dialog geometry
/// and agreeing with the renderer by luck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The search field. Clicking it takes the caret; typing then filters.
    Search,
    /// A toolbar button.
    Toolbar(ToolbarAction),
    /// A table column header — clicking sorts by it, or flips the order.
    Column(SortColumn),
    /// A table row, named by entry **id** and not by position: the row under
    /// the pointer is the row that gets selected even after a re-sort has
    /// moved it since the frame was drawn.
    Row(u64),
    /// The table viewport. Recorded *under* the rows, so the wheel works over
    /// the empty space below the last one and a click there deselects.
    Table,
    /// A text field of the add/edit dialog, by index into its field order.
    DialogField(usize),
    /// The type cycler in the add/edit dialog.
    DialogType,
    /// The impact cycler in the add/edit dialog.
    DialogImpact,
    /// The add/edit dialog's Save button.
    DialogSave,
    /// The add/edit dialog's Cancel button.
    DialogCancel,
    /// The confirm-delete dialog's Delete button.
    DeleteConfirm,
    /// The confirm-delete dialog's Cancel button.
    DeleteCancel,
    /// Everything an open dialog covers. It swallows the click rather than
    /// letting it fall through to the table underneath.
    Scrim,
}

/// The frame type this app draws into.
type Frame = guitk::frame::Frame<Target>;

/// A width or height that arrived from outside, made usable.
///
/// A window can be handed a zero, and a compositor that has lost its mind can
/// hand over a NaN; either would propagate through every rectangle below.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Take `want` pixels off the top of what is left between `y` and `limit`, or
/// whatever is left if there is less than that.
///
/// Layouts here **shrink; they never clamp.** `Frame` does not clip to the
/// window, so a rectangle that kept its full height in a short window would
/// record a hit box below the bottom edge — a control you could click by
/// clicking nothing.
fn take_top(y: &mut f32, limit: f32, width: f32, want: f32) -> Rect {
    let h = want.min((limit - *y).max(0.0));
    let r = Rect::new(0.0, *y, width, h);
    *y += h;
    r
}

/// The mirror of [`take_top`], working up from `bottom` and never crossing
/// `floor` (the point the top-down half of the layout reached).
fn take_bottom(bottom: &mut f32, floor: f32, width: f32, want: f32) -> Rect {
    let h = want.min((*bottom - floor).max(0.0));
    *bottom -= h;
    Rect::new(0.0, *bottom, width, h)
}

/// A `want_w` x `want_h` box centred in the window, shrunk to fit if the
/// window is smaller than it.
fn centred(width: f32, height: f32, want_w: f32, want_h: f32) -> Rect {
    let w = want_w.min(width);
    let h = want_h.min(height);
    Rect::new(
        ((width - w) / 2.0).max(0.0),
        ((height - h) / 2.0).max(0.0),
        w,
        h,
    )
}

/// Trim `r` to `bounds`, collapsing it if nothing is left.
fn trim(r: Rect, bounds: Rect) -> Rect {
    r.intersect(bounds).unwrap_or(Rect::EMPTY)
}

/// Where everything sits for one particular window size.
///
/// Derived from the live size on every frame and never remembered: a stored
/// layout is a layout that disagrees with the window the moment it is resized.
struct Layout {
    /// The sanitised window width the rest of these rectangles were built for.
    width: f32,
    /// The sanitised window height.
    height: f32,
    header: Rect,
    toolbar: Rect,
    buttons: [Rect; TOOLBAR_BUTTON_COUNT],
    search_bar: Rect,
    search: Rect,
    table_header: Rect,
    columns: [Rect; COLUMN_COUNT],
    table: Rect,
    details: Rect,
    status: Rect,
    dialog: Rect,
    confirm: Rect,
}

impl Layout {
    fn new(width: f32, height: f32) -> Self {
        let width = sane(width);
        let height = sane(height);

        // Chrome from the top, then chrome from the bottom; the table gets
        // what is left between them, which may legitimately be nothing.
        let mut y = 0.0_f32;
        let header = take_top(&mut y, height, width, HEADER_HEIGHT);
        let toolbar = take_top(&mut y, height, width, TOOLBAR_HEIGHT);
        let search_bar = take_top(&mut y, height, width, SEARCH_BAR_HEIGHT);
        let table_header = take_top(&mut y, height, width, TABLE_HEADER_HEIGHT);

        let mut b = height;
        let status = take_bottom(&mut b, y, width, STATUS_BAR_HEIGHT);
        let details = take_bottom(&mut b, y, width, DETAILS_PANEL_HEIGHT);
        let table = Rect::new(0.0, y, width, (b - y).max(0.0));

        let by = toolbar.y + ((toolbar.h - BUTTON_HEIGHT) / 2.0).max(0.0);
        let bh = BUTTON_HEIGHT.min(toolbar.h);
        let mut buttons = [Rect::EMPTY; TOOLBAR_BUTTON_COUNT];
        for (i, slot) in buttons.iter_mut().enumerate() {
            let bx = PADDING + i as f32 * (BUTTON_WIDTH + BUTTON_GAP);
            *slot = trim(Rect::new(bx, by, BUTTON_WIDTH, bh), toolbar);
        }

        let search = trim(
            Rect::new(
                PADDING,
                search_bar.y + SEARCH_FIELD_INSET,
                (width - PADDING * 2.0).max(0.0),
                (SEARCH_BAR_HEIGHT - SEARCH_FIELD_INSET * 2.0).max(0.0),
            ),
            search_bar,
        );

        let mut columns = [Rect::EMPTY; COLUMN_COUNT];
        let mut cx = PADDING;
        for (slot, col) in columns.iter_mut().zip(SortColumn::all()) {
            *slot = trim(
                Rect::new(cx, table_header.y, col.width(), table_header.h),
                table_header,
            );
            cx += col.width();
        }

        Self {
            width,
            height,
            header,
            toolbar,
            buttons,
            search_bar,
            search,
            table_header,
            columns,
            table,
            details,
            status,
            dialog: centred(width, height, DIALOG_WIDTH, DIALOG_HEIGHT),
            confirm: centred(width, height, CONFIRM_WIDTH, CONFIRM_HEIGHT),
        }
    }

    /// The whole window, for the scrim and for bounds checks.
    fn window(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    /// How many table rows fit in the viewport.
    fn rows(&self) -> usize {
        if ROW_HEIGHT <= 0.0 {
            return 0;
        }
        (self.table.h / ROW_HEIGHT) as usize
    }

    /// Screen rectangle of the `i`th row *of the viewport* (not of the list).
    fn row(&self, i: usize) -> Rect {
        trim(
            Rect::new(
                self.table.x,
                self.table.y + i as f32 * ROW_HEIGHT,
                self.table.w,
                ROW_HEIGHT,
            ),
            self.table,
        )
    }

    /// Top edge of the `i`th labelled block in the add/edit dialog. Index
    /// `FIELD_COUNT` is the type/impact selector row that follows the fields.
    fn dialog_field_top(&self, i: usize) -> f32 {
        self.dialog.y + DIALOG_FIELD_TOP + i as f32 * FIELD_GAP
    }

    /// The input box of the `i`th text field.
    fn dialog_field(&self, i: usize) -> Rect {
        trim(
            Rect::new(
                self.dialog.x + PADDING,
                self.dialog_field_top(i) + FIELD_LABEL_HEIGHT,
                (self.dialog.w - PADDING * 2.0).max(0.0),
                FIELD_INPUT_HEIGHT,
            ),
            self.dialog,
        )
    }

    /// Baseline of the type/impact selector row.
    fn selector_y(&self) -> f32 {
        self.dialog_field_top(AddEditDialog::FIELD_COUNT)
    }

    /// Click box of the type cycler.
    fn dialog_type(&self) -> Rect {
        self.selector_box(SELECTOR_TYPE_VALUE_X)
    }

    /// Click box of the impact cycler.
    fn dialog_impact(&self) -> Rect {
        self.selector_box(SELECTOR_IMPACT_VALUE_X)
    }

    fn selector_box(&self, dx: f32) -> Rect {
        trim(
            Rect::new(
                self.dialog.x + PADDING + dx,
                self.selector_y() - SELECTOR_PAD,
                SELECTOR_WIDTH,
                SELECTOR_HEIGHT,
            ),
            self.dialog,
        )
    }

    /// The `(cancel, confirm)` pair along the bottom of a dialog box.
    fn buttons_in(dlg: Rect) -> (Rect, Rect) {
        let y = dlg.bottom() - BUTTON_HEIGHT - PADDING;
        let cancel = Rect::new(
            dlg.right() - BUTTON_WIDTH * 2.0 - PADDING - BUTTON_GAP,
            y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        );
        let confirm = Rect::new(
            dlg.right() - BUTTON_WIDTH - PADDING,
            y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
        );
        (trim(cancel, dlg), trim(confirm, dlg))
    }

    /// `(Cancel, Save)` of the add/edit dialog.
    fn dialog_buttons(&self) -> (Rect, Rect) {
        Self::buttons_in(self.dialog)
    }

    /// `(Cancel, Delete)` of the confirm-delete dialog.
    fn confirm_buttons(&self) -> (Rect, Rect) {
        Self::buttons_in(self.confirm)
    }
}

/// Full application state for the startup manager UI.
pub struct StartupUI {
    pub manager: StartupManager,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    pub search_query: String,
    pub selected_id: Option<u64>,
    pub dialog: DialogState,
    /// First visible row of the table, as an index into the filtered list.
    pub scroll_offset: usize,
    pub window_width: f32,
    pub window_height: f32,
    /// Whether typing goes to the search box. A click puts the caret there and
    /// a click anywhere else takes it away, so keystrokes meant for the table
    /// cannot silently filter it instead.
    pub search_focused: bool,
    /// The last thing the app has to say — a validation refusal, or the result
    /// of an action. Drawn in the dialog footer while a dialog is open, and in
    /// the header otherwise.
    pub status: String,
}

impl StartupUI {
    /// Create a new UI with sample data.
    pub fn new() -> Self {
        let mut manager = StartupManager::new();
        manager.populate_sample_data();
        Self {
            manager,
            sort_column: SortColumn::Name,
            sort_order: SortOrder::Ascending,
            search_query: String::new(),
            selected_id: Option::None,
            dialog: DialogState::Closed,
            scroll_offset: 0,
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
            search_focused: false,
            status: String::new(),
        }
    }

    // ========================================================================
    // Geometry
    // ========================================================================

    /// The layout for the size the window is currently believed to be.
    fn layout(&self) -> Layout {
        Layout::new(self.window_width, self.window_height)
    }

    /// How many table rows fit in the visible area.
    pub fn visible_rows(&self) -> usize {
        self.layout().rows()
    }

    /// The filtered, sorted list in full.
    fn all_entries(&self) -> Vec<&StartupEntry> {
        self.manager
            .filtered_sorted(&self.search_query, self.sort_column, self.sort_order)
    }

    /// The slice of the list that `l`'s viewport shows.
    fn entries_in(&self, l: &Layout) -> Vec<&StartupEntry> {
        let all = self.all_entries();
        let start = self.scroll_offset.min(all.len());
        let end = start.saturating_add(l.rows()).min(all.len());
        all.get(start..end).unwrap_or(&[]).to_vec()
    }

    /// Get the currently visible filtered and sorted entries.
    pub fn visible_entries(&self) -> Vec<&StartupEntry> {
        self.entries_in(&self.layout())
    }

    /// Total number of filtered entries.
    pub fn filtered_count(&self) -> usize {
        self.manager.search_entries(&self.search_query).len()
    }

    /// The largest first-visible-row index that still fills the viewport.
    fn max_scroll(&self) -> usize {
        self.filtered_count().saturating_sub(self.visible_rows())
    }

    /// Pull the offset back inside its bounds after the list or the viewport
    /// changed shape under it.
    pub fn clamp_scroll(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.max_scroll());
    }

    /// Adopt a new window size and pull anything that hung off the old one
    /// back inside.
    fn resize(&mut self, width: f32, height: f32) {
        self.window_width = sane(width);
        self.window_height = sane(height);
        self.clamp_scroll();
    }

    /// Scroll the viewport so the entry at list position `pos` is on screen.
    fn scroll_into_view(&mut self, pos: usize) {
        let vis = self.visible_rows();
        if pos < self.scroll_offset {
            self.scroll_offset = pos;
        } else if vis > 0 && pos >= self.scroll_offset.saturating_add(vis) {
            self.scroll_offset = pos.saturating_sub(vis).saturating_add(1);
        }
        self.clamp_scroll();
    }

    /// Position of the selected entry in the filtered list, if it is in it.
    fn selected_pos(&self) -> Option<usize> {
        let id = self.selected_id?;
        self.all_entries().iter().position(|e| e.id == id)
    }

    // ========================================================================
    // Commands
    // ========================================================================

    /// Sort by the given column, toggling order if same column.
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_column = column;
            self.sort_order = SortOrder::Ascending;
        }
    }

    /// Move the selection to list position `pos`, dragging the viewport along.
    fn select_pos(&mut self, pos: usize) {
        let id = self.all_entries().get(pos).map(|e| e.id);
        if let Some(id) = id {
            self.selected_id = Some(id);
            self.scroll_into_view(pos);
        }
    }

    /// Select the next entry in the filtered list.
    pub fn select_next(&mut self) {
        let len = self.all_entries().len();
        if len == 0 {
            return;
        }
        let next = match self.selected_pos() {
            Option::None => 0,
            Some(pos) => pos.saturating_add(1).min(len.saturating_sub(1)),
        };
        self.select_pos(next);
    }

    /// Select the previous entry in the filtered list.
    pub fn select_prev(&mut self) {
        if self.all_entries().is_empty() {
            return;
        }
        let prev = self.selected_pos().unwrap_or(0).saturating_sub(1);
        self.select_pos(prev);
    }

    /// Open the add dialog.
    pub fn open_add_dialog(&mut self) {
        self.status.clear();
        self.dialog = DialogState::AddEdit(AddEditDialog::new_add());
    }

    /// Open the edit dialog for the selected entry.
    pub fn open_edit_dialog(&mut self) {
        if let Some(id) = self.selected_id
            && let Some(entry) = self.manager.get_entry(id)
        {
            self.dialog = DialogState::AddEdit(AddEditDialog::new_edit(entry));
            self.status.clear();
        }
    }

    /// Open the confirm-delete dialog for the selected entry.
    pub fn open_delete_dialog(&mut self) {
        if let Some(id) = self.selected_id
            && self.manager.get_entry(id).is_some()
        {
            self.dialog = DialogState::ConfirmDelete(id);
            self.status.clear();
        }
    }

    /// Close any open dialog.
    pub fn close_dialog(&mut self) {
        self.dialog = DialogState::Closed;
        self.status.clear();
    }

    /// Confirm adding/editing from the dialog.
    pub fn confirm_add_edit(&mut self) -> Result<(), &'static str> {
        let dlg = match &self.dialog {
            DialogState::AddEdit(d) => d.clone(),
            _ => return Err("No add/edit dialog open"),
        };
        dlg.validate()?;

        if let Some(id) = dlg.editing_id {
            self.manager.update_entry(
                id,
                dlg.name.trim(),
                dlg.path.trim(),
                dlg.args.trim(),
                dlg.selected_type(),
                dlg.selected_impact(),
                dlg.publisher.trim(),
                dlg.description.trim(),
            );
        } else {
            let new_id = self.manager.add_entry(
                dlg.name.trim(),
                dlg.path.trim(),
                dlg.args.trim(),
                dlg.selected_type(),
                dlg.selected_impact(),
                dlg.publisher.trim(),
                dlg.description.trim(),
                0, // Timestamp would come from system clock in production.
            );
            self.selected_id = Some(new_id);
        }

        self.dialog = DialogState::Closed;
        self.clamp_scroll();
        Ok(())
    }

    /// Confirm deletion from the dialog.
    pub fn confirm_delete(&mut self) {
        if let DialogState::ConfirmDelete(id) = self.dialog {
            self.manager.remove_entry(id);
            if self.selected_id == Some(id) {
                self.selected_id = Option::None;
            }
        }
        self.dialog = DialogState::Closed;
        self.clamp_scroll();
    }

    /// Enable the selected entry.
    pub fn enable_selected(&mut self) {
        if let Some(id) = self.selected_id {
            self.manager.enable_entry(id);
        }
    }

    /// Disable the selected entry.
    pub fn disable_selected(&mut self) {
        if let Some(id) = self.selected_id {
            self.manager.disable_entry(id);
        }
    }

    /// Drop a selection that no longer names a live entry and pull the
    /// viewport back inside the list.
    ///
    /// In a build that read the real system this would re-scan it; here the
    /// manager *is* the list, so the only honest work left is the tidying —
    /// which is exactly what a refresh is for after an entry disappears.
    pub fn refresh(&mut self) {
        if let Some(id) = self.selected_id
            && self.manager.get_entry(id).is_none()
        {
            self.selected_id = Option::None;
        }
        self.clamp_scroll();
        self.status = format!("{} entries", self.manager.entry_count());
    }

    /// Run a toolbar action, from the button or from its keyboard shortcut.
    pub fn run(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::Add => self.open_add_dialog(),
            ToolbarAction::Remove => {
                if self.selected_id.is_some() {
                    self.open_delete_dialog();
                } else {
                    self.status = String::from("Select an entry first");
                }
            }
            ToolbarAction::Enable => {
                if self.selected_id.is_some() {
                    self.enable_selected();
                    self.status = String::from("Enabled");
                } else {
                    self.status = String::from("Select an entry first");
                }
            }
            ToolbarAction::Disable => {
                if self.selected_id.is_some() {
                    self.disable_selected();
                    self.status = String::from("Disabled");
                } else {
                    self.status = String::from("Select an entry first");
                }
            }
            ToolbarAction::Refresh => self.refresh(),
        }
    }

    /// Save from the add/edit dialog, leaving the dialog open and saying why
    /// when the fields do not validate.
    fn save_dialog(&mut self) {
        match self.confirm_add_edit() {
            Ok(()) => self.status = String::from("Saved"),
            Err(msg) => self.status = String::from(msg),
        }
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// The control under `(x, y)`, or `None` for bare background.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.frame(self.window_width, self.window_height)
            .hit_test(x, y)
    }

    /// Handle a UI event (keyboard or mouse).
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Resize { width, height } => {
                self.resize(*width as f32, *height as f32);
                EventResult::Consumed
            }
            Event::Mouse(mouse) => {
                let (x, y) = (mouse.x, mouse.y);
                match mouse.kind {
                    MouseEventKind::Press(MouseButton::Left) => self.handle_click(x, y),
                    MouseEventKind::DoubleClick(MouseButton::Left) => self.handle_double(x, y),
                    MouseEventKind::Scroll { dy, .. } => self.handle_scroll(x, y, dy),
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_scroll(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        match self.target_at(x, y) {
            Some(Target::Table | Target::Row(_)) => {
                self.scroll_rows(wheel::rows_f(dy));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Move the viewport by `rows`, positive towards the end of the list.
    fn scroll_rows(&mut self, rows: f32) {
        if !rows.is_finite() {
            return;
        }
        let current = isize::try_from(self.scroll_offset).unwrap_or(isize::MAX);
        let moved = current.saturating_add(rows.round() as isize).max(0);
        self.scroll_offset = usize::try_from(moved).unwrap_or(0);
        self.clamp_scroll();
    }

    /// A double-click on a row edits it — the same thing Enter does, for the
    /// half of the world that reaches for the mouse.
    fn handle_double(&mut self, x: f32, y: f32) -> EventResult {
        if let Some(Target::Row(id)) = self.target_at(x, y) {
            self.selected_id = Some(id);
            self.open_edit_dialog();
            return EventResult::Consumed;
        }
        self.handle_click(x, y)
    }

    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        let target = self.target_at(x, y);
        // Any click that is not in the search box takes the caret out of it,
        // so the next keystroke does not silently filter the table.
        if target != Some(Target::Search) {
            self.search_focused = false;
        }

        match target {
            Some(Target::Search) => {
                self.search_focused = true;
                EventResult::Consumed
            }
            Some(Target::Toolbar(action)) => {
                self.run(action);
                EventResult::Consumed
            }
            Some(Target::Column(column)) => {
                self.sort_by(column);
                self.clamp_scroll();
                EventResult::Consumed
            }
            Some(Target::Row(id)) => {
                self.selected_id = Some(id);
                EventResult::Consumed
            }
            Some(Target::Table) => {
                self.selected_id = Option::None;
                EventResult::Consumed
            }
            Some(Target::DialogField(i)) => {
                if let DialogState::AddEdit(dlg) = &mut self.dialog {
                    dlg.focused_field = i.min(AddEditDialog::FIELD_COUNT.saturating_sub(1));
                }
                EventResult::Consumed
            }
            Some(Target::DialogType) => {
                if let DialogState::AddEdit(dlg) = &mut self.dialog {
                    dlg.next_type();
                }
                EventResult::Consumed
            }
            Some(Target::DialogImpact) => {
                if let DialogState::AddEdit(dlg) = &mut self.dialog {
                    dlg.next_impact();
                }
                EventResult::Consumed
            }
            Some(Target::DialogSave) => {
                self.save_dialog();
                EventResult::Consumed
            }
            Some(Target::DeleteConfirm) => {
                self.confirm_delete();
                EventResult::Consumed
            }
            Some(Target::DialogCancel | Target::DeleteCancel) => {
                self.close_dialog();
                EventResult::Consumed
            }
            // The scrim exists to eat the click, which is the whole of its job.
            Some(Target::Scrim) => EventResult::Consumed,
            Option::None => EventResult::Ignored,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if matches!(self.dialog, DialogState::AddEdit(_)) {
            return self.handle_add_edit_key(key);
        }
        if matches!(self.dialog, DialogState::ConfirmDelete(_)) {
            return match key.key {
                Key::Escape => {
                    self.close_dialog();
                    EventResult::Consumed
                }
                Key::Enter => {
                    self.confirm_delete();
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            };
        }
        self.handle_table_key(key)
    }

    fn handle_add_edit_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.close_dialog();
                return EventResult::Consumed;
            }
            Key::Enter => {
                self.save_dialog();
                return EventResult::Consumed;
            }
            _ => {}
        }

        let DialogState::AddEdit(dlg) = &mut self.dialog else {
            return EventResult::Ignored;
        };
        match key.key {
            Key::Tab if key.modifiers.shift => dlg.focus_prev(),
            Key::Tab | Key::Down => dlg.focus_next(),
            Key::Up => dlg.focus_prev(),
            Key::Backspace => {
                dlg.focused_text_mut().pop();
            }
            _ => {
                if !key.text.is_empty() && !key.modifiers.ctrl && !key.modifiers.alt {
                    dlg.focused_text_mut().push_str(&key.text);
                }
            }
        }
        EventResult::Consumed
    }

    fn handle_table_key(&mut self, key: &KeyEvent) -> EventResult {
        if key.modifiers.ctrl {
            return match key.key {
                Key::N => {
                    self.run(ToolbarAction::Add);
                    EventResult::Consumed
                }
                Key::F => {
                    self.search_focused = true;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match key.key {
            Key::Escape => self.handle_escape(),
            Key::Up => {
                self.select_prev();
                EventResult::Consumed
            }
            Key::Down => {
                self.select_next();
                EventResult::Consumed
            }
            Key::Home => {
                self.select_pos(0);
                EventResult::Consumed
            }
            Key::End => {
                self.select_pos(self.all_entries().len().saturating_sub(1));
                EventResult::Consumed
            }
            Key::PageUp | Key::PageDown => {
                let page = self.visible_rows().max(1);
                let pos = self.selected_pos().unwrap_or(0);
                let target = if key.key == Key::PageUp {
                    pos.saturating_sub(page)
                } else {
                    pos.saturating_add(page)
                        .min(self.all_entries().len().saturating_sub(1))
                };
                self.select_pos(target);
                EventResult::Consumed
            }
            Key::Enter => {
                self.open_edit_dialog();
                EventResult::Consumed
            }
            Key::Delete => {
                self.run(ToolbarAction::Remove);
                EventResult::Consumed
            }
            Key::F5 => {
                self.run(ToolbarAction::Refresh);
                EventResult::Consumed
            }
            Key::Backspace if self.search_focused => {
                self.search_query.pop();
                self.scroll_offset = 0;
                self.clamp_scroll();
                EventResult::Consumed
            }
            _ if self.search_focused && !key.text.is_empty() && !key.modifiers.alt => {
                self.search_query.push_str(&key.text);
                self.scroll_offset = 0;
                self.clamp_scroll();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Escape backs out of one thing at a time: the caret, then the filter,
    /// then the selection. It never closes the window.
    fn handle_escape(&mut self) -> EventResult {
        if self.search_focused {
            self.search_focused = false;
        } else if !self.search_query.is_empty() {
            self.search_query.clear();
            self.scroll_offset = 0;
        } else if self.selected_id.is_some() {
            self.selected_id = Option::None;
        } else if !self.status.is_empty() {
            self.status.clear();
        } else {
            return EventResult::Ignored;
        }
        self.clamp_scroll();
        EventResult::Consumed
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full UI into a `RenderTree`.
    ///
    /// Kept alongside [`App::render`] because it needs no window: a test can
    /// ask what the app draws without standing one up.
    pub fn render(&self) -> RenderTree {
        self.frame(self.window_width, self.window_height)
            .into_tree()
    }

    /// Draw the window at `width` x `height`, recording a hit box for every
    /// control as it goes.
    fn frame(&self, width: f32, height: f32) -> Frame {
        let l = Layout::new(width, height);
        let mut frame = Frame::new(l.width, l.height);

        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.width,
            height: l.height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.draw_header(&mut frame, &l);
        self.draw_toolbar(&mut frame, &l);
        self.draw_search(&mut frame, &l);
        self.draw_table_header(&mut frame, &l);
        self.draw_table(&mut frame, &l);
        self.draw_details(&mut frame, &l);
        self.draw_status(&mut frame, &l);

        match &self.dialog {
            DialogState::Closed => {}
            DialogState::AddEdit(dlg) => {
                // Everything under the dialog stops being clickable the moment
                // the dialog is up; the scrim below takes its place.
                frame.discard_hits();
                Self::draw_scrim(&mut frame, &l);
                self.draw_add_edit_dialog(&mut frame, &l, dlg);
            }
            DialogState::ConfirmDelete(id) => {
                frame.discard_hits();
                Self::draw_scrim(&mut frame, &l);
                self.draw_confirm_delete_dialog(&mut frame, &l, *id);
            }
        }

        frame
    }

    fn draw_header(&self, frame: &mut Frame, l: &Layout) {
        if l.header.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: l.header.x,
            y: l.header.y,
            width: l.header.w,
            height: l.header.h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        frame.push(RenderCommand::Text {
            x: PADDING,
            y: l.header.y + ((l.header.h - FONT_SIZE_HEADING) / 2.0).max(0.0),
            text: String::from("Startup Apps Manager"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
            overflow: TextOverflow::Clip,
        });

        // The status line only lives here while no dialog is open; a dialog
        // shows it in its own footer, next to the button that produced it.
        if !self.status.is_empty() && self.dialog == DialogState::Closed {
            let w = text::measure(&self.status, FONT_SIZE_SMALL, FontWeightHint::Regular);
            frame.push(RenderCommand::Text {
                x: (l.width - PADDING - w).max(PADDING),
                y: l.header.y + ((l.header.h - FONT_SIZE_SMALL) / 2.0).max(0.0),
                text: self.status.clone(),
                color: COLOR_YELLOW,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some((l.width / 2.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: l.header.bottom(),
            x2: l.width,
            y2: l.header.bottom(),
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    fn draw_toolbar(&self, frame: &mut Frame, l: &Layout) {
        if !l.toolbar.is_empty() {
            frame.push(RenderCommand::FillRect {
                x: l.toolbar.x,
                y: l.toolbar.y,
                width: l.toolbar.w,
                height: l.toolbar.h,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::ZERO,
            });
        }
        for (rect, action) in l.buttons.iter().zip(ToolbarAction::all()) {
            Self::draw_button(
                frame,
                Target::Toolbar(*action),
                *rect,
                action.label(),
                action.color(),
            );
        }
    }

    /// Draw a button and record it. An empty rectangle draws and records
    /// nothing — a window too small for the control simply does not have it.
    fn draw_button(frame: &mut Frame, target: Target, rect: Rect, label: &str, color: Color) {
        if rect.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: Color::rgba(color.r, color.g, color.b, 40),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        frame.push(RenderCommand::Text {
            x: text::center_x(
                label,
                rect.x + rect.w / 2.0,
                FONT_SIZE,
                FontWeightHint::Bold,
            ),
            y: rect.y + ((rect.h - FONT_SIZE) / 2.0).max(0.0),
            text: label.to_string(),
            color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(target, rect);
    }

    fn draw_search(&self, frame: &mut Frame, l: &Layout) {
        if !l.search_bar.is_empty() {
            frame.push(RenderCommand::FillRect {
                x: l.search_bar.x,
                y: l.search_bar.y,
                width: l.search_bar.w,
                height: l.search_bar.h,
                color: COLOR_BASE,
                corner_radii: CornerRadii::ZERO,
            });
        }
        if l.search.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: l.search.x,
            y: l.search.y,
            width: l.search.w,
            height: l.search.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        if self.search_focused {
            frame.push(RenderCommand::StrokeRect {
                x: l.search.x,
                y: l.search.y,
                width: l.search.w,
                height: l.search.h,
                color: COLOR_BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        let empty = self.search_query.is_empty();
        let display = if empty {
            "Search by name, publisher, or path..."
        } else {
            &self.search_query
        };
        frame.push(RenderCommand::Text {
            x: l.search.x + 8.0,
            y: l.search.y + ((l.search.h - FONT_SIZE) / 2.0).max(0.0),
            text: display.to_string(),
            color: if empty { COLOR_OVERLAY0 } else { COLOR_TEXT },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.search.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(Target::Search, l.search);
    }

    fn draw_table_header(&self, frame: &mut Frame, l: &Layout) {
        if !l.table_header.is_empty() {
            frame.push(RenderCommand::FillRect {
                x: l.table_header.x,
                y: l.table_header.y,
                width: l.table_header.w,
                height: l.table_header.h,
                color: COLOR_SURFACE1,
                corner_radii: CornerRadii::ZERO,
            });
        }

        for (rect, col) in l.columns.iter().zip(SortColumn::all()) {
            if rect.is_empty() {
                continue;
            }
            let active = *col == self.sort_column;
            let mut label = col.header().to_string();
            if active {
                label.push_str(match self.sort_order {
                    SortOrder::Ascending => " ^",
                    SortOrder::Descending => " v",
                });
            }
            frame.push(RenderCommand::Text {
                x: rect.x + 4.0,
                y: rect.y + ((rect.h - FONT_SIZE_SMALL) / 2.0).max(0.0),
                text: label,
                color: if active { COLOR_BLUE } else { COLOR_TEXT },
                font_size: FONT_SIZE_SMALL,
                font_weight: if active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((rect.w - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            frame.hit(Target::Column(*col), *rect);
        }

        if !l.table_header.is_empty() {
            frame.push(RenderCommand::Line {
                x1: 0.0,
                y1: l.table_header.bottom(),
                x2: l.width,
                y2: l.table_header.bottom(),
                color: COLOR_SURFACE0,
                width: 1.0,
            });
        }
    }

    fn draw_table(&self, frame: &mut Frame, l: &Layout) {
        if l.table.is_empty() {
            return;
        }
        // Recorded first, so the rows drawn on top of it win the hit test
        // where they are drawn and the wheel still works everywhere else.
        frame.hit(Target::Table, l.table);

        for (i, entry) in self.entries_in(l).iter().enumerate() {
            let row = l.row(i);
            if row.is_empty() {
                continue;
            }
            let selected = self.selected_id == Some(entry.id);
            let bg = if selected {
                Color::rgba(COLOR_BLUE.r, COLOR_BLUE.g, COLOR_BLUE.b, 30)
            } else if i % 2 == 1 {
                Color::rgba(COLOR_SURFACE0.r, COLOR_SURFACE0.g, COLOR_SURFACE0.b, 80)
            } else {
                COLOR_BASE
            };
            frame.push(RenderCommand::FillRect {
                x: row.x,
                y: row.y,
                width: row.w,
                height: row.h,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });
            if selected {
                frame.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: 3.0_f32.min(row.w),
                    height: row.h,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            let text_y = row.y + ((row.h - FONT_SIZE) / 2.0).max(0.0);
            let cells: [(&str, Color, f32, FontWeightHint, f32); 6] = [
                (
                    &entry.name,
                    COLOR_TEXT,
                    COL_NAME_WIDTH,
                    FontWeightHint::Regular,
                    FONT_SIZE,
                ),
                (
                    &entry.publisher,
                    COLOR_SUBTEXT,
                    COL_PUBLISHER_WIDTH,
                    FontWeightHint::Regular,
                    FONT_SIZE,
                ),
                (
                    entry.status_label(),
                    entry.status_color(),
                    COL_STATUS_WIDTH,
                    FontWeightHint::Bold,
                    FONT_SIZE,
                ),
                (
                    entry.impact.label(),
                    entry.impact.color(),
                    COL_IMPACT_WIDTH,
                    FontWeightHint::Regular,
                    FONT_SIZE,
                ),
                (
                    entry.startup_type.label(),
                    COLOR_SUBTEXT,
                    COL_TYPE_WIDTH,
                    FontWeightHint::Regular,
                    FONT_SIZE,
                ),
                (
                    &entry.path,
                    COLOR_OVERLAY0,
                    COL_PATH_WIDTH,
                    FontWeightHint::Regular,
                    FONT_SIZE_SMALL,
                ),
            ];
            let mut cx = PADDING;
            for (value, color, width, weight, size) in cells {
                frame.push(RenderCommand::Text {
                    x: cx + 4.0,
                    y: text_y,
                    text: value.to_string(),
                    color,
                    font_size: size,
                    font_weight: weight,
                    max_width: Some((width - 8.0).max(0.0)),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += width;
            }

            frame.hit(Target::Row(entry.id), row);
        }
    }

    fn draw_details(&self, frame: &mut Frame, l: &Layout) {
        if l.details.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: l.details.x,
            y: l.details.y,
            width: l.details.w,
            height: l.details.h,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        frame.push(RenderCommand::Line {
            x1: 0.0,
            y1: l.details.y,
            x2: l.width,
            y2: l.details.y,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        let entry = self.selected_id.and_then(|id| self.manager.get_entry(id));
        let Some(entry) = entry else {
            frame.push(RenderCommand::Text {
                x: PADDING,
                y: l.details.y + (l.details.h / 2.0 - FONT_SIZE / 2.0).max(0.0),
                text: String::from("Select an entry to view details"),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Option::None,
                overflow: TextOverflow::Clip,
            });
            return;
        };

        let x_left = PADDING;
        let x_right = l.width / 2.0;
        let mut ly = l.details.y + 8.0;

        frame.push(RenderCommand::Text {
            x: x_left,
            y: ly,
            text: entry.name.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some((l.width - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        ly += DETAIL_LINE_SPACING + 4.0;

        let mut ry = ly;
        Self::draw_detail_row(frame, l, x_left, ly, "Path:", &entry.path);
        ly += DETAIL_LINE_SPACING;
        if !entry.args.is_empty() {
            Self::draw_detail_row(frame, l, x_left, ly, "Args:", &entry.args);
            ly += DETAIL_LINE_SPACING;
        }
        Self::draw_detail_row(frame, l, x_left, ly, "Publisher:", &entry.publisher);

        Self::draw_detail_row(frame, l, x_right, ry, "Type:", entry.startup_type.label());
        ry += DETAIL_LINE_SPACING;
        Self::draw_detail_row(frame, l, x_right, ry, "Impact:", entry.impact.label());
        ry += DETAIL_LINE_SPACING;
        Self::draw_detail_row(frame, l, x_right, ry, "Status:", entry.status_label());
    }

    fn draw_detail_row(frame: &mut Frame, l: &Layout, x: f32, y: f32, label: &str, value: &str) {
        if y + FONT_SIZE_SMALL > l.details.bottom() {
            return;
        }
        frame.push(RenderCommand::Text {
            x,
            y,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
            overflow: TextOverflow::Clip,
        });
        frame.push(RenderCommand::Text {
            x: x + text::measure(label, FONT_SIZE_SMALL, FontWeightHint::Bold) + 8.0,
            y,
            text: value.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.width / 2.0 - 40.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn draw_status(&self, frame: &mut Frame, l: &Layout) {
        if l.status.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: l.status.x,
            y: l.status.y,
            width: l.status.w,
            height: l.status.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let stats = self.manager.stats();
        let shown = self.filtered_count();
        let summary = format!(
            "Total: {}  |  Shown: {}  |  Enabled: {}  |  Disabled: {}  |  Login: {}  Service: {}  Scheduled: {}  Driver: {}  |  Boot Impact: {}",
            stats.total,
            shown,
            stats.enabled,
            stats.disabled,
            stats.login_count,
            stats.service_count,
            stats.scheduled_count,
            stats.driver_count,
            stats.impact_summary(),
        );
        frame.push(RenderCommand::Text {
            x: PADDING,
            y: l.status.y + ((l.status.h - FONT_SIZE_SMALL) / 2.0).max(0.0),
            text: summary,
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.width - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The dimmer behind an open dialog, and the hit box that makes the window
    /// behind it inert.
    fn draw_scrim(frame: &mut Frame, l: &Layout) {
        let window = l.window();
        if window.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: l.width,
            height: l.height,
            color: Color::rgba(0, 0, 0, 150),
            corner_radii: CornerRadii::ZERO,
        });
        frame.hit(Target::Scrim, window);
    }

    /// Box, border and title of a dialog. Returns nothing: the caller already
    /// has the rectangle.
    fn draw_dialog_chrome(frame: &mut Frame, rect: Rect, title: &str, border: Color, tint: Color) {
        if rect.is_empty() {
            return;
        }
        frame.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: border,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });
        frame.push(RenderCommand::Text {
            x: rect.x + PADDING,
            y: rect.y + 12.0,
            text: title.to_string(),
            color: tint,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some((rect.w - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// The status line, drawn in a dialog's footer beside its buttons — where
    /// "Name is required" belongs, next to the Save that refused.
    fn draw_dialog_status(&self, frame: &mut Frame, rect: Rect) {
        if self.status.is_empty() || rect.is_empty() {
            return;
        }
        let width = (rect.w - PADDING * 2.0 - BUTTON_WIDTH * 2.0 - BUTTON_GAP * 2.0).max(0.0);
        if width <= 0.0 {
            return;
        }
        frame.push(RenderCommand::Text {
            x: rect.x + PADDING,
            y: rect.bottom() - BUTTON_HEIGHT - PADDING + (BUTTON_HEIGHT - FONT_SIZE_SMALL) / 2.0,
            text: self.status.clone(),
            color: COLOR_YELLOW,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(width),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn draw_add_edit_dialog(&self, frame: &mut Frame, l: &Layout, dlg: &AddEditDialog) {
        let title = if dlg.editing_id.is_some() {
            "Edit Startup Entry"
        } else {
            "Add Startup Entry"
        };
        Self::draw_dialog_chrome(frame, l.dialog, title, COLOR_SURFACE1, COLOR_TEXT);
        if l.dialog.is_empty() {
            return;
        }

        for (i, (label, value)) in dlg.fields().into_iter().enumerate() {
            Self::draw_form_field(
                frame,
                Target::DialogField(i),
                l.dialog_field(i),
                l.dialog_field_top(i),
                label,
                value,
                dlg.focused_field == i,
            );
        }

        Self::draw_selector(
            frame,
            Target::DialogType,
            l.dialog_type(),
            (l.dialog.x + PADDING, l.selector_y()),
            "Type:",
            dlg.selected_type().label(),
            COLOR_BLUE,
        );
        Self::draw_selector(
            frame,
            Target::DialogImpact,
            l.dialog_impact(),
            (
                l.dialog.x + PADDING + SELECTOR_IMPACT_LABEL_X,
                l.selector_y(),
            ),
            "Impact:",
            dlg.selected_impact().label(),
            dlg.selected_impact().color(),
        );

        self.draw_dialog_status(frame, l.dialog);
        let (cancel, save) = l.dialog_buttons();
        Self::draw_button(
            frame,
            Target::DialogCancel,
            cancel,
            "Cancel",
            COLOR_OVERLAY0,
        );
        Self::draw_button(frame, Target::DialogSave, save, "Save", COLOR_GREEN);
    }

    /// A labelled text input. `label_y` is where the caption goes; `input` is
    /// the box, which is also the hit box.
    fn draw_form_field(
        frame: &mut Frame,
        target: Target,
        input: Rect,
        label_y: f32,
        label: &str,
        value: &str,
        focused: bool,
    ) {
        if input.is_empty() {
            return;
        }
        frame.push(RenderCommand::Text {
            x: input.x,
            y: label_y,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(input.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.push(RenderCommand::FillRect {
            x: input.x,
            y: input.y,
            width: input.w,
            height: input.h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(4.0),
        });
        frame.push(RenderCommand::StrokeRect {
            x: input.x,
            y: input.y,
            width: input.w,
            height: input.h,
            color: if focused { COLOR_BLUE } else { COLOR_SURFACE1 },
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        let empty = value.is_empty();
        frame.push(RenderCommand::Text {
            x: input.x + 6.0,
            y: input.y + ((input.h - FONT_SIZE) / 2.0).max(0.0),
            text: if empty { label } else { value }.to_string(),
            color: if empty { COLOR_OVERLAY0 } else { COLOR_TEXT },
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((input.w - 12.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(target, input);
    }

    /// A "Label: Value" pair whose value cycles when clicked.
    fn draw_selector(
        frame: &mut Frame,
        target: Target,
        hit: Rect,
        label_pos: (f32, f32),
        label: &str,
        value: &str,
        color: Color,
    ) {
        let (label_x, y) = label_pos;
        frame.push(RenderCommand::Text {
            x: label_x,
            y,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
            overflow: TextOverflow::Clip,
        });
        if hit.is_empty() {
            return;
        }
        frame.push(RenderCommand::Text {
            x: hit.x,
            y,
            text: value.to_string(),
            color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(hit.w),
            overflow: TextOverflow::Ellipsis,
        });
        frame.hit(target, hit);
    }

    fn draw_confirm_delete_dialog(&self, frame: &mut Frame, l: &Layout, id: u64) {
        Self::draw_dialog_chrome(frame, l.confirm, "Confirm Delete", COLOR_RED, COLOR_RED);
        if l.confirm.is_empty() {
            return;
        }

        let name = self
            .manager
            .get_entry(id)
            .map_or("Unknown", |e| e.name.as_str());
        frame.push(RenderCommand::Text {
            x: l.confirm.x + PADDING,
            y: l.confirm.y + 50.0,
            text: format!("Remove \"{name}\" from startup?"),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.confirm.w - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        frame.push(RenderCommand::Text {
            x: l.confirm.x + PADDING,
            y: l.confirm.y + 74.0,
            text: String::from("This action cannot be undone."),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some((l.confirm.w - PADDING * 2.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        let (cancel, delete) = l.confirm_buttons();
        Self::draw_button(
            frame,
            Target::DeleteCancel,
            cancel,
            "Cancel",
            COLOR_OVERLAY0,
        );
        Self::draw_button(frame, Target::DeleteConfirm, delete, "Delete", COLOR_RED);
    }
}

impl Default for StartupUI {
    fn default() -> Self {
        Self::new()
    }
}

impl App for StartupUI {
    fn title(&self) -> String {
        String::from("Startup Apps Manager")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn on_event(&mut self, event: &Event) -> Response {
        // Ctrl+Q closes the window. Escape does not: here it backs out of a
        // dialog, a filter or a selection, which is what the key is for.
        if let Event::Key(key) = event
            && key.pressed
            && key.key == Key::Q
            && key.modifiers.ctrl
        {
            return Response::Exit;
        }
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The remembered size is only ever a starting guess; this is the real
        // one, and the hit test reads it back through `handle_event`.
        self.resize(width, height);
        self.frame(width, height).into_tree()
    }
}

impl Probe for StartupUI {
    type Target = Target;
    type Outcome = EventResult;

    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(button),
        }))
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) -> Self::Outcome {
        self.resize(size.0, size.1);
        self.handle_event(&Event::Key(key.clone()))
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() -> ExitCode {
    app::launch("startupmanager", &mut StartupUI::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that cannot index a slice or unwrap an `Option` it has just built
    // is a test that spends more lines apologising than asserting. Panicking on
    // bad data is the point here -- it is how the test fails.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;
    // Not in the production imports: nothing outside the tests names a
    // modifier set, because the app reads `key.modifiers.ctrl` off the event
    // the window hands it.
    use guitk::event::Modifiers;
    use guitk::probe;

    // -- StartupType tests --------------------------------------------------

    #[test]
    fn test_startup_type_label() {
        assert_eq!(StartupType::Login.label(), "Login");
        assert_eq!(StartupType::Service.label(), "Service");
        assert_eq!(StartupType::Scheduled.label(), "Scheduled");
        assert_eq!(StartupType::Driver.label(), "Driver");
    }

    #[test]
    fn test_startup_type_from_label() {
        assert_eq!(StartupType::from_label("login"), Some(StartupType::Login));
        assert_eq!(
            StartupType::from_label("SERVICE"),
            Some(StartupType::Service)
        );
        assert_eq!(
            StartupType::from_label("Scheduled"),
            Some(StartupType::Scheduled)
        );
        assert_eq!(StartupType::from_label("DRIVER"), Some(StartupType::Driver));
        assert_eq!(StartupType::from_label("unknown"), None);
    }

    #[test]
    fn test_startup_type_from_label_empty() {
        assert_eq!(StartupType::from_label(""), None);
    }

    #[test]
    fn test_startup_type_all() {
        let all = StartupType::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&StartupType::Login));
        assert!(all.contains(&StartupType::Driver));
    }

    // -- StartupImpact tests ------------------------------------------------

    #[test]
    fn test_impact_label() {
        assert_eq!(StartupImpact::None.label(), "None");
        assert_eq!(StartupImpact::Low.label(), "Low");
        assert_eq!(StartupImpact::Medium.label(), "Medium");
        assert_eq!(StartupImpact::High.label(), "High");
    }

    #[test]
    fn test_impact_from_label() {
        assert_eq!(StartupImpact::from_label("none"), Some(StartupImpact::None));
        assert_eq!(StartupImpact::from_label("LOW"), Some(StartupImpact::Low));
        assert_eq!(
            StartupImpact::from_label("Medium"),
            Some(StartupImpact::Medium)
        );
        assert_eq!(StartupImpact::from_label("HIGH"), Some(StartupImpact::High));
        assert_eq!(StartupImpact::from_label("extreme"), None);
    }

    #[test]
    fn test_impact_weight_ordering() {
        assert!(StartupImpact::None.weight() < StartupImpact::Low.weight());
        assert!(StartupImpact::Low.weight() < StartupImpact::Medium.weight());
        assert!(StartupImpact::Medium.weight() < StartupImpact::High.weight());
    }

    #[test]
    fn test_impact_color_distinct() {
        // Each impact level should have a distinct color.
        let colors: Vec<Color> = StartupImpact::all().iter().map(|i| i.color()).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j], "impact colors should be distinct");
            }
        }
    }

    #[test]
    fn test_impact_all() {
        let all = StartupImpact::all();
        assert_eq!(all.len(), 4);
    }

    // -- StartupEntry tests -------------------------------------------------

    #[test]
    fn test_entry_creation() {
        let entry = StartupEntry::new(
            1,
            "Test",
            "/bin/test",
            "--flag",
            StartupType::Login,
            StartupImpact::Low,
            "Publisher",
            "A test entry",
            1000,
        );
        assert_eq!(entry.id, 1);
        assert_eq!(entry.name, "Test");
        assert!(entry.enabled); // Defaults to enabled.
    }

    #[test]
    fn test_entry_status_label() {
        let mut entry = StartupEntry::new(
            1,
            "T",
            "/bin/t",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        assert_eq!(entry.status_label(), "Enabled");
        entry.enabled = false;
        assert_eq!(entry.status_label(), "Disabled");
    }

    #[test]
    fn test_entry_status_color() {
        let mut entry = StartupEntry::new(
            1,
            "T",
            "/bin/t",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        assert_eq!(entry.status_color(), COLOR_GREEN);
        entry.enabled = false;
        assert_eq!(entry.status_color(), COLOR_OVERLAY0);
    }

    // -- StartupManager CRUD tests ------------------------------------------

    #[test]
    fn test_manager_add_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "Test",
            "/bin/test",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "Pub",
            "Desc",
            1000,
        );
        assert_eq!(id, 1);
        assert_eq!(mgr.entry_count(), 1);
    }

    #[test]
    fn test_manager_add_multiple_entries() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        let id2 = mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Service,
            StartupImpact::High,
            "",
            "",
            0,
        );
        assert_ne!(id1, id2);
        assert_eq!(mgr.entry_count(), 2);
    }

    #[test]
    fn test_manager_remove_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "T",
            "/t",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        assert!(mgr.remove_entry(id));
        assert_eq!(mgr.entry_count(), 0);
    }

    #[test]
    fn test_manager_remove_nonexistent() {
        let mut mgr = StartupManager::new();
        assert!(!mgr.remove_entry(999));
    }

    #[test]
    fn test_manager_enable_disable() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "T",
            "/t",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );

        assert!(mgr.disable_entry(id));
        assert!(!mgr.get_entry(id).map(|e| e.enabled).unwrap_or(true));

        assert!(mgr.enable_entry(id));
        assert!(mgr.get_entry(id).map(|e| e.enabled).unwrap_or(false));
    }

    #[test]
    fn test_manager_enable_nonexistent() {
        let mut mgr = StartupManager::new();
        assert!(!mgr.enable_entry(999));
        assert!(!mgr.disable_entry(999));
    }

    #[test]
    fn test_manager_toggle_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "T",
            "/t",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );

        // Initially enabled, toggle should disable.
        let new_state = mgr.toggle_entry(id);
        assert_eq!(new_state, Some(false));

        // Toggle again should enable.
        let new_state = mgr.toggle_entry(id);
        assert_eq!(new_state, Some(true));
    }

    #[test]
    fn test_manager_toggle_nonexistent() {
        let mut mgr = StartupManager::new();
        assert_eq!(mgr.toggle_entry(999), None);
    }

    #[test]
    fn test_manager_get_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "Test",
            "/bin/test",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "P",
            "D",
            42,
        );
        let entry = mgr.get_entry(id);
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.name.as_str()), Some("Test"));
        assert_eq!(entry.map(|e| e.added_timestamp), Some(42));
    }

    #[test]
    fn test_manager_get_entry_nonexistent() {
        let mgr = StartupManager::new();
        assert!(mgr.get_entry(999).is_none());
    }

    #[test]
    fn test_manager_update_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "Old",
            "/old",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        let ok = mgr.update_entry(
            id,
            "New",
            "/new",
            "--arg",
            StartupType::Service,
            StartupImpact::High,
            "Pub",
            "Desc",
        );
        assert!(ok);
        let entry = mgr.get_entry(id);
        assert_eq!(entry.map(|e| e.name.as_str()), Some("New"));
        assert_eq!(entry.map(|e| e.startup_type), Some(StartupType::Service));
    }

    #[test]
    fn test_manager_update_nonexistent() {
        let mut mgr = StartupManager::new();
        assert!(!mgr.update_entry(
            999,
            "N",
            "/n",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            ""
        ));
    }

    #[test]
    fn test_manager_entry_ids() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let id2 = mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Service,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        let ids = mgr.entry_ids();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
        assert_eq!(ids.len(), 2);
    }

    // -- Sorting tests ------------------------------------------------------

    #[test]
    fn test_sort_by_name_ascending() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Zebra",
            "/z",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Apple",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let sorted = mgr.sorted_entries(SortColumn::Name, SortOrder::Ascending);
        assert_eq!(sorted[0].name, "Apple");
        assert_eq!(sorted[1].name, "Zebra");
    }

    #[test]
    fn test_sort_by_name_descending() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Apple",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Zebra",
            "/z",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let sorted = mgr.sorted_entries(SortColumn::Name, SortOrder::Descending);
        assert_eq!(sorted[0].name, "Zebra");
        assert_eq!(sorted[1].name, "Apple");
    }

    #[test]
    fn test_sort_by_impact() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "High",
            "/h",
            "",
            StartupType::Login,
            StartupImpact::High,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Low",
            "/l",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Med",
            "/m",
            "",
            StartupType::Login,
            StartupImpact::Medium,
            "",
            "",
            0,
        );
        let sorted = mgr.sorted_entries(SortColumn::Impact, SortOrder::Ascending);
        assert_eq!(sorted[0].name, "Low");
        assert_eq!(sorted[1].name, "Med");
        assert_eq!(sorted[2].name, "High");
    }

    #[test]
    fn test_sort_by_type() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Drv",
            "/d",
            "",
            StartupType::Driver,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Log",
            "/l",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let sorted = mgr.sorted_entries(SortColumn::Type, SortOrder::Ascending);
        assert_eq!(sorted[0].startup_type, StartupType::Login);
        assert_eq!(sorted[1].startup_type, StartupType::Driver);
    }

    #[test]
    fn test_sort_by_status() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry(
            "Dis",
            "/d",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "En",
            "/e",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.disable_entry(id1);
        let sorted = mgr.sorted_entries(SortColumn::Status, SortOrder::Ascending);
        assert!(!sorted[0].enabled);
        assert!(sorted[1].enabled);
    }

    // -- Search tests -------------------------------------------------------

    #[test]
    fn test_search_by_name() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Firefox",
            "/ff",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Chrome",
            "/ch",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        let results = mgr.search_entries("fire");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn test_search_by_publisher() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "App",
            "/app",
            "",
            StartupType::Login,
            StartupImpact::None,
            "MyCorp",
            "",
            0,
        );
        mgr.add_entry(
            "Other",
            "/o",
            "",
            StartupType::Login,
            StartupImpact::None,
            "TheirCorp",
            "",
            0,
        );
        let results = mgr.search_entries("mycorp");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].publisher, "MyCorp");
    }

    #[test]
    fn test_search_by_path() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/usr/bin/foo",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "B",
            "/opt/bar",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let results = mgr.search_entries("/opt/");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "B");
    }

    #[test]
    fn test_search_empty_query_returns_all() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let results = mgr.search_entries("");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "MyApp",
            "/my",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let results = mgr.search_entries("MYAPP");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let results = mgr.search_entries("zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_filtered_sorted() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Zebra App",
            "/z",
            "",
            StartupType::Login,
            StartupImpact::High,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Alpha App",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "Other Thing",
            "/o",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let results = mgr.filtered_sorted("app", SortColumn::Name, SortOrder::Ascending);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Alpha App");
        assert_eq!(results[1].name, "Zebra App");
    }

    // -- Statistics tests ---------------------------------------------------

    #[test]
    fn test_stats_empty() {
        let mgr = StartupManager::new();
        let s = mgr.stats();
        assert_eq!(s.total, 0);
        assert_eq!(s.enabled, 0);
        assert_eq!(s.disabled, 0);
    }

    #[test]
    fn test_stats_counts() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        let id2 = mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Service,
            StartupImpact::High,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "C",
            "/c",
            "",
            StartupType::Scheduled,
            StartupImpact::Medium,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "D",
            "/d",
            "",
            StartupType::Driver,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.disable_entry(id2);

        let s = mgr.stats();
        assert_eq!(s.total, 4);
        assert_eq!(s.enabled, 3);
        assert_eq!(s.disabled, 1);
        assert_eq!(s.login_count, 1);
        assert_eq!(s.service_count, 1);
        assert_eq!(s.scheduled_count, 1);
        assert_eq!(s.driver_count, 1);
    }

    #[test]
    fn test_stats_impact_weight() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::Low,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Login,
            StartupImpact::High,
            "",
            "",
            0,
        );
        let s = mgr.stats();
        // Low=1, High=6, total=7
        assert_eq!(s.total_impact_weight, 7);
    }

    #[test]
    fn test_stats_disabled_excluded_from_impact() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::High,
            "",
            "",
            0,
        );
        mgr.disable_entry(id);
        let s = mgr.stats();
        assert_eq!(s.total_impact_weight, 0);
    }

    #[test]
    fn test_stats_impact_summary() {
        let s = StartupStats {
            total_impact_weight: 0,
            ..Default::default()
        };
        assert_eq!(s.impact_summary(), "Minimal");
        let s = StartupStats {
            total_impact_weight: 3,
            ..Default::default()
        };
        assert_eq!(s.impact_summary(), "Low");
        let s = StartupStats {
            total_impact_weight: 10,
            ..Default::default()
        };
        assert_eq!(s.impact_summary(), "Medium");
        let s = StartupStats {
            total_impact_weight: 20,
            ..Default::default()
        };
        assert_eq!(s.impact_summary(), "High");
        let s = StartupStats {
            total_impact_weight: 50,
            ..Default::default()
        };
        assert_eq!(s.impact_summary(), "Very High");
    }

    // -- Config serialization tests -----------------------------------------

    #[test]
    fn test_config_roundtrip() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "Test App",
            "/bin/test",
            "--flag",
            StartupType::Login,
            StartupImpact::Medium,
            "TestCo",
            "A test",
            1000,
        );
        let id2 = mgr.add_entry(
            "Service",
            "/sbin/svc",
            "",
            StartupType::Service,
            StartupImpact::High,
            "Slate OS",
            "Core svc",
            2000,
        );
        mgr.disable_entry(id2);

        let text = StartupConfig::serialize(&mgr);
        let restored = StartupConfig::deserialize(&text).expect("should deserialize");
        assert_eq!(restored.entry_count(), 2);

        let e1 = restored.get_entry(1).expect("entry 1 should exist");
        assert_eq!(e1.name, "Test App");
        assert_eq!(e1.args, "--flag");
        assert!(e1.enabled);

        let e2 = restored.get_entry(2).expect("entry 2 should exist");
        assert_eq!(e2.name, "Service");
        assert!(!e2.enabled);
    }

    #[test]
    fn test_config_deserialize_empty() {
        let mgr = StartupConfig::deserialize("").expect("should handle empty");
        assert_eq!(mgr.entry_count(), 0);
    }

    #[test]
    fn test_config_deserialize_comments_and_blanks() {
        let text = "# comment\n\n# another\nVERSION|1\n";
        let mgr = StartupConfig::deserialize(text).expect("should handle");
        assert_eq!(mgr.entry_count(), 0);
    }

    #[test]
    fn test_config_escape_pipe_in_fields() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "My|App",
            "/bin/test|me",
            "",
            StartupType::Login,
            StartupImpact::None,
            "A|B",
            "has|pipes",
            0,
        );
        let text = StartupConfig::serialize(&mgr);
        let restored = StartupConfig::deserialize(&text).expect("should roundtrip");
        let e = restored.get_entry(1).expect("should exist");
        assert_eq!(e.name, "My|App");
        assert_eq!(e.path, "/bin/test|me");
        assert_eq!(e.publisher, "A|B");
    }

    #[test]
    fn test_config_escape_backslash_in_fields() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "C:\\App",
            "C:\\Program Files\\app.exe",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let text = StartupConfig::serialize(&mgr);
        let restored = StartupConfig::deserialize(&text).expect("should roundtrip");
        let e = restored.get_entry(1).expect("should exist");
        assert_eq!(e.name, "C:\\App");
        assert_eq!(e.path, "C:\\Program Files\\app.exe");
    }

    #[test]
    fn test_config_unsupported_version() {
        let text = "VERSION|99\n";
        let result = StartupConfig::deserialize(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_malformed_entry() {
        let text = "VERSION|1\nENTRY|bad\n";
        let result = StartupConfig::deserialize(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_next_id_after_import() {
        let mut mgr = StartupManager::new();
        mgr.add_entry(
            "A",
            "/a",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        mgr.add_entry(
            "B",
            "/b",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        let text = StartupConfig::serialize(&mgr);
        let mut restored = StartupConfig::deserialize(&text).expect("should deserialize");
        // Adding a new entry should get id=3 (max existing is 2).
        let new_id = restored.add_entry(
            "C",
            "/c",
            "",
            StartupType::Login,
            StartupImpact::None,
            "",
            "",
            0,
        );
        assert_eq!(new_id, 3);
    }

    // -- SortColumn / SortOrder tests ---------------------------------------

    #[test]
    fn test_sort_order_toggle() {
        assert_eq!(SortOrder::Ascending.toggle(), SortOrder::Descending);
        assert_eq!(SortOrder::Descending.toggle(), SortOrder::Ascending);
    }

    #[test]
    fn test_sort_column_all() {
        let cols = SortColumn::all();
        assert_eq!(cols.len(), 6);
    }

    #[test]
    fn test_sort_column_headers() {
        assert_eq!(SortColumn::Name.header(), "Name");
        assert_eq!(SortColumn::Impact.header(), "Impact");
        assert_eq!(SortColumn::Path.header(), "Path");
    }

    #[test]
    fn test_sort_column_widths_positive() {
        for col in SortColumn::all() {
            assert!(col.width() > 0.0);
        }
    }

    // -- AddEditDialog tests ------------------------------------------------

    #[test]
    fn test_add_dialog_defaults() {
        let dlg = AddEditDialog::new_add();
        assert!(dlg.editing_id.is_none());
        assert!(dlg.name.is_empty());
        assert!(dlg.path.is_empty());
        assert_eq!(dlg.focused_field, 0);
    }

    #[test]
    fn test_edit_dialog_from_entry() {
        let entry = StartupEntry::new(
            5,
            "Test",
            "/test",
            "--arg",
            StartupType::Service,
            StartupImpact::High,
            "Pub",
            "Desc",
            100,
        );
        let dlg = AddEditDialog::new_edit(&entry);
        assert_eq!(dlg.editing_id, Some(5));
        assert_eq!(dlg.name, "Test");
        assert_eq!(dlg.path, "/test");
        assert_eq!(dlg.args, "--arg");
        assert_eq!(dlg.selected_type(), StartupType::Service);
        assert_eq!(dlg.selected_impact(), StartupImpact::High);
    }

    #[test]
    fn test_dialog_validate_empty_name() {
        let dlg = AddEditDialog::new_add();
        assert!(dlg.validate().is_err());
    }

    #[test]
    fn test_dialog_validate_empty_path() {
        let mut dlg = AddEditDialog::new_add();
        dlg.name = "Test".to_string();
        assert!(dlg.validate().is_err());
    }

    #[test]
    fn test_dialog_validate_ok() {
        let mut dlg = AddEditDialog::new_add();
        dlg.name = "Test".to_string();
        dlg.path = "/bin/test".to_string();
        assert!(dlg.validate().is_ok());
    }

    #[test]
    fn test_dialog_next_type_cycles() {
        let mut dlg = AddEditDialog::new_add();
        let initial = dlg.selected_type();
        for _ in 0..StartupType::all().len() {
            dlg.next_type();
        }
        assert_eq!(dlg.selected_type(), initial);
    }

    #[test]
    fn test_dialog_next_impact_cycles() {
        let mut dlg = AddEditDialog::new_add();
        let initial = dlg.selected_impact();
        for _ in 0..StartupImpact::all().len() {
            dlg.next_impact();
        }
        assert_eq!(dlg.selected_impact(), initial);
    }

    #[test]
    fn test_dialog_focus_navigation() {
        let mut dlg = AddEditDialog::new_add();
        assert_eq!(dlg.focused_field, 0);
        dlg.focus_next();
        assert_eq!(dlg.focused_field, 1);
        dlg.focus_prev();
        assert_eq!(dlg.focused_field, 0);
        dlg.focus_prev(); // Wraps to last.
        assert_eq!(dlg.focused_field, 4);
        dlg.focus_next(); // Wraps to first.
        assert_eq!(dlg.focused_field, 0);
    }

    // -- StartupUI tests ----------------------------------------------------

    #[test]
    fn test_ui_creation_has_sample_data() {
        let ui = StartupUI::new();
        assert!(ui.manager.entry_count() > 0);
    }

    #[test]
    fn test_ui_sort_toggle() {
        let mut ui = StartupUI::new();
        ui.sort_by(SortColumn::Name);
        assert_eq!(ui.sort_column, SortColumn::Name);
        let first_order = ui.sort_order;
        ui.sort_by(SortColumn::Name);
        assert_ne!(ui.sort_order, first_order);
    }

    #[test]
    fn test_ui_sort_change_column() {
        let mut ui = StartupUI::new();
        ui.sort_by(SortColumn::Impact);
        assert_eq!(ui.sort_column, SortColumn::Impact);
        assert_eq!(ui.sort_order, SortOrder::Ascending);
    }

    #[test]
    fn test_ui_select_next_prev() {
        let mut ui = StartupUI::new();
        assert!(ui.selected_id.is_none());
        ui.select_next();
        assert!(ui.selected_id.is_some());
        let first = ui.selected_id;
        ui.select_next();
        // Should have moved (unless only 1 entry).
        if ui.manager.entry_count() > 1 {
            assert_ne!(ui.selected_id, first);
        }
        ui.select_prev();
        assert_eq!(ui.selected_id, first);
    }

    #[test]
    fn test_ui_open_close_add_dialog() {
        let mut ui = StartupUI::new();
        ui.open_add_dialog();
        assert!(matches!(ui.dialog, DialogState::AddEdit(_)));
        ui.close_dialog();
        assert_eq!(ui.dialog, DialogState::Closed);
    }

    #[test]
    fn test_ui_confirm_add() {
        let mut ui = StartupUI::new();
        let count_before = ui.manager.entry_count();
        ui.open_add_dialog();
        if let DialogState::AddEdit(ref mut dlg) = ui.dialog {
            dlg.name = "New Entry".to_string();
            dlg.path = "/bin/new".to_string();
        }
        let result = ui.confirm_add_edit();
        assert!(result.is_ok());
        assert_eq!(ui.manager.entry_count(), count_before + 1);
        assert_eq!(ui.dialog, DialogState::Closed);
    }

    #[test]
    fn test_ui_confirm_add_validation_fails() {
        let mut ui = StartupUI::new();
        ui.open_add_dialog();
        // Name and path are empty.
        let result = ui.confirm_add_edit();
        assert!(result.is_err());
    }

    #[test]
    fn test_ui_confirm_delete() {
        let mut ui = StartupUI::new();
        let ids = ui.manager.entry_ids();
        let id = ids[0];
        let count_before = ui.manager.entry_count();
        ui.selected_id = Some(id);
        ui.open_delete_dialog();
        assert!(matches!(ui.dialog, DialogState::ConfirmDelete(_)));
        ui.confirm_delete();
        assert_eq!(ui.manager.entry_count(), count_before - 1);
        assert_eq!(ui.dialog, DialogState::Closed);
    }

    #[test]
    fn test_ui_enable_disable_selected() {
        let mut ui = StartupUI::new();
        let ids = ui.manager.entry_ids();
        let id = ids[0];
        ui.selected_id = Some(id);
        ui.disable_selected();
        assert!(!ui.manager.get_entry(id).map(|e| e.enabled).unwrap_or(true));
        ui.enable_selected();
        assert!(ui.manager.get_entry(id).map(|e| e.enabled).unwrap_or(false));
    }

    #[test]
    fn test_ui_render_produces_commands() {
        let ui = StartupUI::new();
        let tree = ui.render();
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn test_ui_render_with_selection() {
        let mut ui = StartupUI::new();
        ui.select_next();
        let tree = ui.render();
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn test_ui_render_with_dialog() {
        let mut ui = StartupUI::new();
        ui.open_add_dialog();
        let tree = ui.render();
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn test_ui_render_delete_dialog() {
        let mut ui = StartupUI::new();
        let ids = ui.manager.entry_ids();
        ui.selected_id = Some(ids[0]);
        ui.open_delete_dialog();
        let tree = ui.render();
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn test_ui_visible_rows() {
        let ui = StartupUI::new();
        assert!(ui.visible_rows() > 0);
    }

    #[test]
    fn test_ui_filtered_count() {
        let mut ui = StartupUI::new();
        let total = ui.filtered_count();
        assert_eq!(total, ui.manager.entry_count());
        ui.search_query = "zzzznotfound".to_string();
        assert_eq!(ui.filtered_count(), 0);
    }

    #[test]
    fn test_ui_populate_sample_data() {
        let mut mgr = StartupManager::new();
        mgr.populate_sample_data();
        assert!(mgr.entry_count() >= 5);
        // Should have a mix of types.
        let s = mgr.stats();
        assert!(s.login_count > 0);
        assert!(s.service_count > 0);
    }

    // -- Escape/unescape tests ----------------------------------------------

    #[test]
    fn test_escape_unescape_roundtrip() {
        let cases = [
            "simple",
            "has|pipe",
            "has\\backslash",
            "both|and\\mixed",
            "end\\",
            "",
            "|||",
            "\\\\\\\\",
        ];
        for &input in &cases {
            let escaped = StartupConfig::escape_field(input);
            let unescaped = StartupConfig::unescape_field(&escaped);
            assert_eq!(unescaped, input, "roundtrip failed for: {input:?}");
        }
    }

    // -- ConfigError Display test -------------------------------------------

    #[test]
    fn test_config_error_display() {
        let e = ConfigError::UnsupportedVersion("99".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("99"));

        let e = ConfigError::MalformedEntry("bad".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("bad"));

        let e = ConfigError::InvalidField("id".to_string());
        let msg = format!("{e}");
        assert!(msg.contains("id"));
    }

    // -- Default trait tests ------------------------------------------------

    #[test]
    fn test_startup_manager_default() {
        let mgr = StartupManager::default();
        assert_eq!(mgr.entry_count(), 0);
    }

    #[test]
    fn test_startup_ui_default() {
        let ui = StartupUI::default();
        assert!(ui.manager.entry_count() > 0);
    }

    // -- Wiring: layout, hit testing, events --------------------------------
    //
    // Everything below drives the app the way the window does -- a click at a
    // coordinate, a keystroke, a wheel notch -- and reads the result out of
    // the same frame the renderer produced. Nothing here recomputes geometry.

    /// The default layout, at the size the window opens at.
    fn layout() -> Layout {
        Layout::new(WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    /// A window short enough that only three table rows fit, for the scroll
    /// tests: 400 - (48 + 40 + 36 + 32 + 120 + 28) = 96px of table.
    const SHORT: (f32, f32) = (WINDOW_WIDTH, 400.0);

    /// Controls the frame always records, whatever the state.
    const ALWAYS_DRAWN: [Target; 8] = [
        Target::Search,
        Target::Toolbar(ToolbarAction::Add),
        Target::Toolbar(ToolbarAction::Remove),
        Target::Toolbar(ToolbarAction::Enable),
        Target::Toolbar(ToolbarAction::Disable),
        Target::Toolbar(ToolbarAction::Refresh),
        Target::Column(SortColumn::Name),
        Target::Column(SortColumn::Path),
    ];

    /// Select the first visible row and return the entry id it holds.
    fn select_first_row(ui: &mut StartupUI) -> u64 {
        let id = ui.visible_entries().first().map(|e| e.id).unwrap();
        assert_eq!(probe::click(ui, Target::Row(id)), EventResult::Consumed);
        assert_eq!(ui.selected_id, Some(id));
        id
    }

    #[test]
    fn every_control_answers_where_the_frame_draws_it() {
        let ui = StartupUI::new();
        for target in ALWAYS_DRAWN {
            let rect =
                probe::rect_of(&ui, target).unwrap_or_else(|| panic!("{target:?} was never drawn"));
            let (x, y) = rect.centre();
            assert_eq!(
                ui.target_at(x, y),
                Some(target),
                "{target:?} does not answer at the centre of its own hit box"
            );
        }
    }

    #[test]
    fn no_size_puts_a_hit_box_outside_the_window() {
        // A `Frame` does not clip to the window, so a layout that clamped
        // instead of shrinking would record controls off the edge -- clickable
        // by clicking nothing. Includes a dialog, which is the widest thing
        // drawn and therefore the first to hang over.
        for (w, h) in [
            (WINDOW_WIDTH, WINDOW_HEIGHT),
            (640.0, 480.0),
            (320.0, 240.0),
            (200.0, 120.0),
            (40.0, 40.0),
            (1.0, 1.0),
        ] {
            for open_dialog in [false, true] {
                let mut ui = StartupUI::new();
                if open_dialog {
                    ui.open_add_dialog();
                }
                let frame = ui.frame(w, h);
                let window = Rect::new(0.0, 0.0, w, h);
                for (target, rect) in frame.hits() {
                    assert!(
                        rect.intersect(window) == Some(*rect),
                        "{target:?} at {rect:?} sticks out of a {w}x{h} window"
                    );
                }
                assert!(frame.is_balanced(), "unbalanced clip/translate at {w}x{h}");
            }
        }
    }

    #[test]
    fn a_click_selects_the_row_it_lands_on() {
        let mut ui = StartupUI::new();
        assert!(ui.selected_id.is_none());
        let second = ui.visible_entries().get(1).map(|e| e.id).unwrap();
        assert_eq!(
            probe::click(&mut ui, Target::Row(second)),
            EventResult::Consumed
        );
        assert_eq!(ui.selected_id, Some(second));
    }

    #[test]
    fn the_empty_table_below_the_last_row_deselects() {
        let mut ui = StartupUI::new();
        select_first_row(&mut ui);
        // The default window fits 11 rows and the sample data has 8, so the
        // bottom of the viewport is bare table.
        let l = layout();
        let (x, y) = l.row(l.rows().saturating_sub(1)).centre();
        assert_eq!(ui.target_at(x, y), Some(Target::Table));
        assert_eq!(
            ui.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            EventResult::Consumed
        );
        assert!(ui.selected_id.is_none());
    }

    #[test]
    fn a_column_header_sorts_by_it_and_a_second_click_flips_the_order() {
        let mut ui = StartupUI::new();
        assert_eq!(ui.sort_column, SortColumn::Name);

        probe::click(&mut ui, Target::Column(SortColumn::Impact));
        assert_eq!(ui.sort_column, SortColumn::Impact);
        assert_eq!(ui.sort_order, SortOrder::Ascending);

        probe::click(&mut ui, Target::Column(SortColumn::Impact));
        assert_eq!(ui.sort_column, SortColumn::Impact);
        assert_eq!(ui.sort_order, SortOrder::Descending);

        // And the table really is in that order, not merely labelled so.
        let impacts: Vec<StartupImpact> = ui.visible_entries().iter().map(|e| e.impact).collect();
        assert!(
            impacts.windows(2).all(|w| w[0] >= w[1]),
            "the header says descending impact but the rows read {impacts:?}"
        );

        probe::click(&mut ui, Target::Column(SortColumn::Name));
        assert_eq!(
            ui.sort_order,
            SortOrder::Ascending,
            "a new column starts ascending"
        );
        let names: Vec<String> = ui
            .visible_entries()
            .iter()
            .map(|e| e.name.to_ascii_lowercase())
            .collect();
        assert!(
            names.windows(2).all(|w| w[0] <= w[1]),
            "not by name: {names:?}"
        );
    }

    #[test]
    fn typing_only_reaches_the_search_box_once_it_has_been_clicked() {
        let mut ui = StartupUI::new();
        probe::type_str(&mut ui, "audio");
        assert_eq!(ui.search_query, "", "the table stole the keystrokes");

        probe::click(&mut ui, Target::Search);
        assert!(ui.search_focused);
        probe::type_str(&mut ui, "audio");
        assert_eq!(ui.search_query, "audio");
        assert_eq!(ui.filtered_count(), 1);
        assert_eq!(
            ui.visible_entries().first().map(|e| e.name.as_str()),
            Some("Audio Service")
        );
    }

    #[test]
    fn a_click_anywhere_else_takes_the_caret_out_of_the_search_box() {
        let mut ui = StartupUI::new();
        probe::click(&mut ui, Target::Search);
        assert!(ui.search_focused);
        probe::click(&mut ui, Target::Column(SortColumn::Name));
        assert!(!ui.search_focused);
        probe::type_str(&mut ui, "x");
        assert_eq!(ui.search_query, "");
    }

    #[test]
    fn backspace_edits_the_search_box_and_resets_the_viewport() {
        let mut ui = StartupUI::new();
        probe::click(&mut ui, Target::Search);
        probe::type_str(&mut ui, "cloud");
        assert_eq!(ui.filtered_count(), 1);
        for _ in 0..5 {
            probe::key(&mut ui, &probe::press(Key::Backspace));
        }
        assert_eq!(ui.search_query, "");
        assert_eq!(ui.scroll_offset, 0);
        assert_eq!(ui.filtered_count(), ui.manager.entry_count());
    }

    #[test]
    fn escape_backs_out_of_one_thing_at_a_time() {
        let mut ui = StartupUI::new();
        probe::click(&mut ui, Target::Search);
        probe::type_str(&mut ui, "s");
        select_first_row(&mut ui);
        // The row click already dropped the caret, so re-take it.
        probe::click(&mut ui, Target::Search);

        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Escape)),
            EventResult::Consumed
        );
        assert!(!ui.search_focused, "first escape gives up the caret");
        assert_eq!(ui.search_query, "s");

        probe::key(&mut ui, &probe::press(Key::Escape));
        assert_eq!(ui.search_query, "", "second escape clears the filter");
        assert!(ui.selected_id.is_some());

        probe::key(&mut ui, &probe::press(Key::Escape));
        assert!(ui.selected_id.is_none(), "third escape drops the selection");

        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Escape)),
            EventResult::Ignored,
            "with nothing left to back out of, escape is not ours"
        );
    }

    #[test]
    fn an_open_dialog_takes_the_clicks_of_everything_it_covers() {
        let mut ui = StartupUI::new();
        let search = probe::rect_of(&ui, Target::Search).unwrap();
        ui.open_add_dialog();
        assert!(
            probe::rect_of(&ui, Target::Search).is_none(),
            "the search box is still clickable behind a modal dialog"
        );
        let (x, y) = search.centre();
        assert_eq!(ui.target_at(x, y), Some(Target::Scrim));
        assert_eq!(
            ui.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            EventResult::Consumed
        );
        assert!(!ui.search_focused);
        assert!(
            matches!(ui.dialog, DialogState::AddEdit(_)),
            "the scrim closed the dialog"
        );
    }

    #[test]
    fn the_add_dialog_registers_an_entry_that_was_typed_into_it() {
        let mut ui = StartupUI::new();
        let before = ui.manager.entry_count();

        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Add));
        assert!(matches!(ui.dialog, DialogState::AddEdit(_)));

        probe::click(&mut ui, Target::DialogField(0));
        probe::type_str(&mut ui, "Backup Agent");
        probe::click(&mut ui, Target::DialogField(1));
        probe::type_str(&mut ui, "/opt/backup/agent");
        probe::click(&mut ui, Target::DialogField(3));
        probe::type_str(&mut ui, "BackupCo");
        probe::click(&mut ui, Target::DialogSave);

        assert_eq!(ui.dialog, DialogState::Closed);
        assert_eq!(ui.manager.entry_count(), before + 1);
        let added = ui
            .selected_id
            .and_then(|id| ui.manager.get_entry(id))
            .unwrap();
        assert_eq!(added.name, "Backup Agent");
        assert_eq!(added.path, "/opt/backup/agent");
        assert_eq!(added.publisher, "BackupCo");
    }

    #[test]
    fn the_add_dialog_stays_open_and_says_why_when_it_cannot_save() {
        let mut ui = StartupUI::new();
        let before = ui.manager.entry_count();
        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Add));
        probe::click(&mut ui, Target::DialogField(0));
        probe::type_str(&mut ui, "No Path");
        probe::click(&mut ui, Target::DialogSave);

        assert!(
            matches!(ui.dialog, DialogState::AddEdit(_)),
            "it closed anyway"
        );
        assert_eq!(ui.manager.entry_count(), before);
        assert!(
            ui.status.contains("Path"),
            "the refusal was silent: status is {:?}",
            ui.status
        );
        // And the message is on screen, not just in a field.
        let tree = ui.render();
        assert!(
            tree.commands.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == &ui.status
            )),
            "the status was never drawn"
        );
    }

    #[test]
    fn the_dialog_cyclers_walk_the_type_and_the_impact() {
        let mut ui = StartupUI::new();
        ui.open_add_dialog();
        let type_of = |ui: &StartupUI| match &ui.dialog {
            DialogState::AddEdit(d) => d.selected_type(),
            _ => panic!("dialog closed"),
        };
        let impact_of = |ui: &StartupUI| match &ui.dialog {
            DialogState::AddEdit(d) => d.selected_impact(),
            _ => panic!("dialog closed"),
        };

        let start = type_of(&ui);
        probe::click(&mut ui, Target::DialogType);
        assert_ne!(type_of(&ui), start);
        for _ in 1..StartupType::all().len() {
            probe::click(&mut ui, Target::DialogType);
        }
        assert_eq!(type_of(&ui), start, "the cycler does not come back around");

        let start = impact_of(&ui);
        probe::click(&mut ui, Target::DialogImpact);
        assert_ne!(impact_of(&ui), start);
    }

    #[test]
    fn tab_walks_the_dialog_fields_and_shift_tab_walks_back() {
        let mut ui = StartupUI::new();
        ui.open_add_dialog();
        let focus = |ui: &StartupUI| match &ui.dialog {
            DialogState::AddEdit(d) => d.focused_field,
            _ => panic!("dialog closed"),
        };
        assert_eq!(focus(&ui), 0);
        probe::key(&mut ui, &probe::press(Key::Tab));
        assert_eq!(focus(&ui), 1);
        probe::key(&mut ui, &probe::shift(Key::Tab));
        assert_eq!(focus(&ui), 0);
        probe::key(&mut ui, &probe::shift(Key::Tab));
        assert_eq!(
            focus(&ui),
            AddEditDialog::FIELD_COUNT - 1,
            "shift-tab wraps"
        );

        // And typing lands in whichever field the focus reached.
        probe::type_str(&mut ui, "notes");
        match &ui.dialog {
            DialogState::AddEdit(d) => assert_eq!(d.description, "notes"),
            _ => panic!("dialog closed"),
        }
    }

    #[test]
    fn enter_edits_the_selected_entry_and_the_dialog_saves_the_change() {
        let mut ui = StartupUI::new();
        let id = select_first_row(&mut ui);
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::Enter)),
            EventResult::Consumed
        );
        match &ui.dialog {
            DialogState::AddEdit(d) => assert_eq!(d.editing_id, Some(id)),
            other => panic!("expected the edit dialog, got {other:?}"),
        }

        probe::click(&mut ui, Target::DialogField(3));
        probe::type_str(&mut ui, "!");
        probe::click(&mut ui, Target::DialogSave);
        assert_eq!(ui.dialog, DialogState::Closed);
        assert!(ui.manager.get_entry(id).unwrap().publisher.ends_with('!'));
        assert_eq!(
            ui.manager.entry_count(),
            8,
            "editing an entry created a second one"
        );
    }

    #[test]
    fn delete_asks_first_and_only_removes_when_confirmed() {
        let mut ui = StartupUI::new();
        let id = select_first_row(&mut ui);
        let before = ui.manager.entry_count();

        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Remove));
        assert_eq!(ui.dialog, DialogState::ConfirmDelete(id));
        probe::click(&mut ui, Target::DeleteCancel);
        assert_eq!(ui.dialog, DialogState::Closed);
        assert_eq!(ui.manager.entry_count(), before, "cancel deleted it anyway");

        probe::key(&mut ui, &probe::press(Key::Delete));
        assert_eq!(ui.dialog, DialogState::ConfirmDelete(id));
        probe::click(&mut ui, Target::DeleteConfirm);
        assert_eq!(ui.manager.entry_count(), before - 1);
        assert!(ui.manager.get_entry(id).is_none());
        assert!(ui.selected_id.is_none(), "the selection outlived its entry");
    }

    #[test]
    fn remove_with_nothing_selected_says_so_instead_of_opening_a_dialog() {
        let mut ui = StartupUI::new();
        assert!(ui.selected_id.is_none());
        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Remove));
        assert_eq!(ui.dialog, DialogState::Closed);
        assert!(ui.status.contains("Select"), "status is {:?}", ui.status);
    }

    #[test]
    fn the_enable_and_disable_buttons_toggle_the_selected_entry() {
        let mut ui = StartupUI::new();
        let id = select_first_row(&mut ui);
        assert!(ui.manager.get_entry(id).unwrap().enabled);

        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Disable));
        assert!(!ui.manager.get_entry(id).unwrap().enabled);
        probe::click(&mut ui, Target::Toolbar(ToolbarAction::Enable));
        assert!(ui.manager.get_entry(id).unwrap().enabled);
    }

    #[test]
    fn refresh_drops_a_selection_whose_entry_is_gone() {
        let mut ui = StartupUI::new();
        let id = select_first_row(&mut ui);
        // Remove it behind the UI's back, the way a rescan would find.
        ui.manager.remove_entry(id);
        assert_eq!(ui.selected_id, Some(id));
        assert_eq!(
            probe::key(&mut ui, &probe::press(Key::F5)),
            EventResult::Consumed
        );
        assert!(ui.selected_id.is_none());
        assert!(ui.status.contains('7'), "status is {:?}", ui.status);
    }

    #[test]
    fn the_wheel_scrolls_the_table_and_stops_at_both_ends() {
        let mut ui = StartupUI::new();
        ui.resize(SHORT.0, SHORT.1);
        assert_eq!(
            ui.visible_rows(),
            3,
            "the short window should fit three rows"
        );

        let (x, y) = probe::rect_of_sized(&ui, Target::Table, SHORT)
            .unwrap()
            .centre();
        let wheel_at = |ui: &mut StartupUI, dy: f32| {
            ui.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            }))
        };

        assert_eq!(wheel_at(&mut ui, -1.0), EventResult::Consumed);
        assert_eq!(ui.scroll_offset, 3, "one notch is three rows");
        wheel_at(&mut ui, -10.0);
        assert_eq!(ui.scroll_offset, 5, "8 entries less a 3-row viewport");
        wheel_at(&mut ui, 10.0);
        assert_eq!(ui.scroll_offset, 0, "the wheel scrolled past the top");
    }

    #[test]
    fn the_wheel_over_the_toolbar_leaves_the_table_alone() {
        let mut ui = StartupUI::new();
        ui.resize(SHORT.0, SHORT.1);
        let (x, y) = probe::rect_of_sized(&ui, Target::Toolbar(ToolbarAction::Add), SHORT)
            .unwrap()
            .centre();
        assert_eq!(
            ui.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy: -3.0 },
            })),
            EventResult::Ignored
        );
        assert_eq!(ui.scroll_offset, 0);
    }

    #[test]
    fn the_arrows_walk_the_selection_and_drag_the_viewport_with_them() {
        // Every event goes in at `SHORT`, not at `Probe::SIZE`: the probe
        // helpers resize to the default first, which would silently give the
        // viewport all eight rows and make the scrolling untestable.
        let mut ui = StartupUI::new();
        for _ in 0..8 {
            ui.key_at(&probe::press(Key::Down), SHORT);
        }
        assert_eq!(ui.scroll_offset, 5, "the selection walked off the bottom");
        let last = ui.selected_id;
        // Still there after one more: the walk stops at the end of the list.
        ui.key_at(&probe::press(Key::Down), SHORT);
        assert_eq!(ui.selected_id, last);

        for _ in 0..8 {
            ui.key_at(&probe::press(Key::Up), SHORT);
        }
        assert_eq!(ui.scroll_offset, 0, "the viewport did not follow back up");
    }

    #[test]
    fn home_and_end_jump_to_the_ends_of_the_list() {
        let mut ui = StartupUI::new();
        ui.key_at(&probe::press(Key::End), SHORT);
        assert_eq!(ui.scroll_offset, 5);
        let last = ui.all_entries().last().map(|e| e.id);
        assert_eq!(ui.selected_id, last);

        ui.key_at(&probe::press(Key::Home), SHORT);
        assert_eq!(ui.scroll_offset, 0);
        assert_eq!(ui.selected_id, ui.all_entries().first().map(|e| e.id));
    }

    #[test]
    fn a_selection_that_scrolled_out_of_view_is_still_the_selection() {
        let mut ui = StartupUI::new();
        ui.resize(SHORT.0, SHORT.1);
        let id = ui.visible_entries().first().map(|e| e.id).unwrap();
        probe::click_sized(&mut ui, Target::Row(id), MouseButton::Left, SHORT);
        assert_eq!(ui.selected_id, Some(id));
        ui.scroll_rows(3.0);
        assert_eq!(ui.scroll_offset, 3);
        assert_eq!(ui.selected_id, Some(id));
        assert!(
            probe::rect_of_sized(&ui, Target::Row(id), SHORT).is_none(),
            "the row is off screen but still drawn"
        );
    }

    #[test]
    fn ctrl_n_opens_the_add_dialog_and_ctrl_f_takes_the_caret() {
        let mut ui = StartupUI::new();
        probe::key(&mut ui, &probe::ctrl(Key::N));
        assert!(matches!(ui.dialog, DialogState::AddEdit(_)));
        probe::key(&mut ui, &probe::press(Key::Escape));
        assert_eq!(ui.dialog, DialogState::Closed);

        probe::key(&mut ui, &probe::ctrl(Key::F));
        assert!(ui.search_focused);
        probe::type_str(&mut ui, "gpu");
        assert_eq!(ui.filtered_count(), 1);
    }

    #[test]
    fn a_resized_window_lays_out_again_and_the_hit_test_follows() {
        let mut ui = StartupUI::new();
        let wide = probe::rect_of(&ui, Target::Search).unwrap();
        assert_eq!(
            ui.handle_event(&Event::Resize {
                width: 500,
                height: 400,
            }),
            EventResult::Consumed
        );
        let narrow = probe::rect_of_sized(&ui, Target::Search, (500.0, 400.0)).unwrap();
        assert!(
            narrow.w < wide.w,
            "the search box did not shrink with the window"
        );
        let (x, y) = narrow.centre();
        assert_eq!(ui.target_at(x, y), Some(Target::Search));
    }

    #[test]
    fn render_lays_out_at_the_size_it_is_handed() {
        let mut ui = StartupUI::new();
        let tree = App::render(&mut ui, 700.0, 500.0);
        assert!(!tree.commands.is_empty());
        assert_eq!(ui.window_width, 700.0);
        assert_eq!(ui.window_height, 500.0);
        // 500 - (48 + 40 + 36 + 32 + 120 + 28) = 196 -> six rows.
        assert_eq!(ui.visible_rows(), 6);
    }

    #[test]
    fn a_degenerate_size_does_not_poison_the_layout() {
        let mut ui = StartupUI::new();
        for (w, h) in [(0.0, 0.0), (f32::NAN, 100.0), (-40.0, f32::INFINITY)] {
            let tree = App::render(&mut ui, w, h);
            assert!(ui.window_width.is_finite() && ui.window_width >= 0.0);
            assert!(ui.window_height.is_finite() && ui.window_height >= 0.0);
            // Drawing nothing is a perfectly good answer for a zero-size
            // window; drawing a NaN rectangle is not.
            for command in &tree.commands {
                if let RenderCommand::FillRect { x, y, .. } = command {
                    assert!(x.is_finite() && y.is_finite(), "NaN geometry at {w}x{h}");
                }
            }
            assert_eq!(ui.visible_rows(), 0);
        }
    }

    #[test]
    fn ctrl_q_closes_the_window_and_the_close_button_does_too() {
        let mut ui = StartupUI::new();
        let quit = KeyEvent {
            key: Key::Q,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            text: String::new(),
        };
        assert_eq!(ui.on_event(&Event::Key(quit)), Response::Exit);
        assert_eq!(ui.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn only_an_event_that_changes_something_asks_for_a_frame() {
        let mut ui = StartupUI::new();
        // A move over the window changes nothing and must not repaint.
        assert_eq!(
            ui.on_event(&Event::Mouse(MouseEvent {
                x: 10.0,
                y: 10.0,
                kind: MouseEventKind::Move,
            })),
            Response::Idle
        );
        assert_eq!(ui.on_event(&Event::FocusIn), Response::Idle);
        let add = probe::rect_of(&ui, Target::Toolbar(ToolbarAction::Add)).unwrap();
        let (x, y) = add.centre();
        assert_eq!(
            ui.on_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            })),
            Response::Redraw
        );
    }

    #[test]
    fn every_target_the_frame_records_is_one_the_app_handles() {
        // Three states, because the dialogs replace the whole hit list.
        let states: [fn(&mut StartupUI); 3] = [
            |_| {},
            |ui| ui.open_add_dialog(),
            |ui| {
                ui.select_next();
                ui.open_delete_dialog();
            },
        ];
        let mut seen: Vec<String> = Vec::new();
        for build in states {
            let mut probe_ui = StartupUI::new();
            build(&mut probe_ui);
            let targets: Vec<Target> = probe_ui
                .frame(WINDOW_WIDTH, WINDOW_HEIGHT)
                .hits()
                .iter()
                .map(|(t, _)| *t)
                .collect();
            for name in probe::control_names(&probe_ui) {
                if !seen.contains(&name) {
                    seen.push(name);
                }
            }
            for target in targets {
                let mut fresh = StartupUI::new();
                build(&mut fresh);
                assert_eq!(
                    probe::click(&mut fresh, target),
                    EventResult::Consumed,
                    "{target:?} is drawn but nothing handles it"
                );
            }
        }

        for expected in [
            "Search",
            "Toolbar",
            "Column",
            "Row",
            "Table",
            "DialogField",
            "DialogType",
            "DialogImpact",
            "DialogSave",
            "DialogCancel",
            "DeleteConfirm",
            "DeleteCancel",
            "Scrim",
        ] {
            assert!(
                seen.iter().any(|n| n == expected),
                "{expected} is a target no state ever draws; seen: {seen:?}"
            );
        }
    }
}
