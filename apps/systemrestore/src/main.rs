//! systemrestore -- OurOS System Restore / Snapshot Manager
//!
//! A graphical application for creating, managing, and restoring system
//! snapshots. Supports a tree-based snapshot model (branching like VirtualBox),
//! scheduled automatic snapshots with retention policies, snapshot comparison,
//! export/import, and storage management.
//!
//! # Architecture
//!
//! ```text
//! Snapshot            -- a single point-in-time system snapshot
//!     |
//!     v
//! SnapshotTree        -- parent-child tree with branching support
//!     |
//!     v
//! SnapshotManager     -- CRUD, scheduling, retention, compare, export/import
//!     |
//!     v
//! SystemRestoreUI     -- guitk-based GUI with tree view, timeline, details panel
//! ```

#![allow(dead_code)]

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand, RenderTree};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::BTreeMap;
use std::fmt;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const COLOR_BASE: Color = Color::from_hex(0x1E1E2E);
const COLOR_MANTLE: Color = Color::from_hex(0x181825);
const COLOR_SURFACE0: Color = Color::from_hex(0x313244);
const COLOR_SURFACE1: Color = Color::from_hex(0x45475A);
const COLOR_SURFACE2: Color = Color::from_hex(0x585B70);
const COLOR_TEXT: Color = Color::from_hex(0xCDD6F4);
const COLOR_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const COLOR_SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const COLOR_BLUE: Color = Color::from_hex(0x89B4FA);
const COLOR_GREEN: Color = Color::from_hex(0xA6E3A1);
const COLOR_RED: Color = Color::from_hex(0xF38BA8);
const COLOR_YELLOW: Color = Color::from_hex(0xF9E2AF);
const COLOR_PEACH: Color = Color::from_hex(0xFAB387);
const COLOR_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const COLOR_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout constants
// ============================================================================

const WINDOW_WIDTH: f32 = 1050.0;
const WINDOW_HEIGHT: f32 = 700.0;
const HEADER_HEIGHT: f32 = 48.0;
const TOOLBAR_HEIGHT: f32 = 40.0;
const SIDEBAR_WIDTH: f32 = 280.0;
const DETAILS_PANEL_HEIGHT: f32 = 160.0;
const STATUS_BAR_HEIGHT: f32 = 28.0;
const PADDING: f32 = 12.0;
const SMALL_PADDING: f32 = 6.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 11.0;
const FONT_SIZE_HEADING: f32 = 16.0;
const FONT_SIZE_TITLE: f32 = 20.0;
const BUTTON_WIDTH: f32 = 100.0;
const BUTTON_HEIGHT: f32 = 30.0;
const CORNER_RADIUS: f32 = 6.0;
const TREE_INDENT: f32 = 24.0;
const TREE_ROW_HEIGHT: f32 = 36.0;
const TIMELINE_ENTRY_HEIGHT: f32 = 48.0;
const TIMELINE_DOT_RADIUS: f32 = 6.0;
const CHECKBOX_SIZE: f32 = 16.0;
const PROGRESS_BAR_HEIGHT: f32 = 20.0;

// ============================================================================
// SnapshotType
// ============================================================================

/// How a snapshot was created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SnapshotType {
    /// Created manually by the user.
    Manual,
    /// Created automatically by a schedule.
    Automatic,
    /// Created before a system update.
    PreUpdate,
    /// Created before installing new software.
    PreInstall,
    /// Created by a scheduled policy.
    Scheduled,
}

impl SnapshotType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::Automatic => "Automatic",
            Self::PreUpdate => "Pre-Update",
            Self::PreInstall => "Pre-Install",
            Self::Scheduled => "Scheduled",
        }
    }

    /// Parse from a string label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "manual" => Some(Self::Manual),
            "automatic" => Some(Self::Automatic),
            "pre-update" | "preupdate" => Some(Self::PreUpdate),
            "pre-install" | "preinstall" => Some(Self::PreInstall),
            "scheduled" => Some(Self::Scheduled),
            _ => None,
        }
    }

    /// All snapshot type variants.
    pub fn all() -> &'static [Self] {
        &[
            Self::Manual,
            Self::Automatic,
            Self::PreUpdate,
            Self::PreInstall,
            Self::Scheduled,
        ]
    }

    /// Icon indicator color for each type.
    pub fn indicator_color(self) -> Color {
        match self {
            Self::Manual => COLOR_BLUE,
            Self::Automatic => COLOR_GREEN,
            Self::PreUpdate => COLOR_YELLOW,
            Self::PreInstall => COLOR_PEACH,
            Self::Scheduled => COLOR_LAVENDER,
        }
    }
}

impl fmt::Display for SnapshotType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ============================================================================
// SnapshotComponent
// ============================================================================

/// A component that can be included in a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SnapshotComponent {
    /// Core OS files and libraries.
    SystemFiles,
    /// User preferences and settings.
    UserSettings,
    /// Installed applications and their data.
    InstalledApps,
    /// Boot configuration and bootloader.
    BootConfig,
    /// Network configuration (adapters, firewall rules, DNS).
    NetworkConfig,
    /// System services and daemons configuration.
    ServiceConfig,
    /// Device driver state.
    DriverState,
    /// Package manager state and metadata.
    PackageState,
    /// Desktop environment settings (themes, layouts).
    DesktopConfig,
    /// Security policies and capability tables.
    SecurityPolicy,
}

impl SnapshotComponent {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemFiles => "System Files",
            Self::UserSettings => "User Settings",
            Self::InstalledApps => "Installed Apps",
            Self::BootConfig => "Boot Config",
            Self::NetworkConfig => "Network Config",
            Self::ServiceConfig => "Service Config",
            Self::DriverState => "Driver State",
            Self::PackageState => "Package State",
            Self::DesktopConfig => "Desktop Config",
            Self::SecurityPolicy => "Security Policy",
        }
    }

    /// Parse from a label (case-insensitive, supports both forms).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace(' ', "").as_str() {
            "systemfiles" => Some(Self::SystemFiles),
            "usersettings" => Some(Self::UserSettings),
            "installedapps" => Some(Self::InstalledApps),
            "bootconfig" => Some(Self::BootConfig),
            "networkconfig" => Some(Self::NetworkConfig),
            "serviceconfig" => Some(Self::ServiceConfig),
            "driverstate" => Some(Self::DriverState),
            "packagestate" => Some(Self::PackageState),
            "desktopconfig" => Some(Self::DesktopConfig),
            "securitypolicy" => Some(Self::SecurityPolicy),
            _ => None,
        }
    }

    /// Estimated size in bytes for this component.
    pub fn estimated_size_bytes(self) -> u64 {
        match self {
            Self::SystemFiles => 2_000_000_000,   // ~2 GB
            Self::UserSettings => 50_000_000,     // ~50 MB
            Self::InstalledApps => 5_000_000_000, // ~5 GB
            Self::BootConfig => 5_000_000,        // ~5 MB
            Self::NetworkConfig => 2_000_000,     // ~2 MB
            Self::ServiceConfig => 10_000_000,    // ~10 MB
            Self::DriverState => 100_000_000,     // ~100 MB
            Self::PackageState => 200_000_000,    // ~200 MB
            Self::DesktopConfig => 30_000_000,    // ~30 MB
            Self::SecurityPolicy => 1_000_000,    // ~1 MB
        }
    }

    /// All component variants.
    pub fn all() -> &'static [Self] {
        &[
            Self::SystemFiles,
            Self::UserSettings,
            Self::InstalledApps,
            Self::BootConfig,
            Self::NetworkConfig,
            Self::ServiceConfig,
            Self::DriverState,
            Self::PackageState,
            Self::DesktopConfig,
            Self::SecurityPolicy,
        ]
    }

    /// The default set of components for a full snapshot.
    pub fn default_set() -> Vec<Self> {
        vec![
            Self::SystemFiles,
            Self::UserSettings,
            Self::InstalledApps,
            Self::BootConfig,
            Self::NetworkConfig,
            Self::ServiceConfig,
        ]
    }
}

impl fmt::Display for SnapshotComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ============================================================================
// Snapshot
// ============================================================================

/// A single point-in-time system snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Unique identifier.
    pub id: u64,
    /// User-given name.
    pub name: String,
    /// Optional description.
    pub description: String,
    /// Creation timestamp (seconds since epoch).
    pub timestamp: u64,
    /// How the snapshot was created.
    pub snapshot_type: SnapshotType,
    /// Estimated total size in bytes.
    pub size_bytes: u64,
    /// Components included in this snapshot.
    pub components: Vec<SnapshotComponent>,
    /// Parent snapshot ID (None for root snapshots).
    pub parent_id: Option<u64>,
    /// Whether this snapshot is locked (cannot be deleted by retention policy).
    pub locked: bool,
    /// Optional tags for organization.
    pub tags: Vec<String>,
}

impl Snapshot {
    /// Create a new snapshot.
    pub fn new(
        id: u64,
        name: &str,
        description: &str,
        timestamp: u64,
        snapshot_type: SnapshotType,
        components: Vec<SnapshotComponent>,
        parent_id: Option<u64>,
    ) -> Self {
        let size_bytes = components.iter().map(|c| c.estimated_size_bytes()).sum();
        Self {
            id,
            name: name.to_string(),
            description: description.to_string(),
            timestamp,
            snapshot_type,
            size_bytes,
            components,
            parent_id,
            locked: false,
            tags: Vec::new(),
        }
    }

    /// Human-readable size string.
    pub fn size_display(&self) -> String {
        format_bytes(self.size_bytes)
    }

    /// Human-readable age string relative to a reference timestamp.
    pub fn age_display(&self, now: u64) -> String {
        if now <= self.timestamp {
            return "just now".to_string();
        }
        let elapsed = now.saturating_sub(self.timestamp);
        format_duration_short(elapsed)
    }

    /// Number of included components.
    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// Whether this snapshot includes a specific component.
    pub fn has_component(&self, component: SnapshotComponent) -> bool {
        self.components.contains(&component)
    }
}

// ============================================================================
// SnapshotTree
// ============================================================================

/// A tree of snapshots with parent-child relationships and branching.
///
/// Snapshots form a directed tree (each snapshot has at most one parent,
/// but can have multiple children -- branches). The root(s) are snapshots
/// with no parent.
pub struct SnapshotTree {
    snapshots: BTreeMap<u64, Snapshot>,
    /// Maps parent_id -> list of child IDs (sorted by timestamp).
    children: BTreeMap<u64, Vec<u64>>,
    next_id: u64,
}

impl SnapshotTree {
    /// Create a new empty snapshot tree.
    pub fn new() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            children: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Add a snapshot to the tree. Returns the assigned ID.
    pub fn add_snapshot(
        &mut self,
        name: &str,
        description: &str,
        timestamp: u64,
        snapshot_type: SnapshotType,
        components: Vec<SnapshotComponent>,
        parent_id: Option<u64>,
    ) -> Result<u64, SnapshotError> {
        // Validate parent exists if specified.
        if let Some(pid) = parent_id
            && !self.snapshots.contains_key(&pid)
        {
            return Err(SnapshotError::ParentNotFound(pid));
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let snapshot = Snapshot::new(
            id,
            name,
            description,
            timestamp,
            snapshot_type,
            components,
            parent_id,
        );
        self.snapshots.insert(id, snapshot);

        if let Some(pid) = parent_id {
            self.children.entry(pid).or_default().push(id);
        }

        Ok(id)
    }

    /// Remove a snapshot by ID. Fails if it has children (must delete leaf first).
    pub fn remove_snapshot(&mut self, id: u64) -> Result<Snapshot, SnapshotError> {
        // Check the snapshot exists.
        let snapshot = self.snapshots.get(&id).ok_or(SnapshotError::NotFound(id))?;

        // Cannot remove if locked.
        if snapshot.locked {
            return Err(SnapshotError::Locked(id));
        }

        // Cannot remove if it has children.
        if let Some(kids) = self.children.get(&id)
            && !kids.is_empty()
        {
            return Err(SnapshotError::HasChildren(id));
        }

        // Remove from parent's child list.
        if let Some(pid) = snapshot.parent_id
            && let Some(siblings) = self.children.get_mut(&pid)
        {
            siblings.retain(|&cid| cid != id);
        }

        self.children.remove(&id);
        // The snapshot is guaranteed to exist since we checked above.
        Ok(self
            .snapshots
            .remove(&id)
            .expect("snapshot was verified to exist"))
    }

    /// Get a snapshot by ID.
    pub fn get_snapshot(&self, id: u64) -> Option<&Snapshot> {
        self.snapshots.get(&id)
    }

    /// Get a mutable snapshot by ID.
    pub fn get_snapshot_mut(&mut self, id: u64) -> Option<&mut Snapshot> {
        self.snapshots.get_mut(&id)
    }

    /// Get children IDs of a snapshot.
    pub fn children_of(&self, id: u64) -> &[u64] {
        self.children.get(&id).map_or(&[], |v| v.as_slice())
    }

    /// Get IDs of root snapshots (those with no parent).
    pub fn root_ids(&self) -> Vec<u64> {
        self.snapshots
            .values()
            .filter(|s| s.parent_id.is_none())
            .map(|s| s.id)
            .collect()
    }

    /// Get all snapshot IDs sorted by timestamp.
    pub fn all_ids_by_timestamp(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .snapshots
            .values()
            .map(|s| (s.timestamp, s.id))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }

    /// Total number of snapshots.
    pub fn count(&self) -> usize {
        self.snapshots.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Total size of all snapshots in bytes.
    pub fn total_size_bytes(&self) -> u64 {
        self.snapshots.values().map(|s| s.size_bytes).sum()
    }

    /// Get the depth of a snapshot in the tree (root = 0).
    pub fn depth_of(&self, id: u64) -> usize {
        let mut depth = 0;
        let mut current = id;
        while let Some(snap) = self.snapshots.get(&current) {
            if let Some(pid) = snap.parent_id {
                depth += 1;
                current = pid;
            } else {
                break;
            }
        }
        depth
    }

    /// Get the full ancestry chain from root to the given snapshot (inclusive).
    pub fn ancestry_chain(&self, id: u64) -> Vec<u64> {
        let mut chain = Vec::new();
        let mut current = id;
        loop {
            chain.push(current);
            match self.snapshots.get(&current).and_then(|s| s.parent_id) {
                Some(pid) => current = pid,
                None => break,
            }
        }
        chain.reverse();
        chain
    }

    /// Flatten the tree into a list suitable for rendering, with depth info.
    /// Each entry is (id, depth). Uses depth-first traversal.
    pub fn flatten_for_display(&self) -> Vec<(u64, usize)> {
        let mut result = Vec::new();
        let roots = self.root_ids();
        for root_id in roots {
            self.flatten_subtree(root_id, 0, &mut result);
        }
        result
    }

    fn flatten_subtree(&self, id: u64, depth: usize, result: &mut Vec<(u64, usize)>) {
        result.push((id, depth));
        if let Some(kids) = self.children.get(&id) {
            for &kid_id in kids {
                self.flatten_subtree(kid_id, depth + 1, result);
            }
        }
    }

    /// Lock a snapshot (prevent deletion by retention policies).
    pub fn lock_snapshot(&mut self, id: u64) -> Result<(), SnapshotError> {
        let snap = self
            .snapshots
            .get_mut(&id)
            .ok_or(SnapshotError::NotFound(id))?;
        snap.locked = true;
        Ok(())
    }

    /// Unlock a snapshot.
    pub fn unlock_snapshot(&mut self, id: u64) -> Result<(), SnapshotError> {
        let snap = self
            .snapshots
            .get_mut(&id)
            .ok_or(SnapshotError::NotFound(id))?;
        snap.locked = false;
        Ok(())
    }

    /// Add a tag to a snapshot.
    pub fn add_tag(&mut self, id: u64, tag: &str) -> Result<(), SnapshotError> {
        let snap = self
            .snapshots
            .get_mut(&id)
            .ok_or(SnapshotError::NotFound(id))?;
        let tag_str = tag.to_string();
        if !snap.tags.contains(&tag_str) {
            snap.tags.push(tag_str);
        }
        Ok(())
    }

    /// Remove a tag from a snapshot.
    pub fn remove_tag(&mut self, id: u64, tag: &str) -> Result<(), SnapshotError> {
        let snap = self
            .snapshots
            .get_mut(&id)
            .ok_or(SnapshotError::NotFound(id))?;
        snap.tags.retain(|t| t != tag);
        Ok(())
    }

    /// Find snapshots matching a search query (name or description, case-insensitive).
    pub fn search(&self, query: &str) -> Vec<u64> {
        let q = query.to_ascii_lowercase();
        self.snapshots
            .values()
            .filter(|s| {
                s.name.to_ascii_lowercase().contains(&q)
                    || s.description.to_ascii_lowercase().contains(&q)
            })
            .map(|s| s.id)
            .collect()
    }

    /// Filter snapshots by type.
    pub fn filter_by_type(&self, snap_type: SnapshotType) -> Vec<u64> {
        self.snapshots
            .values()
            .filter(|s| s.snapshot_type == snap_type)
            .map(|s| s.id)
            .collect()
    }

    /// Filter snapshots that include a specific component.
    pub fn filter_by_component(&self, component: SnapshotComponent) -> Vec<u64> {
        self.snapshots
            .values()
            .filter(|s| s.has_component(component))
            .map(|s| s.id)
            .collect()
    }
}

impl Default for SnapshotTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SnapshotError
// ============================================================================

/// Errors that can occur during snapshot operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// Snapshot with this ID was not found.
    NotFound(u64),
    /// Parent snapshot with this ID was not found.
    ParentNotFound(u64),
    /// Cannot delete snapshot that has children.
    HasChildren(u64),
    /// Snapshot is locked and cannot be deleted.
    Locked(u64),
    /// Invalid schedule configuration.
    InvalidSchedule(String),
    /// Export/import format error.
    FormatError(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Snapshot {} not found", id),
            Self::ParentNotFound(id) => write!(f, "Parent snapshot {} not found", id),
            Self::HasChildren(id) => {
                write!(f, "Snapshot {} has children and cannot be deleted", id)
            }
            Self::Locked(id) => write!(f, "Snapshot {} is locked", id),
            Self::InvalidSchedule(msg) => write!(f, "Invalid schedule: {}", msg),
            Self::FormatError(msg) => write!(f, "Format error: {}", msg),
        }
    }
}

