//! startupmanager -- OurOS Startup Apps Manager
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

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;

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
        if self.enabled { COLOR_GREEN } else { COLOR_OVERLAY0 }
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
                SortColumn::Name => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Publisher => a
                    .publisher
                    .to_ascii_lowercase()
                    .cmp(&b.publisher.to_ascii_lowercase()),
                SortColumn::Status => a.enabled.cmp(&b.enabled),
                SortColumn::Impact => a.impact.cmp(&b.impact),
                SortColumn::Type => a.startup_type.cmp(&b.startup_type),
                SortColumn::Path => a.path.to_ascii_lowercase().cmp(&b.path.to_ascii_lowercase()),
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
                SortColumn::Name => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Publisher => a
                    .publisher
                    .to_ascii_lowercase()
                    .cmp(&b.publisher.to_ascii_lowercase()),
                SortColumn::Status => a.enabled.cmp(&b.enabled),
                SortColumn::Impact => a.impact.cmp(&b.impact),
                SortColumn::Type => a.startup_type.cmp(&b.startup_type),
                SortColumn::Path => a.path.to_ascii_lowercase().cmp(&b.path.to_ascii_lowercase()),
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
        let mut s = StartupStats::default();
        s.total = self.entries.len();
        for entry in self.entries.values() {
            if entry.enabled {
                s.enabled += 1;
                s.total_impact_weight = s
                    .total_impact_weight
                    .saturating_add(entry.impact.weight());
            } else {
                s.disabled += 1;
            }
            match entry.startup_type {
                StartupType::Login => s.login_count += 1,
                StartupType::Service => s.service_count += 1,
                StartupType::Scheduled => s.scheduled_count += 1,
                StartupType::Driver => s.driver_count += 1,
            }
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
            "OurOS",
            "System tray notification area",
            1700000000,
        );
        self.add_entry(
            "Network Manager",
            "/usr/sbin/networkd",
            "--daemon",
            StartupType::Service,
            StartupImpact::Medium,
            "OurOS",
            "Manages network connections and interfaces",
            1700000100,
        );
        self.add_entry(
            "Audio Service",
            "/usr/sbin/audiod",
            "",
            StartupType::Service,
            StartupImpact::Low,
            "OurOS",
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
            "OurOS",
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
            "OurOS",
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
        out.push_str("# OurOS Startup Manager Configuration\n");
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

            if trimmed.starts_with("VERSION|") {
                // Version check — we only support version 1.
                let ver_str = &trimmed["VERSION|".len()..];
                if ver_str.trim() != "1" {
                    return Err(ConfigError::UnsupportedVersion(ver_str.trim().to_string()));
                }
                continue;
            }

            if trimmed.starts_with("ENTRY|") {
                let rest = &trimmed["ENTRY|".len()..];
                let fields: Vec<&str> = rest.splitn(10, '|').collect();
                if fields.len() < 10 {
                    return Err(ConfigError::MalformedEntry(trimmed.to_string()));
                }

                let id: u64 = fields[0]
                    .parse()
                    .map_err(|_| ConfigError::InvalidField("id".to_string()))?;
                let name = Self::unescape_field(fields[1]);
                let path = Self::unescape_field(fields[2]);
                let args = Self::unescape_field(fields[3]);
                let startup_type = StartupType::from_label(fields[4])
                    .ok_or_else(|| ConfigError::InvalidField("type".to_string()))?;
                let enabled = fields[5] == "1";
                let impact = StartupImpact::from_label(fields[6])
                    .ok_or_else(|| ConfigError::InvalidField("impact".to_string()))?;
                let publisher = Self::unescape_field(fields[7]);
                let description = Self::unescape_field(fields[8]);
                let added_timestamp: u64 = fields[9]
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
                continue;
            }

            // Unknown line types are silently skipped for forward compatibility.
        }

        // Ensure next_id is beyond any imported entry.
        manager.next_id = max_id.saturating_add(1);
        Ok(manager)
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
        let count = StartupType::all().len();
        if count > 0 {
            self.startup_type_index = (self.startup_type_index + 1) % count;
        }
    }

    /// Cycle the impact level forward.
    pub fn next_impact(&mut self) {
        let count = StartupImpact::all().len();
        if count > 0 {
            self.impact_index = (self.impact_index + 1) % count;
        }
    }

    /// Number of text fields that can be focused.
    pub fn field_count(&self) -> usize {
        5
    }

    /// Move focus to the next field.
    pub fn focus_next(&mut self) {
        self.focused_field = (self.focused_field + 1) % self.field_count();
    }

    /// Move focus to the previous field.
    pub fn focus_prev(&mut self) {
        if self.focused_field == 0 {
            self.focused_field = self.field_count() - 1;
        } else {
            self.focused_field -= 1;
        }
    }
}

