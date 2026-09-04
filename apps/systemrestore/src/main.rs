//! systemrestore -- Slate OS System Restore / Snapshot Manager
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

use guitk::color::Color;
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::ratio;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

use std::collections::BTreeMap;
use std::fmt;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// How long one step of a create or restore is shown for.
///
/// The operation is simulated -- see `OperationProgress::simulate_restore` --
/// so this is the speed of the animation and not of any real work. Slow enough
/// to read the step name, fast enough that a restore of eight components does
/// not outlast the user's patience.
///
/// A `Duration` rather than a count of milliseconds, because the only thing it
/// is ever used as is a `Duration`.
const PROGRESS_STEP: Duration = Duration::from_millis(400);

/// How often the clock is re-read when nothing is running.
///
/// A minute, because `age_display` rounds to minutes: anything shorter redraws
/// an identical frame, anything longer leaves a countdown visibly stale.
const CLOCK_STEP: Duration = Duration::from_mins(1);

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
/// Room the details panel reserves for a snapshot's description row.
///
/// A one-line description is shorter than this; the rest of the panel was laid
/// out against this figure, so a short description keeps the original spacing.
const DESCRIPTION_ROW_HEIGHT: f32 = 20.0;

/// How far a snapshot description may wrap in the details panel.
///
/// The panel is a fixed [`DETAILS_PANEL_HEIGHT`] box with the ancestry chain
/// anchored to its *bottom*, so the running cursor above cannot grow without
/// bound. Measured from `panel_y`: name 24, description 20, metadata 20,
/// components 20, tags 18 — 102px of 160, with the chain row starting at 138.
/// That leaves 36px of slack, i.e. room for two extra 17px lines; two total is
/// the cap that still clears the chain with a line to spare. A description
/// longer than that is ellipsised, which `Paragraph::max_lines` marks so it
/// does not read as a complete sentence.
const DESCRIPTION_MAX_LINES: usize = 2;