// ============================================================================
// SnapshotDiff — compare two snapshots
// ============================================================================

/// A single difference between two snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffEntry {
    /// Component was added (present in newer, absent in older).
    ComponentAdded(SnapshotComponent),
    /// Component was removed (present in older, absent in newer).
    ComponentRemoved(SnapshotComponent),
    /// A file was added.
    FileAdded(String),
    /// A file was modified.
    FileModified(String),
    /// A file was removed.
    FileRemoved(String),
    /// A setting was changed.
    SettingChanged {
        key: String,
        old_value: String,
        new_value: String,
    },
    /// A package was installed.
    PackageInstalled(String),
    /// A package was removed.
    PackageUninstalled(String),
    /// A package version changed.
    PackageUpdated {
        name: String,
        old_version: String,
        new_version: String,
    },
}

impl DiffEntry {
    /// Category label for grouping diffs.
    pub fn category(&self) -> &'static str {
        match self {
            Self::ComponentAdded(_) | Self::ComponentRemoved(_) => "Components",
            Self::FileAdded(_) | Self::FileModified(_) | Self::FileRemoved(_) => "Files",
            Self::SettingChanged { .. } => "Settings",
            Self::PackageInstalled(_)
            | Self::PackageUninstalled(_)
            | Self::PackageUpdated { .. } => "Packages",
        }
    }

    /// Short summary for display.
    pub fn summary(&self) -> String {
        match self {
            Self::ComponentAdded(c) => format!("+ Component: {}", c.label()),
            Self::ComponentRemoved(c) => format!("- Component: {}", c.label()),
            Self::FileAdded(path) => format!("+ File: {}", path),
            Self::FileModified(path) => format!("~ File: {}", path),
            Self::FileRemoved(path) => format!("- File: {}", path),
            Self::SettingChanged {
                key,
                old_value,
                new_value,
            } => {
                format!("~ Setting: {} ({} -> {})", key, old_value, new_value)
            }
            Self::PackageInstalled(name) => format!("+ Package: {}", name),
            Self::PackageUninstalled(name) => format!("- Package: {}", name),
            Self::PackageUpdated {
                name,
                old_version,
                new_version,
            } => {
                format!("~ Package: {} ({} -> {})", name, old_version, new_version)
            }
        }
    }

    /// Whether this diff entry represents an addition.
    pub fn is_addition(&self) -> bool {
        matches!(
            self,
            Self::ComponentAdded(_) | Self::FileAdded(_) | Self::PackageInstalled(_)
        )
    }

    /// Whether this diff entry represents a removal.
    pub fn is_removal(&self) -> bool {
        matches!(
            self,
            Self::ComponentRemoved(_) | Self::FileRemoved(_) | Self::PackageUninstalled(_)
        )
    }

    /// Whether this diff entry represents a modification.
    pub fn is_modification(&self) -> bool {
        matches!(
            self,
            Self::FileModified(_) | Self::SettingChanged { .. } | Self::PackageUpdated { .. }
        )
    }
}

/// Result of comparing two snapshots.
#[derive(Clone, Debug)]
pub struct SnapshotDiffResult {
    /// ID of the older (base) snapshot.
    pub older_id: u64,
    /// ID of the newer (target) snapshot.
    pub newer_id: u64,
    /// List of differences.
    pub entries: Vec<DiffEntry>,
}

impl SnapshotDiffResult {
    /// Number of additions.
    pub fn addition_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_addition()).count()
    }

    /// Number of removals.
    pub fn removal_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_removal()).count()
    }

    /// Number of modifications.
    pub fn modification_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_modification()).count()
    }

    /// Total number of changes.
    pub fn total_changes(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no differences.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get entries filtered by category.
    pub fn by_category(&self, category: &str) -> Vec<&DiffEntry> {
        self.entries
            .iter()
            .filter(|e| e.category() == category)
            .collect()
    }
}

// ============================================================================
// ScheduleFrequency / RetentionPolicy / ScheduleConfig
// ============================================================================

/// How often automatic snapshots are created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleFrequency {
    Daily,
    Weekly,
    Monthly,
}

impl ScheduleFrequency {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }

    /// Parse from label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }

    /// Interval in seconds between snapshots.
    pub fn interval_secs(self) -> u64 {
        match self {
            Self::Daily => 86_400,
            Self::Weekly => 604_800,
            Self::Monthly => 2_592_000, // 30 days
        }
    }

    /// All frequency variants.
    pub fn all() -> &'static [Self] {
        &[Self::Daily, Self::Weekly, Self::Monthly]
    }
}

impl fmt::Display for ScheduleFrequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Retention policy for automatic cleanup of old snapshots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Maximum number of snapshots to keep (0 = unlimited).
    pub max_count: usize,
    /// Maximum age of snapshots in seconds (0 = unlimited).
    pub max_age_secs: u64,
    /// Maximum total storage in bytes (0 = unlimited).
    pub max_total_bytes: u64,
}

impl RetentionPolicy {
    /// Create a new retention policy.
    pub fn new(max_count: usize, max_age_secs: u64, max_total_bytes: u64) -> Self {
        Self {
            max_count,
            max_age_secs,
            max_total_bytes,
        }
    }

    /// No limits.
    pub fn unlimited() -> Self {
        Self {
            max_count: 0,
            max_age_secs: 0,
            max_total_bytes: 0,
        }
    }

    /// Whether this policy has a count limit.
    pub fn has_count_limit(&self) -> bool {
        self.max_count > 0
    }

    /// Whether this policy has an age limit.
    pub fn has_age_limit(&self) -> bool {
        self.max_age_secs > 0
    }

    /// Whether this policy has a size limit.
    pub fn has_size_limit(&self) -> bool {
        self.max_total_bytes > 0
    }

    /// Determine which snapshots should be deleted to satisfy this policy.
    /// Takes snapshots sorted oldest-first. Returns IDs to delete.
    /// Locked snapshots are never returned for deletion.
    pub fn snapshots_to_prune(
        &self,
        snapshots: &[(u64, u64, u64, bool)], // (id, timestamp, size_bytes, locked)
        now: u64,
    ) -> Vec<u64> {
        let mut to_delete = Vec::new();

        // Age-based pruning: delete snapshots older than max_age_secs.
        if self.has_age_limit() {
            for &(id, ts, _, locked) in snapshots {
                if !locked && now.saturating_sub(ts) > self.max_age_secs {
                    to_delete.push(id);
                }
            }
        }

        // Count-based pruning: keep only max_count newest snapshots.
        if self.has_count_limit() {
            let non_deleted: Vec<_> = snapshots
                .iter()
                .filter(|(id, _, _, locked)| !locked && !to_delete.contains(id))
                .collect();
            if non_deleted.len() > self.max_count {
                let excess = non_deleted.len() - self.max_count;
                // Delete the oldest excess snapshots.
                for &(id, _, _, _) in non_deleted.iter().take(excess) {
                    if !to_delete.contains(id) {
                        to_delete.push(*id);
                    }
                }
            }
        }

        // Size-based pruning: delete oldest until under max_total_bytes.
        if self.has_size_limit() {
            let mut total: u64 = snapshots
                .iter()
                .filter(|(id, _, _, _)| !to_delete.contains(id))
                .map(|(_, _, sz, _)| sz)
                .sum();
            // Delete oldest first until we're under limit.
            for &(id, _, sz, locked) in snapshots {
                if total <= self.max_total_bytes {
                    break;
                }
                if !locked && !to_delete.contains(&id) {
                    to_delete.push(id);
                    total = total.saturating_sub(sz);
                }
            }
        }

        to_delete
    }

    /// Human-readable summary of retention settings.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.has_count_limit() {
            parts.push(format!("keep {} snapshots", self.max_count));
        }
        if self.has_age_limit() {
            parts.push(format!(
                "max age {}",
                format_duration_short(self.max_age_secs)
            ));
        }
        if self.has_size_limit() {
            parts.push(format!("max size {}", format_bytes(self.max_total_bytes)));
        }
        if parts.is_empty() {
            "No limits".to_string()
        } else {
            parts.join(", ")
        }
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Full schedule configuration for automatic snapshots.
#[derive(Clone, Debug)]
pub struct ScheduleConfig {
    /// Whether scheduling is enabled.
    pub enabled: bool,
    /// How often to create snapshots.
    pub frequency: ScheduleFrequency,
    /// Components to include in scheduled snapshots.
    pub components: Vec<SnapshotComponent>,
    /// Retention policy for automatic cleanup.
    pub retention: RetentionPolicy,
    /// Timestamp of last scheduled snapshot.
    pub last_snapshot_timestamp: u64,
}

impl ScheduleConfig {
    /// Create a new schedule config.
    pub fn new(frequency: ScheduleFrequency, components: Vec<SnapshotComponent>) -> Self {
        Self {
            enabled: true,
            frequency,
            components,
            retention: RetentionPolicy::default(),
            last_snapshot_timestamp: 0,
        }
    }

    /// Whether a new snapshot is due given the current time.
    pub fn is_due(&self, now: u64) -> bool {
        if !self.enabled {
            return false;
        }
        now.saturating_sub(self.last_snapshot_timestamp) >= self.frequency.interval_secs()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.components.is_empty() {
            return Err(SnapshotError::InvalidSchedule(
                "At least one component must be selected".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            frequency: ScheduleFrequency::Weekly,
            components: SnapshotComponent::default_set(),
            retention: RetentionPolicy::new(10, 30 * 86_400, 50_000_000_000),
            last_snapshot_timestamp: 0,
        }
    }
}

// ============================================================================
// StorageStats
// ============================================================================

/// Aggregate storage statistics.
#[derive(Clone, Debug, Default)]
pub struct StorageStats {
    /// Total storage used by all snapshots.
    pub total_bytes: u64,
    /// Number of snapshots.
    pub snapshot_count: usize,
    /// Average size per snapshot.
    pub avg_bytes_per_snapshot: u64,
    /// Largest snapshot size.
    pub largest_snapshot_bytes: u64,
    /// Smallest snapshot size (0 if no snapshots).
    pub smallest_snapshot_bytes: u64,
    /// Size of manual snapshots.
    pub manual_bytes: u64,
    /// Size of automatic/scheduled snapshots.
    pub auto_bytes: u64,
}

impl StorageStats {
    /// Compute storage stats from a snapshot tree.
    pub fn from_tree(tree: &SnapshotTree) -> Self {
        if tree.is_empty() {
            return Self::default();
        }

        let mut total: u64 = 0;
        let mut largest: u64 = 0;
        let mut smallest: u64 = u64::MAX;
        let mut manual: u64 = 0;
        let mut auto: u64 = 0;

        for id in tree.all_ids_by_timestamp() {
            if let Some(snap) = tree.get_snapshot(id) {
                total = total.saturating_add(snap.size_bytes);
                if snap.size_bytes > largest {
                    largest = snap.size_bytes;
                }
                if snap.size_bytes < smallest {
                    smallest = snap.size_bytes;
                }
                match snap.snapshot_type {
                    SnapshotType::Manual => {
                        manual = manual.saturating_add(snap.size_bytes);
                    }
                    _ => {
                        auto = auto.saturating_add(snap.size_bytes);
                    }
                }
            }
        }

        let count = tree.count();
        Self {
            total_bytes: total,
            snapshot_count: count,
            avg_bytes_per_snapshot: if count > 0 { total / count as u64 } else { 0 },
            largest_snapshot_bytes: largest,
            smallest_snapshot_bytes: if smallest == u64::MAX { 0 } else { smallest },
            manual_bytes: manual,
            auto_bytes: auto,
        }
    }

    /// Human-readable total size.
    pub fn total_display(&self) -> String {
        format_bytes(self.total_bytes)
    }

    /// Human-readable average size.
    pub fn avg_display(&self) -> String {
        format_bytes(self.avg_bytes_per_snapshot)
    }
}

// ============================================================================
// SnapshotExport / SnapshotImport
// ============================================================================

/// Exported snapshot metadata in a simple text format.
///
/// Format:
/// ```text
/// [snapshot]
/// id=<id>
/// name=<name>
/// description=<description>
/// timestamp=<timestamp>
/// type=<type>
/// size=<size_bytes>
/// parent=<parent_id or "none">
/// locked=<true|false>
/// components=<comp1,comp2,...>
/// tags=<tag1,tag2,...>
/// ```
pub struct SnapshotExport;

impl SnapshotExport {
    /// Export a single snapshot to text format.
    pub fn export_one(snap: &Snapshot) -> String {
        let mut lines = Vec::new();
        lines.push("[snapshot]".to_string());
        lines.push(format!("id={}", snap.id));
        lines.push(format!("name={}", snap.name));
        lines.push(format!("description={}", snap.description));
        lines.push(format!("timestamp={}", snap.timestamp));
        lines.push(format!("type={}", snap.snapshot_type.label()));
        lines.push(format!("size={}", snap.size_bytes));
        lines.push(format!(
            "parent={}",
            snap.parent_id
                .map_or_else(|| "none".to_string(), |id| id.to_string())
        ));
        lines.push(format!("locked={}", snap.locked));
        let comp_str: Vec<&str> = snap.components.iter().map(|c| c.label()).collect();
        lines.push(format!("components={}", comp_str.join(",")));
        let tag_str = snap.tags.join(",");
        lines.push(format!("tags={}", tag_str));
        lines.join("\n")
    }

    /// Export all snapshots from a tree to text format.
    pub fn export_all(tree: &SnapshotTree) -> String {
        let ids = tree.all_ids_by_timestamp();
        let mut sections = Vec::new();
        for id in ids {
            if let Some(snap) = tree.get_snapshot(id) {
                sections.push(Self::export_one(snap));
            }
        }
        sections.join("\n\n")
    }

    /// Parse one snapshot from key-value lines. Returns (Snapshot, original_id).
    pub fn parse_one(lines: &[&str]) -> Result<(Snapshot, u64), SnapshotError> {
        let mut id: u64 = 0;
        let mut name = String::new();
        let mut description = String::new();
        let mut timestamp: u64 = 0;
        let mut snap_type = SnapshotType::Manual;
        let mut size_bytes: u64 = 0;
        let mut parent_id: Option<u64> = None;
        let mut locked = false;
        let mut components = Vec::new();
        let mut tags = Vec::new();

        for line in lines {
            let line = line.trim();
            if line.is_empty() || line == "[snapshot]" {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "id" => {
                        id = value.trim().parse::<u64>().map_err(|e| {
                            SnapshotError::FormatError(format!("invalid id: {}", e))
                        })?;
                    }
                    "name" => name = value.trim().to_string(),
                    "description" => description = value.trim().to_string(),
                    "timestamp" => {
                        timestamp = value.trim().parse::<u64>().map_err(|e| {
                            SnapshotError::FormatError(format!("invalid timestamp: {}", e))
                        })?;
                    }
                    "type" => {
                        snap_type = SnapshotType::from_label(value.trim()).ok_or_else(|| {
                            SnapshotError::FormatError(format!("unknown type: {}", value.trim()))
                        })?;
                    }
                    "size" => {
                        size_bytes = value.trim().parse::<u64>().map_err(|e| {
                            SnapshotError::FormatError(format!("invalid size: {}", e))
                        })?;
                    }
                    "parent" => {
                        let v = value.trim();
                        parent_id = if v == "none" {
                            None
                        } else {
                            Some(v.parse::<u64>().map_err(|e| {
                                SnapshotError::FormatError(format!("invalid parent: {}", e))
                            })?)
                        };
                    }
                    "locked" => locked = value.trim() == "true",
                    "components" => {
                        for c_str in value.split(',') {
                            let c_str = c_str.trim();
                            if !c_str.is_empty()
                                && let Some(c) = SnapshotComponent::from_label(c_str)
                            {
                                components.push(c);
                            }
                        }
                    }
                    "tags" => {
                        for t in value.split(',') {
                            let t = t.trim();
                            if !t.is_empty() {
                                tags.push(t.to_string());
                            }
                        }
                    }
                    _ => {} // Ignore unknown keys for forward compatibility.
                }
            }
        }

        let mut snap = Snapshot::new(
            id,
            &name,
            &description,
            timestamp,
            snap_type,
            components,
            parent_id,
        );
        snap.size_bytes = size_bytes;
        snap.locked = locked;
        snap.tags = tags;
        Ok((snap, id))
    }

    /// Import snapshots from text. Returns a list of parsed snapshots.
    pub fn import_all(text: &str) -> Result<Vec<Snapshot>, SnapshotError> {
        let mut snapshots = Vec::new();
        let mut current_lines: Vec<&str> = Vec::new();
        let mut in_section = false;

        for line in text.lines() {
            if line.trim() == "[snapshot]" {
                if in_section && !current_lines.is_empty() {
                    let (snap, _) = Self::parse_one(&current_lines)?;
                    snapshots.push(snap);
                    current_lines.clear();
                }
                in_section = true;
                current_lines.push(line);
            } else if in_section {
                current_lines.push(line);
            }
        }

        // Handle last section.
        if in_section && !current_lines.is_empty() {
            let (snap, _) = Self::parse_one(&current_lines)?;
            snapshots.push(snap);
        }

        Ok(snapshots)
    }
}

// ============================================================================
// SnapshotManager — high-level management
// ============================================================================

/// High-level manager combining the tree, scheduling, comparison, and storage.
pub struct SnapshotManager {
    /// The snapshot tree.
    pub tree: SnapshotTree,
    /// Schedule configuration.
    pub schedule: ScheduleConfig,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new() -> Self {
        Self {
            tree: SnapshotTree::new(),
            schedule: ScheduleConfig::default(),
        }
    }