// ============================================================================
// StartupUI — GUI state and rendering
// ============================================================================

/// Full application state for the startup manager UI.
pub struct StartupUI {
    pub manager: StartupManager,
    pub sort_column: SortColumn,
    pub sort_order: SortOrder,
    pub search_query: String,
    pub selected_id: Option<u64>,
    pub dialog: DialogState,
    pub scroll_offset: usize,
    pub window_width: f32,
    pub window_height: f32,
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
        }
    }

    /// How many table rows fit in the visible area.
    pub fn visible_rows(&self) -> usize {
        let available = self.window_height
            - HEADER_HEIGHT
            - TOOLBAR_HEIGHT
            - SEARCH_BAR_HEIGHT
            - TABLE_HEADER_HEIGHT
            - DETAILS_PANEL_HEIGHT
            - STATUS_BAR_HEIGHT;
        if available <= 0.0 {
            return 0;
        }
        (available / ROW_HEIGHT) as usize
    }

    /// Get the currently visible filtered and sorted entries.
    pub fn visible_entries(&self) -> Vec<&StartupEntry> {
        let all = self.manager.filtered_sorted(
            &self.search_query,
            self.sort_column,
            self.sort_order,
        );
        let start = self.scroll_offset.min(all.len());
        let end = (start + self.visible_rows()).min(all.len());
        all.get(start..end).unwrap_or(&[]).to_vec()
    }

    /// Total number of filtered entries.
    pub fn filtered_count(&self) -> usize {
        self.manager.search_entries(&self.search_query).len()
    }

    /// Sort by the given column, toggling order if same column.
    pub fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_order = self.sort_order.toggle();
        } else {
            self.sort_column = column;
            self.sort_order = SortOrder::Ascending;
        }
    }

    /// Select the next entry in the filtered list.
    pub fn select_next(&mut self) {
        let entries = self.manager.filtered_sorted(
            &self.search_query,
            self.sort_column,
            self.sort_order,
        );
        if entries.is_empty() {
            return;
        }
        let current_pos = self
            .selected_id
            .and_then(|id| entries.iter().position(|e| e.id == id));
        let next_pos = match current_pos {
            Some(pos) if pos + 1 < entries.len() => pos + 1,
            Some(pos) => pos,
            Option::None => 0,
        };
        if let Some(entry) = entries.get(next_pos) {
            self.selected_id = Some(entry.id);
        }
        // Adjust scroll to keep selection visible.
        let vis = self.visible_rows();
        if vis > 0 && next_pos >= self.scroll_offset + vis {
            self.scroll_offset = next_pos - vis + 1;
        }
    }

    /// Select the previous entry in the filtered list.
    pub fn select_prev(&mut self) {
        let entries = self.manager.filtered_sorted(
            &self.search_query,
            self.sort_column,
            self.sort_order,
        );
        if entries.is_empty() {
            return;
        }
        let current_pos = self
            .selected_id
            .and_then(|id| entries.iter().position(|e| e.id == id));
        let prev_pos = match current_pos {
            Some(0) => 0,
            Some(pos) => pos - 1,
            Option::None => 0,
        };
        if let Some(entry) = entries.get(prev_pos) {
            self.selected_id = Some(entry.id);
        }
        if prev_pos < self.scroll_offset {
            self.scroll_offset = prev_pos;
        }
    }

    /// Open the add dialog.
    pub fn open_add_dialog(&mut self) {
        self.dialog = DialogState::AddEdit(AddEditDialog::new_add());
    }

    /// Open the edit dialog for the selected entry.
    pub fn open_edit_dialog(&mut self) {
        if let Some(id) = self.selected_id {
            if let Some(entry) = self.manager.get_entry(id) {
                self.dialog = DialogState::AddEdit(AddEditDialog::new_edit(entry));
            }
        }
    }

    /// Open the confirm-delete dialog for the selected entry.
    pub fn open_delete_dialog(&mut self) {
        if let Some(id) = self.selected_id {
            if self.manager.get_entry(id).is_some() {
                self.dialog = DialogState::ConfirmDelete(id);
            }
        }
    }

    /// Close any open dialog.
    pub fn close_dialog(&mut self) {
        self.dialog = DialogState::Closed;
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

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full UI into a `RenderTree`.
    pub fn render(&self) -> RenderTree {
        let mut tree = RenderTree::new();

        // Background fill.
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut tree);
        self.render_toolbar(&mut tree);
        self.render_search_bar(&mut tree);
        self.render_table_header(&mut tree);
        self.render_table_body(&mut tree);
        self.render_details_panel(&mut tree);
        self.render_status_bar(&mut tree);

        // Dialogs render on top.
        match &self.dialog {
            DialogState::Closed => {}
            DialogState::AddEdit(dlg) => self.render_add_edit_dialog(&mut tree, dlg),
            DialogState::ConfirmDelete(id) => self.render_confirm_delete_dialog(&mut tree, *id),
        }

        tree
    }

    /// Render the header bar.
    fn render_header(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: HEADER_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        tree.push(RenderCommand::Text {
            x: PADDING,
            y: (HEADER_HEIGHT - FONT_SIZE_HEADING) / 2.0,
            text: "Startup Apps Manager".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });
        // Separator line.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: HEADER_HEIGHT,
            x2: self.window_width,
            y2: HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the toolbar with action buttons.
    fn render_toolbar(&self, tree: &mut RenderTree) {
        let y = HEADER_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let buttons = ["Add", "Remove", "Enable", "Disable", "Refresh"];
        let button_colors = [
            COLOR_BLUE,
            COLOR_RED,
            COLOR_GREEN,
            COLOR_PEACH,
            COLOR_SUBTEXT,
        ];

        let mut bx = PADDING;
        let by = y + (TOOLBAR_HEIGHT - BUTTON_HEIGHT) / 2.0;

        for (i, label) in buttons.iter().enumerate() {
            let color = button_colors.get(i).copied().unwrap_or(COLOR_SUBTEXT);
            self.render_button(tree, bx, by, BUTTON_WIDTH, BUTTON_HEIGHT, label, color);
            bx += BUTTON_WIDTH + 8.0;
        }
    }

    /// Render a single button.
    fn render_button(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        color: Color,
    ) {
        tree.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: Color::rgba(color.r, color.g, color.b, 40),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y,
            width: w,
            height: h,
            color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        tree.push(RenderCommand::Text {
            x: x + w / 2.0 - (label.len() as f32 * FONT_SIZE * 0.3),
            y: y + (h - FONT_SIZE) / 2.0,
            text: label.to_string(),
            color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(w - 8.0),
        });
    }

    /// Render the search bar.
    fn render_search_bar(&self, tree: &mut RenderTree) {
        let y = HEADER_HEIGHT + TOOLBAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: SEARCH_BAR_HEIGHT,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Search field background.
        let field_x = PADDING;
        let field_y = y + 4.0;
        let field_w = self.window_width - PADDING * 2.0;
        let field_h = SEARCH_BAR_HEIGHT - 8.0;

        tree.push(RenderCommand::FillRect {
            x: field_x,
            y: field_y,
            width: field_w,
            height: field_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        let display_text = if self.search_query.is_empty() {
            "Search by name, publisher, or path..."
        } else {
            &self.search_query
        };
        let text_color = if self.search_query.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };

        tree.push(RenderCommand::Text {
            x: field_x + 8.0,
            y: field_y + (field_h - FONT_SIZE) / 2.0,
            text: display_text.to_string(),
            color: text_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 16.0),
        });
    }

    /// Render the table header row.
    fn render_table_header(&self, tree: &mut RenderTree) {
        let y = HEADER_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::ZERO,
        });

        let mut cx = PADDING;
        for col in SortColumn::all() {
            let is_active = *col == self.sort_column;
            let color = if is_active { COLOR_BLUE } else { COLOR_TEXT };
            let weight = if is_active {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            };

            let mut label = col.header().to_string();
            if is_active {
                let arrow = match self.sort_order {
                    SortOrder::Ascending => " ^",
                    SortOrder::Descending => " v",
                };
                label.push_str(arrow);
            }

            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: y + (TABLE_HEADER_HEIGHT - FONT_SIZE_SMALL) / 2.0,
                text: label,
                color,
                font_size: FONT_SIZE_SMALL,
                font_weight: weight,
                max_width: Some(col.width() - 8.0),
            });
            cx += col.width();
        }

        // Bottom border.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + TABLE_HEADER_HEIGHT,
            x2: self.window_width,
            y2: y + TABLE_HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the table body rows.
    fn render_table_body(&self, tree: &mut RenderTree) {
        let base_y = HEADER_HEIGHT + TOOLBAR_HEIGHT + SEARCH_BAR_HEIGHT + TABLE_HEADER_HEIGHT;
        let entries = self.visible_entries();

        for (i, entry) in entries.iter().enumerate() {
            let row_y = base_y + i as f32 * ROW_HEIGHT;
            let is_selected = self.selected_id == Some(entry.id);

            // Row background.
            let bg = if is_selected {
                Color::rgba(COLOR_BLUE.r, COLOR_BLUE.g, COLOR_BLUE.b, 30)
            } else if i % 2 == 1 {
                Color::rgba(
                    COLOR_SURFACE0.r,
                    COLOR_SURFACE0.g,
                    COLOR_SURFACE0.b,
                    80,
                )
            } else {
                COLOR_BASE
            };

            tree.push(RenderCommand::FillRect {
                x: 0.0,
                y: row_y,
                width: self.window_width,
                height: ROW_HEIGHT,
                color: bg,
                corner_radii: CornerRadii::ZERO,
            });

            // Selection indicator.
            if is_selected {
                tree.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: row_y,
                    width: 3.0,
                    height: ROW_HEIGHT,
                    color: COLOR_BLUE,
                    corner_radii: CornerRadii::ZERO,
                });
            }

            let text_y = row_y + (ROW_HEIGHT - FONT_SIZE) / 2.0;
            let mut cx = PADDING;

            // Name.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.name.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(COL_NAME_WIDTH - 8.0),
            });
            cx += COL_NAME_WIDTH;

            // Publisher.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.publisher.clone(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(COL_PUBLISHER_WIDTH - 8.0),
            });
            cx += COL_PUBLISHER_WIDTH;

            // Status.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.status_label().to_string(),
                color: entry.status_color(),
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(COL_STATUS_WIDTH - 8.0),
            });
            cx += COL_STATUS_WIDTH;

            // Impact.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.impact.label().to_string(),
                color: entry.impact.color(),
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(COL_IMPACT_WIDTH - 8.0),
            });
            cx += COL_IMPACT_WIDTH;

            // Type.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.startup_type.label().to_string(),
                color: COLOR_SUBTEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(COL_TYPE_WIDTH - 8.0),
            });
            cx += COL_TYPE_WIDTH;

            // Path.
            tree.push(RenderCommand::Text {
                x: cx + 4.0,
                y: text_y,
                text: entry.path.clone(),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(COL_PATH_WIDTH - 8.0),
            });
        }
    }

    /// Render the details panel at the bottom (above status bar).
    fn render_details_panel(&self, tree: &mut RenderTree) {
        let y = self.window_height - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: DETAILS_PANEL_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border.
        tree.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        if let Some(id) = self.selected_id {
            if let Some(entry) = self.manager.get_entry(id) {
                self.render_details_content(tree, y, entry);
                return;
            }
        }

        // No selection message.
        tree.push(RenderCommand::Text {
            x: PADDING,
            y: y + DETAILS_PANEL_HEIGHT / 2.0 - FONT_SIZE / 2.0,
            text: "Select an entry to view details".to_string(),
            color: COLOR_OVERLAY0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Option::None,
        });
    }

    /// Render details content for a selected entry.
    fn render_details_content(&self, tree: &mut RenderTree, base_y: f32, entry: &StartupEntry) {
        let x_left = PADDING;
        let x_right = self.window_width / 2.0;
        let mut ly = base_y + 8.0;
        let line_spacing = 18.0;

        // Name (bold heading).
        tree.push(RenderCommand::Text {
            x: x_left,
            y: ly,
            text: entry.name.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.window_width - PADDING * 2.0),
        });
        ly += line_spacing + 4.0;

        // Left column: path, args.
        self.render_detail_row(tree, x_left, ly, "Path:", &entry.path);
        ly += line_spacing;
        if !entry.args.is_empty() {
            self.render_detail_row(tree, x_left, ly, "Args:", &entry.args);
            ly += line_spacing;
        }
        self.render_detail_row(tree, x_left, ly, "Publisher:", &entry.publisher);

        // Right column: type, impact, status.
        let mut ry = base_y + 8.0 + line_spacing + 4.0;
        self.render_detail_row(tree, x_right, ry, "Type:", entry.startup_type.label());
        ry += line_spacing;
        self.render_detail_row(tree, x_right, ry, "Impact:", entry.impact.label());
        ry += line_spacing;
        self.render_detail_row(tree, x_right, ry, "Status:", entry.status_label());
    }

    /// Render a "Label: Value" pair.
    fn render_detail_row(&self, tree: &mut RenderTree, x: f32, y: f32, label: &str, value: &str) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });
        tree.push(RenderCommand::Text {
            x: x + label.len() as f32 * 7.0 + 8.0,
            y,
            text: value.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width / 2.0 - 40.0),
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, tree: &mut RenderTree) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let stats = self.manager.stats();
        let summary = format!(
            "Total: {}  |  Enabled: {}  |  Disabled: {}  |  Login: {}  Service: {}  Scheduled: {}  Driver: {}  |  Boot Impact: {}",
            stats.total,
            stats.enabled,
            stats.disabled,
            stats.login_count,
            stats.service_count,
            stats.scheduled_count,
            stats.driver_count,
            stats.impact_summary(),
        );

        tree.push(RenderCommand::Text {
            x: PADDING,
            y: y + (STATUS_BAR_HEIGHT - FONT_SIZE_SMALL) / 2.0,
            text: summary,
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width - PADDING * 2.0),
        });
    }

    /// Render a semi-transparent dialog backdrop.
    fn render_dialog_backdrop(&self, tree: &mut RenderTree) {
        tree.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: Color::rgba(0, 0, 0, 150),
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// Render the add/edit dialog.
    fn render_add_edit_dialog(&self, tree: &mut RenderTree, dlg: &AddEditDialog) {
        self.render_dialog_backdrop(tree);

        let dlg_w = 440.0_f32;
        let dlg_h = 380.0_f32;
        let dlg_x = (self.window_width - dlg_w) / 2.0;
        let dlg_y = (self.window_height - dlg_h) / 2.0;

        // Dialog background.
        tree.push(RenderCommand::FillRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title.
        let title = if dlg.editing_id.is_some() {
            "Edit Startup Entry"
        } else {
            "Add Startup Entry"
        };
        tree.push(RenderCommand::Text {
            x: dlg_x + PADDING,
            y: dlg_y + 12.0,
            text: title.to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });

        // Form fields.
        let field_x = dlg_x + PADDING;
        let field_w = dlg_w - PADDING * 2.0;
        let mut fy = dlg_y + 44.0;
        let field_gap = 42.0;

        let fields: [(&str, &str, usize); 5] = [
            ("Name", &dlg.name, 0),
            ("Path", &dlg.path, 1),
            ("Arguments", &dlg.args, 2),
            ("Publisher", &dlg.publisher, 3),
            ("Description", &dlg.description, 4),
        ];

        for (label, value, idx) in &fields {
            let is_focused = dlg.focused_field == *idx;
            self.render_form_field(tree, field_x, fy, field_w, label, value, is_focused);
            fy += field_gap;
        }

        // Type selector.
        let sel_y = fy;
        tree.push(RenderCommand::Text {
            x: field_x,
            y: sel_y,
            text: "Type:".to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });
        tree.push(RenderCommand::Text {
            x: field_x + 80.0,
            y: sel_y,
            text: dlg.selected_type().label().to_string(),
            color: COLOR_BLUE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Option::None,
        });

        // Impact selector.
        tree.push(RenderCommand::Text {
            x: field_x + 200.0,
            y: sel_y,
            text: "Impact:".to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });
        tree.push(RenderCommand::Text {
            x: field_x + 280.0,
            y: sel_y,
            text: dlg.selected_impact().label().to_string(),
            color: dlg.selected_impact().color(),
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Option::None,
        });

        // Dialog buttons (Cancel / Save).
        let btn_y = dlg_y + dlg_h - BUTTON_HEIGHT - 12.0;
        self.render_button(
            tree,
            dlg_x + dlg_w - BUTTON_WIDTH * 2.0 - PADDING - 8.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Cancel",
            COLOR_OVERLAY0,
        );
        self.render_button(
            tree,
            dlg_x + dlg_w - BUTTON_WIDTH - PADDING,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Save",
            COLOR_GREEN,
        );
    }

    /// Render a labeled text input field.
    fn render_form_field(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        value: &str,
        focused: bool,
    ) {
        tree.push(RenderCommand::Text {
            x,
            y,
            text: label.to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });

        let input_y = y + 14.0;
        let input_h = 24.0_f32;
        let border_color = if focused { COLOR_BLUE } else { COLOR_SURFACE1 };

        tree.push(RenderCommand::FillRect {
            x,
            y: input_y,
            width: w,
            height: input_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(4.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x,
            y: input_y,
            width: w,
            height: input_h,
            color: border_color,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        let display = if value.is_empty() {
            label
        } else {
            value
        };
        let text_color = if value.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };

        tree.push(RenderCommand::Text {
            x: x + 6.0,
            y: input_y + (input_h - FONT_SIZE) / 2.0,
            text: display.to_string(),
            color: text_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 12.0),
        });
    }

    /// Render the confirm-delete dialog.
    fn render_confirm_delete_dialog(&self, tree: &mut RenderTree, id: u64) {
        self.render_dialog_backdrop(tree);

        let dlg_w = 360.0_f32;
        let dlg_h = 160.0_f32;
        let dlg_x = (self.window_width - dlg_w) / 2.0;
        let dlg_y = (self.window_height - dlg_h) / 2.0;

        tree.push(RenderCommand::FillRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        tree.push(RenderCommand::StrokeRect {
            x: dlg_x,
            y: dlg_y,
            width: dlg_w,
            height: dlg_h,
            color: COLOR_RED,
            line_width: 1.0,
            corner_radii: CornerRadii::all(8.0),
        });

        tree.push(RenderCommand::Text {
            x: dlg_x + PADDING,
            y: dlg_y + 16.0,
            text: "Confirm Delete".to_string(),
            color: COLOR_RED,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Option::None,
        });

        let name = self
            .manager
            .get_entry(id)
            .map(|e| e.name.as_str())
            .unwrap_or("Unknown");
        let msg = format!("Remove \"{}\" from startup?", name);
        tree.push(RenderCommand::Text {
            x: dlg_x + PADDING,
            y: dlg_y + 50.0,
            text: msg,
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dlg_w - PADDING * 2.0),
        });

        tree.push(RenderCommand::Text {
            x: dlg_x + PADDING,
            y: dlg_y + 74.0,
            text: "This action cannot be undone.".to_string(),
            color: COLOR_SUBTEXT,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Option::None,
        });

        let btn_y = dlg_y + dlg_h - BUTTON_HEIGHT - 12.0;
        self.render_button(
            tree,
            dlg_x + dlg_w - BUTTON_WIDTH * 2.0 - PADDING - 8.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Cancel",
            COLOR_OVERLAY0,
        );
        self.render_button(
            tree,
            dlg_x + dlg_w - BUTTON_WIDTH - PADDING,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            "Delete",
            COLOR_RED,
        );
    }
}

impl Default for StartupUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(StartupType::from_label("SERVICE"), Some(StartupType::Service));
        assert_eq!(StartupType::from_label("Scheduled"), Some(StartupType::Scheduled));
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
        assert_eq!(StartupImpact::from_label("Medium"), Some(StartupImpact::Medium));
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
            1, "Test", "/bin/test", "--flag",
            StartupType::Login, StartupImpact::Low,
            "Publisher", "A test entry", 1000,
        );
        assert_eq!(entry.id, 1);
        assert_eq!(entry.name, "Test");
        assert!(entry.enabled); // Defaults to enabled.
    }

    #[test]
    fn test_entry_status_label() {
        let mut entry = StartupEntry::new(
            1, "T", "/bin/t", "",
            StartupType::Login, StartupImpact::None,
            "", "", 0,
        );
        assert_eq!(entry.status_label(), "Enabled");
        entry.enabled = false;
        assert_eq!(entry.status_label(), "Disabled");
    }

    #[test]
    fn test_entry_status_color() {
        let mut entry = StartupEntry::new(
            1, "T", "/bin/t", "",
            StartupType::Login, StartupImpact::None,
            "", "", 0,
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
            "Test", "/bin/test", "", StartupType::Login,
            StartupImpact::Low, "Pub", "Desc", 1000,
        );
        assert_eq!(id, 1);
        assert_eq!(mgr.entry_count(), 1);
    }

    #[test]
    fn test_manager_add_multiple_entries() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        let id2 = mgr.add_entry("B", "/b", "", StartupType::Service, StartupImpact::High, "", "", 0);
        assert_ne!(id1, id2);
        assert_eq!(mgr.entry_count(), 2);
    }

    #[test]
    fn test_manager_remove_entry() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry("T", "/t", "", StartupType::Login, StartupImpact::None, "", "", 0);
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
        let id = mgr.add_entry("T", "/t", "", StartupType::Login, StartupImpact::None, "", "", 0);

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
        let id = mgr.add_entry("T", "/t", "", StartupType::Login, StartupImpact::None, "", "", 0);

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
        let id = mgr.add_entry("Test", "/bin/test", "", StartupType::Login, StartupImpact::Low, "P", "D", 42);
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
        let id = mgr.add_entry("Old", "/old", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        let ok = mgr.update_entry(id, "New", "/new", "--arg", StartupType::Service, StartupImpact::High, "Pub", "Desc");
        assert!(ok);
        let entry = mgr.get_entry(id);
        assert_eq!(entry.map(|e| e.name.as_str()), Some("New"));
        assert_eq!(entry.map(|e| e.startup_type), Some(StartupType::Service));
    }

    #[test]
    fn test_manager_update_nonexistent() {
        let mut mgr = StartupManager::new();
        assert!(!mgr.update_entry(999, "N", "/n", "", StartupType::Login, StartupImpact::None, "", ""));
    }

    #[test]
    fn test_manager_entry_ids() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let id2 = mgr.add_entry("B", "/b", "", StartupType::Service, StartupImpact::Low, "", "", 0);
        let ids = mgr.entry_ids();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
        assert_eq!(ids.len(), 2);
    }

    // -- Sorting tests ------------------------------------------------------

    #[test]
    fn test_sort_by_name_ascending() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Zebra", "/z", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("Apple", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let sorted = mgr.sorted_entries(SortColumn::Name, SortOrder::Ascending);
        assert_eq!(sorted[0].name, "Apple");
        assert_eq!(sorted[1].name, "Zebra");
    }

    #[test]
    fn test_sort_by_name_descending() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Apple", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("Zebra", "/z", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let sorted = mgr.sorted_entries(SortColumn::Name, SortOrder::Descending);
        assert_eq!(sorted[0].name, "Zebra");
        assert_eq!(sorted[1].name, "Apple");
    }

    #[test]
    fn test_sort_by_impact() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("High", "/h", "", StartupType::Login, StartupImpact::High, "", "", 0);
        mgr.add_entry("Low", "/l", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        mgr.add_entry("Med", "/m", "", StartupType::Login, StartupImpact::Medium, "", "", 0);
        let sorted = mgr.sorted_entries(SortColumn::Impact, SortOrder::Ascending);
        assert_eq!(sorted[0].name, "Low");
        assert_eq!(sorted[1].name, "Med");
        assert_eq!(sorted[2].name, "High");
    }

    #[test]
    fn test_sort_by_type() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Drv", "/d", "", StartupType::Driver, StartupImpact::None, "", "", 0);
        mgr.add_entry("Log", "/l", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let sorted = mgr.sorted_entries(SortColumn::Type, SortOrder::Ascending);
        assert_eq!(sorted[0].startup_type, StartupType::Login);
        assert_eq!(sorted[1].startup_type, StartupType::Driver);
    }

    #[test]
    fn test_sort_by_status() {
        let mut mgr = StartupManager::new();
        let id1 = mgr.add_entry("Dis", "/d", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("En", "/e", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.disable_entry(id1);
        let sorted = mgr.sorted_entries(SortColumn::Status, SortOrder::Ascending);
        assert!(!sorted[0].enabled);
        assert!(sorted[1].enabled);
    }

    // -- Search tests -------------------------------------------------------

    #[test]
    fn test_search_by_name() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Firefox", "/ff", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        mgr.add_entry("Chrome", "/ch", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        let results = mgr.search_entries("fire");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn test_search_by_publisher() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("App", "/app", "", StartupType::Login, StartupImpact::None, "MyCorp", "", 0);
        mgr.add_entry("Other", "/o", "", StartupType::Login, StartupImpact::None, "TheirCorp", "", 0);
        let results = mgr.search_entries("mycorp");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].publisher, "MyCorp");
    }

    #[test]
    fn test_search_by_path() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("A", "/usr/bin/foo", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("B", "/opt/bar", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let results = mgr.search_entries("/opt/");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "B");
    }

    #[test]
    fn test_search_empty_query_returns_all() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("B", "/b", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let results = mgr.search_entries("");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("MyApp", "/my", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let results = mgr.search_entries("MYAPP");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let results = mgr.search_entries("zzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_filtered_sorted() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Zebra App", "/z", "", StartupType::Login, StartupImpact::High, "", "", 0);
        mgr.add_entry("Alpha App", "/a", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        mgr.add_entry("Other Thing", "/o", "", StartupType::Login, StartupImpact::None, "", "", 0);
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
        mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        let id2 = mgr.add_entry("B", "/b", "", StartupType::Service, StartupImpact::High, "", "", 0);
        mgr.add_entry("C", "/c", "", StartupType::Scheduled, StartupImpact::Medium, "", "", 0);
        mgr.add_entry("D", "/d", "", StartupType::Driver, StartupImpact::None, "", "", 0);
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
        mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::Low, "", "", 0);
        mgr.add_entry("B", "/b", "", StartupType::Login, StartupImpact::High, "", "", 0);
        let s = mgr.stats();
        // Low=1, High=6, total=7
        assert_eq!(s.total_impact_weight, 7);
    }

    #[test]
    fn test_stats_disabled_excluded_from_impact() {
        let mut mgr = StartupManager::new();
        let id = mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::High, "", "", 0);
        mgr.disable_entry(id);
        let s = mgr.stats();
        assert_eq!(s.total_impact_weight, 0);
    }

    #[test]
    fn test_stats_impact_summary() {
        let s = StartupStats { total_impact_weight: 0, ..Default::default() };
        assert_eq!(s.impact_summary(), "Minimal");
        let s = StartupStats { total_impact_weight: 3, ..Default::default() };
        assert_eq!(s.impact_summary(), "Low");
        let s = StartupStats { total_impact_weight: 10, ..Default::default() };
        assert_eq!(s.impact_summary(), "Medium");
        let s = StartupStats { total_impact_weight: 20, ..Default::default() };
        assert_eq!(s.impact_summary(), "High");
        let s = StartupStats { total_impact_weight: 50, ..Default::default() };
        assert_eq!(s.impact_summary(), "Very High");
    }

    // -- Config serialization tests -----------------------------------------

    #[test]
    fn test_config_roundtrip() {
        let mut mgr = StartupManager::new();
        mgr.add_entry("Test App", "/bin/test", "--flag", StartupType::Login, StartupImpact::Medium, "TestCo", "A test", 1000);
        let id2 = mgr.add_entry("Service", "/sbin/svc", "", StartupType::Service, StartupImpact::High, "OurOS", "Core svc", 2000);
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
        mgr.add_entry("My|App", "/bin/test|me", "", StartupType::Login, StartupImpact::None, "A|B", "has|pipes", 0);
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
        mgr.add_entry("C:\\App", "C:\\Program Files\\app.exe", "", StartupType::Login, StartupImpact::None, "", "", 0);
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
        mgr.add_entry("A", "/a", "", StartupType::Login, StartupImpact::None, "", "", 0);
        mgr.add_entry("B", "/b", "", StartupType::Login, StartupImpact::None, "", "", 0);
        let text = StartupConfig::serialize(&mgr);
        let mut restored = StartupConfig::deserialize(&text).expect("should deserialize");
        // Adding a new entry should get id=3 (max existing is 2).
        let new_id = restored.add_entry("C", "/c", "", StartupType::Login, StartupImpact::None, "", "", 0);
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
            5, "Test", "/test", "--arg", StartupType::Service,
            StartupImpact::High, "Pub", "Desc", 100,
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
}