/// How wide one link of the ancestry chain may be drawn.
const CHAIN_LINK_WIDTH: f32 = 150.0;
/// The `" > "` between two links, and the space it advances the cursor by.
const CHAIN_SEPARATOR_WIDTH: f32 = 20.0;
/// The gap after each link, so two links never touch.
const CHAIN_LINK_GAP: f32 = 4.0;
/// Marks a link that was cut, and the head of a chain that did not all fit.
const CHAIN_ELLIPSIS: &str = "...";

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

    /// Re-parent an existing snapshot, or detach it with `parent_id = None`.
    ///
    /// # Why this exists
    ///
    /// [`Self::add_snapshot`] can only accept a parent that already exists, so
    /// a caller restoring a whole tree from a file cannot always add snapshots
    /// in an order that satisfies it — a child may appear before its parent.
    /// Import does two passes instead: add everything detached, then link it up
    /// through this.
    ///
    /// # Cycles
    ///
    /// A cycle would hang the application, not merely corrupt it:
    /// [`Self::depth_of`] and [`Self::ancestry_chain`] walk parent links until
    /// they reach a root, so a loop never terminates. Because the parent comes
    /// from a file the user can edit, this rejects any link that would create
    /// one, and reports it as `ParentNotFound` — the parent is not reachable as
    /// an ancestor-free node, which is the same thing from the caller's side.
    ///
    /// # Errors
    ///
    /// `NotFound` if `id` is not in the tree, `ParentNotFound` if the parent is
    /// not in the tree or the link would create a cycle.
    pub fn set_parent(&mut self, id: u64, parent_id: Option<u64>) -> Result<(), SnapshotError> {
        if !self.snapshots.contains_key(&id) {
            return Err(SnapshotError::NotFound(id));
        }
        if let Some(pid) = parent_id {
            if pid == id || !self.snapshots.contains_key(&pid) {
                return Err(SnapshotError::ParentNotFound(pid));
            }
            // Walking up from the proposed parent must reach a root without
            // passing through `id`. Bounded by the tree size so a pre-existing
            // cycle cannot hang this check either.
            let mut cursor = Some(pid);
            for _ in 0..=self.snapshots.len() {
                match cursor {
                    None => break,
                    Some(c) if c == id => return Err(SnapshotError::ParentNotFound(pid)),
                    Some(c) => cursor = self.snapshots.get(&c).and_then(|s| s.parent_id),
                }
            }
            if cursor.is_some() {
                // Ran out of steps with links still to follow: the existing
                // chain is longer than the tree, i.e. already cyclic.
                return Err(SnapshotError::ParentNotFound(pid));
            }
        }

        let old_parent = self.snapshots.get(&id).and_then(|s| s.parent_id);
        if old_parent == parent_id {
            return Ok(());
        }
        if let Some(old) = old_parent
            && let Some(siblings) = self.children.get_mut(&old)
        {
            siblings.retain(|&cid| cid != id);
        }
        if let Some(snap) = self.snapshots.get_mut(&id) {
            snap.parent_id = parent_id;
        }
        if let Some(pid) = parent_id {
            self.children.entry(pid).or_default().push(id);
        }
        Ok(())
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
        // The removal *is* the existence check. It was written as a check
        // earlier in the function followed by an `expect` here, which is two
        // lookups that have to agree -- and a panic if they ever stop agreeing.
        self.snapshots
            .remove(&id)
            .ok_or(SnapshotError::NotFound(id))
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
        // Unstable: the key is `(timestamp, id)` and ids are unique, so no
        // two elements compare equal and there is no order to preserve.
        ids.sort_unstable();
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
    ///
    /// The walk is bounded by the number of snapshots. `add_snapshot` and
    /// `set_parent` both refuse links that would form a cycle, so the bound
    /// should be unreachable — but this runs in a GUI event loop, where an
    /// unbounded walk over a corrupt tree is not a wrong answer, it is a frozen
    /// application the user has to kill.
    pub fn depth_of(&self, id: u64) -> usize {
        let mut depth = 0usize;
        let mut current = id;
        for _ in 0..self.snapshots.len() {
            match self.snapshots.get(&current).and_then(|s| s.parent_id) {
                Some(pid) => {
                    // Bounded by the loop, which runs at most once per
                    // snapshot; saturating anyway, because the bound is a
                    // property of the caller rather than of this line.
                    depth = depth.saturating_add(1);
                    current = pid;
                }
                None => break,
            }
        }
        depth
    }

    /// Get the full ancestry chain from root to the given snapshot (inclusive).
    ///
    /// Bounded for the same reason as [`Self::depth_of`].
    pub fn ancestry_chain(&self, id: u64) -> Vec<u64> {
        let mut chain = Vec::new();
        let mut current = id;
        for _ in 0..=self.snapshots.len() {
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
                self.flatten_subtree(kid_id, depth.saturating_add(1), result);
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
                // The `>` above is the guard; `saturating_sub` states it in the
                // arithmetic rather than one line away from it.
                let excess = non_deleted.len().saturating_sub(self.max_count);
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
            avg_bytes_per_snapshot: total.checked_div(count as u64).unwrap_or(0),
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
        // Exported IDs cannot be reused — they may collide with snapshots
        // already in this tree — so every snapshot gets a fresh one and the
        // parent links are translated through this map.
        let mut remap: BTreeMap<u64, u64> = BTreeMap::new();

        // Pass 1: add every snapshot detached. `add_snapshot` only accepts a
        // parent that already exists, and nothing guarantees the file lists
        // parents before children, so linking has to wait for pass 2.
        for snap in &imported {
            let id = self.tree.add_snapshot(
                &snap.name,
                &snap.description,
                snap.timestamp.max(base_timestamp),
                snap.snapshot_type,
                snap.components.clone(),
                None,
            )?;
            remap.insert(snap.id, id);
            // `?` rather than `let _ =`. Neither can fail for an id
            // `add_snapshot` just returned, but discarding the result is how a
            // later change to that guarantee would silently unlock a snapshot
            // the user marked as protected — the lock is what stops retention
            // from pruning it.
            if snap.locked {
                self.tree.lock_snapshot(id)?;
            }
            for tag in &snap.tags {
                self.tree.add_tag(id, tag)?;
            }
            new_ids.push(id);
        }

        // Pass 2: restore the tree shape. This used to be dropped outright,
        // so an export/import round trip flattened every incremental snapshot
        // into a root — losing which full snapshot each one was taken against,
        // which is the information a restore needs.
        for snap in &imported {
            let Some(&new_id) = remap.get(&snap.id) else {
                continue;
            };
            let Some(old_parent) = snap.parent_id else {
                continue;
            };
            // A parent outside the file — an export of one sub-tree, or a
            // hand-edited file — leaves the snapshot as a root rather than
            // failing the import. Losing a snapshot's position in the tree is
            // recoverable; refusing the import loses the snapshot itself.
            if let Some(&new_parent) = remap.get(&old_parent) {
                self.tree.set_parent(new_id, Some(new_parent))?;
            }
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
                old_auto_count = old_auto_count.saturating_add(1);
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
    ///
    /// Measured in bytes where there are bytes to measure, in steps where
    /// there are not, and reported complete when there is neither — a restore
    /// of an empty snapshot has nothing left to do.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        ratio::fraction(self.bytes_processed, self.total_bytes)
            .or_else(|| ratio::fraction(self.step_index, self.total_steps))
            .unwrap_or(1.0) as f32
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

/// A control in the toolbar, and what pressing it does.
///
/// One law, two callers: [`SystemRestoreUI::toolbar_controls`] is what the
/// renderer draws and what the pointer hit-tests. Until it existed the renderer
/// walked private `tab_x` and `btn_x` accumulators, so nothing outside it knew
/// where the five view tabs or the four action buttons were -- and this program
/// had no pointer handling of any kind to want to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarControl {
    /// Switch to a view.
    Tab(ViewMode),
    /// Open the new-snapshot form.
    Create,
    /// Ask to restore the selected snapshot.
    Restore,
    /// Ask to delete the selected snapshot.
    Delete,
    /// Open the export dialog.
    Export,
}

/// Which text field of the new-snapshot form has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FormField {
    /// The snapshot's name.
    #[default]
    Name,
    /// Its description.
    Description,
}

/// A button along the bottom of a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    /// Go through with whatever the dialog is asking about.
    Confirm,
    /// Close the dialog and do nothing.
    Cancel,
}

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
    /// Which form field the keyboard is typing into.
    pub form_field: FormField,
    /// The remaining frames of a running operation.
    ///
    /// `OperationProgress::simulate_create` and `simulate_restore` each return
    /// the whole filmstrip at once; the tick shows one frame per step. Neither
    /// had a caller, so the progress overlay -- which the renderer draws in
    /// full, with a bar and a step name -- could never appear.
    pub pending_steps: std::collections::VecDeque<OperationProgress>,
    /// How wide the window is, in pixels.
    ///
    /// Every layout in this file used the `WINDOW_WIDTH` constant directly, so
    /// the program drew a 1050x700 picture whatever size window it was given:
    /// widen it and the status bar stopped short of the edge, narrow it and the
    /// action buttons hung off the side. The constants are the size the window
    /// asks for; these two are the size it got.
    pub window_width: f32,
    /// How tall the window is, in pixels.
    pub window_height: f32,
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
            form_field: FormField::Name,
            pending_steps: std::collections::VecDeque::new(),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
        }
    }

    /// Get the list of visible snapshot IDs based on current filters.
    pub fn visible_ids(&self) -> Vec<u64> {
        self.visible_rows().into_iter().map(|(id, _)| id).collect()
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
    /// Draw the whole window.
    ///
    /// Not `render`: [`App::render`] is the one the window calls, and an
    /// inherent method of the same name shadows a trait method at equal arity.
    pub fn render_tree(&self) -> RenderTree {
        let mut rt = RenderTree::new();

        // Background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
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

    /// Every snapshot the current view shows, with its depth in the tree.
    ///
    /// The filter -- the type filter and the search box -- is stated here and
    /// nowhere else. The tree view carried a second copy of it inline, which is
    /// the arrangement where one view starts disagreeing with the other about
    /// what the search box means; and the keyboard needs the same list to know
    /// what "the next snapshot" is.
    ///
    /// Depth is zero in the timeline view, which is a flat chronology and has
    /// no parent-child indentation to express.
    pub fn visible_rows(&self) -> Vec<(u64, usize)> {
        let all: Vec<(u64, usize)> = if self.view_mode == ViewMode::Timeline {
            self.manager
                .tree
                .all_ids_by_timestamp()
                .into_iter()
                .map(|id| (id, 0))
                .collect()
        } else {
            self.manager.tree.flatten_for_display()
        };

        all.into_iter()
            .filter(|&(id, _)| self.passes_filters(id))
            .collect()
    }

    /// Whether a snapshot survives the type filter and the search box.
    fn passes_filters(&self, id: u64) -> bool {
        let Some(snap) = self.manager.tree.get_snapshot(id) else {
            return false;
        };
        if let Some(filter_type) = self.type_filter
            && snap.snapshot_type != filter_type
        {
            return false;
        }
        if self.search_query.is_empty() {
            return true;
        }
        let q = self.search_query.to_ascii_lowercase();
        snap.name.to_ascii_lowercase().contains(&q)
            || snap.description.to_ascii_lowercase().contains(&q)
    }

    /// The top of the content area, below the header and the toolbar.
    fn content_top(&self) -> f32 {
        HEADER_HEIGHT + TOOLBAR_HEIGHT
    }

    /// How tall one row of the list is in the current view.
    fn row_height(&self) -> f32 {
        if self.view_mode == ViewMode::Timeline {
            TIMELINE_ENTRY_HEIGHT
        } else {
            TREE_ROW_HEIGHT
        }
    }

    /// Where each visible snapshot's row is drawn, and which snapshot it is.
    ///
    /// Rows scrolled out of the content area are left out rather than returned
    /// with an off-screen rectangle: a click cannot land on them, and returning
    /// them would make the hit test's answer depend on a clip it cannot see.
    pub fn row_rects(&self) -> Vec<(Rect, u64)> {
        let top = self.content_top();
        let bottom = self.window_height - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        let height = self.row_height();
        let first_y = if self.view_mode == ViewMode::Timeline {
            top + PADDING
        } else {
            top + SMALL_PADDING
        };

        let mut out = Vec::new();
        for (index, (id, _)) in self.visible_rows().into_iter().enumerate() {
            let y = first_y + index as f32 * height - self.scroll_offset;
            if y + height <= top || y >= bottom {
                continue;
            }
            out.push((
                Rect::new(PADDING, y, self.window_width - 2.0 * PADDING, height),
                id,
            ));
        }
        out
    }

    /// Where every toolbar control is drawn, and what it does.
    pub fn toolbar_controls(&self) -> Vec<(Rect, ToolbarControl)> {
        let toolbar_y = HEADER_HEIGHT;
        let mut out = Vec::with_capacity(9);

        let mut tab_x = PADDING;
        for mode in ViewMode::all() {
            out.push((
                Rect::new(tab_x, toolbar_y + 5.0, 80.0, TOOLBAR_HEIGHT - 10.0),
                ToolbarControl::Tab(*mode),
            ));
            tab_x += 84.0;
        }

        let actions = [
            ToolbarControl::Create,
            ToolbarControl::Restore,
            ToolbarControl::Delete,
            ToolbarControl::Export,
        ];
        let mut btn_x = self.window_width - (actions.len() as f32 * (BUTTON_WIDTH + 8.0)) - PADDING;
        for action in actions {
            out.push((
                Rect::new(btn_x, toolbar_y + 5.0, BUTTON_WIDTH, BUTTON_HEIGHT),
                action,
            ));
            btn_x += BUTTON_WIDTH + 8.0;
        }

        out
    }

    /// Where the open dialog's frame is, if one is open.
    ///
    /// Every dialog is centred and 480x320 apart from the create form, which is
    /// taller because it lists the components. The renderer computed those
    /// numbers five times over; this is the same arithmetic, once, so that a
    /// click can be told whether it landed on the dialog or on the window
    /// behind it.
    pub fn dialog_frame(&self) -> Option<Rect> {
        let (w, h) = match self.dialog {
            DialogKind::None => return None,
            DialogKind::CreateSnapshot => (500.0, 440.0),
            DialogKind::ConfirmRestore(_) => (420.0, 240.0),
            DialogKind::ConfirmDelete(_) => (380.0, 180.0),
            DialogKind::ExportDialog | DialogKind::ImportDialog => (400.0, 200.0),
        };
        Some(Rect::new(
            (self.window_width - w) / 2.0,
            (self.window_height - h) / 2.0,
            w,
            h,
        ))
    }

    /// Where the open dialog's buttons are.
    pub fn dialog_buttons(&self) -> Vec<(Rect, DialogButton)> {
        let Some(frame) = self.dialog_frame() else {
            return Vec::new();
        };
        // The offsets the five dialogs all draw with: `btn_y = dy + dialog_h -
        // 40`, cancel at `dialog_w - 220`, confirm at `dialog_w - 112`. Written
        // out here rather than derived from `PADDING`, because the numbers a
        // click has to match are the numbers the renderer used.
        let y = frame.y + frame.h - 40.0;
        let cancel_x = frame.x + frame.w - 220.0;
        let confirm_x = frame.x + frame.w - 112.0;
        vec![
            (
                Rect::new(cancel_x, y, BUTTON_WIDTH, BUTTON_HEIGHT),
                DialogButton::Cancel,
            ),
            (
                Rect::new(confirm_x, y, BUTTON_WIDTH, BUTTON_HEIGHT),
                DialogButton::Confirm,
            ),
        ]
    }

    // ====================================================================
    // Input
    //
    // This program had none. It drew five view tabs, four action buttons and
    // five dialogs, and there was no way to press any of them: no key handler,
    // no mouse handler, no `handle_event`. Fifteen of its methods -- including
    // `delete_snapshot`, `simulate_restore`, `check_schedule`,
    // `apply_retention` and `import_snapshots`, which is most of what it is
    // for -- had no caller outside the tests.
    // ====================================================================

    /// Handle one event from the window.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window wider than 16 million pixels does not exist"
                )]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                EventResult::Consumed
            }
            Event::Tick { .. } => self.handle_tick(),
            _ => EventResult::Ignored,
        }
    }

    /// One step of whatever is running, or one minute of the clock.
    ///
    /// The two are separate because they happen at different rates: a running
    /// operation steps every `PROGRESS_STEP_MS`, and the clock only needs
    /// re-reading once a minute. `tick_interval` returns whichever is current,
    /// so this arrives at the right rate for whichever is happening.
    fn handle_tick(&mut self) -> EventResult {
        if self.progress.is_some() {
            return self.advance_operation();
        }
        // Re-read rather than added to, so the clock survives a suspend and
        // does not drift: `Event::Tick` says how long the harness *intended* to
        // wait, and a laptop that was shut for an hour would otherwise wake up
        // an hour behind and take an hour to catch up, taking every scheduled
        // snapshot it had missed one minute apart.
        let Some(now) = system_now_secs() else {
            return EventResult::Ignored;
        };
        self.tick_to(now)
    }

    /// What a tick does, given the time. Separate from [`Self::handle_tick`]
    /// only because a function that reads the wall clock is a function no test
    /// can pin down.
    pub fn tick_to(&mut self, now: u64) -> EventResult {
        if now == self.current_timestamp {
            return EventResult::Ignored;
        }
        self.set_now(now);
        self.run_schedule(now);
        EventResult::Consumed
    }

    /// Take a scheduled snapshot if one is due, and prune by the retention
    /// policy.
    ///
    /// This is the program's whole purpose and nothing called it. A snapshot
    /// manager with automatic snapshots configured, a retention policy, a
    /// countdown to the next one on screen, and no clock to run any of it.
    fn run_schedule(&mut self, now: u64) {
        // A failure to take a scheduled snapshot is the schedule being
        // misconfigured -- no components selected -- which the Schedule view
        // already shows. Retrying every minute and reporting nothing is what
        // any scheduler does with a job it cannot run.
        if let Ok(Some(id)) = self.manager.check_schedule(now) {
            self.selected_id = Some(id);
        }
        // After, not before: a snapshot taken this minute must be in the tree
        // when the retention policy counts how many there are, or a policy of
        // "keep 5" would keep 5 and then admit a 6th.
        let pruned = self.manager.apply_retention(now);
        if self.selected_id.is_some_and(|id| pruned.contains(&id)) {
            self.selected_id = None;
        }
    }

    /// Show the next state of the running operation, or finish it.
    fn advance_operation(&mut self) -> EventResult {
        if self.progress.is_none() {
            return EventResult::Ignored;
        }
        // Running out of frames is the end, and it is the *only* end. A test
        // for `progress.complete` used to stand in front of this, on the theory
        // that a finished operation should close on the frame that says so --
        // but the finished frame is the last one in the filmstrip, so the queue
        // is empty on exactly the tick that check would have fired, and a
        // mutation that deleted the check changed nothing anyone could see. Two
        // conditions for one event is one condition that is never the reason.
        let Some(next) = self.pending_steps.pop_front() else {
            self.progress = None;
            return EventResult::Consumed;
        };
        self.progress = Some(next);
        EventResult::Consumed
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if self.progress.is_some() {
            // An operation is running and the window is showing a progress
            // overlay over everything. Escape abandons it; nothing else reaches
            // through.
            if key.key == Key::Escape {
                self.progress = None;
                self.pending_steps.clear();
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        if self.dialog != DialogKind::None {
            return self.handle_dialog_key(key);
        }

        if key.modifiers.ctrl {
            return match key.key {
                Key::N => {
                    self.open_create_dialog();
                    EventResult::Consumed
                }
                Key::E => {
                    self.dialog = DialogKind::ExportDialog;
                    EventResult::Consumed
                }
                Key::I => {
                    self.dialog = DialogKind::ImportDialog;
                    EventResult::Consumed
                }
                Key::L => {
                    self.toggle_lock();
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match key.key {
            Key::Tab => {
                self.cycle_view(if key.modifiers.shift { -1 } else { 1 });
                EventResult::Consumed
            }
            Key::Up => {
                self.move_selection(-1);
                EventResult::Consumed
            }
            Key::Down => {
                self.move_selection(1);
                EventResult::Consumed
            }
            Key::Home => {
                self.select_at(0);
                EventResult::Consumed
            }
            Key::End => {
                let rows = self.visible_rows();
                self.select_at(rows.len().saturating_sub(1));
                EventResult::Consumed
            }
            Key::Enter => {
                if let Some(id) = self.selected_id {
                    self.dialog = DialogKind::ConfirmRestore(id);
                }
                EventResult::Consumed
            }
            Key::Delete => {
                if let Some(id) = self.selected_id {
                    self.dialog = DialogKind::ConfirmDelete(id);
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                self.search_query.pop();
                self.reanchor_selection();
                EventResult::Consumed
            }
            Key::Escape => {
                // The search box is the only thing Escape can clear here, and
                // clearing it is the only way to get back to the whole list
                // once a query has hidden most of it.
                if self.search_query.is_empty() {
                    return EventResult::Ignored;
                }
                self.search_query.clear();
                self.reanchor_selection();
                EventResult::Consumed
            }
            _ => {
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                self.search_query.push_str(&typed);
                self.reanchor_selection();
                EventResult::Consumed
            }
        }
    }

    /// Keys while a dialog is open.
    fn handle_dialog_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.dialog = DialogKind::None;
                EventResult::Consumed
            }
            Key::Enter => {
                self.confirm_dialog();
                EventResult::Consumed
            }
            Key::Tab if self.dialog == DialogKind::CreateSnapshot => {
                self.form_field = match self.form_field {
                    FormField::Name => FormField::Description,
                    FormField::Description => FormField::Name,
                };
                EventResult::Consumed
            }
            Key::Backspace if self.dialog == DialogKind::CreateSnapshot => {
                self.form_text_mut().pop();
                EventResult::Consumed
            }
            _ if self.dialog == DialogKind::CreateSnapshot && !key.modifiers.ctrl => {
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                self.form_text_mut().push_str(&typed);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a mouse event.
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match mouse.kind {
            MouseEventKind::Press(MouseButton::Left) => self.handle_click(mouse.x, mouse.y),
            MouseEventKind::Scroll { dy, .. } => {
                // The toolkit's own notch-to-pixel conversion, scaled by the
                // row height of whichever view is showing, so the wheel travels
                // the same three rows here as it does over any other list --
                // and the same distance on the timeline, whose rows are taller.
                self.scroll_by(guitk::wheel::pixels(dy, self.row_height()));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a left click.
    fn handle_click(&mut self, x: f32, y: f32) -> EventResult {
        // A progress overlay covers the window: nothing behind it can be
        // clicked, and it has no buttons of its own.
        if self.progress.is_some() {
            return EventResult::Consumed;
        }

        if let Some(frame) = self.dialog_frame() {
            if frame.contains(x, y) {
                if let Some(button) = self
                    .dialog_buttons()
                    .into_iter()
                    .find(|(rect, _)| rect.contains(x, y))
                    .map(|(_, button)| button)
                {
                    match button {
                        DialogButton::Confirm => self.confirm_dialog(),
                        DialogButton::Cancel => self.dialog = DialogKind::None,
                    }
                }
                return EventResult::Consumed;
            }
            // A click outside a modal dialog dismisses it, and does not also
            // reach the window behind: the whole point of the dimmed backdrop
            // the renderer draws is that it is in the way.
            self.dialog = DialogKind::None;
            return EventResult::Consumed;
        }

        if let Some(control) = self
            .toolbar_controls()
            .into_iter()
            .find(|(rect, _)| rect.contains(x, y))
            .map(|(_, control)| control)
        {
            self.apply_toolbar_control(control);
            return EventResult::Consumed;
        }
        // The toolbar band is claimed even between controls, so a click on the
        // strip does not fall through to the list underneath it.
        if y >= HEADER_HEIGHT && y < HEADER_HEIGHT + TOOLBAR_HEIGHT {
            return EventResult::Consumed;
        }

        if let Some(id) = self
            .row_rects()
            .into_iter()
            .find(|(rect, _)| rect.contains(x, y))
            .map(|(_, id)| id)
        {
            // A second click on the already-selected snapshot picks it as the
            // other side of a comparison, which is the only way to fill
            // `compare_id` -- the Compare view had no way to be given two
            // snapshots and drew an empty frame for ever.
            if self.selected_id == Some(id) {
                self.compare_id = if self.compare_id == Some(id) {
                    None
                } else {
                    Some(id)
                };
            } else {
                self.selected_id = Some(id);
            }
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    /// Do what a toolbar control says.
    fn apply_toolbar_control(&mut self, control: ToolbarControl) {
        match control {
            ToolbarControl::Tab(mode) => {
                self.view_mode = mode;
                // The two views measure their rows differently, so an offset
                // carried across is a different number of rows down.
                self.scroll_offset = 0.0;
            }
            ToolbarControl::Create => self.open_create_dialog(),
            ToolbarControl::Restore => {
                if let Some(id) = self.selected_id {
                    self.dialog = DialogKind::ConfirmRestore(id);
                }
            }
            ToolbarControl::Delete => {
                if let Some(id) = self.selected_id {
                    self.dialog = DialogKind::ConfirmDelete(id);
                }
            }
            ToolbarControl::Export => self.dialog = DialogKind::ExportDialog,
        }
    }

    /// Go through with whatever the open dialog is asking about.
    fn confirm_dialog(&mut self) {
        match self.dialog {
            DialogKind::None => return,
            DialogKind::CreateSnapshot => self.begin_create(),
            DialogKind::ConfirmRestore(id) => self.begin_restore(id),
            DialogKind::ConfirmDelete(id) => self.delete(id),
            DialogKind::ExportDialog | DialogKind::ImportDialog => {
                // Both need a file, and this program has no file dialog. See
                // `known-issues.md` ->
                // `TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES`:
                // `export_snapshots` and `import_snapshots` work on a string
                // and there is nowhere for one to come from or go. Closing the
                // dialog is honest; pretending to write a file would not be.
            }
        }
        self.dialog = DialogKind::None;
    }

    /// Start creating a snapshot from the form.
    fn begin_create(&mut self) {
        let components = self.form_selected_components();
        if components.is_empty() {
            // Nothing to snapshot. The form shows the estimated size as zero,
            // which is the same statement.
            return;
        }
        let mut states = OperationProgress::simulate_create(&components).into_iter();
        let Some(first) = states.next() else {
            return;
        };
        self.progress = Some(first);
        self.pending_steps = states.collect();

        let name = if self.form_name.trim().is_empty() {
            format!("Snapshot {}", self.manager.tree.count().saturating_add(1))
        } else {
            self.form_name.clone()
        };
        if let Ok(id) = self.manager.create_snapshot(
            &name,
            &self.form_description,
            self.current_timestamp,
            self.form_type,
            components,
            self.form_parent_id,
        ) {
            self.selected_id = Some(id);
        }
    }

    /// Start restoring a snapshot.
    fn begin_restore(&mut self, id: u64) {
        let Some(snap) = self.manager.tree.get_snapshot(id) else {
            return;
        };
        let mut states = OperationProgress::simulate_restore(snap).into_iter();
        let Some(first) = states.next() else {
            return;
        };
        self.progress = Some(first);
        self.pending_steps = states.collect();
        self.selected_id = Some(id);
    }

    /// Delete a snapshot, and leave the selection somewhere real.
    fn delete(&mut self, id: u64) {
        if self.manager.delete_snapshot(id).is_err() {
            // Locked, or the root of a tree with children. The lock is shown on
            // the row and the error is what the lock is for.
            return;
        }
        if self.selected_id == Some(id) {
            self.selected_id = None;
        }
        if self.compare_id == Some(id) {
            self.compare_id = None;
        }
        self.reanchor_selection();
    }

    /// Lock or unlock the selected snapshot.
    ///
    /// A locked snapshot cannot be deleted or pruned by the retention policy,
    /// which is what the padlock on the row means. `unlock_snapshot` had no
    /// caller, so a snapshot locked by anything was locked for ever.
    fn toggle_lock(&mut self) {
        let Some(id) = self.selected_id else {
            return;
        };
        let locked = self
            .manager
            .tree
            .get_snapshot(id)
            .is_some_and(|snap| snap.locked);
        let result = if locked {
            self.manager.tree.unlock_snapshot(id)
        } else {
            self.manager.tree.lock_snapshot(id)
        };
        // Both fail only for an id that is not in the tree, which the line
        // above has just established is not the case.
        debug_assert!(result.is_ok(), "the id came from the tree");
        drop(result);
    }

    /// Open the new-snapshot form, aimed at the selected snapshot.
    fn open_create_dialog(&mut self) {
        self.dialog = DialogKind::CreateSnapshot;
        self.form_field = FormField::Name;
        self.form_name.clear();
        self.form_description.clear();
        // Branching from what is selected, which is what makes the tree a tree.
        // Left at `None` the form always added another root, and the branching
        // this program is built around could not be reached.
        self.form_parent_id = self.selected_id;
    }

    /// Move through the views. `delta` is in tabs, and it wraps.
    fn cycle_view(&mut self, delta: isize) {
        let modes = ViewMode::all();
        let count = modes.len();
        let current = modes.iter().position(|m| *m == self.view_mode).unwrap_or(0);
        // `rem_euclid` so that going back from the first view lands on the
        // last rather than on a negative index.
        // Every one of these is bounded by `modes.len()`, which is 5, so the
        // arithmetic cannot overflow -- but saying so in the operators is
        // cheaper than a comment nobody re-checks.
        let next = (current as isize)
            .saturating_add(delta)
            .rem_euclid(count as isize);
        if let Some(mode) = modes.get(next.unsigned_abs()) {
            self.view_mode = *mode;
            self.scroll_offset = 0.0;
        }
    }

    /// Move the selection by `delta` rows through what is on screen.
    fn move_selection(&mut self, delta: isize) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            self.selected_id = None;
            return;
        }
        // `rows` is non-empty -- the branch above returned -- so there is a
        // last index and it is not negative.
        let last = (rows.len() as isize).saturating_sub(1);
        let current = self
            .selected_id
            .and_then(|id| rows.iter().position(|(row_id, _)| *row_id == id));
        let next = match current {
            // Stopping at the ends rather than wrapping: a list that jumps from
            // the last snapshot to the first is one the user has to notice.
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            // Nothing selected: the first row for a downward move, the last for
            // an upward one, so both keys reach the list from outside it.
            None if delta < 0 => last,
            None => 0,
        };
        self.select_at(next.unsigned_abs());
    }

    /// Select the `index`th visible row, and scroll it into view.
    fn select_at(&mut self, index: usize) {
        let rows = self.visible_rows();
        let Some((id, _)) = rows.get(index) else {
            return;
        };
        self.selected_id = Some(*id);
        self.scroll_row_into_view(index);
    }

    /// Keep the selection on a snapshot that is still on screen.
    ///
    /// Called after anything that changes what is visible -- a search, a
    /// deletion, a retention sweep. Selection is held as an id rather than an
    /// index for exactly this reason: an index into a list that has just been
    /// re-filtered names a different snapshot than it did a moment ago.
    fn reanchor_selection(&mut self) {
        let rows = self.visible_rows();
        if self
            .selected_id
            .is_some_and(|id| rows.iter().any(|(row_id, _)| *row_id == id))
        {
            return;
        }
        self.selected_id = rows.first().map(|(id, _)| *id);
        self.scroll_offset = 0.0;
    }

    /// Scroll so that the `index`th row is inside the content area.
    fn scroll_row_into_view(&mut self, index: usize) {
        let height = self.row_height();
        let viewport =
            self.window_height - self.content_top() - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        let row_top = index as f32 * height;
        let row_bottom = row_top + height;
        if row_top < self.scroll_offset {
            self.scroll_offset = row_top;
        } else if row_bottom > self.scroll_offset + viewport {
            self.scroll_offset = row_bottom - viewport;
        }
        self.clamp_scroll();
    }

    /// Scroll the list, keeping the offset inside its range.
    fn scroll_by(&mut self, delta: f32) {
        self.scroll_offset += delta;
        self.clamp_scroll();
    }

    /// Keep the scroll offset between zero and the last screenful.
    fn clamp_scroll(&mut self) {
        let viewport =
            self.window_height - self.content_top() - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        let content = self.visible_rows().len() as f32 * self.row_height();
        // `max(0.0)` before the clamp: a list shorter than the viewport has a
        // negative maximum, and clamping to a negative upper bound is the one
        // shape `clamp` panics on.
        let max = (content - viewport).max(0.0);
        self.scroll_offset = self.scroll_offset.clamp(0.0, max);
    }

    /// The form field the keyboard is typing into.
    fn form_text_mut(&mut self) -> &mut String {
        match self.form_field {
            FormField::Name => &mut self.form_name,
            FormField::Description => &mut self.form_description,
        }
    }

    /// Move the sample timeline so that it ends where it was designed to, just
    /// before `now`.
    ///
    /// The samples are laid out against a fixed origin -- November 2023 -- so
    /// that tests can name exact instants, and `current_timestamp` was that
    /// origin plus 25 days. Nothing ever moved it. Every age on screen ("3 days
    /// ago"), every countdown in the schedule view ("next in 4h"), and every
    /// cleanup suggestion was measured against a constant, so none of them
    /// could change and all of them were fiction.
    ///
    /// Anchoring shifts every snapshot by the same amount, which preserves the
    /// intervals *between* them -- the tree's shape, the ages relative to one
    /// another, the schedule's spacing -- while putting the newest one 25 days
    /// behind today rather than 25 days behind a date three years past. The
    /// alternative, leaving the samples in 2023 and setting only the clock,
    /// would be equally truthful and would open on a screen where everything is
    /// years old and every scheduled snapshot is years overdue.
    ///
    /// Kept out of `new` so that `new` stays deterministic: a constructor that
    /// reads the wall clock is a constructor no test can assert against.
    pub fn anchor_to(&mut self, now: u64) {
        let origin = self.current_timestamp;
        for id in self.manager.tree.all_ids_by_timestamp() {
            if let Some(snap) = self.manager.tree.get_snapshot_mut(id) {
                snap.timestamp = shift(snap.timestamp, origin, now);
            }
        }
        // The schedule's clock moves with the snapshots it made, or the next
        // automatic snapshot would be three years overdue the moment the
        // window opened.
        self.manager.schedule.last_snapshot_timestamp =
            shift(self.manager.schedule.last_snapshot_timestamp, origin, now);
        self.current_timestamp = now;
    }

    /// Advance the clock. Called from the tick, and it is the whole of what the
    /// tick does when no operation is running.
    pub fn set_now(&mut self, now: u64) {
        self.current_timestamp = now;
    }

    /// Render the header bar.
    fn render_header(&self, rt: &mut RenderTree) {
        // Header background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });

        // Search box.
        let search_x = self.window_width - 260.0;
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
            overflow: TextOverflow::Ellipsis,
        });

        // Header bottom border.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: HEADER_HEIGHT,
            x2: self.window_width,
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
            width: self.window_width,
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
                x: text::center_x(
                    mode.label(),
                    tab_x + tab_width / 2.0,
                    FONT_SIZE,
                    if is_active {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                ),
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
                overflow: TextOverflow::Ellipsis,
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
        let mut btn_x = self.window_width - (actions.len() as f32 * (BUTTON_WIDTH + 8.0)) - PADDING;
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
                x: text::center_x(
                    label,
                    btn_x + BUTTON_WIDTH / 2.0,
                    FONT_SIZE,
                    FontWeightHint::Bold,
                ),
                y: toolbar_y + TOOLBAR_HEIGHT / 2.0 - FONT_SIZE / 2.0,
                text: label.to_string(),
                color: COLOR_BASE,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(BUTTON_WIDTH - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
            btn_x += BUTTON_WIDTH + 8.0;
        }

        // Bottom border.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: toolbar_y + TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: toolbar_y + TOOLBAR_HEIGHT,
            color: COLOR_SURFACE0,
            width: 1.0,
        });
    }

    /// Render the main content area based on current view mode.
    fn render_main_area(&self, rt: &mut RenderTree) {
        let content_y = HEADER_HEIGHT + TOOLBAR_HEIGHT;
        let content_height =
            self.window_height - content_y - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;

        // Clip to content area.
        rt.push(RenderCommand::PushClip {
            x: 0.0,
            y: content_y,
            width: self.window_width,
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
        // `visible_rows`, not a second copy of the filter. This function used to
        // flatten the tree itself and re-implement the type filter and the
        // search inline, which is the arrangement where the tree view and the
        // timeline start disagreeing about what the search box means -- and
        // where a click cannot be told which row it landed on, because the list
        // of rows existed only inside this loop.
        let flattened = self.visible_rows();
        let mut row_y = y + SMALL_PADDING - self.scroll_offset;

        for (id, depth) in &flattened {
            if let Some(snap) = self.manager.tree.get_snapshot(*id) {
                let indent = *depth as f32 * TREE_INDENT;
                let is_selected = self.selected_id == Some(*id);

                // Selection highlight.
                if is_selected {
                    rt.push(RenderCommand::FillRect {
                        x: PADDING,
                        y: row_y,
                        width: self.window_width - 2.0 * PADDING,
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
                    overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
                });

                // Lock indicator.
                if snap.locked {
                    let lock_x = self.window_width - 60.0;
                    rt.push(RenderCommand::Text {
                        x: lock_x,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: "Locked".to_string(),
                        color: COLOR_YELLOW,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(50.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                // Children count indicator.
                let kids = self.manager.tree.children_of(*id);
                if !kids.is_empty() {
                    let branch_x = self.window_width - 120.0;
                    rt.push(RenderCommand::Text {
                        x: branch_x,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: format!("{} children", kids.len()),
                        color: COLOR_OVERLAY0,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(80.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                row_y += TREE_ROW_HEIGHT;
            }
        }
    }

    /// Render the timeline view with chronological entries and type dots.
    fn render_timeline_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let ids: Vec<u64> = self.visible_rows().into_iter().map(|(id, _)| id).collect();
        // The gutter left of the timeline line holds each snapshot's date. It
        // was 60px wide because the date it held was `D20683` — a day count
        // from the epoch. A real `2026-08-18` needs about 66px at
        // `FONT_SIZE_SMALL`, so the gutter grew with the thing it holds
        // rather than the date being ellipsised to fit a width chosen for a
        // placeholder.
        let timeline_x = 92.0;
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
                        width: self.window_width - timeline_x - 40.0,
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
                    overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
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
                    max_width: Some(80.0),
                    overflow: TextOverflow::Ellipsis,
                });

                entry_y += TIMELINE_ENTRY_HEIGHT;
            }
        }
    }

    /// Render the compare view showing differences between two snapshots.
    fn render_compare_view(&self, rt: &mut RenderTree, y: f32, height: f32) {
        let panel_x = PADDING;
        let panel_width = self.window_width - 2.0 * PADDING;

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Compare Snapshots".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
            overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
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
                        overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render the schedule configuration view.
    fn render_schedule_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let panel_x = PADDING;
        let panel_width = self.window_width - 2.0 * PADDING;
        let schedule = &self.manager.schedule;

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Snapshot Schedule".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.to_string(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - 200.0),
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: due_text,
                color: COLOR_LAVENDER,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Render the storage management view.
    fn render_storage_view(&self, rt: &mut RenderTree, y: f32, _height: f32) {
        let panel_x = PADDING;
        let panel_width = self.window_width - 2.0 * PADDING;
        let stats = self.manager.storage_stats();

        rt.push(RenderCommand::Text {
            x: panel_x,
            y: y + PADDING,
            text: "Storage Management".to_string(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
                });
                info_y += 18.0;
            }
        }
    }

    /// Render the details panel at the bottom.
    fn render_details_panel(&self, rt: &mut RenderTree) {
        let panel_y = self.window_height - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;

        // Separator line.
        rt.push(RenderCommand::Line {
            x1: 0.0,
            y1: panel_y,
            x2: self.window_width,
            y2: panel_y,
            color: COLOR_SURFACE0,
            width: 1.0,
        });

        // Panel background.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: panel_y,
            width: self.window_width,
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
        let col2_x = self.window_width / 2.0;
        let mut y = panel_y + PADDING;

        // Name and type.
        rt.push(RenderCommand::Text {
            x: col1_x,
            y,
            text: snap.name.clone(),
            color: COLOR_TEXT,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(self.window_width / 2.0 - PADDING),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });

        y += 24.0;

        // Description. User-supplied free prose, so it wraps rather than being
        // clipped to its first line — but the panel is a fixed
        // DETAILS_PANEL_HEIGHT box with the ancestry chain anchored to its
        // bottom, so the wrap is capped (see DESCRIPTION_MAX_LINES) and the
        // cursor advances by the height actually drawn.
        let description_used = text::Paragraph::new(&snap.description, COLOR_SUBTEXT0)
            .at(col1_x, y, self.window_width - 2.0 * PADDING)
            .font(FONT_SIZE, FontWeightHint::Regular)
            .max_lines(DESCRIPTION_MAX_LINES)
            .draw(rt);
        if description_used > 0.0 {
            // An absent description takes no room at all; a present one takes
            // at least the row height the rest of the panel was laid out for.
            y += description_used.max(DESCRIPTION_ROW_HEIGHT);
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
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: label_x + 65.0,
                y,
                text: value.clone(),
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0),
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
        let comp_names: Vec<&str> = snap.components.iter().map(|c| c.label()).collect();
        rt.push(RenderCommand::Text {
            x: col1_x + 70.0,
            y,
            text: comp_names.join(", "),
            color: COLOR_SUBTEXT1,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_width - col1_x - 90.0),
            overflow: TextOverflow::Ellipsis,
        });

        y += 20.0;

        // Tags.
        if !snap.tags.is_empty() {
            let mut tag_x = col1_x;
            for tag in &snap.tags {
                let tag_width =
                    text::padded_width(tag, 8.0, FONT_SIZE_SMALL, FontWeightHint::Regular);
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
                    overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
            cx += 40.0;

            // Each name is capped at CHAIN_LINK_WIDTH, so the chain advances by
            // what was *drawn*, not by the full name: a long name used to push
            // the next link off past the clip and a short accented one used to
            // collide with it. Elide rather than clip, so the reader can see it
            // was cut.
            let links: Vec<(u64, String, f32)> = chain
                .iter()
                .filter_map(|&id| self.manager.tree.get_snapshot(id).map(|a| (id, &a.name)))
                .map(|(id, name)| {
                    let shown = text::elide(
                        name,
                        CHAIN_LINK_WIDTH,
                        CHAIN_ELLIPSIS,
                        FONT_SIZE_SMALL,
                        FontWeightHint::Regular,
                    );
                    let w = text::measure(&shown, FONT_SIZE_SMALL, FontWeightHint::Regular);
                    (id, shown, w)
                })
                .collect();

            // Capping each link said nothing about the chain: the cursor
            // advanced once per ancestor with no reference to the panel's right
            // edge, so a deep history ran off the side of the window. Keep the
            // links nearest the selected snapshot and mark the dropped head.
            let widths: Vec<f32> = links.iter().map(|&(_, _, w)| w).collect();
            let budget = self.window_width - PADDING - cx;
            let first = ancestry_first_visible(&widths, budget);

            if first > 0 {
                let marker_w =
                    text::measure(CHAIN_ELLIPSIS, FONT_SIZE_SMALL, FontWeightHint::Regular);
                rt.push(RenderCommand::Text {
                    x: cx,
                    y: chain_y,
                    text: CHAIN_ELLIPSIS.to_string(),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(marker_w),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += marker_w + CHAIN_LINK_GAP;
                rt.push(RenderCommand::Text {
                    x: cx,
                    y: chain_y,
                    text: " > ".to_string(),
                    color: COLOR_OVERLAY0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(CHAIN_SEPARATOR_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += CHAIN_SEPARATOR_WIDTH;
            }

            for (i, (ancestor_id, shown, shown_w)) in links.iter().enumerate().skip(first) {
                if i > first {
                    rt.push(RenderCommand::Text {
                        x: cx,
                        y: chain_y,
                        text: " > ".to_string(),
                        color: COLOR_OVERLAY0,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(CHAIN_SEPARATOR_WIDTH),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cx += CHAIN_SEPARATOR_WIDTH;
                }
                let name_color = if *ancestor_id == snap.id {
                    COLOR_BLUE
                } else {
                    COLOR_SUBTEXT0
                };
                rt.push(RenderCommand::Text {
                    x: cx,
                    y: chain_y,
                    text: shown.clone(),
                    color: name_color,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(CHAIN_LINK_WIDTH),
                    overflow: TextOverflow::Ellipsis,
                });
                cx += shown_w + CHAIN_LINK_GAP;
            }
        }
    }

    /// Render placeholder when no snapshot is selected.
    fn render_no_selection(&self, rt: &mut RenderTree, panel_y: f32) {
        rt.push(RenderCommand::Text {
            x: text::center_x(
                "Select a snapshot to view details",
                self.window_width / 2.0,
                FONT_SIZE,
                FontWeightHint::Regular,
            ),
            y: panel_y + DETAILS_PANEL_HEIGHT / 2.0 - FONT_SIZE / 2.0,
            text: "Select a snapshot to view details".to_string(),
            color: COLOR_OVERLAY0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(250.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, rt: &mut RenderTree) {
        let bar_y = self.window_height - STATUS_BAR_HEIGHT;

        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: bar_y,
            width: self.window_width,
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
            overflow: TextOverflow::Ellipsis,
        });

        // Center: storage summary.
        let stats = self.manager.storage_stats();
        let storage_text = format!(
            "{} snapshots | {} total",
            stats.snapshot_count,
            stats.total_display(),
        );
        rt.push(RenderCommand::Text {
            x: text::center_x(
                &storage_text,
                self.window_width / 2.0,
                FONT_SIZE_SMALL,
                FontWeightHint::Regular,
            ),
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: storage_text,
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(200.0),
            overflow: TextOverflow::Ellipsis,
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
            x: self.window_width - 200.0,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render a dialog overlay.
    fn render_dialog(&self, rt: &mut RenderTree) {
        // Dim overlay.
        rt.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
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
        let dx = (self.window_width - dialog_w) / 2.0;
        let dy = (self.window_height - dialog_h) / 2.0;

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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the confirm restore dialog.
    fn render_restore_dialog(&self, rt: &mut RenderTree, id: u64) {
        let dialog_w = 420.0;
        let dialog_h = 240.0;
        let dx = (self.window_width - dialog_w) / 2.0;
        let dy = (self.window_height - dialog_h) / 2.0;

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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: "Warning: This will revert system state to this snapshot.".to_string(),
                color: COLOR_RED,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 94.0,
                text: format!("Components affected: {}", snap.component_count()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 114.0,
                text: format!("Size: {}", snap.size_display()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the confirm delete dialog.
    fn render_delete_dialog(&self, rt: &mut RenderTree, id: u64) {
        let dialog_w = 380.0;
        let dialog_h = 180.0;
        let dx = (self.window_width - dialog_w) / 2.0;
        let dy = (self.window_height - dialog_h) / 2.0;

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
            overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: format!("This will free {}.", snap.size_display()),
                color: COLOR_SUBTEXT0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the export dialog.
    fn render_export_dialog(&self, rt: &mut RenderTree) {
        let dialog_w = 400.0;
        let dialog_h = 200.0;
        let dx = (self.window_width - dialog_w) / 2.0;
        let dy = (self.window_height - dialog_h) / 2.0;

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
            overflow: TextOverflow::Ellipsis,
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: format!("Export {} snapshot(s) to file.", self.manager.tree.count()),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the import dialog.
    fn render_import_dialog(&self, rt: &mut RenderTree) {
        let dialog_w = 400.0;
        let dialog_h = 200.0;
        let dx = (self.window_width - dialog_w) / 2.0;
        let dy = (self.window_height - dialog_h) / 2.0;

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
            overflow: TextOverflow::Ellipsis,
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: "Import snapshot metadata from file.".to_string(),
            color: COLOR_SUBTEXT0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
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
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the progress overlay.
    fn render_progress_overlay(&self, rt: &mut RenderTree) {
        if let Some(progress) = &self.progress {
            let overlay_w = 400.0;
            let overlay_h = 140.0;
            let ox = (self.window_width - overlay_w) / 2.0;
            let oy = (self.window_height - overlay_h) / 2.0;

            // Dim background.
            rt.push(RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: self.window_width,
                height: self.window_height,
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
                overflow: TextOverflow::Ellipsis,
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
            let percent = format!("{}%", progress.percentage());
            rt.push(RenderCommand::Text {
                x: text::center_x(
                    &percent,
                    ox + overlay_w / 2.0,
                    FONT_SIZE_SMALL,
                    FontWeightHint::Bold,
                ),
                y: bar_y + 3.0,
                text: percent,
                color: COLOR_TEXT,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
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
                overflow: TextOverflow::Ellipsis,
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
                    overflow: TextOverflow::Ellipsis,
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
    guitk::bytes::iec(bytes)
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

/// The date a snapshot was taken, for the timeline's left gutter.
///
/// This used to render `D20683` — the number of days since 1 January 1970.
/// A restore point is the most consequence-laden thing in the system to pick
/// by date, and `D20683` is not a date; it is an internal counter shown to
/// the user because turning it into one was work nobody had done.
///
/// [`guitk::datetime::iso_date`] rather than a fuller stamp because this is a
/// gutter beside a list, and the time of day is in the detail panel. The ISO
/// shape also sorts lexicographically in the order it sorts chronologically,
/// which is what a *timeline* wants.
///
/// UTC, explicitly: there is no per-process zone plumbing yet (known-issues
/// `TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). Writing it as `Tz::utc()` leaves
/// a mark that can be found when there is one.
fn format_timestamp_short(ts: u64) -> String {
    guitk::datetime::iso_date(
        i64::try_from(ts).unwrap_or(i64::MAX),
        &guitk::tzrules::Tz::utc(),
    )
}

/// The width one ancestry link occupies, given the width of its drawn name and
/// whether a `" > "` separator precedes it.
fn chain_link_cost(name_width: f32, preceded: bool) -> f32 {
    let sep = if preceded { CHAIN_SEPARATOR_WIDTH } else { 0.0 };
    name_width + CHAIN_LINK_GAP + sep
}

/// Choose the first ancestry link to draw so the whole chain fits in `budget`.
///
/// Each link was individually capped at [`CHAIN_LINK_WIDTH`], but nothing
/// capped the *chain*: the cursor advanced once per ancestor with no reference
/// to the panel's right edge, so a deep enough history simply ran off the side
/// of the window. Twelve long-named ancestors cost 2068px against a 986px
/// budget.
///
/// Links are dropped from the **front**. The tail of the chain is the selected
/// snapshot — the one the whole panel is describing — and the links nearest it
/// are the ones that say where it came from; the distant root is the least
/// informative part. When anything is dropped the caller draws a leading
/// [`CHAIN_ELLIPSIS`], and its cost is reserved here so the marker cannot
/// itself push the chain over the edge.
///
/// The last link is always kept even if it alone exceeds the budget: it is
/// already capped at [`CHAIN_LINK_WIDTH`], and a panel that silently drew no
/// path at all would be worse than one that is a few pixels tight.
fn ancestry_first_visible(name_widths: &[f32], budget: f32) -> usize {
    let Some(last) = name_widths.len().checked_sub(1) else {
        return 0;
    };

    let total: f32 = name_widths
        .iter()
        .enumerate()
        .map(|(i, w)| chain_link_cost(*w, i > 0))
        .sum();
    if total <= budget {
        return 0;
    }

    // The chain will be cut, so the leading marker is going to be drawn and
    // has to be paid for out of the same budget.
    let marker = chain_link_cost(
        text::measure(CHAIN_ELLIPSIS, FONT_SIZE_SMALL, FontWeightHint::Regular),
        false,
    ) + CHAIN_SEPARATOR_WIDTH;

    let mut used = marker;
    let mut first = last;
    for i in (0..=last).rev() {
        let cost = chain_link_cost(*name_widths.get(i).unwrap_or(&0.0), i < last);
        if i < last && used + cost > budget {
            break;
        }
        used += cost;
        first = i;
    }
    first
}

// ============================================================================
// main
// ============================================================================

/// `t`, moved by the same amount that carries `origin` to `now`.
///
/// Saturating in both directions: the shift is normally forwards by about three
/// years, but a machine whose clock is behind the sample origin shifts
/// backwards, and a timestamp that would go below zero is clamped rather than
/// wrapping to the far future -- which would put a snapshot after the ones that
/// come after it and invert the tree's order on screen.
fn shift(t: u64, origin: u64, now: u64) -> u64 {
    // Both differences are guarded by the branch they are in; written
    // saturating so the guard is in the operator rather than beside it.
    if now >= origin {
        t.saturating_add(now.saturating_sub(origin))
    } else {
        t.saturating_sub(origin.saturating_sub(now))
    }
}

/// The wall clock, in seconds since the epoch.
///
/// `None` if the system clock is before 1970 or cannot be read, in which case
/// the caller keeps the sample timeline's own origin. Refusing to answer is
/// better than answering zero: an age measured against 1970 reads "56 years
/// ago" for every snapshot, which looks like data rather than a missing clock.
fn system_now_secs() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

impl App for SystemRestoreUI {
    fn title(&self) -> String {
        // Which snapshot is selected, because that is what every action in the
        // toolbar acts on and the one thing a user switching windows needs to
        // see. The harness re-reads this as the program runs.
        match self
            .selected_id
            .and_then(|id| self.manager.tree.get_snapshot(id))
        {
            Some(snap) => format!("{} - System Restore", snap.name),
            None => "System Restore".to_string(),
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// A minute, and this is one of the few applications that genuinely needs
    /// one.
    ///
    /// Three things on screen age without anyone touching the keyboard: every
    /// snapshot's "3 days ago", the schedule view's countdown to the next
    /// automatic snapshot, and the storage view's cleanup suggestions, which
    /// are chosen by age. A minute is the resolution of the coarsest of them --
    /// `age_display` rounds to minutes -- so a shorter interval would redraw an
    /// identical frame and a longer one would let a countdown sit visibly
    /// stale.
    ///
    /// The tick also *runs* the schedule. Until it did, `check_schedule` and
    /// `apply_retention` had no caller anywhere in the program: an application
    /// whose entire purpose is taking snapshots on a timer, with no timer.
    fn tick_interval(&self) -> Option<Duration> {
        // A running operation steps a frame at a time, which is what makes the
        // progress bar move rather than jump from empty to full.
        if self.progress.is_some() {
            Some(PROGRESS_STEP)
        } else {
            Some(CLOCK_STEP)
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // From the frame being drawn rather than the last `Resize`: the
        // compositor may grant a size nobody asked for, and every hit test in
        // this file is derived from these two numbers.
        self.window_width = width;
        self.window_height = height;
        self.render_tree()
    }
}

fn main() -> ExitCode {
    let mut ui = SystemRestoreUI::new();
    if let Some(now) = system_now_secs() {
        ui.anchor_to(now);
    }
    app::launch("systemrestore", &mut ui)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis, not a hazard. The defensive lints
    // exist to keep panics out of code that runs on a user's data, which this
    // is not.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

    use super::*;

    // ------------------------------------------------------------------
    // Input
    //
    // This program had no key handler, no mouse handler and no `handle_event`
    // at all. Everything below is new ground.
    // ------------------------------------------------------------------

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    fn types(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn click(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

    fn centre(rect: Rect) -> (f32, f32) {
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    // -- the clock --

    /// Every age, every countdown and every cleanup suggestion was measured
    /// against `current_timestamp`, and nothing ever moved it: it was a
    /// constant 25 days after a fixed November 2023 origin.
    #[test]
    fn anchoring_moves_the_whole_timeline_and_keeps_its_shape() {
        let mut ui = SystemRestoreUI::new();
        let before: Vec<(u64, u64)> = ui
            .manager
            .tree
            .all_ids_by_timestamp()
            .into_iter()
            .filter_map(|id| ui.manager.tree.get_snapshot(id).map(|s| (id, s.timestamp)))
            .collect();
        let origin = ui.current_timestamp;

        let now = origin + 90 * 86_400;
        ui.anchor_to(now);

        assert_eq!(
            ui.current_timestamp, now,
            "the clock should be the one given"
        );
        for (id, was) in before {
            let is = ui
                .manager
                .tree
                .get_snapshot(id)
                .expect("still there")
                .timestamp;
            assert_eq!(
                is,
                was + 90 * 86_400,
                "every snapshot moves by the same amount, so the intervals \
                 between them -- which is what every age on screen is about -- \
                 are unchanged"
            );
        }
    }

    /// The schedule's own clock moves with the snapshots it took, or the next
    /// automatic snapshot is years overdue the moment the window opens.
    #[test]
    fn anchoring_moves_the_schedule_with_it() {
        let mut ui = SystemRestoreUI::new();
        let origin = ui.current_timestamp;
        let before = ui.manager.schedule.last_snapshot_timestamp;

        // The property that matters is not whether it is due -- the sample
        // schedule is weekly and last ran 11 days before the origin, so it is
        // already overdue, and the first tick after the window opens takes one.
        // It is that anchoring does not *change* whether it is due, because the
        // schedule's clock and the program's move together.
        let due_before = ui.manager.schedule.is_due(origin);
        ui.anchor_to(origin + 1000);
        assert_eq!(ui.manager.schedule.last_snapshot_timestamp, before + 1000);
        assert_eq!(
            ui.manager.schedule.is_due(ui.current_timestamp),
            due_before,
            "anchoring moved the schedule relative to the clock"
        );
    }

    /// The sample schedule is weekly and last ran 11 days ago, so it is due --
    /// which means the window demonstrates its own headline feature within a
    /// minute of opening, rather than showing a countdown that never fires.
    #[test]
    fn the_sample_schedule_is_due_so_the_first_tick_shows_what_the_program_does() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.manager.tree.count();
        assert!(ui.manager.schedule.is_due(ui.current_timestamp));

        ui.tick_to(ui.current_timestamp + 60);
        assert_eq!(ui.manager.tree.count(), before + 1);
    }

    /// A machine whose clock is behind the sample origin shifts backwards, and
    /// nothing wraps round to the far future.
    #[test]
    fn anchoring_backwards_clamps_at_the_epoch_rather_than_wrapping() {
        let mut ui = SystemRestoreUI::new();
        ui.anchor_to(0);
        for id in ui.manager.tree.all_ids_by_timestamp() {
            let t = ui
                .manager
                .tree
                .get_snapshot(id)
                .expect("still there")
                .timestamp;
            assert_eq!(t, 0, "clamped, not wrapped to u64::MAX");
        }
    }

    /// The whole point of the program, and nothing called it: an automatic
    /// snapshot manager whose scheduler never ran.
    #[test]
    fn a_tick_takes_the_scheduled_snapshot_that_is_due() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.manager.tree.count();
        let due_at = ui.manager.schedule.last_snapshot_timestamp
            + ui.manager.schedule.frequency.interval_secs();

        assert_eq!(
            ui.tick_to(due_at - 1),
            EventResult::Consumed,
            "the clock moved, so the frame is stale"
        );
        assert_eq!(
            ui.manager.tree.count(),
            before,
            "one second early is not yet due"
        );

        ui.tick_to(due_at);
        assert_eq!(
            ui.manager.tree.count(),
            before + 1,
            "the scheduled snapshot should have been taken"
        );
    }

    /// A tick that finds the clock where it left it has nothing to redraw.
    #[test]
    fn a_tick_at_the_same_second_asks_for_nothing() {
        let mut ui = SystemRestoreUI::new();
        let now = ui.current_timestamp;
        assert_eq!(ui.tick_to(now), EventResult::Ignored);
    }

    // -- the toolbar --

    #[test]
    fn the_view_tabs_switch_views() {
        let mut ui = SystemRestoreUI::new();
        for mode in ViewMode::all() {
            let rect = ui
                .toolbar_controls()
                .into_iter()
                .find(|(_, c)| *c == ToolbarControl::Tab(*mode))
                .expect("every view has a tab")
                .0;
            let (x, y) = centre(rect);
            assert_eq!(ui.handle_event(&click(x, y)), EventResult::Consumed);
            assert_eq!(ui.view_mode, *mode, "the {:?} tab did nothing", mode);
        }
    }

    #[test]
    fn the_action_buttons_open_their_dialogs() {
        for (control, expected) in [
            (ToolbarControl::Create, DialogKind::CreateSnapshot),
            (ToolbarControl::Export, DialogKind::ExportDialog),
        ] {
            let mut ui = SystemRestoreUI::new();
            let rect = ui
                .toolbar_controls()
                .into_iter()
                .find(|(_, c)| *c == control)
                .expect("drawn")
                .0;
            let (x, y) = centre(rect);
            ui.handle_event(&click(x, y));
            assert_eq!(ui.dialog, expected, "{:?} did nothing", control);
        }
    }

    /// Restore and Delete act on the selection, so what they open names it.
    #[test]
    fn restore_and_delete_ask_about_the_selected_snapshot() {
        let mut ui = SystemRestoreUI::new();
        let selected = ui.selected_id.expect("the sample opens with a selection");

        for (control, expected) in [
            (
                ToolbarControl::Restore,
                DialogKind::ConfirmRestore(selected),
            ),
            (ToolbarControl::Delete, DialogKind::ConfirmDelete(selected)),
        ] {
            let rect = ui
                .toolbar_controls()
                .into_iter()
                .find(|(_, c)| *c == control)
                .expect("drawn")
                .0;
            let (x, y) = centre(rect);
            ui.handle_event(&click(x, y));
            assert_eq!(ui.dialog, expected);
            ui.dialog = DialogKind::None;
        }
    }

    /// The toolbar band is the toolbar's, even between controls.
    #[test]
    fn a_click_on_the_empty_toolbar_does_not_reach_the_list() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.selected_id;
        // Between the last tab and the first action button.
        assert_eq!(
            ui.handle_event(&click(500.0, HEADER_HEIGHT + TOOLBAR_HEIGHT / 2.0)),
            EventResult::Consumed
        );
        assert_eq!(ui.selected_id, before);
    }

    // -- the list --

    #[test]
    fn clicking_a_row_selects_that_snapshot() {
        let mut ui = SystemRestoreUI::new();
        let rows = ui.row_rects();
        assert!(rows.len() > 1, "the sample has several snapshots");
        let (rect, id) = rows[1];
        let (x, y) = centre(rect);

        ui.handle_event(&click(x, y));
        assert_eq!(ui.selected_id, Some(id));
    }

    /// The Compare view needs two snapshots and there was no way to give it a
    /// second one: `compare_id` was set at construction to `None` and never
    /// written again, so the view drew an empty frame for ever.
    #[test]
    fn clicking_the_selected_row_again_marks_it_for_comparison() {
        let mut ui = SystemRestoreUI::new();
        let (rect, id) = ui.row_rects()[1];
        let (x, y) = centre(rect);

        ui.handle_event(&click(x, y));
        assert_eq!(ui.compare_id, None);
        ui.handle_event(&click(x, y));
        assert_eq!(
            ui.compare_id,
            Some(id),
            "the second click picks the other side"
        );
        ui.handle_event(&click(x, y));
        assert_eq!(ui.compare_id, None, "and a third takes it back off");
    }

    #[test]
    fn the_arrows_walk_the_list_and_stop_at_the_ends() {
        let mut ui = SystemRestoreUI::new();
        let rows = ui.visible_rows();
        assert!(rows.len() >= 3);

        ui.handle_event(&press(Key::Home));
        assert_eq!(ui.selected_id, Some(rows[0].0));
        ui.handle_event(&press(Key::Up));
        assert_eq!(
            ui.selected_id,
            Some(rows[0].0),
            "stopping rather than wrapping to the bottom"
        );

        ui.handle_event(&press(Key::Down));
        assert_eq!(ui.selected_id, Some(rows[1].0));

        ui.handle_event(&press(Key::End));
        let last = rows.last().expect("non-empty").0;
        assert_eq!(ui.selected_id, Some(last));
        ui.handle_event(&press(Key::Down));
        assert_eq!(
            ui.selected_id,
            Some(last),
            "and stopping at the far end too"
        );
    }

    /// Typing searches, and the selection follows what is left on screen --
    /// which is why the selection is an id and not a row number.
    #[test]
    fn typing_filters_the_list_and_the_selection_stays_on_something_visible() {
        let mut ui = SystemRestoreUI::new();
        for c in "Network".chars() {
            ui.handle_event(&types(c));
        }
        assert_eq!(ui.search_query, "Network");
        let rows = ui.visible_rows();
        assert_eq!(rows.len(), 1, "one snapshot mentions the network");
        assert_eq!(
            ui.selected_id,
            Some(rows[0].0),
            "the selection was on a snapshot the search hid"
        );

        ui.handle_event(&press(Key::Escape));
        assert_eq!(ui.search_query, "", "Escape clears the query");
        assert!(ui.visible_rows().len() > 1);
    }

    #[test]
    fn a_key_that_carries_no_text_is_not_typed_into_the_search_box() {
        let mut ui = SystemRestoreUI::new();
        assert_eq!(ui.handle_event(&press(Key::F5)), EventResult::Ignored);
        assert_eq!(ui.search_query, "");
    }

    // -- dialogs --

    #[test]
    fn enter_confirms_a_delete_and_the_snapshot_is_gone() {
        let mut ui = SystemRestoreUI::new();
        // A leaf, so the tree has no orphans to worry about.
        let id = *ui
            .visible_rows()
            .iter()
            .map(|(id, _)| id)
            .find(|id| ui.manager.tree.children_of(**id).is_empty())
            .expect("some snapshot is a leaf");
        ui.selected_id = Some(id);

        ui.handle_event(&press(Key::Delete));
        assert_eq!(ui.dialog, DialogKind::ConfirmDelete(id));
        ui.handle_event(&press(Key::Enter));

        assert_eq!(ui.dialog, DialogKind::None);
        assert!(
            ui.manager.tree.get_snapshot(id).is_none(),
            "the snapshot should have been deleted"
        );
        assert_ne!(ui.selected_id, Some(id), "and the selection moved off it");
    }

    #[test]
    fn escape_closes_a_dialog_without_doing_it() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.manager.tree.count();
        ui.handle_event(&press(Key::Delete));
        ui.handle_event(&press(Key::Escape));
        assert_eq!(ui.dialog, DialogKind::None);
        assert_eq!(ui.manager.tree.count(), before);
    }

    /// A click outside a modal dialog closes it and does not also reach the
    /// window behind, which is what the dimmed backdrop the renderer draws is
    /// promising.
    #[test]
    fn a_click_outside_a_dialog_closes_it_and_goes_no_further() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.selected_id;
        ui.handle_event(&press(Key::Delete));
        assert_ne!(ui.dialog, DialogKind::None);

        assert_eq!(ui.handle_event(&click(4.0, 4.0)), EventResult::Consumed);
        assert_eq!(ui.dialog, DialogKind::None);
        assert_eq!(ui.selected_id, before, "the click did not reach the list");
    }

    #[test]
    fn the_dialog_buttons_can_be_clicked() {
        let mut ui = SystemRestoreUI::new();
        let before = ui.manager.tree.count();
        ui.handle_event(&press(Key::Delete));

        let cancel = ui
            .dialog_buttons()
            .into_iter()
            .find(|(_, b)| *b == DialogButton::Cancel)
            .expect("drawn")
            .0;
        let (x, y) = centre(cancel);
        ui.handle_event(&click(x, y));
        assert_eq!(ui.dialog, DialogKind::None);
        assert_eq!(ui.manager.tree.count(), before, "Cancel does nothing else");
    }

    /// A locked snapshot cannot be deleted, which is what the padlock means.
    #[test]
    fn a_locked_snapshot_survives_a_confirmed_delete() {
        let mut ui = SystemRestoreUI::new();
        let id = ui.selected_id.expect("selected");
        ui.handle_event(&press_ctrl(Key::L));
        assert!(
            ui.manager.tree.get_snapshot(id).expect("there").locked,
            "Ctrl+L should have locked it"
        );

        ui.handle_event(&press(Key::Delete));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.manager.tree.get_snapshot(id).is_some(), "still there");

        // And it can be unlocked again: `unlock_snapshot` had no caller, so
        // anything locked was locked for the life of the process.
        ui.handle_event(&press_ctrl(Key::L));
        assert!(!ui.manager.tree.get_snapshot(id).expect("there").locked);
    }

    /// The form branches from what is selected, which is what makes the tree a
    /// tree. Left at `None` it always added another root.
    #[test]
    fn the_create_form_branches_from_the_selection() {
        let mut ui = SystemRestoreUI::new();
        let selected = ui.selected_id.expect("selected");
        ui.handle_event(&press_ctrl(Key::N));

        assert_eq!(ui.dialog, DialogKind::CreateSnapshot);
        assert_eq!(ui.form_parent_id, Some(selected));

        for c in "Before upgrade".chars() {
            ui.handle_event(&types(c));
        }
        assert_eq!(ui.form_name, "Before upgrade");
        ui.handle_event(&press(Key::Tab));
        for c in "notes".chars() {
            ui.handle_event(&types(c));
        }
        assert_eq!(ui.form_description, "notes");
        assert_eq!(
            ui.form_name, "Before upgrade",
            "Tab moved to the other field"
        );

        let before = ui.manager.tree.count();
        ui.handle_event(&press(Key::Enter));
        assert_eq!(ui.manager.tree.count(), before + 1);
        let made = ui.selected_id.expect("the new one is selected");
        assert_eq!(
            ui.manager.tree.get_snapshot(made).expect("there").parent_id,
            Some(selected),
            "the new snapshot should hang off the one that was selected"
        );
    }

    // -- operations --

    /// `simulate_restore` returns the whole filmstrip and had no caller, so the
    /// progress overlay the renderer draws in full could never appear.
    #[test]
    fn a_restore_steps_through_its_progress_and_then_finishes() {
        let mut ui = SystemRestoreUI::new();
        ui.handle_event(&press(Key::Enter));
        assert!(matches!(ui.dialog, DialogKind::ConfirmRestore(_)));
        ui.handle_event(&press(Key::Enter));

        assert!(ui.progress.is_some(), "the overlay should be up");
        assert_eq!(
            ui.tick_interval(),
            Some(PROGRESS_STEP),
            "and the clock should be running at the operation's rate"
        );

        let mut steps = 0;
        while ui.progress.is_some() {
            ui.handle_event(&Event::Tick { elapsed_ms: 400 });
            steps += 1;
            assert!(steps < 100, "the operation never ended");
        }
        assert!(steps > 2, "a restore is more than one frame");
        assert_eq!(
            ui.tick_interval(),
            Some(CLOCK_STEP),
            "and the clock goes back to once a minute"
        );
    }

    /// Escape abandons a running operation; nothing else reaches through the
    /// overlay.
    #[test]
    fn a_running_operation_swallows_everything_but_escape() {
        let mut ui = SystemRestoreUI::new();
        ui.handle_event(&press(Key::Enter));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.progress.is_some());

        let before = ui.view_mode;
        assert_eq!(ui.handle_event(&press(Key::Tab)), EventResult::Ignored);
        assert_eq!(
            ui.view_mode, before,
            "the view must not change under an overlay"
        );

        ui.handle_event(&press(Key::Escape));
        assert!(ui.progress.is_none());
        assert!(
            ui.pending_steps.is_empty(),
            "and the rest of the filmstrip is dropped"
        );
    }

    // -- geometry --

    /// One law, two callers. Every row a click can land on is a row the
    /// renderer drew.
    #[test]
    fn the_rows_are_laid_out_without_overlapping_and_inside_the_content_area() {
        let ui = SystemRestoreUI::new();
        let rects = ui.row_rects();
        assert!(!rects.is_empty());
        let top = HEADER_HEIGHT + TOOLBAR_HEIGHT;
        let bottom = WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        for (rect, _) in &rects {
            assert!(
                rect.y + rect.h > top && rect.y < bottom,
                "row drawn outside the list"
            );
        }
        for pair in rects.windows(2) {
            assert!(
                pair[0].0.y + pair[0].0.h <= pair[1].0.y + 0.01,
                "two rows overlap"
            );
        }
    }

    /// The whole file drew at the `WINDOW_WIDTH` constant, so the picture was
    /// the same size whatever window it was given.
    #[test]
    fn the_layout_follows_the_window_it_is_given() {
        let mut ui = SystemRestoreUI::new();
        let wide = ui
            .toolbar_controls()
            .into_iter()
            .find(|(_, c)| *c == ToolbarControl::Export)
            .expect("drawn")
            .0;

        let _ = App::render(&mut ui, 1400.0, 900.0);
        let narrow = ui
            .toolbar_controls()
            .into_iter()
            .find(|(_, c)| *c == ToolbarControl::Export)
            .expect("drawn")
            .0;
        assert!(
            narrow.x > wide.x,
            "the right-aligned buttons should have moved right with the edge"
        );
    }

    /// A list shorter than the window has nowhere to scroll, and clamping to a
    /// negative maximum is the one shape `clamp` panics on.
    #[test]
    fn a_list_shorter_than_the_window_does_not_scroll() {
        let mut ui = SystemRestoreUI::new();
        ui.handle_event(&Event::Mouse(MouseEvent {
            x: 200.0,
            y: 400.0,
            kind: MouseEventKind::Scroll { dx: 0.0, dy: -10.0 },
        }));
        assert_eq!(ui.scroll_offset, 0.0);
    }

    /// The title names the selected snapshot, and follows it.
    #[test]
    fn the_title_names_the_selected_snapshot() {
        let mut ui = SystemRestoreUI::new();
        let first = ui.title();
        assert!(first.ends_with("- System Restore"), "got {first:?}");

        ui.handle_event(&press(Key::End));
        assert_ne!(ui.title(), first, "the title should follow the selection");
    }

    // --- text measurement ---

    #[test]
    fn tag_pills_fit_their_tags() {
        // Snapshot tags are user-entered, so they are arbitrary text.
        for tag in ["auto", "before-upgrade", "manuell", "リリース前"] {
            let w = text::padded_width(tag, 8.0, FONT_SIZE_SMALL, FontWeightHint::Regular);
            let drawn = text::measure(tag, FONT_SIZE_SMALL, FontWeightHint::Regular);
            assert!(drawn + 16.0 <= w + 0.01, "{tag:?} overflows its pill");
        }
    }

    #[test]
    fn an_ancestry_link_advances_by_what_was_drawn() {
        // Each link is capped at 150 px. Advancing by the *full* name pushed
        // the next link past the clip; advancing by a byte estimate made a
        // short accented name collide with it. Elide, then advance by the
        // elided width.
        let long = "a-very-long-snapshot-name-that-will-not-fit-in-one-hundred-and-fifty-pixels";
        let shown = text::elide(long, 150.0, "...", FONT_SIZE_SMALL, FontWeightHint::Regular);
        let w = text::measure(&shown, FONT_SIZE_SMALL, FontWeightHint::Regular);
        assert!(w <= 150.0 + 0.01, "the elided link is {w} wide");
        assert!(shown.ends_with("..."), "a cut link does not say it was cut");

        // A name that fits is left alone and advances by its own width.
        let short = "base";
        let shown = text::elide(
            short,
            150.0,
            "...",
            FONT_SIZE_SMALL,
            FontWeightHint::Regular,
        );
        assert_eq!(shown, short);
    }

    #[test]
    fn an_ancestry_chain_that_fits_is_drawn_whole() {
        assert_eq!(ancestry_first_visible(&[50.0, 50.0, 50.0], 986.0), 0);
    }

    #[test]
    fn an_ancestry_chain_that_does_not_fit_is_cut_from_the_front() {
        // Three 150px links cost 150+4 + 3x2 separators; only the last two fit
        // in 400px once the leading marker is paid for.
        let first = ancestry_first_visible(&[150.0, 150.0, 150.0], 400.0);
        assert!(first > 0, "a chain over budget was not cut at all");
        assert!(
            first < 3,
            "the chain was cut past its end, leaving nothing to draw"
        );
    }

    #[test]
    fn the_last_ancestry_link_is_kept_even_when_it_alone_overflows() {
        // The selected snapshot is what the panel is describing. Drawing no
        // path at all would be worse than one tight link, which is itself
        // already capped at CHAIN_LINK_WIDTH.
        assert_eq!(ancestry_first_visible(&[150.0, 150.0], 10.0), 1);
    }

    #[test]
    fn an_empty_ancestry_chain_has_no_first_link() {
        assert_eq!(ancestry_first_visible(&[], 100.0), 0);
    }

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
        assert!(display.contains("GiB") || display.contains("MiB"));
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
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
        assert_eq!(format_bytes(1_099_511_627_776), "1.0 TiB");
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

    /// A restore point is dated, not numbered.
    ///
    /// The assertions this replaces were `"D0"`, `"D1"` and `"D100"` — and
    /// they were correct, which is the point: the test proved the function
    /// did what it did, and never asked whether what it did was a date.
    #[test]
    fn test_format_timestamp_short() {
        assert_eq!(format_timestamp_short(0), "1970-01-01");
        assert_eq!(format_timestamp_short(86_400), "1970-01-02");
        assert_eq!(format_timestamp_short(86_400 * 100), "1970-04-11");
        // 2026-08-18 16:30:45 UTC — the gutter used to read "D20683".
        assert_eq!(format_timestamp_short(1_787_070_645), "2026-08-18");
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
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
        // Should have a good number of render commands for the full UI.
        assert!(rt.len() > 30);
    }

    #[test]
    fn test_ui_render_with_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::CreateSnapshot;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_with_progress() {
        let mut ui = SystemRestoreUI::new();
        ui.progress = Some(OperationProgress::new_create(&[
            SnapshotComponent::BootConfig,
        ]));
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_timeline_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Timeline;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_compare_view_no_selection() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Compare;
        ui.compare_id = None;
        let rt = ui.render_tree();
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
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_schedule_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Schedule;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_storage_view() {
        let mut ui = SystemRestoreUI::new();
        ui.view_mode = ViewMode::Storage;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_delete_dialog() {
        let mut ui = SystemRestoreUI::new();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmDelete(id);
        }
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_restore_dialog() {
        let mut ui = SystemRestoreUI::new();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmRestore(id);
        }
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_export_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::ExportDialog;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_import_dialog() {
        let mut ui = SystemRestoreUI::new();
        ui.dialog = DialogKind::ImportDialog;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    // --- Details-panel description layout ---

    /// The details panel alone, for a selected snapshot carrying `description`.
    ///
    /// Renders the panel directly rather than the whole window: `render()` also
    /// emits the status bar, which sits *below* the panel and so would trip the
    /// "nothing may be pushed past the panel's anchored bottom row" assertion
    /// for reasons that have nothing to do with the description.
    fn details_panel_with_description(description: &str) -> Vec<RenderCommand> {
        let mut ui = SystemRestoreUI::new();
        let id = *ui
            .manager
            .tree
            .all_ids_by_timestamp()
            .first()
            .expect("the default tree has at least one snapshot");
        ui.manager
            .tree
            .get_snapshot_mut(id)
            .expect("the id came from the tree")
            .description = description.to_string();
        ui.selected_id = Some(id);
        let snap = ui
            .manager
            .tree
            .get_snapshot(id)
            .expect("the id came from the tree")
            .clone();
        let mut rt = RenderTree::new();
        ui.render_snapshot_details(
            &mut rt,
            &snap,
            WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT,
        );
        rt.commands
    }

    /// The details panel for a snapshot `depth` links deep in its own root's
    /// history, every ancestor named too long to fit one link.
    fn details_panel_with_deep_ancestry(depth: usize) -> Vec<RenderCommand> {
        let mut ui = SystemRestoreUI::new();
        let mut parent = None;
        let mut last = 0;
        for i in 0..depth {
            // The index leads the name: a link is elided from its end, so a
            // trailing index would be the first thing cut and the test could
            // not tell the links apart.
            let id = ui
                .manager
                .tree
                .add_snapshot(
                    &format!("{i}-a-very-long-snapshot-name-that-will-not-fit-in-one-link"),
                    "",
                    1_000 + i as u64,
                    SnapshotType::Manual,
                    Vec::new(),
                    parent,
                )
                .expect("the parent was created on the previous iteration");
            parent = Some(id);
            last = id;
        }
        ui.selected_id = Some(last);
        let snap = ui
            .manager
            .tree
            .get_snapshot(last)
            .expect("the snapshot was just created")
            .clone();
        let mut rt = RenderTree::new();
        ui.render_snapshot_details(
            &mut rt,
            &snap,
            WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT,
        );
        rt.commands
    }

    /// The y the ancestry chain is anchored to.
    fn ancestry_row_y() -> f32 {
        WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT + DETAILS_PANEL_HEIGHT - 22.0
    }

    /// The text commands on the ancestry row, left to right.
    fn ancestry_row(cmds: &[RenderCommand]) -> Vec<(f32, String, f32, FontWeightHint)> {
        let chain_y = ancestry_row_y();
        let mut row: Vec<(f32, String, f32, FontWeightHint)> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    font_weight,
                    ..
                } if (y - chain_y).abs() < 0.5 => {
                    Some((*x, text.clone(), *font_size, *font_weight))
                }
                _ => None,
            })
            .collect();
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
        row
    }

    /// Capping each link said nothing about the chain: the cursor advanced once
    /// per ancestor with no reference to the panel's right edge, so a deep
    /// history ran clean off the side of the window.
    #[test]
    fn a_deep_ancestry_chain_stays_inside_the_panel() {
        let cmds = details_panel_with_deep_ancestry(12);
        let right = WINDOW_WIDTH - PADDING;
        let row = ancestry_row(&cmds);
        let mut checked = 0usize;
        for (x, text, size, weight) in &row {
            let end = x + text::measure(text, *size, *weight);
            assert!(
                end <= right + 0.5,
                "chain element {text:?} starts at {x} and ends at {end}, \
                 past the panel's right edge {right}",
            );
            checked = checked.saturating_add(1);
        }
        assert!(
            checked >= 4,
            "expected the Path label and several links on the chain row, checked {checked}",
        );
    }

    /// The tail of the chain is the snapshot the panel is describing, so the
    /// links nearest it are the ones worth keeping — and the reader has to be
    /// told the path shown is partial.
    #[test]
    fn a_cut_ancestry_chain_keeps_the_selected_snapshot_and_marks_the_cut() {
        let cmds = details_panel_with_deep_ancestry(12);
        let row = ancestry_row(&cmds);
        let texts: Vec<&String> = row.iter().map(|(_, t, _, _)| t).collect();

        assert!(
            texts.iter().any(|t| t.as_str() == CHAIN_ELLIPSIS),
            "the dropped head of the chain is not marked, got {texts:?}",
        );
        let last = texts
            .last()
            .unwrap_or_else(|| panic!("the chain row should not be empty"));
        assert!(
            last.starts_with("11-"),
            "the selected snapshot must be the last link drawn, got {last:?}",
        );
        assert!(
            !texts.iter().any(|t| t.starts_with("0-")),
            "the distant root should have been dropped, got {texts:?}",
        );
    }

    /// A chain short enough to fit is drawn in full, with no marker — the fix
    /// must not make the common case look truncated.
    #[test]
    fn a_short_ancestry_chain_is_drawn_whole() {
        let cmds = details_panel_with_deep_ancestry(3);
        let row = ancestry_row(&cmds);
        let texts: Vec<&String> = row.iter().map(|(_, t, _, _)| t).collect();
        assert!(
            texts.iter().any(|t| t.starts_with("0-")),
            "the root of a chain that fits must still be drawn, got {texts:?}",
        );
        assert!(
            !texts.iter().any(|t| t.as_str() == CHAIN_ELLIPSIS),
            "a chain that fits must not be marked as cut, got {texts:?}",
        );
    }

    /// Text commands in the details panel, as `(y, text)`, top-down.
    fn details_panel_text(cmds: &[RenderCommand]) -> Vec<(f32, String)> {
        let mut rows: Vec<(f32, String)> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { y, text, .. } => Some((*y, text.clone())),
                _ => None,
            })
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows
    }

    /// A description longer than one line wraps instead of being cut at the
    /// first line, which is what a bare `Text` command's `max_width` would do.
    #[test]
    fn a_long_description_wraps_rather_than_being_clipped() {
        let words: Vec<String> = (0..60).map(|n| format!("word{n}")).collect();
        let description = words.join(" ");
        let rows = details_panel_text(&details_panel_with_description(&description));

        // The first description line is drawn, and so is a second one carrying
        // words the single-command version would have dropped entirely.
        let drawn: Vec<&String> = rows.iter().map(|(_, t)| t).collect();
        let lines: Vec<&&String> = drawn
            .iter()
            .filter(|t| t.starts_with("word0 ") || t.contains("word"))
            .collect();
        assert!(
            lines.len() >= 2,
            "expected the description to occupy more than one line, got {lines:?}",
        );
    }

    /// The panel is a fixed-height box with the ancestry chain anchored to its
    /// bottom, so however far the description wraps, the rows beneath it must
    /// still clear that chain row.
    #[test]
    fn a_long_description_never_pushes_content_onto_the_ancestry_row() {
        let panel_y = WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        let chain_y = panel_y + DETAILS_PANEL_HEIGHT - 22.0;
        let description = "supercalifragilistic ".repeat(80);
        let rows = details_panel_text(&details_panel_with_description(&description));

        let mut checked = 0;
        for (y, text) in &rows {
            // The chain row itself and anything at or below it is the anchored
            // content; everything above must stay above it.
            if *y < chain_y {
                checked += 1;
                continue;
            }
            assert!(
                text.starts_with("Path:") || text.contains('>') || text.contains('…'),
                "row {text:?} at y={y} has been pushed down onto the ancestry row at {chain_y}",
            );
        }
        assert!(checked >= 5, "expected the panel's rows, checked {checked}");
    }

    /// A short description leaves the panel laid out exactly as before, so the
    /// wrap fix does not shift the common case.
    #[test]
    fn a_short_description_keeps_the_original_row_spacing() {
        let rows = details_panel_text(&details_panel_with_description("Short."));
        let panel_y = WINDOW_HEIGHT - DETAILS_PANEL_HEIGHT - STATUS_BAR_HEIGHT;
        assert!(
            rows.iter()
                .any(|(y, t)| t == "Short." && (*y - (panel_y + PADDING + 24.0)).abs() < 0.5),
            "expected the description on the description row, got {rows:?}",
        );
        assert!(
            rows.iter()
                .any(|(y, t)| t == "Size:" && (*y - (panel_y + PADDING + 44.0)).abs() < 0.5),
            "expected the metadata row unmoved, got {rows:?}",
        );
    }

    #[test]
    fn test_ui_render_no_selection_details() {
        let mut ui = SystemRestoreUI::new();
        ui.selected_id = None;
        let rt = ui.render_tree();
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

    /// Build a manager holding `full` ← `inc1` ← `inc2`, with `inc2` locked
    /// and tagged, for the round-trip tests below.
    fn chained_manager() -> SnapshotManager {
        let mut mgr = SnapshotManager::new();
        let full = mgr
            .create_snapshot(
                "Full",
                "base",
                1000,
                SnapshotType::Manual,
                vec![SnapshotComponent::SystemFiles],
                None,
            )
            .unwrap();
        let inc1 = mgr
            .create_snapshot(
                "Inc1",
                "first delta",
                2000,
                SnapshotType::Scheduled,
                vec![SnapshotComponent::BootConfig],
                Some(full),
            )
            .unwrap();
        let inc2 = mgr
            .create_snapshot(
                "Inc2",
                "second delta",
                3000,
                SnapshotType::Scheduled,
                vec![SnapshotComponent::NetworkConfig],
                Some(inc1),
            )
            .unwrap();
        mgr.tree.lock_snapshot(inc2).unwrap();
        mgr.tree.add_tag(inc2, "keep").unwrap();
        mgr
    }

    /// An incremental snapshot is only meaningful relative to the snapshot it
    /// was taken against. Import used to pass `None` for every parent, so a
    /// round trip flattened the whole chain into three unrelated roots and lost
    /// which full snapshot each delta belonged to.
    #[test]
    fn an_import_preserves_the_snapshot_chain() {
        let exported = chained_manager().export_all();

        let mut restored = SnapshotManager::new();
        let ids = restored.import_snapshots(&exported, 0).unwrap();
        assert_eq!(ids.len(), 3);

        // Exactly one root, and a three-deep chain under it.
        assert_eq!(restored.tree.root_ids().len(), 1, "the chain was flattened");
        let deepest = ids
            .iter()
            .copied()
            .max_by_key(|&id| restored.tree.depth_of(id))
            .unwrap();
        assert_eq!(restored.tree.depth_of(deepest), 2);

        let chain: Vec<String> = restored
            .tree
            .ancestry_chain(deepest)
            .into_iter()
            .filter_map(|id| restored.tree.get_snapshot(id).map(|s| s.name.clone()))
            .collect();
        assert_eq!(chain, ["Full", "Inc1", "Inc2"]);
    }

    /// The lock is what stops retention from pruning a snapshot the user marked
    /// as protected, and the tags are what they labelled it with. Both must
    /// survive the round trip.
    #[test]
    fn an_import_preserves_locks_and_tags() {
        let exported = chained_manager().export_all();

        let mut restored = SnapshotManager::new();
        let ids = restored.import_snapshots(&exported, 0).unwrap();

        let inc2 = ids
            .iter()
            .copied()
            .find(|&id| {
                restored
                    .tree
                    .get_snapshot(id)
                    .is_some_and(|s| s.name == "Inc2")
            })
            .expect("Inc2 was not imported");
        let snap = restored.tree.get_snapshot(inc2).unwrap();
        assert!(snap.locked, "the lock was lost on import");
        assert_eq!(snap.tags, ["keep"], "the tags were lost on import");
    }

    /// The file lists snapshots in timestamp order, but nothing enforces that a
    /// parent precedes its child — a hand-edited or concatenated file need not.
    /// The two-pass import must link them anyway.
    #[test]
    fn an_import_links_a_child_that_appears_before_its_parent() {
        let exported = chained_manager().export_all();
        // Reverse the section order.
        let mut sections: Vec<&str> = exported.split("\n\n").collect();
        sections.reverse();
        let reversed = sections.join("\n\n");

        let mut restored = SnapshotManager::new();
        let ids = restored.import_snapshots(&reversed, 0).unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(
            restored.tree.root_ids().len(),
            1,
            "a child listed before its parent was left detached"
        );
    }

    /// A parent that is not in the file leaves the snapshot as a root. Losing
    /// its position is recoverable; refusing the import would lose the snapshot.
    #[test]
    fn an_import_of_a_subtree_without_its_parent_keeps_the_snapshot() {
        let exported = chained_manager().export_all();
        // Keep only the last section (Inc2), whose parent is absent.
        let sections: Vec<&str> = exported.split("\n\n").collect();
        let last = (*sections.last().unwrap()).to_string();

        let mut restored = SnapshotManager::new();
        let ids = restored.import_snapshots(&last, 0).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(restored.tree.depth_of(ids[0]), 0);
        assert_eq!(restored.tree.get_snapshot(ids[0]).unwrap().name, "Inc2");
    }

    // --- set_parent ---

    /// A cycle would not merely corrupt the tree, it would hang the app:
    /// `depth_of` and `ancestry_chain` follow parent links until they reach a
    /// root.
    #[test]
    fn set_parent_refuses_to_create_a_cycle() {
        let mut tree = SnapshotTree::new();
        let a = tree
            .add_snapshot("a", "", 1, SnapshotType::Manual, vec![], None)
            .unwrap();
        let b = tree
            .add_snapshot("b", "", 2, SnapshotType::Manual, vec![], Some(a))
            .unwrap();
        let c = tree
            .add_snapshot("c", "", 3, SnapshotType::Manual, vec![], Some(b))
            .unwrap();

        assert!(matches!(
            tree.set_parent(a, Some(c)),
            Err(SnapshotError::ParentNotFound(_))
        ));
        assert!(matches!(
            tree.set_parent(a, Some(a)),
            Err(SnapshotError::ParentNotFound(_))
        ));
        // The tree is untouched by the refusals.
        assert_eq!(tree.depth_of(c), 2);
        assert_eq!(tree.root_ids(), vec![a]);
    }

    #[test]
    fn set_parent_moves_a_snapshot_between_parents() {
        let mut tree = SnapshotTree::new();
        let a = tree
            .add_snapshot("a", "", 1, SnapshotType::Manual, vec![], None)
            .unwrap();
        let b = tree
            .add_snapshot("b", "", 2, SnapshotType::Manual, vec![], None)
            .unwrap();
        let c = tree
            .add_snapshot("c", "", 3, SnapshotType::Manual, vec![], Some(a))
            .unwrap();

        tree.set_parent(c, Some(b)).unwrap();
        assert_eq!(tree.children_of(a), &[] as &[u64]);
        assert_eq!(tree.children_of(b), &[c]);
        assert_eq!(tree.get_snapshot(c).unwrap().parent_id, Some(b));

        // Detaching makes it a root and clears the old child link.
        tree.set_parent(c, None).unwrap();
        assert_eq!(tree.children_of(b), &[] as &[u64]);
        assert_eq!(tree.depth_of(c), 0);
    }

    #[test]
    fn set_parent_rejects_unknown_ids() {
        let mut tree = SnapshotTree::new();
        let a = tree
            .add_snapshot("a", "", 1, SnapshotType::Manual, vec![], None)
            .unwrap();
        assert!(matches!(
            tree.set_parent(999, None),
            Err(SnapshotError::NotFound(999))
        ));
        assert!(matches!(
            tree.set_parent(a, Some(999)),
            Err(SnapshotError::ParentNotFound(999))
        ));
    }
}