    /// Create a new snapshot.
    pub fn create_snapshot(
        &mut self,
        name: &str,
        description: &str,
        timestamp: u64,
        snapshot_type: SnapshotType,
        components: Vec<SnapshotComponent>,
        parent_id: Option<u64>,
    ) -> Result<u64, SnapshotError> {
        self.tree.add_snapshot(
            name,
            description,
            timestamp,
            snapshot_type,
            components,
            parent_id,
        )
    }

    /// Delete a snapshot.
    pub fn delete_snapshot(&mut self, id: u64) -> Result<Snapshot, SnapshotError> {
        self.tree.remove_snapshot(id)
    }

    /// Compare two snapshots by their component sets.
    /// Generates diff entries based on component differences.
    pub fn compare_snapshots(
        &self,
        older_id: u64,
        newer_id: u64,
    ) -> Result<SnapshotDiffResult, SnapshotError> {
        let older = self
            .tree
            .get_snapshot(older_id)
            .ok_or(SnapshotError::NotFound(older_id))?;
        let newer = self
            .tree
            .get_snapshot(newer_id)
            .ok_or(SnapshotError::NotFound(newer_id))?;

        let mut entries = Vec::new();

        // Compare component sets.
        for &comp in &newer.components {
            if !older.has_component(comp) {
                entries.push(DiffEntry::ComponentAdded(comp));
            }
        }
        for &comp in &older.components {
            if !newer.has_component(comp) {
                entries.push(DiffEntry::ComponentRemoved(comp));
            }
        }

        // Simulate file diffs based on component differences and time gap.
        let time_gap = newer.timestamp.saturating_sub(older.timestamp);
        if time_gap > 86_400 {
            // More than a day apart: simulate some file changes.
            let file_change_count = (time_gap / 86_400).min(20) as usize;
            for i in 0..file_change_count {
                match i % 3 {
                    0 => entries.push(DiffEntry::FileModified(format!(
                        "/system/lib/module_{}.so",
                        i
                    ))),
                    1 => entries.push(DiffEntry::FileAdded(format!("/system/etc/conf_{}.yaml", i))),
                    _ => entries.push(DiffEntry::FileRemoved(format!("/tmp/cache_{}.dat", i))),
                }
            }
        }

        // Simulate package diffs.
        if newer.has_component(SnapshotComponent::InstalledApps)
            && older.has_component(SnapshotComponent::InstalledApps)
            && time_gap > 604_800
        {
            entries.push(DiffEntry::PackageUpdated {
                name: "core-libs".to_string(),
                old_version: "1.2.0".to_string(),
                new_version: "1.3.0".to_string(),
            });
            entries.push(DiffEntry::PackageInstalled("new-tool".to_string()));
        }

        // Simulate setting changes.
        if newer.has_component(SnapshotComponent::UserSettings)
            && older.has_component(SnapshotComponent::UserSettings)
            && time_gap > 172_800
        {
            entries.push(DiffEntry::SettingChanged {
                key: "display.theme".to_string(),
                old_value: "dark".to_string(),
                new_value: "mocha".to_string(),
            });
        }

        Ok(SnapshotDiffResult {
            older_id,
            newer_id,
            entries,
        })
    }

    /// Check if a scheduled snapshot is due and create one if so.
    pub fn check_schedule(&mut self, now: u64) -> Result<Option<u64>, SnapshotError> {
        if !self.schedule.is_due(now) {
            return Ok(None);
        }
        self.schedule.validate()?;

        let name = format!("Scheduled-{}", now);
        let components = self.schedule.components.clone();
        let id = self.tree.add_snapshot(
            &name,
            "Automatically created by schedule",
            now,
            SnapshotType::Scheduled,
            components,
            None,
        )?;
        self.schedule.last_snapshot_timestamp = now;
        Ok(Some(id))
    }

    /// Run retention policy and return IDs of snapshots that were pruned.
    pub fn apply_retention(&mut self, now: u64) -> Vec<u64> {
        let snapshot_info: Vec<(u64, u64, u64, bool)> = self
            .tree
            .all_ids_by_timestamp()
            .iter()
            .filter_map(|&id| {
                self.tree
                    .get_snapshot(id)
                    .map(|s| (s.id, s.timestamp, s.size_bytes, s.locked))
            })
            .collect();

        let to_prune = self
            .schedule
            .retention
            .snapshots_to_prune(&snapshot_info, now);

        let mut pruned = Vec::new();
        for id in to_prune {
            // Only prune leaf snapshots (no children). Skip non-leaf silently.
            if self.tree.children_of(id).is_empty() && self.tree.remove_snapshot(id).is_ok() {
                pruned.push(id);
            }
        }
        pruned
    }

    /// Get storage statistics.
    pub fn storage_stats(&self) -> StorageStats {
        StorageStats::from_tree(&self.tree)
    }

    /// Export all snapshots.
    pub fn export_all(&self) -> String {
        SnapshotExport::export_all(&self.tree)
    }

    /// Import snapshots from text (adds them to the tree with new IDs).
    pub fn import_snapshots(
        &mut self,
        text: &str,
        base_timestamp: u64,
    ) -> Result<Vec<u64>, SnapshotError> {
        let imported = SnapshotExport::import_all(text)?;
        let mut new_ids = Vec::new();

        for snap in imported {
            // Assign new IDs when importing; parent relationships are not preserved.
            let id = self.tree.add_snapshot(
                &snap.name,
                &snap.description,
                snap.timestamp.max(base_timestamp),
                snap.snapshot_type,
                snap.components,
                None, // Parent relationships from export are not guaranteed to exist.
            )?;
            if snap.locked {
                let _ = self.tree.lock_snapshot(id);
            }
            for tag in &snap.tags {
                let _ = self.tree.add_tag(id, tag);
            }
            new_ids.push(id);
        }

        Ok(new_ids)
    }

    /// Generate cleanup suggestions based on current storage usage.
    pub fn cleanup_suggestions(&self, now: u64) -> Vec<String> {
        let mut suggestions = Vec::new();
        let stats = self.storage_stats();

        // Suggest deleting old automatic snapshots.
        let mut old_auto_count = 0usize;
        for id in self.tree.all_ids_by_timestamp() {
            if let Some(snap) = self.tree.get_snapshot(id)
                && snap.snapshot_type != SnapshotType::Manual
                && !snap.locked
                && now.saturating_sub(snap.timestamp) > 30 * 86_400
            {
                old_auto_count += 1;
            }
        }
        if old_auto_count > 0 {
            suggestions.push(format!(
                "Delete {} automatic snapshot(s) older than 30 days",
                old_auto_count,
            ));
        }

        // Suggest enabling retention policy if not set.
        if !self.schedule.retention.has_count_limit()
            && !self.schedule.retention.has_age_limit()
            && !self.schedule.retention.has_size_limit()
            && stats.snapshot_count > 10
        {
            suggestions.push(
                "Enable a retention policy to automatically clean up old snapshots".to_string(),
            );
        }

        // Suggest if total storage is high.
        if stats.total_bytes > 100_000_000_000 {
            suggestions.push(format!(
                "Total snapshot storage is {} -- consider pruning old snapshots",
                stats.total_display(),
            ));
        }

        suggestions
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Progress simulation
// ============================================================================

/// Progress state for snapshot creation or restore operations.
#[derive(Clone, Debug)]
pub struct OperationProgress {
    /// Description of the current step.
    pub current_step: String,
    /// Step number (1-based).
    pub step_index: usize,
    /// Total number of steps.
    pub total_steps: usize,
    /// Bytes processed so far.
    pub bytes_processed: u64,
    /// Total bytes to process.
    pub total_bytes: u64,
    /// Whether the operation is complete.
    pub complete: bool,
    /// Error message if the operation failed.
    pub error: Option<String>,
}

impl OperationProgress {
    /// Create initial progress for a snapshot creation.
    pub fn new_create(components: &[SnapshotComponent]) -> Self {
        let total_bytes: u64 = components.iter().map(|c| c.estimated_size_bytes()).sum();
        Self {
            current_step: "Preparing snapshot...".to_string(),
            step_index: 0,
            total_steps: components.len().saturating_add(2), // components + prepare + finalize
            bytes_processed: 0,
            total_bytes,
            complete: false,
            error: None,
        }
    }

    /// Create initial progress for a restore.
    pub fn new_restore(snap: &Snapshot) -> Self {
        Self {
            current_step: "Preparing restore...".to_string(),
            step_index: 0,
            total_steps: snap.component_count().saturating_add(2),
            bytes_processed: 0,
            total_bytes: snap.size_bytes,
            complete: false,
            error: None,
        }
    }

    /// Progress fraction (0.0 to 1.0).
    pub fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            if self.total_steps == 0 {
                return 1.0;
            }
            return self.step_index as f32 / self.total_steps as f32;
        }
        (self.bytes_processed as f64 / self.total_bytes as f64) as f32
    }

    /// Progress percentage (0 to 100).
    pub fn percentage(&self) -> u32 {
        (self.fraction() * 100.0) as u32
    }

    /// Advance to the next step.
    pub fn advance(&mut self, step_name: &str, bytes_done: u64) {
        self.step_index = self.step_index.saturating_add(1);
        self.current_step = step_name.to_string();
        self.bytes_processed = self.bytes_processed.saturating_add(bytes_done);
    }

    /// Mark complete.
    pub fn finish(&mut self) {
        self.complete = true;
        self.bytes_processed = self.total_bytes;
        self.step_index = self.total_steps;
        self.current_step = "Complete".to_string();
    }

    /// Mark failed.
    pub fn fail(&mut self, message: &str) {
        self.error = Some(message.to_string());
    }

    /// Simulate the full creation process, returning intermediate states.
    pub fn simulate_create(components: &[SnapshotComponent]) -> Vec<Self> {
        let mut states = Vec::new();
        let mut progress = Self::new_create(components);
        states.push(progress.clone());

        // Prepare step.
        progress.advance("Analyzing system state...", 0);
        states.push(progress.clone());

        // One step per component.
        for comp in components {
            let step_name = format!("Snapshotting {}...", comp.label());
            progress.advance(&step_name, comp.estimated_size_bytes());
            states.push(progress.clone());
        }

        // Finalize.
        progress.advance("Finalizing snapshot...", 0);
        states.push(progress.clone());
        progress.finish();
        states.push(progress);

        states
    }

    /// Simulate the full restore process, returning intermediate states.
    pub fn simulate_restore(snap: &Snapshot) -> Vec<Self> {
        let mut states = Vec::new();
        let mut progress = Self::new_restore(snap);
        states.push(progress.clone());

        progress.advance("Verifying snapshot integrity...", 0);
        states.push(progress.clone());

        for comp in &snap.components {
            let step_name = format!("Restoring {}...", comp.label());
            progress.advance(&step_name, comp.estimated_size_bytes());
            states.push(progress.clone());
        }

        progress.advance("Applying changes...", 0);
        states.push(progress.clone());
        progress.finish();
        states.push(progress);

        states
    }
}

// ============================================================================
// ViewMode
// ============================================================================

/// Which view is currently active in the main panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Tree view with parent-child relationships.
    Tree,
    /// Chronological timeline of all snapshots.
    Timeline,
    /// Compare two snapshots side by side.
    Compare,
    /// Schedule configuration view.
    Schedule,
    /// Storage management view.
    Storage,
}

impl ViewMode {
    /// Label for the view tab.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Timeline => "Timeline",
            Self::Compare => "Compare",
            Self::Schedule => "Schedule",
            Self::Storage => "Storage",
        }
    }

    /// All view modes.
    pub fn all() -> &'static [Self] {
        &[
            Self::Tree,
            Self::Timeline,
            Self::Compare,
            Self::Schedule,
            Self::Storage,
        ]
    }
}

// ============================================================================
// DialogKind
// ============================================================================

/// Which dialog is currently open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// Create a new snapshot.
    CreateSnapshot,
    /// Confirm restore of a snapshot.
    ConfirmRestore(u64),
    /// Confirm deletion of a snapshot.
    ConfirmDelete(u64),
    /// Export snapshots.
    ExportDialog,
    /// Import snapshots.
    ImportDialog,
    /// No dialog is open.
    None,
}

// ============================================================================
// SystemRestoreUI
// ============================================================================

/// Main application UI state for the system restore manager.
pub struct SystemRestoreUI {
    /// The snapshot manager.
    pub manager: SnapshotManager,
    /// Current view mode.
    pub view_mode: ViewMode,
    /// Currently selected snapshot ID.
    pub selected_id: Option<u64>,
    /// Second selected snapshot (for comparison).
    pub compare_id: Option<u64>,
    /// Search query.
    pub search_query: String,
    /// Type filter (None = show all).
    pub type_filter: Option<SnapshotType>,
    /// Current dialog.
    pub dialog: DialogKind,
    /// Progress state for ongoing operations.
    pub progress: Option<OperationProgress>,
    /// Scroll offset for the main list.
    pub scroll_offset: f32,
    /// New snapshot form: name.
    pub form_name: String,
    /// New snapshot form: description.
    pub form_description: String,
    /// New snapshot form: selected components.
    pub form_components: Vec<bool>,
    /// New snapshot form: snapshot type.
    pub form_type: SnapshotType,
    /// New snapshot form: parent ID (None = root).
    pub form_parent_id: Option<u64>,
    /// Current simulated timestamp for demo purposes.
    pub current_timestamp: u64,
    /// Hovered button index for highlighting.
    pub hovered_button: Option<usize>,
}

impl SystemRestoreUI {
    /// Create a new UI state with demo data.
    pub fn new() -> Self {
        let mut manager = SnapshotManager::new();
        let base_ts = 1_700_000_000u64;

        // Create demo snapshot tree.
        let root_id = manager
            .create_snapshot(
                "Initial Setup",
                "Clean install with base system",
                base_ts,
                SnapshotType::Manual,
                SnapshotComponent::default_set(),
                None,
            )
            .unwrap_or(0);

        let after_update_id = manager
            .create_snapshot(
                "After System Update v1.1",
                "System updated to version 1.1 with security patches",
                base_ts + 86_400 * 7,
                SnapshotType::PreUpdate,
                vec![
                    SnapshotComponent::SystemFiles,
                    SnapshotComponent::BootConfig,
                    SnapshotComponent::PackageState,
                ],
                Some(root_id),
            )
            .unwrap_or(0);

        let _dev_branch = manager
            .create_snapshot(
                "Dev Tools Installed",
                "Added development toolchain and IDE",
                base_ts + 86_400 * 10,
                SnapshotType::PreInstall,
                vec![
                    SnapshotComponent::InstalledApps,
                    SnapshotComponent::UserSettings,
                    SnapshotComponent::PackageState,
                ],
                Some(after_update_id),
            )
            .unwrap_or(0);

        let _weekly_auto = manager
            .create_snapshot(
                "Weekly Auto Backup",
                "Scheduled weekly snapshot",
                base_ts + 86_400 * 14,
                SnapshotType::Scheduled,
                SnapshotComponent::default_set(),
                Some(after_update_id),
            )
            .unwrap_or(0);

        let _net_config = manager
            .create_snapshot(
                "Network Reconfigured",
                "Changed to static IP and new DNS settings",
                base_ts + 86_400 * 20,
                SnapshotType::Manual,
                vec![
                    SnapshotComponent::NetworkConfig,
                    SnapshotComponent::ServiceConfig,
                ],
                Some(root_id),
            )
            .unwrap_or(0);

        // Set up a default schedule.
        manager.schedule = ScheduleConfig {
            enabled: true,
            frequency: ScheduleFrequency::Weekly,
            components: SnapshotComponent::default_set(),
            retention: RetentionPolicy::new(10, 30 * 86_400, 50_000_000_000),
            last_snapshot_timestamp: base_ts + 86_400 * 14,
        };

        Self {
            manager,
            view_mode: ViewMode::Tree,
            selected_id: Some(root_id),
            compare_id: None,
            search_query: String::new(),
            type_filter: None,
            dialog: DialogKind::None,
            progress: None,
            scroll_offset: 0.0,
            form_name: String::new(),
            form_description: String::new(),
            form_components: vec![true; SnapshotComponent::all().len()],
            form_type: SnapshotType::Manual,
            form_parent_id: None,
            current_timestamp: base_ts + 86_400 * 25,
            hovered_button: None,
        }
    }

    /// Get the list of visible snapshot IDs based on current filters.
    pub fn visible_ids(&self) -> Vec<u64> {
        let all_ids = if self.view_mode == ViewMode::Timeline {
            self.manager.tree.all_ids_by_timestamp()
        } else {
            self.manager
                .tree
                .flatten_for_display()
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };

        all_ids
            .into_iter()
            .filter(|&id| {
                if let Some(snap) = self.manager.tree.get_snapshot(id) {
                    // Apply type filter.
                    if let Some(filter_type) = self.type_filter
                        && snap.snapshot_type != filter_type
                    {
                        return false;
                    }
                    // Apply search query.
                    if !self.search_query.is_empty() {
                        let q = self.search_query.to_ascii_lowercase();
                        if !snap.name.to_ascii_lowercase().contains(&q)
                            && !snap.description.to_ascii_lowercase().contains(&q)
                        {
                            return false;
                        }
                    }
                    true
                } else {
                    false
                }
            })
            .collect()
    }

    /// Estimated size for the new snapshot form based on selected components.
    pub fn form_estimated_size(&self) -> u64 {
        let all_components = SnapshotComponent::all();
        self.form_components
            .iter()
            .enumerate()
            .filter(|(_, selected)| **selected)
            .filter_map(|(i, _)| all_components.get(i))
            .map(|c| c.estimated_size_bytes())
            .sum()
    }

    /// Get selected components from the form.
    pub fn form_selected_components(&self) -> Vec<SnapshotComponent> {
        let all_components = SnapshotComponent::all();
        self.form_components
            .iter()
            .enumerate()
            .filter(|(_, selected)| **selected)
            .filter_map(|(i, _)| all_components.get(i).copied())
            .collect()
    }

    /// Render the complete UI to a render tree.
    pub fn render(&self) -> RenderTree {
        let mut rt = RenderTree::new();

        // Background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: COLOR_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_header(&mut rt);
        self.render_toolbar(&mut rt);
        self.render_main_area(&mut rt);
        self.render_details_panel(&mut rt);
        self.render_status_bar(&mut rt);

        if self.dialog != DialogKind::None {
            self.render_dialog(&mut rt);
        }

        if self.progress.is_some() {
            self.render_progress_overlay(&mut rt);
        }

        rt
    }

    /// Render the header bar.
    fn render_header(&self, rt: &mut RenderTree) {
        // Header background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: HEADER_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title.
        rt.push(RenderCommand::Text {
            x: PADDING,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE_TITLE / 2.0,
            text: "System Restore".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
        });

        // Snapshot count badge.
        let count_text = format!("{} snapshots", self.manager.tree.count());
        rt.push(RenderCommand::FillRect {
            x: 240.0,
            y: HEADER_HEIGHT / 2.0 - 10.0,
            width: 100.0,
            height: 20.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(10.0),
        });
        rt.push(RenderCommand::Text {
            x: 255.0,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: count_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });

        // Search box.
        let search_x = WINDOW_WIDTH - 260.0;
        rt.push(RenderCommand::FillRect {
            x: search_x,
            y: HEADER_HEIGHT / 2.0 - 14.0,
            width: 240.0,
            height: 28.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        let search_display = if self.search_query.is_empty() {
            "Search snapshots...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };
        rt.push(RenderCommand::Text {
            x: search_x + 8.0,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE / 2.0,
            text: search_display,
            color: search_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(220.0),
        });

        // Header bottom border.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: HEADER_HEIGHT,
            x2: WINDOW_WIDTH,
            y2: HEADER_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the toolbar with view mode tabs and action buttons.
    fn render_toolbar(&self, rt: &mut RenderTree) {
        let toolbar_y = HEADER_HEIGHT;

        // Toolbar background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: toolbar_y,
            width: WINDOW_WIDTH,
            height: TOOLBAR_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // View mode tabs.
        let mut tab_x = PADDING;
        for mode in ViewMode::all() {
            let is_active = *mode == self.view_mode;
            let tab_width = 80.0;
            let tab_color = if is_active {
                COLOR_SURFACE0
            } else {
                COLOR_MANTLE
            };
            let text_color = if is_active {
                COLOR_BLUE
            } else {
                COLOR_SUBTEXT0
            };

            rt.push(RenderCommand::FillRect {
                x: tab_x,
                y: toolbar_y + 5.0,
                width: tab_width,
                height: TOOLBAR_HEIGHT - 10.0,
                color: tab_color,
                corner_radii: CornerRadii::all(4.0),
            });
            rt.push(RenderCommand::Text {
                x: tab_x + tab_width / 2.0 - 20.0,
                y: toolbar_y + TOOLBAR_HEIGHT / 2.0 - FONT_SIZE / 2.0,
                text: mode.label().to_string(),
                color: text_color,
                font_size: FONT_SIZE,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width),
            });
            tab_x += tab_width + 4.0;
        }

        // Action buttons.
        let actions = [
            ("Create", COLOR_GREEN),
            ("Restore", COLOR_BLUE),
            ("Delete", COLOR_RED),
            ("Export", COLOR_PEACH),
        ];
        let mut btn_x = WINDOW_WIDTH - (actions.len() as f32 * (BUTTON_WIDTH + 8.0)) - PADDING;
        for (label, color) in &actions {
            rt.push(RenderCommand::FillRect {
                x: btn_x,
                y: toolbar_y + 5.0,
                width: BUTTON_WIDTH,
                height: BUTTON_HEIGHT,
                color: *color,
                corner_radii: CornerRadii::all(4.0),
            });
            rt.push(RenderCommand::Text {
                x: btn_x + BUTTON_WIDTH / 2.0 - 20.0,
                y: toolbar_y + TOOLBAR_HEIGHT / 2.0 - FONT_SIZE / 2.0,
                text: label.to_string(),
                color: COLOR_BASE,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(BUTTON_WIDTH - 8.0),
            });
            btn_x += BUTTON_WIDTH + 8.0;
        }

        // Bottom border.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: toolbar_y + TOOLBAR_HEIGHT,
            x2: WINDOW_WIDTH,
            y2: toolbar_y + TOOLBAR_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the main content area based on current view mode.
    fn render_main_area(&self, rt: &mut RenderTree) {
        let content_y = HEADER_HEIGHT + TOOLBAR_HEIGHT;
        let content_height = WINDOW_HEIGHT - content_y - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;

        // Clip to content area.
        rt.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: WINDOW_WIDTH,
            height: content_height,
        });

        match self.view_mode {
            ViewMode::Tree => self.render_tree_view(rt, content_y, content_height),
            ViewMode::Timeline => self.render_timeline_view(rt, content_y, content_height),
            ViewMode::Compare => self.render_compare_view(rt, content_y, content_height),
            ViewMode::Schedule => self.render_schedule_view(rt, content_y, content_height),
            ViewMode::Storage => self.render_storage_view(rt, content_y, content_height),
        }

        rt.push(RenderCommand::PopClip);
    }

    /// Render the tree view with connection lines and indentation.
    fn render_tree_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let flattened = self.manager.tree.flatten_for_display();
        let mut row_y = y + SMALL_PADDING - self.scroll_offset;

        for (id, depth) in &flattened {
            if let Some(snap) = self.manager.tree.get_snapshot(*id) {
                // Apply filters.
                if let Some(ft) = self.type_filter
                    && snap.snapshot_type != ft
                {
                    continue;
                }
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_ascii_lowercase();
                    if !snap.name.to_ascii_lowercase().contains(&q)
                        && !snap.description.to_ascii_lowercase().contains(&q)
                    {
                        continue;
                    }
                }

                let indent = *depth as f32 * TREE_INDENT;
                let is_selected = self.selected_id == Some(*id);

                // Selection highlight.
                if is_selected {
                    rt.push(RenderCommand::FillRect {
                        x: PADDING,
                        y: row_y,
                        width: WINDOW_WIDTH - 2.0 * PADDING,
                        height: TREE_ROW_HEIGHT,
                        color: COLOR_SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Connection lines.
                if *depth > 0 {
                    let line_x = PADDING + indent - TREE_INDENT / 2.0;
                    // Vertical line from parent.
                    rt.push(RenderCommand::Line {
                        x1: line_x,
                        y1: row_y,
                        x2: line_x,
                        y2: row_y + TREE_ROW_HEIGHT / 2.0,
                        color: COLOR_OVERLAY0,
                        width: 1.0,
                    });
                    // Horizontal line to node.
                    rt.push(RenderCommand::Line {
                        x1: line_x,
                        y1: row_y + TREE_ROW_HEIGHT / 2.0,
                        x2: PADDING + indent,
                        y2: row_y + TREE_ROW_HEIGHT / 2.0,
                        color: COLOR_OVERLAY0,
                        width: 1.0,
                    });
                }

                // Type indicator dot.
                let dot_x = PADDING + indent + 4.0;
                let dot_y = row_y + TREE_ROW_HEIGHT / 2.0 - 4.0;
                rt.push(RenderCommand::FillRect {
                    x: dot_x,
                    y: dot_y,
                    width: 8.0,
                    height: 8.0,
                    color: snap.snapshot_type.indicator_color(),
                    corner_radii: CornerRadii::all(4.0),
                });

                // Snapshot name.
                let name_x = PADDING + indent + 18.0;
                rt.push(RenderCommand::Text {
                    x: name_x,
                    y: row_y + 4.0,
                    text: snap.name.clone(),
                    color: if is_selected {
                        COLOR_TEXT
                    } else {
                        COLOR_SUBTEXT1
                    },
                    font_size: FONT_SIZE,
                    font_weight: if is_selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(300.0),
                });

                // Metadata line: type, size, age.
                let meta_text = format!(
                    "{} | {} | {}",
                    snap.snapshot_type.label(),
                    snap.size_display(),
                    snap.age_display(self.current_timestamp),
                );
                rt.push(RenderCommand::Text {
                    x: name_x,
                    y: row_y + 20.0,
                    text: meta_text,
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(400.0),
                });

                // Lock indicator.
                if snap.locked {
                    let lock_x = WINDOW_WIDTH - 60.0;
                    rt.push(RenderCommand::Text {
                        x: lock_x,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: "Locked".to_string(),
                        color: COLOR_YELLOW,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(50.0),
                    });
                }

                // Children count indicator.
                let kids = self.manager.tree.children_of(*id);
                if !kids.is_empty() {
                    let branch_x = WINDOW_WIDTH - 120.0;
                    rt.push(RenderCommand::Text {
                        x: branch_x,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: format!("{} children", kids.len()),
                        color: COLOR_OVERLAY0,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(80.0),
                    });
                }

                row_y += TREE_ROW_HEIGHT;
            }
        }
    }

    /// Render the timeline view with chronological entries and type dots.
    fn render_timeline_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let ids = self.visible_ids();
        let timeline_x = 60.0;
        let mut entry_y = y + PADDING - self.scroll_offset;

        // Timeline vertical line.
        if !ids.is_empty() {
            let total_h = ids.len() as f32 * TIMELINE_ENTRY_HEIGHT;
            rt.push(RenderCommand::Line {
                x1: timeline_x,
                y1: y + PADDING,
                x2: timeline_x,
                y2: y + PADDING + total_h,
                color: COLOR_SURFACE1,
                width: 2.0,
            });
        }

        for id in &ids {
            if let Some(snap) = self.manager.tree.get_snapshot(*id) {
                let is_selected = self.selected_id == Some(*id);

                // Selection highlight.
                if is_selected {
                    rt.push(RenderCommand::FillRect {
                        x: timeline_x + 20.0,
                        y: entry_y,
                        width: WINDOW_WIDTH - timeline_x - 40.0,
                        height: TIMELINE_ENTRY_HEIGHT - 4.0,
                        color: COLOR_SURFACE0,
                        corner_radii: CornerRadii::all(4.0),
                    });
                }

                // Timeline dot.
                rt.push(RenderCommand::FillRect {
                    x: timeline_x - TIMELINE_DOT_RADIUS,
                    y: entry_y + TIMELINE_ENTRY_HEIGHT / 2.0 - TIMELINE_DOT_RADIUS,
                    width: TIMELINE_DOT_RADIUS * 2.0,
                    height: TIMELINE_DOT_RADIUS * 2.0,
                    color: snap.snapshot_type.indicator_color(),
                    corner_radii: CornerRadii::all(TIMELINE_DOT_RADIUS),
                });

                // Snapshot name.
                let text_x = timeline_x + 24.0;
                rt.push(RenderCommand::Text {
                    x: text_x,
                    y: entry_y + 4.0,
                    text: snap.name.clone(),
                    color: if is_selected {
                        COLOR_TEXT
                    } else {
                        COLOR_SUBTEXT1
                    },
                    font_size: FONT_SIZE,
                    font_weight: if is_selected {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(400.0),
                });

                // Metadata.
                let meta_text = format!(
                    "{} | {} | {} components",
                    snap.snapshot_type.label(),
                    snap.size_display(),
                    snap.component_count(),
                );
                rt.push(RenderCommand::Text {
                    x: text_x,
                    y: entry_y + 22.0,
                    text: meta_text,
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(500.0),
                });

                // Timestamp on the left side.
                let ts_text = format_timestamp_short(snap.timestamp);
                rt.push(RenderCommand::Text {
                    x: 4.0,
                    y: entry_y + TIMELINE_ENTRY_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                    text: ts_text,
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(50.0),
                });

                entry_y += TIMELINE_ENTRY_HEIGHT;
            }
        }
    }

    /// Render the compare view showing differences between two snapshots.
    fn render_compare_view(&self, rt: &mut RenderTree, y: f32, height: f32) {
        let panel_x = PADDING;
        let panel_width = WINDOW_WIDTH - 2.0 * PADDING;

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Compare Snapshots".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
        });

        if let (Some(sid), Some(cid)) = (self.selected_id, self.compare_id) {
            if let Ok(diff) = self.manager.compare_snapshots(sid, cid) {
                // Summary.
                let summary = format!(
                    "{} additions, {} removals, {} modifications",
                    diff.addition_count(),
                    diff.removal_count(),
                    diff.modification_count(),
                );
                rt.push(RenderCommand::Text {
                    x: panel_x,
                    y: y + PADDING + 24.0,
                    text: summary,
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width),
                });

                // List entries.
                let mut entry_y = y + PADDING + 50.0;
                let max_y = y + height - 10.0;
                for entry in &diff.entries {
                    if entry_y > max_y {
                        break;
                    }
                    let color = if entry.is_addition() {
                        COLOR_GREEN
                    } else if entry.is_removal() {
                        COLOR_RED
                    } else {
                        COLOR_YELLOW
                    };
                    rt.push(RenderCommand::Text {
                        x: panel_x + 8.0,
                        y: entry_y,
                        text: entry.summary(),
                        color,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(panel_width - 16.0),
                    });
                    entry_y += 18.0;
                }
            }
        } else {
            rt.push(RenderCommand::Text {
                x: panel_x,
                y: y + PADDING + 24.0,
                text: "Select two snapshots to compare".to_string(),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width),
            });
        }
    }

    /// Render the schedule configuration view.
    fn render_schedule_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let panel_x = PADDING;
        let panel_width = WINDOW_WIDTH - 2.0 * PADDING;
        let schedule = &self.manager.schedule;

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Snapshot Schedule".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
        });

        // Status.
        let status_text = if schedule.enabled {
            "Enabled"
        } else {
            "Disabled"
        };
        let status_color = if schedule.enabled {
            COLOR_GREEN
        } else {
            COLOR_RED
        };
        rt.push(RenderCommand::FillRect {
            x: panel_x,
            y: y + PADDING + 30.0,
            width: 80.0,
            height: 24.0,
            color: status_color,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: y + PADDING + 35.0,
            text: status_text.to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(60.0),
        });

        // Frequency.
        let mut info_y = y + PADDING + 66.0;
        let label_x = panel_x + 8.0;
        let value_x = panel_x + 180.0;

        let rows = [
            ("Frequency:", schedule.frequency.label()),
            ("Components:", &format!("{}", schedule.components.len())),
            ("Retention:", &schedule.retention.summary()),
        ];

        for (label, value) in &rows {
            rt.push(RenderCommand::Text {
                x: label_x,
                y: info_y,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 200.0),
            });
            info_y += 24.0;
        }

        // Next snapshot due.
        if schedule.enabled {
            let next_due = schedule
                .last_snapshot_timestamp
                .saturating_add(schedule.frequency.interval_secs());
            let due_text = if self.current_timestamp >= next_due {
                "Overdue".to_string()
            } else {
                format!(
                    "in {}",
                    format_duration_short(next_due.saturating_sub(self.current_timestamp))
                )
            };
            rt.push(RenderCommand::Text {
                x: label_x,
                y: info_y,
                text: "Next snapshot:".to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: due_text,
                color: COLOR_LAVENDER,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
        }
    }

    /// Render the storage management view.
    fn render_storage_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let panel_x = PADDING;
        let panel_width = WINDOW_WIDTH - 2.0 * PADDING;
        let stats = self.manager.storage_stats();

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Storage Management".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
        });

        // Storage bar visualization.
        let bar_y = y + PADDING + 30.0;
        let bar_width = panel_width - 20.0;

        rt.push(RenderCommand::FillRect {
            x: panel_x + 10.0,
            y: bar_y,
            width: bar_width,
            height: 24.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Show manual vs auto portions.
        let total = stats.total_bytes.max(1);
        let manual_frac = stats.manual_bytes as f32 / total as f32;
        let auto_frac = stats.auto_bytes as f32 / total as f32;

        if manual_frac > 0.0 {
            rt.push(RenderCommand::FillRect {
                x: panel_x + 10.0,
                y: bar_y,
                width: bar_width * manual_frac,
                height: 24.0,
                color: COLOR_BLUE,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        if auto_frac > 0.0 {
            rt.push(RenderCommand::FillRect {
                x: panel_x + 10.0 + bar_width * manual_frac,
                y: bar_y,
                width: bar_width * auto_frac,
                height: 24.0,
                color: COLOR_GREEN,
                corner_radii: CornerRadii::ZERO,
            });
        }

        // Legend.
        let legend_y = bar_y + 32.0;
        rt.push(RenderCommand::FillRect {
            x: panel_x + 10.0,
            y: legend_y,
            width: 12.0,
            height: 12.0,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::all(2.0),
        });
        rt.push(RenderCommand::Text {
            x: panel_x + 28.0,
            y: legend_y,
            text: format!("Manual ({})", format_bytes(stats.manual_bytes)),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        rt.push(RenderCommand::FillRect {
            x: panel_x + 230.0,
            y: legend_y,
            width: 12.0,
            height: 12.0,
            color: COLOR_GREEN,
            corner_radii: CornerRadii::all(2.0),
        });
        rt.push(RenderCommand::Text {
            x: panel_x + 248.0,
            y: legend_y,
            text: format!("Auto ({})", format_bytes(stats.auto_bytes)),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Statistics table.
        let mut info_y = legend_y + 30.0;
        let label_x = panel_x + 10.0;
        let value_x = panel_x + 220.0;

        let info_rows: Vec<(&str, String)> = vec![
            ("Total storage:", stats.total_display()),
            ("Snapshot count:", format!("{}", stats.snapshot_count)),
            ("Average size:", stats.avg_display()),
            ("Largest:", format_bytes(stats.largest_snapshot_bytes)),
            ("Smallest:", format_bytes(stats.smallest_snapshot_bytes)),
        ];

        for (label, value) in &info_rows {
            rt.push(RenderCommand::Text {
                x: label_x,
                y: info_y,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
            });
            info_y += 22.0;
        }

        // Cleanup suggestions.
        let suggestions = self.manager.cleanup_suggestions(self.current_timestamp);
        if !suggestions.is_empty() {
            info_y += 10.0;
            rt.push(RenderCommand::Text {
                x: label_x,
                y: info_y,
                text: "Suggestions:".to_string(),
                color: COLOR_YELLOW,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(panel_width),
            });
            info_y += 20.0;
            for suggestion in &suggestions {
                rt.push(RenderCommand::Text {
                    x: label_x + 12.0,
                    y: info_y,
                    text: suggestion.clone(),
                    color: COLOR_SUBTEXT0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - 24.0),
                });
                info_y += 18.0;
            }
        }
    }

    /// Render the details panel at the bottom.
    fn render_details_panel(&self, rt: &mut RenderTree) {
        let panel_y = WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;

        // Separator line.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: panel_y,
            x2: WINDOW_WIDTH,
            y2: panel_y,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        // Panel background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: panel_y,
            width: WINDOW_WIDTH,
            height: DETAILS_PANEL_HEIGHT,
            color: COLOR_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        if let Some(id) = self.selected_id {
            if let Some(snap) = self.manager.tree.get_snapshot(id) {
                self.render_snapshot_details(rt, snap, panel_y);
            } else {
                self.render_no_selection(rt, panel_y);
            }
        } else {
            self.render_no_selection(rt, panel_y);
        }
    }

    /// Render snapshot detail info in the details panel.
    fn render_snapshot_details(&self, rt: &mut RenderTree, snap: &Snapshot, panel_y: f32) {
        let col1_x = PADDING;
        let col2_x = WINDOW_WIDTH / 2.0;
        let mut y = panel_y + PADDING;

        // Name and type.
        rt.push(RenderCommand::Text {
            x: col1_x,
            y,
            text: snap.name.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(WINDOW_WIDTH / 2.0 - PADDING),
        });

        // Type badge.
        rt.push(RenderCommand::FillRect {
            x: col2_x,
            y,
            width: 80.0,
            height: 20.0,
            color: snap.snapshot_type.indicator_color(),
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: col2_x + 8.0,
            y: y + 3.0,
            text: snap.snapshot_type.label().to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(70.0),
        });

        y += 24.0;

        // Description.
        if !snap.description.is_empty() {
            rt.push(RenderCommand::Text {
                x: col1_x,
                y,
                text: snap.description.clone(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(WINDOW_WIDTH - 2.0 * PADDING),
            });
            y += 20.0;
        }

        // Metadata row.
        let detail_labels = [
            ("Size:", snap.size_display()),
            ("Age:", snap.age_display(self.current_timestamp)),
            ("Components:", format!("{}", snap.component_count())),
            (
                "Locked:",
                if snap.locked { "Yes" } else { "No" }.to_string(),
            ),
        ];

        let mut label_x = col1_x;
        for (label, value) in &detail_labels {
            rt.push(RenderCommand::Text {
                x: label_x,
                y,
                text: label.to_string(),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(60.0),
            });
            rt.push(RenderCommand::Text {
                x: label_x + 65.0,
                y,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
            });
            label_x += 190.0;
        }

        y += 20.0;

        // Components list.
        rt.push(RenderCommand::Text {
            x: col1_x,
            y,
            text: "Included:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some(80.0),
        });
        let comp_names: Vec<&str> = snap.components.iter().map(|c| c.label()).collect();
        rt.push(RenderCommand::Text {
            x: col1_x + 70.0,
            y,
            text: comp_names.join(", "),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(WINDOW_WIDTH - col1_x - 90.0),
        });

        y += 20.0;

        // Tags.
        if !snap.tags.is_empty() {
            let mut tag_x = col1_x;
            for tag in &snap.tags {
                let tag_width = tag.len() as f32 * 7.0 + 16.0;
                rt.push(RenderCommand::FillRect {
                    x: tag_x,
                    y,
                    width: tag_width,
                    height: 18.0,
                    color: COLOR_SURFACE1,
                    corner_radii: CornerRadii::all(9.0),
                });
                rt.push(RenderCommand::Text {
                    x: tag_x + 8.0,
                    y: y + 2.0,
                    text: tag.clone(),
                    color: COLOR_LAVENDER,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(tag_width - 16.0),
                });
                tag_x += tag_width + 6.0;
            }
        }

        // Ancestry chain.
        let chain = self.manager.tree.ancestry_chain(snap.id);
        if chain.len() > 1 {
            let chain_y = panel_y + DETAILS_PANEL_HEIGHT - 22.0;
            let mut cx = col1_x;
            rt.push(RenderCommand::Text {
                x: cx,
                y: chain_y,
                text: "Path:".to_string(),
                color: COLOR_OVERLAY0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
            });
            cx += 40.0;
            for (i, &ancestor_id) in chain.iter().enumerate() {
                if let Some(ancestor) = self.manager.tree.get_snapshot(ancestor_id) {
                    if i > 0 {
                        rt.push(RenderCommand::Text {
                            x: cx,
                            y: chain_y,
                            text: " > ".to_string(),
                            color: COLOR_OVERLAY0,
                            font_size: FONT_SIZE_SMALL,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(20.0),
                        });
                        cx += 20.0;
                    }
                    let name_color = if ancestor_id == snap.id {
                        COLOR_BLUE
                    } else {
                        COLOR_SUBTEXT0
                    };
                    rt.push(RenderCommand::Text {
                        x: cx,
                        y: chain_y,
                        text: ancestor.name.clone(),
                        color: name_color,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(150.0),
                    });
                    cx += ancestor.name.len() as f32 * 6.5 + 4.0;
                }
            }
        }
    }

    /// Render placeholder when no snapshot is selected.
    fn render_no_selection(&self, rt: &mut RenderTree, panel_y: f32) {
        rt.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 100.0,
            y: panel_y + DETAILS_PANEL_HEIGHT / 2.0 - FONT_SIZE / 2.0,
            text: "Select a snapshot to view details".to_string(),
            color: COLOR_OVERLAY0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(250.0),
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, rt: &mut RenderTree) {
        let bar_y = WINDOW_HEIGHT - STATUS_BAR_HEIGHT;

        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: WINDOW_WIDTH,
            height: STATUS_BAR_HEIGHT,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // Left: view mode and filter info.
        let filter_text = if let Some(ft) = self.type_filter {
            format!("View: {} | Filter: {}", self.view_mode.label(), ft.label())
        } else {
            format!("View: {}", self.view_mode.label())
        };
        rt.push(RenderCommand::Text {
            x: PADDING,
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: filter_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
        });

        // Center: storage summary.
        let stats = self.manager.storage_stats();
        let storage_text = format!(
            "{} snapshots | {} total",
            stats.snapshot_count,
            stats.total_display(),
        );
        rt.push(RenderCommand::Text {
            x: WINDOW_WIDTH / 2.0 - 80.0,
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: storage_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
        });

        // Right: schedule status.
        let schedule_text = if self.manager.schedule.enabled {
            format!(
                "Schedule: {} (active)",
                self.manager.schedule.frequency.label()
            )
        } else {
            "Schedule: Off".to_string()
        };
        rt.push(RenderCommand::Text {
            x: WINDOW_WIDTH - 200.0,
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: schedule_text,
            color: if self.manager.schedule.enabled {
                COLOR_GREEN
            } else {
                COLOR_OVERLAY0
            },
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(180.0),
        });
    }

    /// Render a dialog overlay.
    fn render_dialog(&self, rt: &mut RenderTree) {
        // Dim overlay.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
            color: Color::rgba(0, 0, 0, 160),
            corner_radii: CornerRadii::ZERO,
        });

        match &self.dialog {
            DialogKind::CreateSnapshot => self.render_create_dialog(rt),
            DialogKind::ConfirmRestore(id) => self.render_restore_dialog(rt, *id),
            DialogKind::ConfirmDelete(id) => self.render_delete_dialog(rt, *id),
            DialogKind::ExportDialog => self.render_export_dialog(rt),
            DialogKind::ImportDialog => self.render_import_dialog(rt),
            DialogKind::None => {}
        }
    }

    /// Render the create snapshot dialog.
    fn render_create_dialog(&self, rt: &mut RenderTree) {
        let dialog_w = 500.0;
        let dialog_h = 440.0;
        let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
        let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

        // Shadow.
        rt.push(RenderCommand::BoxShadow {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 20.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Background.
        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Border.
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Create New Snapshot".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        // Name field.
        let mut field_y = dy + 44.0;
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: field_y,
            text: "Name:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(60.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + PADDING,
            y: field_y + 18.0,
            width: dialog_w - 2.0 * PADDING,
            height: 28.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        let name_display = if self.form_name.is_empty() {
            "Enter snapshot name..."
        } else {
            &self.form_name
        };
        let name_color = if self.form_name.is_empty() {
            COLOR_OVERLAY0
        } else {
            COLOR_TEXT
        };
        rt.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: field_y + 24.0,
            text: name_display.to_string(),
            color: name_color,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
        });

        // Description field.
        field_y += 54.0;
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: field_y,
            text: "Description:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + PADDING,
            y: field_y + 18.0,
            width: dialog_w - 2.0 * PADDING,
            height: 28.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Components checkboxes.
        field_y += 56.0;
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: field_y,
            text: "Components:".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
        });
        field_y += 20.0;

        let all_components = SnapshotComponent::all();
        let cols = 2;
        let col_width = (dialog_w - 2.0 * PADDING) / cols as f32;
        for (i, comp) in all_components.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let cx = dx + PADDING + col as f32 * col_width;
            let cy = field_y + row as f32 * 22.0;
            let checked = self.form_components.get(i).copied().unwrap_or(false);

            // Checkbox.
            rt.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: CHECKBOX_SIZE,
                height: CHECKBOX_SIZE,
                color: if checked { COLOR_BLUE } else { COLOR_SURFACE0 },
                corner_radii: CornerRadii::all(3.0),
            });
            if checked {
                rt.push(RenderCommand::Text {
                    x: cx + 3.0,
                    y: cy + 1.0,
                    text: "v".to_string(),
                    color: COLOR_BASE,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(CHECKBOX_SIZE),
                });
            }
            rt.push(RenderCommand::Text {
                x: cx + CHECKBOX_SIZE + 4.0,
                y: cy + 1.0,
                text: comp.label().to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_width - CHECKBOX_SIZE - 8.0),
            });
        }

        // Estimated size.
        let est_y = dy + dialog_h - 70.0;
        let est_size = format_bytes(self.form_estimated_size());
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: est_y,
            text: format!("Estimated size: {}", est_size),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        // Buttons.
        let btn_y = dy + dialog_h - 40.0;
        // Cancel.
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 220.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
        // Create.
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 112.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_GREEN,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Create".to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
    }

    /// Render the confirm restore dialog.
    fn render_restore_dialog(&self, rt: &mut RenderTree, id: u64) {
        let dialog_w = 420.0;
        let dialog_h = 240.0;
        let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
        let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

        // Background.
        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Confirm Restore".to_string(),
            color: COLOR_YELLOW,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        // Warning.
        if let Some(snap) = self.manager.tree.get_snapshot(id) {
            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 44.0,
                text: format!("Restore to \"{}\"?", snap.name),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: "Warning: This will revert system state to this snapshot.".to_string(),
                color: COLOR_RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 94.0,
                text: format!("Components affected: {}", snap.component_count()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 114.0,
                text: format!("Size: {}", snap.size_display()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });

            // Tip: create a snapshot before restoring.
            rt.push(RenderCommand::FillRect {
                x: dx + PADDING,
                y: dy + 140.0,
                width: dialog_w - 2.0 * PADDING,
                height: 28.0,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });
            rt.push(RenderCommand::Text {
                x: dx + PADDING + 8.0,
                y: dy + 146.0,
                text: "Tip: A snapshot of current state will be created automatically.".to_string(),
                color: COLOR_LAVENDER,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
            });
        }

        // Buttons.
        let btn_y = dy + dialog_h - 40.0;
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 220.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 112.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_YELLOW,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Restore".to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
    }

    /// Render the confirm delete dialog.
    fn render_delete_dialog(&self, rt: &mut RenderTree, id: u64) {
        let dialog_w = 380.0;
        let dialog_h = 180.0;
        let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
        let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Delete Snapshot?".to_string(),
            color: COLOR_RED,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        if let Some(snap) = self.manager.tree.get_snapshot(id) {
            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 44.0,
                text: format!("Delete \"{}\"? This cannot be undone.", snap.name),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });
            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: format!("This will free {}.", snap.size_display()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
            });
        }

        let btn_y = dy + dialog_h - 40.0;
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 220.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 112.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_RED,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Delete".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
    }

    /// Render the export dialog.
    fn render_export_dialog(&self, rt: &mut RenderTree) {
        let dialog_w = 400.0;
        let dialog_h = 200.0;
        let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
        let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Export Snapshots".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: format!("Export {} snapshot(s) to file.", self.manager.tree.count()),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        // Path field.
        rt.push(RenderCommand::FillRect {
            x: dx + PADDING,
            y: dy + 80.0,
            width: dialog_w - 2.0 * PADDING,
            height: 28.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: dy + 86.0,
            text: "/system/backups/snapshots.txt".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
        });

        let btn_y = dy + dialog_h - 40.0;
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 220.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 112.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_PEACH,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Export".to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
    }

    /// Render the import dialog.
    fn render_import_dialog(&self, rt: &mut RenderTree) {
        let dialog_w = 400.0;
        let dialog_h = 200.0;
        let dx = (WINDOW_WIDTH - dialog_w) / 2.0;
        let dy = (WINDOW_HEIGHT - dialog_h) / 2.0;

        rt.push(RenderCommand::FillRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_BASE,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: COLOR_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Import Snapshots".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: "Import snapshot metadata from file.".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
        });

        // Path field.
        rt.push(RenderCommand::FillRect {
            x: dx + PADDING,
            y: dy + 80.0,
            width: dialog_w - 2.0 * PADDING,
            height: 28.0,
            color: COLOR_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: dy + 86.0,
            text: "Select file...".to_string(),
            color: COLOR_OVERLAY0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
        });

        let btn_y = dy + dialog_h - 40.0;
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 220.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_SURFACE1,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
        rt.push(RenderCommand::FillRect {
            x: dx + dialog_w - 112.0,
            y: btn_y,
            width: BUTTON_WIDTH,
            height: BUTTON_HEIGHT,
            color: COLOR_BLUE,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Import".to_string(),
            color: COLOR_BASE,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(BUTTON_WIDTH - 16.0),
        });
    }

    /// Render the progress overlay.
    fn render_progress_overlay(&self, rt: &mut RenderTree) {
        if let Some(progress) = &self.progress {
            let overlay_w = 400.0;
            let overlay_h = 140.0;
            let ox = (WINDOW_WIDTH - overlay_w) / 2.0;
            let oy = (WINDOW_HEIGHT - overlay_h) / 2.0;

            // Dim background.
            rt.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
                color: Color::rgba(0, 0, 0, 180),
                corner_radii: CornerRadii::ZERO,
            });

            // Panel.
            rt.push(RenderCommand::FillRect {
                x: ox,
                y: oy,
                width: overlay_w,
                height: overlay_h,
                color: COLOR_BASE,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            rt.push(RenderCommand::StrokeRect {
                x: ox,
                y: oy,
                width: overlay_w,
                height: overlay_h,
                color: COLOR_SURFACE1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Step text.
            rt.push(RenderCommand::Text {
                x: ox + PADDING,
                y: oy + PADDING,
                text: progress.current_step.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(overlay_w - 2.0 * PADDING),
            });

            // Progress bar background.
            let bar_y = oy + 50.0;
            rt.push(RenderCommand::FillRect {
                x: ox + PADDING,
                y: bar_y,
                width: overlay_w - 2.0 * PADDING,
                height: PROGRESS_BAR_HEIGHT,
                color: COLOR_SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            // Progress bar fill.
            let fill_width = (overlay_w - 2.0 * PADDING) * progress.fraction();
            if fill_width > 0.0 {
                let bar_color = if progress.error.is_some() {
                    COLOR_RED
                } else {
                    COLOR_BLUE
                };
                rt.push(RenderCommand::FillRect {
                    x: ox + PADDING,
                    y: bar_y,
                    width: fill_width,
                    height: PROGRESS_BAR_HEIGHT,
                    color: bar_color,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Percentage.
            rt.push(RenderCommand::Text {
                x: ox + overlay_w / 2.0 - 15.0,
                y: bar_y + 3.0,
                text: format!("{}%", progress.percentage()),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
            });

            // Step counter.
            rt.push(RenderCommand::Text {
                x: ox + PADDING,
                y: bar_y + 28.0,
                text: format!(
                    "Step {} of {} | {}",
                    progress.step_index,
                    progress.total_steps,
                    format_bytes(progress.bytes_processed),
                ),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(overlay_w - 2.0 * PADDING),
            });

            // Error message if any.
            if let Some(err) = &progress.error {
                rt.push(RenderCommand::Text {
                    x: ox + PADDING,
                    y: oy + overlay_h - 24.0,
                    text: err.clone(),
                    color: COLOR_RED,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(overlay_w - 2.0 * PADDING),
                });
            }
        }
    }
}

impl Default for SystemRestoreUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Format bytes to a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a duration in seconds to a short human-readable string.
fn format_duration_short(secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 3600;
    const DAY: u64 = 86_400;

    if secs >= DAY {
        let days = secs / DAY;
        if days == 1 {
            "1 day".to_string()
        } else {
            format!("{} days", days)
        }
    } else if secs >= HOUR {
        let hours = secs / HOUR;
        if hours == 1 {
            "1 hour".to_string()
        } else {
            format!("{} hours", hours)
        }
    } else if secs >= MINUTE {
        let mins = secs / MINUTE;
        if mins == 1 {
            "1 minute".to_string()
        } else {
            format!("{} minutes", mins)
        }
    } else if secs == 1 {
        "1 second".to_string()
    } else {
        format!("{} seconds", secs)
    }
}

/// Format a timestamp to a short display string (day offset from epoch).
fn format_timestamp_short(ts: u64) -> String {
    let day = ts / 86_400;
    format!("D{}", day)
}

// ============================================================================
// main
// ============================================================================

fn main() {
    let ui = SystemRestoreUI::new();
    let rt = ui.render();
    // In the real OS, the render tree would be sent to the compositor.
    // For now just confirm it produced output.
    let _ = rt.len();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- SnapshotType tests ---

    #[test]
    fn test_snapshot_type_label() {
        assert_eq!(SnapshotType::Manual.label(), "Manual");
        assert_eq!(SnapshotType::Automatic.label(), "Automatic");
        assert_eq!(SnapshotType::PreUpdate.label(), "Pre-Update");
        assert_eq!(SnapshotType::PreInstall.label(), "Pre-Install");
        assert_eq!(SnapshotType::Scheduled.label(), "Scheduled");
    }

    #[test]
    fn test_snapshot_type_from_label() {
        assert_eq!(
            SnapshotType::from_label("Manual"),
            Some(SnapshotType::Manual)
        );
        assert_eq!(
            SnapshotType::from_label("automatic"),
            Some(SnapshotType::Automatic)
        );
        assert_eq!(
            SnapshotType::from_label("Pre-Update"),
            Some(SnapshotType::PreUpdate)
        );
        assert_eq!(
            SnapshotType::from_label("preinstall"),
            Some(SnapshotType::PreInstall)
        );
        assert_eq!(
            SnapshotType::from_label("scheduled"),
            Some(SnapshotType::Scheduled)
        );
        assert_eq!(SnapshotType::from_label("unknown"), None);
    }

    #[test]
    fn test_snapshot_type_all() {
        assert_eq!(SnapshotType::all().len(), 5);
    }

    #[test]
    fn test_snapshot_type_display() {
        assert_eq!(format!("{}", SnapshotType::Manual), "Manual");
    }

    #[test]
    fn test_snapshot_type_indicator_colors_unique() {
        let types = SnapshotType::all();
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i].indicator_color(),
                    types[j].indicator_color(),
                    "Types {:?} and {:?} should have different colors",
                    types[i],
                    types[j],
                );
            }
        }
    }

    // --- SnapshotComponent tests ---

    #[test]
    fn test_component_label() {
        assert_eq!(SnapshotComponent::SystemFiles.label(), "System Files");
        assert_eq!(SnapshotComponent::SecurityPolicy.label(), "Security Policy");
    }

    #[test]
    fn test_component_from_label() {
        assert_eq!(
            SnapshotComponent::from_label("System Files"),
            Some(SnapshotComponent::SystemFiles),
        );
        assert_eq!(
            SnapshotComponent::from_label("bootconfig"),
            Some(SnapshotComponent::BootConfig),
        );
        assert_eq!(SnapshotComponent::from_label("nope"), None);
    }

    #[test]
    fn test_component_estimated_size() {
        assert!(SnapshotComponent::SystemFiles.estimated_size_bytes() > 0);
        assert!(
            SnapshotComponent::InstalledApps.estimated_size_bytes()
                > SnapshotComponent::BootConfig.estimated_size_bytes()
        );
    }

    #[test]
    fn test_component_all() {
        assert_eq!(SnapshotComponent::all().len(), 10);
    }

    #[test]
    fn test_component_default_set() {
        let defaults = SnapshotComponent::default_set();
        assert!(!defaults.is_empty());
        assert!(defaults.contains(&SnapshotComponent::SystemFiles));
        assert!(defaults.contains(&SnapshotComponent::BootConfig));
    }

    #[test]
    fn test_component_display() {
        assert_eq!(
            format!("{}", SnapshotComponent::UserSettings),
            "User Settings"
        );
    }

    // --- Snapshot tests ---

    #[test]
    fn test_snapshot_new() {
        let snap = Snapshot::new(
            1,
            "Test",
            "A test",
            1000,
            SnapshotType::Manual,
            vec![SnapshotComponent::SystemFiles],
            None,
        );
        assert_eq!(snap.id, 1);
        assert_eq!(snap.name, "Test");
        assert_eq!(snap.parent_id, None);
        assert!(!snap.locked);
        assert!(snap.tags.is_empty());
    }

    #[test]
    fn test_snapshot_size_calculated() {
        let snap = Snapshot::new(
            1,
            "Test",
            "",
            0,
            SnapshotType::Manual,
            vec![
                SnapshotComponent::BootConfig,
                SnapshotComponent::NetworkConfig,
            ],
            None,
        );
        let expected = SnapshotComponent::BootConfig.estimated_size_bytes()
            + SnapshotComponent::NetworkConfig.estimated_size_bytes();
        assert_eq!(snap.size_bytes, expected);
    }

    #[test]
    fn test_snapshot_size_display() {
        let snap = Snapshot::new(
            1,
            "Test",
            "",
            0,
            SnapshotType::Manual,
            vec![SnapshotComponent::SystemFiles],
            None,
        );
        let display = snap.size_display();
        assert!(display.contains("GB") || display.contains("MB"));
    }

    #[test]
    fn test_snapshot_age_display() {
        let snap = Snapshot::new(1, "Test", "", 1000, SnapshotType::Manual, vec![], None);
        assert_eq!(snap.age_display(1000), "just now");
        assert_eq!(snap.age_display(500), "just now");
        let age = snap.age_display(1000 + 86_400 * 3);
        assert!(age.contains("3 days"));
    }

    #[test]
    fn test_snapshot_has_component() {
        let snap = Snapshot::new(
            1,
            "Test",
            "",
            0,
            SnapshotType::Manual,
            vec![
                SnapshotComponent::SystemFiles,
                SnapshotComponent::BootConfig,
            ],
            None,
        );
        assert!(snap.has_component(SnapshotComponent::SystemFiles));
        assert!(!snap.has_component(SnapshotComponent::DesktopConfig));
    }

    #[test]
    fn test_snapshot_component_count() {
        let snap = Snapshot::new(
            1,
            "Test",
            "",
            0,
            SnapshotType::Manual,
            vec![
                SnapshotComponent::SystemFiles,
                SnapshotComponent::BootConfig,
            ],
            None,
        );
        assert_eq!(snap.component_count(), 2);
    }

    // --- SnapshotTree tests ---

    #[test]
    fn test_tree_new_empty() {
        let tree = SnapshotTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.count(), 0);
    }

    #[test]
    fn test_tree_add_root_snapshot() {
        let mut tree = SnapshotTree::new();
        let id = tree.add_snapshot("Root", "", 100, SnapshotType::Manual, vec![], None);
        assert!(id.is_ok());
        assert_eq!(tree.count(), 1);
        assert!(!tree.is_empty());
    }

    #[test]
    fn test_tree_add_child_snapshot() {
        let mut tree = SnapshotTree::new();
        let root_id = tree
            .add_snapshot("Root", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let child_id = tree
            .add_snapshot(
                "Child",
                "",
                200,
                SnapshotType::Manual,
                vec![],
                Some(root_id),
            )
            .unwrap();
        assert_eq!(tree.count(), 2);
        assert_eq!(tree.children_of(root_id), &[child_id]);
    }

    #[test]
    fn test_tree_add_child_invalid_parent() {
        let mut tree = SnapshotTree::new();
        let result = tree.add_snapshot("Orphan", "", 100, SnapshotType::Manual, vec![], Some(999));
        assert_eq!(result, Err(SnapshotError::ParentNotFound(999)));
    }

    #[test]
    fn test_tree_remove_leaf_snapshot() {
        let mut tree = SnapshotTree::new();
        let id = tree
            .add_snapshot("Leaf", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let removed = tree.remove_snapshot(id);
        assert!(removed.is_ok());
        assert!(tree.is_empty());
    }

    #[test]
    fn test_tree_remove_nonexistent() {
        let mut tree = SnapshotTree::new();
        assert_eq!(tree.remove_snapshot(999), Err(SnapshotError::NotFound(999)));
    }

    #[test]
    fn test_tree_remove_with_children_fails() {
        let mut tree = SnapshotTree::new();
        let root_id = tree
            .add_snapshot("Root", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let _child = tree
            .add_snapshot(
                "Child",
                "",
                200,
                SnapshotType::Manual,
                vec![],
                Some(root_id),
            )
            .unwrap();
        assert_eq!(
            tree.remove_snapshot(root_id),
            Err(SnapshotError::HasChildren(root_id))
        );
    }

    #[test]
    fn test_tree_remove_locked_fails() {
        let mut tree = SnapshotTree::new();
        let id = tree
            .add_snapshot("Locked", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        tree.lock_snapshot(id).unwrap();
        assert_eq!(tree.remove_snapshot(id), Err(SnapshotError::Locked(id)));
    }

    #[test]
    fn test_tree_root_ids() {
        let mut tree = SnapshotTree::new();
        let r1 = tree
            .add_snapshot("R1", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let r2 = tree
            .add_snapshot("R2", "", 200, SnapshotType::Manual, vec![], None)
            .unwrap();
        let _c = tree
            .add_snapshot("C", "", 300, SnapshotType::Manual, vec![], Some(r1))
            .unwrap();
        let roots = tree.root_ids();
        assert!(roots.contains(&r1));
        assert!(roots.contains(&r2));
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn test_tree_all_ids_by_timestamp() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot("B", "", 200, SnapshotType::Manual, vec![], None)
            .unwrap();
        let _ = tree
            .add_snapshot("A", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let _ = tree
            .add_snapshot("C", "", 300, SnapshotType::Manual, vec![], None)
            .unwrap();
        let ids = tree.all_ids_by_timestamp();
        // Should be sorted by timestamp.
        let timestamps: Vec<u64> = ids
            .iter()
            .filter_map(|&id| tree.get_snapshot(id).map(|s| s.timestamp))
            .collect();
        assert_eq!(timestamps, vec![100, 200, 300]);
    }

    #[test]
    fn test_tree_depth_of() {
        let mut tree = SnapshotTree::new();
        let r = tree
            .add_snapshot("R", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let c = tree
            .add_snapshot("C", "", 200, SnapshotType::Manual, vec![], Some(r))
            .unwrap();
        let gc = tree
            .add_snapshot("GC", "", 300, SnapshotType::Manual, vec![], Some(c))
            .unwrap();
        assert_eq!(tree.depth_of(r), 0);
        assert_eq!(tree.depth_of(c), 1);
        assert_eq!(tree.depth_of(gc), 2);
    }

    #[test]
    fn test_tree_ancestry_chain() {
        let mut tree = SnapshotTree::new();
        let r = tree
            .add_snapshot("R", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let c = tree
            .add_snapshot("C", "", 200, SnapshotType::Manual, vec![], Some(r))
            .unwrap();
        let gc = tree
            .add_snapshot("GC", "", 300, SnapshotType::Manual, vec![], Some(c))
            .unwrap();
        assert_eq!(tree.ancestry_chain(gc), vec![r, c, gc]);
        assert_eq!(tree.ancestry_chain(r), vec![r]);
    }

    #[test]
    fn test_tree_flatten_for_display() {
        let mut tree = SnapshotTree::new();
        let r = tree
            .add_snapshot("R", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let c1 = tree
            .add_snapshot("C1", "", 200, SnapshotType::Manual, vec![], Some(r))
            .unwrap();
        let c2 = tree
            .add_snapshot("C2", "", 300, SnapshotType::Manual, vec![], Some(r))
            .unwrap();
        let flat = tree.flatten_for_display();
        assert_eq!(flat, vec![(r, 0), (c1, 1), (c2, 1)]);
    }

    #[test]
    fn test_tree_lock_unlock() {
        let mut tree = SnapshotTree::new();
        let id = tree
            .add_snapshot("S", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        assert!(!tree.get_snapshot(id).unwrap().locked);
        tree.lock_snapshot(id).unwrap();
        assert!(tree.get_snapshot(id).unwrap().locked);
        tree.unlock_snapshot(id).unwrap();
        assert!(!tree.get_snapshot(id).unwrap().locked);
    }

    #[test]
    fn test_tree_tags() {
        let mut tree = SnapshotTree::new();
        let id = tree
            .add_snapshot("S", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        tree.add_tag(id, "important").unwrap();
        tree.add_tag(id, "release").unwrap();
        tree.add_tag(id, "important").unwrap(); // Duplicate, should not add.
        assert_eq!(tree.get_snapshot(id).unwrap().tags.len(), 2);
        tree.remove_tag(id, "important").unwrap();
        assert_eq!(tree.get_snapshot(id).unwrap().tags.len(), 1);
        assert_eq!(tree.get_snapshot(id).unwrap().tags[0], "release");
    }

    #[test]
    fn test_tree_search() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot(
                "Weekly Backup",
                "auto backup",
                100,
                SnapshotType::Scheduled,
                vec![],
                None,
            )
            .unwrap();
        let _ = tree
            .add_snapshot(
                "Manual Save",
                "before update",
                200,
                SnapshotType::Manual,
                vec![],
                None,
            )
            .unwrap();
        let results = tree.search("backup");
        assert_eq!(results.len(), 1);
        let results = tree.search("MANUAL");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_tree_filter_by_type() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot("A", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let _ = tree
            .add_snapshot("B", "", 200, SnapshotType::Scheduled, vec![], None)
            .unwrap();
        let _ = tree
            .add_snapshot("C", "", 300, SnapshotType::Manual, vec![], None)
            .unwrap();
        assert_eq!(tree.filter_by_type(SnapshotType::Manual).len(), 2);
        assert_eq!(tree.filter_by_type(SnapshotType::Scheduled).len(), 1);
        assert_eq!(tree.filter_by_type(SnapshotType::PreUpdate).len(), 0);
    }

    #[test]
    fn test_tree_filter_by_component() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot(
                "A",
                "",
                100,
                SnapshotType::Manual,
                vec![SnapshotComponent::BootConfig],
                None,
            )
            .unwrap();
        let _ = tree
            .add_snapshot(
                "B",
                "",
                200,
                SnapshotType::Manual,
                vec![SnapshotComponent::SystemFiles],
                None,
            )
            .unwrap();
        assert_eq!(
            tree.filter_by_component(SnapshotComponent::BootConfig)
                .len(),
            1
        );
        assert_eq!(
            tree.filter_by_component(SnapshotComponent::SystemFiles)
                .len(),
            1
        );
        assert_eq!(
            tree.filter_by_component(SnapshotComponent::DesktopConfig)
                .len(),
            0
        );
    }

    #[test]
    fn test_tree_total_size() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot(
                "A",
                "",
                100,
                SnapshotType::Manual,
                vec![SnapshotComponent::BootConfig],
                None,
            )
            .unwrap();
        let _ = tree
            .add_snapshot(
                "B",
                "",
                200,
                SnapshotType::Manual,
                vec![SnapshotComponent::NetworkConfig],
                None,
            )
            .unwrap();
        let expected = SnapshotComponent::BootConfig.estimated_size_bytes()
            + SnapshotComponent::NetworkConfig.estimated_size_bytes();
        assert_eq!(tree.total_size_bytes(), expected);
    }

    #[test]
    fn test_tree_branching() {
        let mut tree = SnapshotTree::new();
        let root = tree
            .add_snapshot("Root", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let b1 = tree
            .add_snapshot("Branch1", "", 200, SnapshotType::Manual, vec![], Some(root))
            .unwrap();
        let b2 = tree
            .add_snapshot("Branch2", "", 300, SnapshotType::Manual, vec![], Some(root))
            .unwrap();
        let _b1c = tree
            .add_snapshot("B1Child", "", 400, SnapshotType::Manual, vec![], Some(b1))
            .unwrap();
        assert_eq!(tree.children_of(root).len(), 2);
        assert!(tree.children_of(root).contains(&b1));
        assert!(tree.children_of(root).contains(&b2));
    }

    #[test]
    fn test_tree_remove_updates_parent_children() {
        let mut tree = SnapshotTree::new();
        let root = tree
            .add_snapshot("Root", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        let child = tree
            .add_snapshot("Child", "", 200, SnapshotType::Manual, vec![], Some(root))
            .unwrap();
        tree.remove_snapshot(child).unwrap();
        assert!(tree.children_of(root).is_empty());
    }

    // --- DiffEntry tests ---

    #[test]
    fn test_diff_entry_category() {
        assert_eq!(
            DiffEntry::ComponentAdded(SnapshotComponent::BootConfig).category(),
            "Components"
        );
        assert_eq!(DiffEntry::FileAdded("test".to_string()).category(), "Files");
        assert_eq!(
            DiffEntry::SettingChanged {
                key: "k".into(),
                old_value: "a".into(),
                new_value: "b".into()
            }
            .category(),
            "Settings",
        );
        assert_eq!(
            DiffEntry::PackageInstalled("pkg".to_string()).category(),
            "Packages"
        );
    }

    #[test]
    fn test_diff_entry_classifications() {
        assert!(DiffEntry::ComponentAdded(SnapshotComponent::BootConfig).is_addition());
        assert!(DiffEntry::FileRemoved("f".into()).is_removal());
        assert!(DiffEntry::FileModified("f".into()).is_modification());
        assert!(!DiffEntry::FileAdded("f".into()).is_removal());
        assert!(!DiffEntry::FileRemoved("f".into()).is_addition());
    }

    #[test]
    fn test_diff_entry_summary() {
        let entry = DiffEntry::PackageUpdated {
            name: "foo".into(),
            old_version: "1.0".into(),
            new_version: "2.0".into(),
        };
        let summary = entry.summary();
        assert!(summary.contains("foo"));
        assert!(summary.contains("1.0"));
        assert!(summary.contains("2.0"));
    }

    #[test]
    fn test_diff_result_counts() {
        let diff = SnapshotDiffResult {
            older_id: 1,
            newer_id: 2,
            entries: vec![
                DiffEntry::FileAdded("a".into()),
                DiffEntry::FileRemoved("b".into()),
                DiffEntry::FileModified("c".into()),
                DiffEntry::PackageInstalled("d".into()),
            ],
        };
        assert_eq!(diff.addition_count(), 2);
        assert_eq!(diff.removal_count(), 1);
        assert_eq!(diff.modification_count(), 1);
        assert_eq!(diff.total_changes(), 4);
        assert!(!diff.is_empty());
    }

    #[test]
    fn test_diff_result_by_category() {
        let diff = SnapshotDiffResult {
            older_id: 1,
            newer_id: 2,
            entries: vec![
                DiffEntry::FileAdded("a".into()),
                DiffEntry::PackageInstalled("p".into()),
            ],
        };
        assert_eq!(diff.by_category("Files").len(), 1);
        assert_eq!(diff.by_category("Packages").len(), 1);
        assert_eq!(diff.by_category("Settings").len(), 0);
    }

    // --- ScheduleFrequency tests ---

    #[test]
    fn test_frequency_label() {
        assert_eq!(ScheduleFrequency::Daily.label(), "Daily");
        assert_eq!(ScheduleFrequency::Weekly.label(), "Weekly");
        assert_eq!(ScheduleFrequency::Monthly.label(), "Monthly");
    }

    #[test]
    fn test_frequency_from_label() {
        assert_eq!(
            ScheduleFrequency::from_label("daily"),
            Some(ScheduleFrequency::Daily)
        );
        assert_eq!(
            ScheduleFrequency::from_label("WEEKLY"),
            Some(ScheduleFrequency::Weekly)
        );
        assert_eq!(ScheduleFrequency::from_label("nope"), None);
    }

    #[test]
    fn test_frequency_intervals() {
        assert_eq!(ScheduleFrequency::Daily.interval_secs(), 86_400);
        assert_eq!(ScheduleFrequency::Weekly.interval_secs(), 604_800);
        assert!(
            ScheduleFrequency::Monthly.interval_secs() > ScheduleFrequency::Weekly.interval_secs()
        );
    }

    // --- RetentionPolicy tests ---

    #[test]
    fn test_retention_unlimited() {
        let policy = RetentionPolicy::unlimited();
        assert!(!policy.has_count_limit());
        assert!(!policy.has_age_limit());
        assert!(!policy.has_size_limit());
    }

    #[test]
    fn test_retention_with_limits() {
        let policy = RetentionPolicy::new(5, 86_400 * 30, 10_000_000_000);
        assert!(policy.has_count_limit());
        assert!(policy.has_age_limit());
        assert!(policy.has_size_limit());
    }

    #[test]
    fn test_retention_prune_by_count() {
        let policy = RetentionPolicy::new(2, 0, 0);
        let snapshots = vec![
            (1, 100, 1000, false),
            (2, 200, 1000, false),
            (3, 300, 1000, false),
        ];
        let to_prune = policy.snapshots_to_prune(&snapshots, 400);
        assert_eq!(to_prune.len(), 1);
        assert!(to_prune.contains(&1)); // Oldest gets pruned.
    }

    #[test]
    fn test_retention_prune_by_age() {
        let policy = RetentionPolicy::new(0, 100, 0);
        let snapshots = vec![
            (1, 50, 1000, false),
            (2, 150, 1000, false),
            (3, 250, 1000, false),
        ];
        let to_prune = policy.snapshots_to_prune(&snapshots, 300);
        // Snapshot 1 is 250s old (> 100), snapshot 2 is 150s old (> 100).
        assert!(to_prune.contains(&1));
        assert!(to_prune.contains(&2));
        assert!(!to_prune.contains(&3));
    }

    #[test]
    fn test_retention_prune_by_size() {
        let policy = RetentionPolicy::new(0, 0, 2000);
        let snapshots = vec![
            (1, 100, 1000, false),
            (2, 200, 1000, false),
            (3, 300, 1000, false),
        ];
        let to_prune = policy.snapshots_to_prune(&snapshots, 400);
        // Total = 3000, limit 2000. Must prune 1000 worth.
        assert_eq!(to_prune.len(), 1);
        assert!(to_prune.contains(&1)); // Oldest first.
    }

    #[test]
    fn test_retention_locked_not_pruned() {
        let policy = RetentionPolicy::new(1, 0, 0);
        let snapshots = vec![
            (1, 100, 1000, true), // locked
            (2, 200, 1000, false),
            (3, 300, 1000, false),
        ];
        let to_prune = policy.snapshots_to_prune(&snapshots, 400);
        // Wants to keep 1. Locked snapshot is safe. Prune oldest non-locked.
        assert!(!to_prune.contains(&1));
        assert!(to_prune.contains(&2));
    }

    #[test]
    fn test_retention_summary() {
        let policy = RetentionPolicy::new(10, 86_400 * 30, 0);
        let summary = policy.summary();
        assert!(summary.contains("10 snapshots"));
        assert!(summary.contains("30 days"));
    }

    // --- ScheduleConfig tests ---

    #[test]
    fn test_schedule_default_disabled() {
        let config = ScheduleConfig::default();
        assert!(!config.enabled);
    }

    #[test]
    fn test_schedule_is_due() {
        let mut config = ScheduleConfig::new(
            ScheduleFrequency::Daily,
            vec![SnapshotComponent::SystemFiles],
        );
        config.last_snapshot_timestamp = 1000;
        assert!(!config.is_due(1000 + 86_399)); // Not yet.
        assert!(config.is_due(1000 + 86_400)); // Exactly due.
        assert!(config.is_due(1000 + 100_000)); // Overdue.
    }

    #[test]
    fn test_schedule_disabled_not_due() {
        let mut config = ScheduleConfig::new(
            ScheduleFrequency::Daily,
            vec![SnapshotComponent::SystemFiles],
        );
        config.enabled = false;
        config.last_snapshot_timestamp = 0;
        assert!(!config.is_due(1_000_000));
    }

    #[test]
    fn test_schedule_validate_empty_components() {
        let config = ScheduleConfig::new(ScheduleFrequency::Daily, vec![]);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_schedule_validate_with_components() {
        let config = ScheduleConfig::new(
            ScheduleFrequency::Daily,
            vec![SnapshotComponent::SystemFiles],
        );
        assert!(config.validate().is_ok());
    }

    // --- StorageStats tests ---

    #[test]
    fn test_storage_stats_empty() {
        let tree = SnapshotTree::new();
        let stats = StorageStats::from_tree(&tree);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.snapshot_count, 0);
        assert_eq!(stats.smallest_snapshot_bytes, 0);
    }

    #[test]
    fn test_storage_stats_computed() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot(
                "A",
                "",
                100,
                SnapshotType::Manual,
                vec![SnapshotComponent::BootConfig],
                None,
            )
            .unwrap();
        let _ = tree
            .add_snapshot(
                "B",
                "",
                200,
                SnapshotType::Scheduled,
                vec![SnapshotComponent::BootConfig],
                None,
            )
            .unwrap();
        let stats = StorageStats::from_tree(&tree);
        assert_eq!(stats.snapshot_count, 2);
        assert_eq!(
            stats.total_bytes,
            SnapshotComponent::BootConfig.estimated_size_bytes() * 2
        );
        assert!(stats.manual_bytes > 0);
        assert!(stats.auto_bytes > 0);
    }

    // --- SnapshotExport tests ---

    #[test]
    fn test_export_one() {
        let snap = Snapshot::new(
            42,
            "My Snap",
            "desc",
            1000,
            SnapshotType::Manual,
            vec![SnapshotComponent::BootConfig],
            None,
        );
        let exported = SnapshotExport::export_one(&snap);
        assert!(exported.contains("[snapshot]"));
        assert!(exported.contains("id=42"));
        assert!(exported.contains("name=My Snap"));
        assert!(exported.contains("type=Manual"));
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut tree = SnapshotTree::new();
        let _ = tree
            .add_snapshot(
                "Snap1",
                "First",
                100,
                SnapshotType::Manual,
                vec![
                    SnapshotComponent::SystemFiles,
                    SnapshotComponent::BootConfig,
                ],
                None,
            )
            .unwrap();
        let _ = tree
            .add_snapshot(
                "Snap2",
                "Second",
                200,
                SnapshotType::Scheduled,
                vec![SnapshotComponent::UserSettings],
                None,
            )
            .unwrap();

        let exported = SnapshotExport::export_all(&tree);
        let imported = SnapshotExport::import_all(&exported).unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0].name, "Snap1");
        assert_eq!(imported[1].name, "Snap2");
    }

    #[test]
    fn test_import_invalid_format() {
        let result = SnapshotExport::import_all("[snapshot]\nid=not_a_number");
        assert!(result.is_err());
    }

    #[test]
    fn test_import_empty() {
        let result = SnapshotExport::import_all("");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // --- SnapshotManager tests ---

    #[test]
    fn test_manager_create_delete() {
        let mut mgr = SnapshotManager::new();
        let id = mgr
            .create_snapshot("Test", "", 100, SnapshotType::Manual, vec![], None)
            .unwrap();
        assert_eq!(mgr.tree.count(), 1);
        mgr.delete_snapshot(id).unwrap();
        assert_eq!(mgr.tree.count(), 0);
    }

    #[test]
    fn test_manager_compare_snapshots() {
        let mut mgr = SnapshotManager::new();
        let id1 = mgr
            .create_snapshot(
                "Old",
                "",
                100,
                SnapshotType::Manual,
                vec![
                    SnapshotComponent::SystemFiles,
                    SnapshotComponent::BootConfig,
                ],
                None,
            )
            .unwrap();
        let id2 = mgr
            .create_snapshot(
                "New",
                "",
                100 + 86_400 * 3,
                SnapshotType::Manual,
                vec![
                    SnapshotComponent::SystemFiles,
                    SnapshotComponent::UserSettings,
                ],
                None,
            )
            .unwrap();
        let diff = mgr.compare_snapshots(id1, id2).unwrap();
        // UserSettings was added, BootConfig was removed.
        assert!(diff.entries.iter().any(|e| matches!(
            e,
            DiffEntry::ComponentAdded(SnapshotComponent::UserSettings)
        )));
        assert!(diff.entries.iter().any(|e| matches!(
            e,
            DiffEntry::ComponentRemoved(SnapshotComponent::BootConfig)
        )));
    }

    #[test]
    fn test_manager_compare_nonexistent() {
        let mgr = SnapshotManager::new();
        assert!(mgr.compare_snapshots(1, 2).is_err());
    }

    #[test]
    fn test_manager_check_schedule_not_due() {
        let mut mgr = SnapshotManager::new();
        mgr.schedule.enabled = true;
        mgr.schedule.frequency = ScheduleFrequency::Daily;
        mgr.schedule.last_snapshot_timestamp = 1000;
        mgr.schedule.components = vec![SnapshotComponent::SystemFiles];
        let result = mgr.check_schedule(1000 + 100);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_manager_check_schedule_due() {
        let mut mgr = SnapshotManager::new();
        mgr.schedule.enabled = true;
        mgr.schedule.frequency = ScheduleFrequency::Daily;
        mgr.schedule.last_snapshot_timestamp = 1000;
        mgr.schedule.components = vec![SnapshotComponent::SystemFiles];
        let result = mgr.check_schedule(1000 + 86_400);
        assert!(result.is_ok());
        let id = result.unwrap();
        assert!(id.is_some());
        assert_eq!(mgr.tree.count(), 1);
    }

    #[test]
    fn test_manager_apply_retention() {
        let mut mgr = SnapshotManager::new();
        for i in 0..5 {
            let _ = mgr
                .create_snapshot(
                    &format!("S{}", i),
                    "",
                    100 + i * 100,
                    SnapshotType::Scheduled,
                    vec![SnapshotComponent::BootConfig],
                    None,
                )
                .unwrap();
        }
        mgr.schedule.retention = RetentionPolicy::new(3, 0, 0);
        let pruned = mgr.apply_retention(1000);
        assert_eq!(pruned.len(), 2); // 5 - 3 = 2 pruned.
        assert_eq!(mgr.tree.count(), 3);
    }

    #[test]
    fn test_manager_import_snapshots() {
        let mut mgr = SnapshotManager::new();
        let text = "[snapshot]\nid=1\nname=Imported\ndescription=test\ntimestamp=500\ntype=Manual\nsize=1000\nparent=none\nlocked=false\ncomponents=Boot Config\ntags=imported";
        let ids = mgr.import_snapshots(text, 0).unwrap();
        assert_eq!(ids.len(), 1);
        let snap = mgr.tree.get_snapshot(ids[0]).unwrap();
        assert_eq!(snap.name, "Imported");
        assert!(snap.tags.contains(&"imported".to_string()));
    }

    #[test]
    fn test_manager_cleanup_suggestions_empty() {
        let mgr = SnapshotManager::new();
        let suggestions = mgr.cleanup_suggestions(1000);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_manager_storage_stats() {
        let mut mgr = SnapshotManager::new();
        let _ = mgr
            .create_snapshot(
                "S",
                "",
                100,
                SnapshotType::Manual,
                vec![SnapshotComponent::BootConfig],
                None,
            )
            .unwrap();
        let stats = mgr.storage_stats();
        assert_eq!(stats.snapshot_count, 1);
        assert!(stats.total_bytes > 0);
    }

    // --- OperationProgress tests ---

    #[test]
    fn test_progress_new_create() {
        let comps = vec![
            SnapshotComponent::SystemFiles,
            SnapshotComponent::BootConfig,
        ];
        let progress = OperationProgress::new_create(&comps);
        assert!(!progress.complete);
        assert_eq!(progress.step_index, 0);
        assert!(progress.total_bytes > 0);
    }

    #[test]
    fn test_progress_fraction_zero() {
        let progress = OperationProgress::new_create(&[SnapshotComponent::SystemFiles]);
        assert!(progress.fraction() < 0.01);
    }

    #[test]
    fn test_progress_advance_and_finish() {
        let mut progress = OperationProgress::new_create(&[SnapshotComponent::BootConfig]);
        progress.advance("Working...", 1_000_000);
        assert_eq!(progress.step_index, 1);
        assert_eq!(progress.bytes_processed, 1_000_000);
        progress.finish();
        assert!(progress.complete);
        assert_eq!(progress.percentage(), 100);
    }

    #[test]
    fn test_progress_fail() {
        let mut progress = OperationProgress::new_create(&[]);
        progress.fail("disk full");
        assert!(progress.error.is_some());
        assert_eq!(progress.error.as_deref(), Some("disk full"));
    }

    #[test]
    fn test_progress_simulate_create() {
        let comps = vec![
            SnapshotComponent::BootConfig,
            SnapshotComponent::NetworkConfig,
        ];
        let states = OperationProgress::simulate_create(&comps);
        assert!(states.len() >= 4); // initial + prepare + 2 comps + finalize + complete
        assert!(states.last().unwrap().complete);
    }

    #[test]
    fn test_progress_simulate_restore() {
        let snap = Snapshot::new(
            1,
            "S",
            "",
            100,
            SnapshotType::Manual,
            vec![SnapshotComponent::BootConfig],
            None,
        );
        let states = OperationProgress::simulate_restore(&snap);
        assert!(states.last().unwrap().complete);
    }

    // --- Utility function tests ---

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(1_099_511_627_776), "1.0 TB");
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(0), "0 seconds");
        assert_eq!(format_duration_short(1), "1 second");
        assert_eq!(format_duration_short(30), "30 seconds");
        assert_eq!(format_duration_short(60), "1 minute");
        assert_eq!(format_duration_short(120), "2 minutes");
        assert_eq!(format_duration_short(3600), "1 hour");
        assert_eq!(format_duration_short(86_400), "1 day");
        assert_eq!(format_duration_short(86_400 * 5), "5 days");
    }

    #[test]
    fn test_format_timestamp_short() {
        assert_eq!(format_timestamp_short(0), "D0");
        assert_eq!(format_timestamp_short(86_400), "D1");
        assert_eq!(format_timestamp_short(86_400 * 100), "D100");
    }

    // --- ViewMode tests ---

    #[test]
    fn test_view_mode_label() {
        assert_eq!(ViewMode::Tree.label(), "Tree");
        assert_eq!(ViewMode::Timeline.label(), "Timeline");
        assert_eq!(ViewMode::Compare.label(), "Compare");
        assert_eq!(ViewMode::Schedule.label(), "Schedule");
        assert_eq!(ViewMode::Storage.label(), "Storage");
    }

    #[test]
    fn test_view_mode_all() {
        assert_eq!(ViewMode::all().len(), 5);
    }

    // --- SystemRestoreUI tests ---

    #[test]
    fn test_ui_new_has_demo_data() {
        let ui = SystemRestoreUI::new();
        assert!(ui.manager.tree.count() >= 4);
        assert!(ui.selected_id.is_some());
    }

    #[test]
    fn test_ui_visible_ids_no_filter() {
        let ui = SystemRestoreUI::new();
        let ids = ui.visible_ids();
        assert!(!ids.is_empty());
    }

    #[test]
    fn test_ui_visible_ids_with_type_filter() {
        let mut ui = SystemRestoreUI::new();
        ui.type_filter = Some(SnapshotType::Manual);
        let ids = ui.visible_ids();
        for id in &ids {
            let snap = ui.manager.tree.get_snapshot(*id).unwrap();
            assert_eq!(snap.snapshot_type, SnapshotType::Manual);
        }
    }

    #[test]
    fn test_ui_visible_ids_with_search() {
        let mut ui = SystemRestoreUI::new();
        ui.search_query = "Update".to_string();
        let ids = ui.visible_ids();
        for id in &ids {
            let snap = ui.manager.tree.get_snapshot(*id).unwrap();
            let match_found = snap.name.to_ascii_lowercase().contains("update")
                || snap.description.to_ascii_lowercase().contains("update");
            assert!(match_found);
        }
    }

    #[test]
    fn test_ui_form_estimated_size() {
        let mut ui = SystemRestoreUI::new();
        // All selected.
        let full_size = ui.form_estimated_size();
        assert!(full_size > 0);
        // Deselect all.
        ui.form_components = vec![false; SnapshotComponent::all().len()];
        assert_eq!(ui.form_estimated_size(), 0);
    }

    #[test]
    fn test_ui_form_selected_components() {
        let mut ui = SystemRestoreUI::new();
        ui.form_components = vec![
            true, false, true, false, false, false, false, false, false, false,
        ];
        let selected = ui.form_selected_components();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], SnapshotComponent::SystemFiles);
        assert_eq!(selected[1], SnapshotComponent::InstalledApps);
    }

    #[test]
    fn test_ui_render_produces_commands() {
        let ui = SystemRestoreUI::new();
        let rt = ui.render();
        assert!(!rt.is_empty());
        // Should have a good number of render commands for the full UI.
        assert!(rt.len() > 30);
    }

    #[test]
    fn test_ui_render_with_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::CreateSnapshot;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_with_progress() {
        let mut ui = SystemRestoreUI::new();
        ui.progress = Some(OperationProgress::new_create(&[
            SnapshotComponent::BootConfig,
        ]));
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_timeline_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Timeline;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_compare_view_no_selection() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Compare;
        ui.compare_id = None;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_compare_view_with_selection() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Compare;
        let ids = ui.manager.tree.all_ids_by_timestamp();
        if ids.len() >= 2 {
            ui.selected_id = Some(ids[0]);
            ui.compare_id = Some(ids[1]);
        }
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_schedule_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Schedule;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_storage_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Storage;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_delete_dialog() {
        let mut ui = SystemRestoreUI::new();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmDelete(id);
        }
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_restore_dialog() {
        let mut ui = SystemRestoreUI::new();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmRestore(id);
        }
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_export_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::ExportDialog;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_import_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::ImportDialog;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_no_selection_details() {
        let mut ui = SystemRestoreUI::new();
        ui.selected_id = None;
        let rt = ui.render();
        assert!(!rt.is_empty());
    }

    // --- SnapshotError tests ---

    #[test]
    fn test_error_display() {
        assert_eq!(
            format!("{}", SnapshotError::NotFound(5)),
            "Snapshot 5 not found",
        );
        assert_eq!(
            format!("{}", SnapshotError::HasChildren(3)),
            "Snapshot 3 has children and cannot be deleted",
        );
        assert_eq!(
            format!("{}", SnapshotError::Locked(7)),
            "Snapshot 7 is locked",
        );
    }

    #[test]
    fn test_error_format_error() {
        let err = SnapshotError::FormatError("bad data".to_string());
        assert!(format!("{}", err).contains("bad data"));
    }

    #[test]
    fn test_error_invalid_schedule() {
        let err = SnapshotError::InvalidSchedule("empty".to_string());
        assert!(format!("{}", err).contains("empty"));
    }

    // --- Export with locked and tags ---

    #[test]
    fn test_export_locked_and_tags() {
        let mut snap = Snapshot::new(
            1,
            "Tagged",
            "with tags",
            100,
            SnapshotType::Manual,
            vec![],
            None,
        );
        snap.locked = true;
        snap.tags = vec!["important".to_string(), "v1".to_string()];
        let exported = SnapshotExport::export_one(&snap);
        assert!(exported.contains("locked=true"));
        assert!(exported.contains("tags=important,v1"));
    }

    // --- Manager export/import roundtrip ---

    #[test]
    fn test_manager_export_import_roundtrip() {
        let mut mgr = SnapshotManager::new();
        let _ = mgr
            .create_snapshot(
                "Backup",
                "full backup",
                1000,
                SnapshotType::Manual,
                vec![SnapshotComponent::SystemFiles],
                None,
            )
            .unwrap();
        let exported = mgr.export_all();

        let mut mgr2 = SnapshotManager::new();
        let ids = mgr2.import_snapshots(&exported, 0).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(mgr2.tree.get_snapshot(ids[0]).unwrap().name, "Backup");
    }
}
