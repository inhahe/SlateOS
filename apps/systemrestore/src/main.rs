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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
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
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod points;
use points::{Job, Locations, Update, Worker};

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

// Part of the complete Catppuccin Mocha palette, kept whole even though no
// widget currently paints with this one: a named palette with a hole in it is
// not the palette it is named after, and the next widget to want one would
// otherwise re-derive the hex by hand.
// ============================================================================
// Layout constants
// ============================================================================

/// How often the reports of running work are taken in while it runs.
///
/// Often enough that the bar moves as the files go by; the work itself runs
/// on its own thread (`points::Worker`), so this paces only the window.
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
    /// Taken by a restore, of the files as they were just before it: the
    /// restore's own undo.
    BeforeRestore,
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
            Self::BeforeRestore => "Before Restore",
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
            "before restore" | "beforerestore" => Some(Self::BeforeRestore),
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
            Self::BeforeRestore,
        ]
    }

    /// The next filter choice, treating "no filter" as the first.
    ///
    /// `None` is inside the cycle rather than on a key of its own, because
    /// the way out of a filter has to be as reachable as the way in -- a
    /// filter you cannot clear hides snapshots and looks like a program that
    /// has lost them.
    #[must_use]
    pub fn step_filter(current: Option<Self>) -> Option<Self> {
        let types = Self::all();
        let at = match current {
            None => 0,
            Some(ty) => types
                .iter()
                .position(|t| *t == ty)
                .map_or(0, |i| i.saturating_add(1)),
        };
        if at >= types.len() {
            None
        } else {
            types.get(at).copied()
        }
    }

    /// Icon indicator color for each type.
    pub fn indicator_color(self, pal: &Palette) -> Color {
        match self {
            Self::Manual => pal.blue,
            Self::Automatic => pal.green,
            Self::PreUpdate => pal.yellow,
            Self::PreInstall => pal.peach,
            Self::Scheduled => pal.lavender,
            Self::BeforeRestore => pal.red,
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
    /// Every program's settings and data: the one folder programs on this
    /// system keep both in (`settingsfile::config_dir`). The component a
    /// restore point can hold -- see `points`.
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
            Self::UserSettings => "Program Settings and Data",
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
            // The old name reads as the new, so a list written before the
            // rename still loads.
            "usersettings" | "programsettingsanddata" => Some(Self::UserSettings),
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

    /// The components a new restore point is taken of unless the user says
    /// otherwise: the ones this system can capture.
    pub fn default_set() -> Vec<Self> {
        vec![Self::UserSettings]
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
    /// The total size of what it holds, measured when it was taken.
    pub size_bytes: u64,
    /// Components included in this snapshot.
    pub components: Vec<SnapshotComponent>,
    /// Parent snapshot ID (None for root snapshots).
    pub parent_id: Option<u64>,
    /// Whether this snapshot is locked (cannot be deleted by retention policy).
    pub locked: bool,
    /// Optional tags for organization.
    pub tags: Vec<String>,
    /// Where each component is kept: the store's id for it.
    pub stored: BTreeMap<SnapshotComponent, String>,
    /// How many items could not be read when it was taken, and so are not
    /// in it (and are never removed by restoring it).
    pub unread: u64,
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
        Self {
            id,
            name: name.to_string(),
            description: description.to_string(),
            timestamp,
            snapshot_type,
            size_bytes: 0,
            components,
            parent_id,
            locked: false,
            tags: Vec::new(),
            stored: BTreeMap::new(),
            unread: 0,
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
#[derive(Debug)]
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

    /// The id the next point added will get.
    #[must_use]
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// A tree of points read back from disk, each keeping its id.
    ///
    /// Two passes, as import does: every point detached, then each linked to
    /// its parent -- the file need not list a parent before its children, and
    /// `set_parent` refuses a link that would close a loop.
    ///
    /// # Errors
    ///
    /// Two points with one id, a parent that is not in the list, a loop, or a
    /// next id not past every point's.
    pub fn from_points(points: Vec<Snapshot>, next_id: u64) -> Result<Self, SnapshotError> {
        let mut tree = Self::new();
        let mut parents = Vec::new();
        for mut point in points {
            if tree.snapshots.contains_key(&point.id) {
                return Err(SnapshotError::FormatError(format!(
                    "two restore points have the id {}",
                    point.id
                )));
            }
            if point.id >= next_id {
                return Err(SnapshotError::FormatError(format!(
                    "restore point {} is past the next id {next_id}",
                    point.id
                )));
            }
            parents.push((point.id, point.parent_id.take()));
            tree.snapshots.insert(point.id, point);
        }
        tree.next_id = next_id;
        for (id, parent) in parents {
            if parent.is_some() {
                tree.set_parent(id, parent)?;
            }
        }
        Ok(tree)
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
#[derive(Debug)]
pub struct SnapshotManager {
    /// The snapshot tree.
    pub tree: SnapshotTree,
    /// Schedule configuration.
    pub schedule: ScheduleConfig,
    /// The restore point the files were last taken as or restored to: what
    /// a new one is taken on top of, in the tree.
    pub current: Option<u64>,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new() -> Self {
        Self {
            tree: SnapshotTree::new(),
            schedule: ScheduleConfig::default(),
            current: None,
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

    /// Whether a scheduled restore point is due now.
    ///
    /// It used to *take* one -- added a point to the tree, with a size the
    /// program had estimated, and nothing on disk behind it. Taking one is
    /// work for `points::Worker`; this only says when.
    pub fn schedule_due(&self, now: u64) -> bool {
        self.schedule.is_due(now) && self.schedule.validate().is_ok()
    }

    /// The scheduled restore points the retention policy would remove, oldest
    /// first.
    ///
    /// Only the schedule's own: a restore point somebody took before a risky
    /// change, or the one a restore took before it ran, is theirs to delete.
    /// And never a locked one, the current one, or one with points taken on
    /// top of it -- the tree would lose its shape.
    pub fn retention_candidates(&self, now: u64) -> Vec<u64> {
        let scheduled: Vec<(u64, u64, u64, bool)> = self
            .tree
            .all_ids_by_timestamp()
            .iter()
            .filter_map(|&id| self.tree.get_snapshot(id))
            .filter(|s| s.snapshot_type == SnapshotType::Scheduled)
            .map(|s| (s.id, s.timestamp, s.size_bytes, s.locked))
            .collect();
        self.schedule
            .retention
            .snapshots_to_prune(&scheduled, now)
            .into_iter()
            .filter(|id| Some(*id) != self.current && self.tree.children_of(*id).is_empty())
            .collect()
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
// Progress
// ============================================================================

/// Work on restore points under way -- taking one, restoring one, deleting
/// one -- and, once it has ended, how it went.
///
/// Driven by what `points::Worker` reports. It was a filmstrip: a list of
/// frames made up in advance from each component's *estimated* size and
/// played one per tick, over work that did not happen.
#[derive(Clone, Debug, Default)]
pub struct OperationProgress {
    /// What is being done, for the overlay's heading.
    pub title: String,
    /// The step under way.
    pub current_step: String,
    /// Files done in the step.
    pub done: u64,
    /// Files the step has.
    pub total: u64,
    /// Whether the work has ended, well or not. The overlay stays up, with
    /// the outcome, until it is dismissed.
    pub complete: bool,
    /// Why it stopped, if it failed.
    pub error: Option<String>,
    /// What is worth knowing about how it went: what could not be read, what
    /// could not be put back, what was left out.
    pub notes: Vec<String>,
}

impl OperationProgress {
    /// Work begun, with its heading.
    #[must_use]
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            current_step: "Starting".to_string(),
            ..Self::default()
        }
    }

    /// Progress fraction (0.0 to 1.0): files done of files found, and whole
    /// once the work has ended.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.complete {
            return 1.0;
        }
        ratio::fraction(self.done, self.total).unwrap_or(0.0) as f32
    }

    /// Progress percentage (0 to 100).
    #[must_use]
    pub fn percentage(&self) -> u32 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a fraction in 0..=1 times 100"
        )]
        let pct = (self.fraction() * 100.0).round() as u32;
        pct.min(100)
    }

    /// A step reported: its name, and how far through its files it is.
    pub fn step(&mut self, name: &str, done: u64, total: u64) {
        name.clone_into(&mut self.current_step);
        self.done = done;
        self.total = total;
    }

    /// The work ended well.
    pub fn finish(&mut self, summary: &str) {
        self.complete = true;
        summary.clone_into(&mut self.current_step);
    }

    /// The work stopped, and why.
    pub fn fail(&mut self, message: &str) {
        self.error = Some(message.to_string());
        self.complete = true;
        "Stopped".clone_into(&mut self.current_step);
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

/// A control in the Schedule view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleControl {
    /// Turn the schedule on or off.
    Toggle,
    /// Take restore points less often.
    Slower,
    /// Take them more often.
    Faster,
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

/// What a piece of running work is for: what to do with what it reports.
#[derive(Clone, Debug)]
enum Pending {
    /// A restore point asked for, or taken by the schedule.
    Create {
        name: String,
        description: String,
        kind: SnapshotType,
        /// The schedule's own: no overlay, and the outcome on the status bar.
        quiet: bool,
    },
    /// A restore to `target`. Its first report is the restore point of the
    /// files as they were before it.
    Restore { target: u64, name: String },
    /// A restore point being deleted.
    Delete {
        id: u64,
        /// The retention policy's: no overlay.
        quiet: bool,
    },
}

/// Why work cannot start while other work runs.
const BUSY: &str = "Other work on restore points is running; wait for it to finish";

/// Main application UI state for the system restore manager.
/// The keys this program answers, raised by `F1` or `?`.
///
/// Nothing here takes a `?`: the search box filters snapshot names and the
/// create form takes a description, but both are reached through a dialog that
/// the card sits above.
///
/// The five `Ctrl` chords are the actions, and the bare keys move around --
/// that split is the app's own and is why `F` filters rather than finds.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+N", "Take a snapshot"),
    ("Ctrl+E / Ctrl+I", "Export / import one"),
    ("Ctrl+L", "Lock or unlock the selected snapshot"),
    ("Ctrl+F", "Cycle which kinds are listed"),
    ("Tab", "The next view; Shift+Tab the one before"),
    ("Up / Down", "Choose a snapshot"),
    ("Home / End", "First / last"),
    ("Enter", "Restore the chosen one"),
    ("Delete", "Delete it"),
    ("Backspace", "Rub out a letter of the search"),
    ("Esc", "Clear the search, or close a dialog"),
    ("Space", "Turn the schedule on or off (Schedule view)"),
    (
        "Left / Right",
        "Take restore points more or less often (Schedule view)",
    ),
    ("F1 / ?", "This list"),
];

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
    /// The clock, in seconds since the epoch: what ages, countdowns and the
    /// schedule are measured against. Set by the tick, from the wall clock.
    pub current_timestamp: u64,
    /// Which form field the keyboard is typing into.
    pub form_field: FormField,
    /// Where restore points are taken from and kept.
    locations: Locations,
    /// The work running now, and what its reports are for.
    work: Option<(Worker, Pending)>,
    /// Why the list of restore points could not be read, if it could not.
    /// Shown, and nothing is written over the file while it stands.
    load_error: Option<String>,
    /// How much disk the store takes, as last measured.
    store_bytes: Option<u64>,
    /// The comparison of the selected and the compared restore point, and for
    /// which pair -- worked out when the pair changes, not on every frame.
    compare_cache: Option<((u64, u64), Result<SnapshotDiffResult, String>)>,
    /// Asked to close while work ran: close when it ends.
    close_when_done: bool,
    /// Scheduled restore points the retention policy is removing, one by one.
    deletions: std::collections::VecDeque<u64>,
    /// What the status bar says about the last thing done in the background.
    pub status: String,
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
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
}

impl SystemRestoreUI {
    /// The window over this user's restore points.
    pub fn new() -> Self {
        Self::with_locations(Locations::from_env())
    }

    /// The window over the restore points kept at `locations`.
    ///
    /// It used to open on five invented restore points -- "Initial Setup",
    /// "After System Update v1.1" and the rest -- under a banner saying they
    /// were not real, with a weekly schedule switched on that took a new
    /// invented one within a minute. It opens on what is kept, which on a
    /// first run is nothing.
    #[must_use]
    pub fn with_locations(locations: Locations) -> Self {
        let (manager, load_error) = match &locations.store {
            Some(store) => match points::load(store) {
                Ok(manager) => (manager, None),
                Err(why) => (SnapshotManager::new(), Some(why)),
            },
            None => (SnapshotManager::new(), None),
        };
        let selected_id = manager
            .current
            .or_else(|| manager.tree.all_ids_by_timestamp().last().copied());
        let form_components = SnapshotComponent::all()
            .iter()
            .map(|c| c.source(&locations).is_ok())
            .collect();
        let mut ui = Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            manager,
            view_mode: ViewMode::Tree,
            selected_id,
            compare_id: None,
            search_query: String::new(),
            type_filter: None,
            dialog: DialogKind::None,
            progress: None,
            scroll_offset: 0.0,
            form_name: String::new(),
            form_description: String::new(),
            form_components,
            form_type: SnapshotType::Manual,
            current_timestamp: 0,
            form_field: FormField::Name,
            locations,
            work: None,
            load_error,
            store_bytes: None,
            compare_cache: None,
            close_when_done: false,
            deletions: std::collections::VecDeque::new(),
            status: String::new(),
            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
        };
        ui.measure_store();
        ui
    }

    /// A window holding a tree of five restore points, for tests.
    ///
    /// Most of this program's tests are about the list, the tree, the
    /// selection, the views and the dialogs -- all of which need *restore
    /// points*, not specifically real ones, and none of which should touch
    /// the disk. This is the tree `new` used to open on; it has nothing in
    /// the store behind it, so restoring one of these says so.
    ///
    /// The window's folders are scratch folders of its own, so what a test
    /// does is kept somewhere -- and nowhere a person keeps anything. They
    /// are handed back with it: the test holds them for as long as it uses
    /// the window, and they are removed when it drops them.
    #[cfg(test)]
    pub fn with_sample_restore_points() -> (scratchdir::ScratchDir, Self) {
        let scratch = scratchdir::ScratchDir::new("systemrestore_sample");
        let mut ui = Self::with_locations(Locations {
            settings: Some(scratch.dir().join("settings")),
            store: Some(scratch.dir().join("store")),
        });
        let base_ts = 1_700_000_000u64;
        let manager = &mut ui.manager;
        let mut add =
            |name: &str, desc: &str, days: u64, kind, comps: Vec<SnapshotComponent>, parent| {
                manager
                    .create_snapshot(
                        name,
                        desc,
                        base_ts.saturating_add(86_400_u64.saturating_mul(days)),
                        kind,
                        comps,
                        parent,
                    )
                    .unwrap_or(0)
            };
        let root_id = add(
            "Initial Setup",
            "Clean install with base system",
            0,
            SnapshotType::Manual,
            SnapshotComponent::default_set(),
            None,
        );
        let after_update_id = add(
            "After System Update v1.1",
            "System updated to version 1.1 with security patches",
            7,
            SnapshotType::PreUpdate,
            vec![
                SnapshotComponent::SystemFiles,
                SnapshotComponent::BootConfig,
                SnapshotComponent::PackageState,
            ],
            Some(root_id),
        );
        add(
            "Dev Tools Installed",
            "Added development toolchain and IDE",
            10,
            SnapshotType::PreInstall,
            vec![
                SnapshotComponent::InstalledApps,
                SnapshotComponent::UserSettings,
                SnapshotComponent::PackageState,
            ],
            Some(after_update_id),
        );
        add(
            "Weekly Auto Backup",
            "Scheduled weekly snapshot",
            14,
            SnapshotType::Scheduled,
            SnapshotComponent::default_set(),
            Some(after_update_id),
        );
        add(
            "Network Reconfigured",
            "Changed to static IP and new DNS settings",
            20,
            SnapshotType::Manual,
            vec![
                SnapshotComponent::NetworkConfig,
                SnapshotComponent::ServiceConfig,
            ],
            Some(root_id),
        );
        ui.selected_id = Some(root_id);
        ui.current_timestamp = base_ts + 86_400 * 25;
        (scratch, ui)
    }

    /// Get the list of visible snapshot IDs based on current filters.
    pub fn visible_ids(&self) -> Vec<u64> {
        self.visible_rows().into_iter().map(|(id, _)| id).collect()
    }

    /// The folders a restore point taken from the form would hold.
    pub fn form_sources(&self) -> Vec<PathBuf> {
        self.form_selected_components()
            .into_iter()
            .filter_map(|c| c.source(&self.locations).ok())
            .collect()
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
            color: self.palette.base,
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

        if self.show_help {
            guitk::shortcut::render_card(
                &mut rt,
                &self.palette,
                (self.window_width, self.window_height),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
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
        let result = self.dispatch(event);
        // The comparison follows the pair, however the pair changed: a key,
        // a click, a deletion. Worked out here rather than when drawn, which
        // happens far more often and must not read the store.
        self.refresh_compare();
        result
    }

    /// Route one event.
    fn dispatch(&mut self, event: &Event) -> EventResult {
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

    /// Take in what the running work has reported, and the time.
    ///
    /// The work reports as it goes, so this arrives often while it runs
    /// (`tick_interval`), and once a minute otherwise, for the clock and the
    /// schedule.
    fn handle_tick(&mut self) -> EventResult {
        let polled = self.poll_work();
        // Re-read rather than added to, so the clock survives a suspend and
        // does not drift: a laptop shut for an hour would otherwise wake an
        // hour behind.
        let ticked =
            system_now_secs().is_some_and(|now| self.tick_to(now) == EventResult::Consumed);
        if polled || ticked {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
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

    /// Start the schedule's work if any is due: first a removal the retention
    /// policy asked for, then a scheduled restore point.
    ///
    /// Only while this window is open -- there is no service to run it
    /// otherwise, and the Schedule view says so.
    fn run_schedule(&mut self, now: u64) {
        if self.work.is_some() || self.cannot_write().is_some() {
            return;
        }
        while let Some(id) = self.deletions.pop_front() {
            if self.manager.tree.get_snapshot(id).is_some() {
                self.start_delete(id, true);
                return;
            }
        }
        if !self.manager.schedule_due(now) {
            return;
        }
        // Marked before it runs, so one that fails is tried at the next
        // interval rather than every minute; the failure is on the status bar.
        self.manager.schedule.last_snapshot_timestamp = now;
        if !self.save() {
            return;
        }
        let components = self.manager.schedule.components.clone();
        self.status = "Taking the scheduled restore point...".to_string();
        self.start(
            Job::Create { components },
            Pending::Create {
                name: format!("Scheduled, {}", format_timestamp_short(now)),
                description: "Taken by the schedule".to_string(),
                kind: SnapshotType::Scheduled,
                quiet: true,
            },
            None,
        );
    }

    /// Take in every report the running work has sent; whether any came.
    fn poll_work(&mut self) -> bool {
        let Some((worker, pending)) = &self.work else {
            return false;
        };
        let updates = worker.updates();
        if updates.is_empty() {
            return false;
        }
        let pending = pending.clone();
        for update in updates {
            self.apply(&pending, update);
        }
        true
    }

    /// One report from the running work.
    fn apply(&mut self, pending: &Pending, update: Update) {
        match update {
            Update::Step { name, done, total } => {
                if let Some(progress) = &mut self.progress {
                    progress.step(&name, done, total);
                }
            }
            Update::Captured(captured) => self.captured(pending, &captured),
            Update::Restored { changed, errors } => {
                let Pending::Restore { target, name } = pending else {
                    return;
                };
                self.work = None;
                self.manager.current = Some(*target);
                self.selected_id = Some(*target);
                let saved = self.save();
                let summary =
                    format!("Restored to \u{201c}{name}\u{201d}: {changed} file(s) changed");
                if let Some(progress) = &mut self.progress {
                    progress.finish(&summary);
                    if !errors.is_empty() {
                        progress.notes.push(format!(
                            "{} could not be put back:",
                            plural(errors.len(), "item", "items")
                        ));
                        progress.notes.extend(
                            errors
                                .iter()
                                .map(|(path, why)| format!("{}: {why}", shown_path(path))),
                        );
                    }
                    if !saved {
                        progress.notes.push(self.status.clone());
                    }
                }
                self.after_work();
            }
            Update::Deleted => {
                let Pending::Delete { id, quiet } = pending else {
                    return;
                };
                self.work = None;
                let name = self
                    .manager
                    .tree
                    .get_snapshot(*id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                self.remove_point(*id);
                self.save();
                if *quiet {
                    self.status = format!(
                        "Removed \u{201c}{name}\u{201d}, by the schedule's retention policy"
                    );
                } else if let Some(progress) = &mut self.progress {
                    progress.finish(&format!("Deleted \u{201c}{name}\u{201d}"));
                }
                self.after_work();
            }
            Update::Failed(why) => {
                self.work = None;
                match &mut self.progress {
                    Some(progress) => progress.fail(&why),
                    None => self.status = why,
                }
                self.after_work();
            }
        }
    }

    /// A restore point taken: the one asked for, or the one a restore takes of
    /// the files before it starts.
    fn captured(&mut self, pending: &Pending, captured: &points::Captured) {
        let mut notes = Vec::new();
        if !captured.unread.is_empty() {
            notes.push(format!(
                "{} could not be read, so it is not in the restore point, and a restore will leave it as it is:",
                plural(captured.unread.len(), "item", "items")
            ));
            notes.extend(
                captured
                    .unread
                    .iter()
                    .map(|(path, why)| format!("{}: {why}", shown_path(path))),
            );
        }
        for (c, why) in &captured.skipped {
            notes.push(format!("{} was not kept: {why}", c.label()));
        }
        match pending {
            Pending::Create {
                name,
                description,
                kind,
                quiet,
            } => {
                self.work = None;
                let added = self.add_point(name, description, *kind, captured);
                let saved = self.save();
                if *quiet {
                    self.status = match added {
                        Some(_) if saved => format!("Took \u{201c}{name}\u{201d}"),
                        Some(_) => self.status.clone(),
                        None => {
                            "The scheduled restore point could not be added to the list".to_string()
                        }
                    };
                    if *kind == SnapshotType::Scheduled {
                        let now = self.current_timestamp.max(captured.taken_at);
                        self.deletions
                            .extend(self.manager.retention_candidates(now));
                    }
                } else if let Some(progress) = &mut self.progress {
                    match added {
                        Some(_) => progress.finish(&format!("Took \u{201c}{name}\u{201d}")),
                        None => progress
                            .fail("The restore point was taken but could not be added to the list"),
                    }
                    progress.notes.extend(notes);
                    if !saved {
                        progress.notes.push(self.status.clone());
                    }
                }
                self.after_work();
            }
            Pending::Restore { name, .. } => {
                // The work goes on to the restore; this is its undo.
                let before = format!("Before restoring to \u{201c}{name}\u{201d}");
                self.add_point(
                    &before,
                    "Taken by the restore, of the files as they were just before it",
                    SnapshotType::BeforeRestore,
                    captured,
                );
                self.save();
                if let Some(progress) = &mut self.progress {
                    progress.notes.extend(notes);
                }
            }
            Pending::Delete { .. } => {}
        }
    }

    /// Add a restore point for what was captured, on top of the current one,
    /// and make it current. `None` if the tree refused it.
    fn add_point(
        &mut self,
        name: &str,
        description: &str,
        kind: SnapshotType,
        captured: &points::Captured,
    ) -> Option<u64> {
        let parent = self
            .manager
            .current
            .filter(|id| self.manager.tree.get_snapshot(*id).is_some());
        let components = captured.stored.keys().copied().collect();
        let id = self
            .manager
            .tree
            .add_snapshot(
                name,
                description,
                captured.taken_at,
                kind,
                components,
                parent,
            )
            .ok()?;
        if let Some(point) = self.manager.tree.get_snapshot_mut(id) {
            point.size_bytes = captured.size;
            point.stored.clone_from(&captured.stored);
            point.unread = u64::try_from(captured.unread.len()).unwrap_or(u64::MAX);
        }
        self.manager.current = Some(id);
        self.selected_id = Some(id);
        Some(id)
    }

    /// Take a restore point out of the tree, keeping its branches: what was
    /// taken on top of it now hangs from its parent. Every restore point
    /// holds all its files, so none depends on another's.
    fn remove_point(&mut self, id: u64) {
        let tree = &mut self.manager.tree;
        let parent = tree.get_snapshot(id).and_then(|p| p.parent_id);
        for child in tree.children_of(id).to_vec() {
            // Moving a child to its grandparent cannot close a loop.
            let _ = tree.set_parent(child, parent);
        }
        if tree.remove_snapshot(id).is_err() {
            return;
        }
        if self.manager.current == Some(id) {
            self.manager.current = parent;
        }
        if self.selected_id == Some(id) {
            self.selected_id = parent;
        }
        if self.compare_id == Some(id) {
            self.compare_id = None;
        }
        self.reanchor_selection();
    }

    /// What follows every piece of work: the store measured again, the
    /// comparison worked out again, and the window closed if it was asked to.
    fn after_work(&mut self) {
        self.measure_store();
        self.compare_cache = None;
        self.refresh_compare();
    }

    /// Why nothing may be written, if something forbids it.
    fn cannot_write(&self) -> Option<String> {
        if let Some(why) = &self.load_error {
            return Some(format!(
                "{why}. Nothing is written over it: repair or remove {} first",
                self.locations
                    .store
                    .as_ref()
                    .map_or_else(String::new, |s| shown_path(&s.join("points.json")))
            ));
        }
        if self.locations.store.is_none() {
            return Some(points::NO_HOME.to_string());
        }
        None
    }

    /// Save the list. A failure is said on the status bar; what is in the
    /// window stands.
    fn save(&mut self) -> bool {
        if let Some(why) = self.cannot_write() {
            self.status = why;
            return false;
        }
        let Some(store) = self.locations.store.clone() else {
            return false;
        };
        match points::save(&store, &self.manager) {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("The list of restore points could not be saved: {e}");
                false
            }
        }
    }

    /// Start `job`, for `pending`, under an overlay headed `title` -- or with
    /// none, for the schedule's quiet work.
    fn start(&mut self, job: Job, pending: Pending, title: Option<&str>) {
        self.progress = title.map(OperationProgress::new);
        self.work = Some((Worker::start(job, self.locations.clone()), pending));
    }

    /// Refuse to start, and say why on the overlay.
    fn refuse(&mut self, title: &str, why: &str) {
        let mut progress = OperationProgress::new(title);
        progress.fail(why);
        self.progress = Some(progress);
    }

    /// Measure how much disk the store takes.
    fn measure_store(&mut self) {
        self.store_bytes = self
            .locations
            .store
            .as_ref()
            .and_then(|root| points::store_bytes(&snapstore::Store::at(root)).ok());
    }

    /// Work out the comparison of the selected and the compared restore point,
    /// if the pair has changed.
    fn refresh_compare(&mut self) {
        let (Some(a), Some(b)) = (self.selected_id, self.compare_id) else {
            self.compare_cache = None;
            return;
        };
        if self
            .compare_cache
            .as_ref()
            .is_some_and(|(key, _)| *key == (a, b))
        {
            return;
        }
        let tree = &self.manager.tree;
        let result = match (tree.get_snapshot(a), tree.get_snapshot(b)) {
            (Some(x), Some(y)) => {
                let (older, newer) = if x.timestamp <= y.timestamp {
                    (x, y)
                } else {
                    (y, x)
                };
                match &self.locations.store {
                    Some(root) => points::compare(&snapstore::Store::at(root), older, newer)
                        .map_err(|e| format!("They could not be compared: {e}")),
                    None => Err(points::NO_HOME.to_string()),
                }
            }
            _ => Err("One of them is no longer in the list".to_string()),
        };
        self.compare_cache = Some(((a, b), result));
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Above the dialog branch below, which returns before the main match:
        // a check placed after it could raise the card from the list and not
        // dismiss it from a dialog.
        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal. Letting keys through would mean restoring a snapshot the
            // reader cannot see.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }

        if let Some(progress) = &self.progress {
            // The overlay is modal. While the work runs nothing reaches
            // through, and nothing abandons it: it was Escape, over a
            // filmstrip, but real work stopped half-way is a folder half
            // restored. Once it has ended, Enter or Escape puts it away.
            if progress.complete && matches!(key.key, Key::Escape | Key::Enter) {
                self.progress = None;
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
                // The snapshot-type filter. `passes_filters` has consulted it
                // since it was written and `type_filter` was `None` at
                // construction with no writer, so the status bar's "Filter:"
                // half could never appear and the list could not be narrowed
                // to, say, the snapshots taken before an update.
                Key::F => {
                    self.type_filter = SnapshotType::step_filter(self.type_filter);
                    // The selection is a snapshot id, and the filter may have
                    // just hidden it. Leaving it selected would show the
                    // details of a row that is not in the list, which reads
                    // as the list being wrong rather than the filter working.
                    let visible = self.visible_ids();
                    if !self.selected_id.is_some_and(|id| visible.contains(&id)) {
                        self.selected_id = visible.first().copied();
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        // The schedule's controls, in its view. Space is a character the
        // search box would take anywhere else.
        if self.view_mode == ViewMode::Schedule {
            match key.key {
                Key::Space => return self.schedule_control(ScheduleControl::Toggle),
                Key::Left => return self.schedule_control(ScheduleControl::Slower),
                Key::Right => return self.schedule_control(ScheduleControl::Faster),
                _ => {}
            }
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
        // clicked. Once the work has ended, a click puts it away.
        if let Some(progress) = &self.progress {
            if progress.complete {
                self.progress = None;
            }
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

        if self.view_mode == ViewMode::Schedule
            && let Some(control) = self
                .schedule_controls()
                .into_iter()
                .find(|(rect, _)| rect.contains(x, y))
                .map(|(_, control)| control)
        {
            return self.schedule_control(control);
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

    /// Where the Schedule view's controls are drawn: the on/off badge, and
    /// the arrows either side of how often.
    ///
    /// One law for the drawing and the pointer, as `toolbar_controls` is: the
    /// badge's box is the one `render_schedule_view` fills, and the arrows sit
    /// in the frequency row it draws.
    fn schedule_controls(&self) -> Vec<(Rect, ScheduleControl)> {
        let y = self.content_top();
        let value_x = PADDING + 180.0;
        let row_y = y + PADDING + 66.0;
        vec![
            (
                Rect::new(PADDING, y + PADDING + 30.0, 80.0, 24.0),
                ScheduleControl::Toggle,
            ),
            (
                Rect::new(value_x - 26.0, row_y - 2.0, 20.0, 20.0),
                ScheduleControl::Slower,
            ),
            (
                Rect::new(value_x + 80.0, row_y - 2.0, 20.0, 20.0),
                ScheduleControl::Faster,
            ),
        ]
    }

    /// Change the schedule, and keep the change.
    ///
    /// The view drew a schedule that nothing could change: it opened switched
    /// on, weekly, with a record of automatic snapshots nobody had taken, and
    /// no key or click reached it.
    fn schedule_control(&mut self, control: ScheduleControl) -> EventResult {
        if let Some(why) = self.cannot_write() {
            self.status = why;
            return EventResult::Consumed;
        }
        let schedule = &mut self.manager.schedule;
        let all = ScheduleFrequency::all();
        let at = all
            .iter()
            .position(|f| *f == schedule.frequency)
            .unwrap_or(0);
        match control {
            ScheduleControl::Toggle => schedule.enabled = !schedule.enabled,
            // Slower is towards Monthly, the end of the list.
            ScheduleControl::Slower => {
                if let Some(next) = all.get(at.saturating_add(1)) {
                    schedule.frequency = *next;
                }
            }
            ScheduleControl::Faster => {
                if let Some(next) = at.checked_sub(1).and_then(|i| all.get(i)) {
                    schedule.frequency = *next;
                }
            }
        }
        self.status = if self.manager.schedule.enabled {
            format!(
                "Restore points are taken {} while System Restore is open",
                self.manager.schedule.frequency.label().to_lowercase()
            )
        } else {
            "The schedule is off".to_string()
        };
        self.save();
        EventResult::Consumed
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

    /// Take a restore point, from the form.
    fn begin_create(&mut self) {
        let title = "Taking a restore point";
        if let Some(why) = self.cannot_write() {
            return self.refuse(title, &why);
        }
        if self.work.is_some() {
            return self.refuse(title, BUSY);
        }
        let components: Vec<SnapshotComponent> = self
            .form_selected_components()
            .into_iter()
            .filter(|c| c.source(&self.locations).is_ok())
            .collect();
        if components.is_empty() {
            return self.refuse(
                title,
                "None of the chosen components can be kept on this system",
            );
        }
        let name = if self.form_name.trim().is_empty() {
            format!(
                "Restore point {}",
                self.manager.tree.count().saturating_add(1)
            )
        } else {
            self.form_name.trim().to_string()
        };
        self.start(
            Job::Create { components },
            Pending::Create {
                name,
                description: self.form_description.trim().to_string(),
                kind: self.form_type,
                quiet: false,
            },
            Some(title),
        );
    }

    /// Put the files back as restore point `id` holds them -- after keeping
    /// them as they are, as a restore point of their own.
    fn begin_restore(&mut self, id: u64) {
        let Some(point) = self.manager.tree.get_snapshot(id).cloned() else {
            return;
        };
        let title = format!("Restoring \u{201c}{}\u{201d}", point.name);
        if let Some(why) = self.cannot_write() {
            return self.refuse(&title, &why);
        }
        if self.work.is_some() {
            return self.refuse(&title, BUSY);
        }
        if point.stored.is_empty() {
            return self.refuse(
                &title,
                "This restore point holds nothing that can be put back",
            );
        }
        self.selected_id = Some(id);
        self.start(
            Job::Restore { point },
            Pending::Restore {
                target: id,
                name: self
                    .manager
                    .tree
                    .get_snapshot(id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
            },
            Some(&title),
        );
    }

    /// Delete a restore point: its files from the store, then it from the list.
    fn delete(&mut self, id: u64) {
        self.start_delete(id, false);
    }

    /// Delete restore point `id`; `quiet` for the retention policy's.
    fn start_delete(&mut self, id: u64, quiet: bool) {
        let Some(point) = self.manager.tree.get_snapshot(id).cloned() else {
            return;
        };
        let title = format!("Deleting \u{201c}{}\u{201d}", point.name);
        let refused = if let Some(why) = self.cannot_write() {
            Some(why)
        } else if self.work.is_some() {
            Some(BUSY.to_string())
        } else if point.locked {
            Some("It is locked. Unlock it first (Ctrl+L)".to_string())
        } else {
            None
        };
        if let Some(why) = refused {
            if quiet {
                self.status = format!("{title}: {why}");
            } else {
                self.refuse(&title, &why);
            }
            return;
        }
        if point.stored.is_empty() {
            // Nothing in the store: only the list changes.
            self.remove_point(id);
            self.save();
            return;
        }
        self.start(
            Job::Delete {
                stored: point.stored.values().cloned().collect(),
            },
            Pending::Delete { id, quiet },
            (!quiet).then_some(title.as_str()),
        );
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
        if let Some(why) = self.cannot_write() {
            self.status = why;
            return;
        }
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
        self.save();
    }

    /// Open the new-restore-point form.
    ///
    /// No parent to choose: a restore point is of the files as they are, and
    /// it is taken on top of the one they were last taken as or restored to.
    /// The form offered "branch from the selection", which described a
    /// simulation -- files cannot be taken as they were in another point.
    fn open_create_dialog(&mut self) {
        self.dialog = DialogKind::CreateSnapshot;
        self.form_field = FormField::Name;
        self.form_name.clear();
        self.form_description.clear();
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

    /// Advance the clock. Called from the tick, and it is the whole of what the
    /// tick does when no operation is running.
    pub fn set_now(&mut self, now: u64) {
        self.current_timestamp = now;
    }

    /// What a restore point holds, in the header: every program's settings
    /// and data, and where they are.
    fn header_note(&self) -> String {
        match SnapshotComponent::UserSettings.source(&self.locations) {
            Ok(dir) => format!(
                "Restore points of every program's settings and data ({})",
                shown_path(&dir)
            ),
            Err(why) => why.to_string(),
        }
    }

    /// Render the header bar.
    fn render_header(&self, rt: &mut RenderTree) {
        // Header background.
        self.palette.push_surface(
            rt,
            0.0,
            0.0,
            self.window_width,
            HEADER_HEIGHT,
            0.0,
            Surface::Card,
        );

        // Title.
        rt.push(RenderCommand::Text {
            x: PADDING,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE_TITLE / 2.0,
            text: "System Restore".to_string(),
            color: self.palette.text,
            font_size: FONT_SIZE_TITLE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // What the restore points hold, or why the list could not be read.
        // Beside the count rather than under it: the count is the claim this
        // qualifies.
        let (note, note_color) = match &self.load_error {
            Some(why) => (why.clone(), self.palette.ink(self.palette.red)),
            None => (self.header_note(), self.palette.subtext0),
        };
        rt.push(RenderCommand::Text {
            x: 360.0,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: note,
            color: note_color,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Bold,
            max_width: Some((self.window_width - 380.0).max(120.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Snapshot count badge.
        let count_text = format!("{} snapshots", self.manager.tree.count());
        self.palette.push_surface(
            rt,
            240.0,
            HEADER_HEIGHT / 2.0 - 10.0,
            100.0,
            20.0,
            10.0,
            Surface::Card,
        );
        rt.push(RenderCommand::Text {
            x: 255.0,
            y: HEADER_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: count_text,
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Search box.
        let search_x = self.window_width - 260.0;
        self.palette.push_surface(
            rt,
            search_x,
            HEADER_HEIGHT / 2.0 - 14.0,
            240.0,
            28.0,
            4.0,
            Surface::Card,
        );
        let search_display = if self.search_query.is_empty() {
            "Search snapshots...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
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
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    /// Render the toolbar with view mode tabs and action buttons.
    fn render_toolbar(&self, rt: &mut RenderTree) {
        let toolbar_y = HEADER_HEIGHT;

        // Toolbar background.
        self.palette.push_surface(
            rt,
            0.0,
            toolbar_y,
            self.window_width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // View mode tabs.
        let mut tab_x = PADDING;
        for mode in ViewMode::all() {
            let is_active = *mode == self.view_mode;
            let tab_width = 80.0;
            let tab_color = if is_active {
                self.palette.surface0
            } else {
                self.palette.mantle
            };
            let text_color = if is_active {
                self.palette.blue
            } else {
                self.palette.subtext0
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
            ("Create", self.palette.green),
            ("Restore", self.palette.blue),
            ("Delete", self.palette.red),
            ("Export", self.palette.peach),
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
                color: self.palette.base,
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
            color: self.palette.surface0,
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
                    self.palette.push_surface(
                        rt,
                        PADDING,
                        row_y,
                        self.window_width - 2.0 * PADDING,
                        TREE_ROW_HEIGHT,
                        4.0,
                        Surface::Selected,
                    );
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
                        color: self.palette.overlay0,
                        width: 1.0,
                    });
                    // Horizontal line to node.
                    rt.push(RenderCommand::Line {
                        x1: line_x,
                        y1: row_y + TREE_ROW_HEIGHT / 2.0,
                        x2: PADDING + indent,
                        y2: row_y + TREE_ROW_HEIGHT / 2.0,
                        color: self.palette.overlay0,
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
                    color: snap.snapshot_type.indicator_color(&self.palette),
                    corner_radii: CornerRadii::all(4.0),
                });

                // Snapshot name.
                let name_x = PADDING + indent + 18.0;
                rt.push(RenderCommand::Text {
                    x: name_x,
                    y: row_y + 4.0,
                    text: snap.name.clone(),
                    color: if is_selected {
                        self.palette.text
                    } else {
                        self.palette.subtext1
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
                    color: self.palette.subtext0,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(400.0),
                    overflow: TextOverflow::Ellipsis,
                });

                // Which one the files are now: what a restore would go back
                // from, and what the next restore point is taken on top of.
                if self.manager.current == Some(*id) {
                    rt.push(RenderCommand::Text {
                        x: self.window_width - 200.0,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: "Current".to_string(),
                        color: self.palette.ink(self.palette.green),
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(70.0),
                        overflow: TextOverflow::Ellipsis,
                    });
                }

                // Lock indicator.
                if snap.locked {
                    let lock_x = self.window_width - 60.0;
                    rt.push(RenderCommand::Text {
                        x: lock_x,
                        y: row_y + TREE_ROW_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
                        text: "Locked".to_string(),
                        color: self.palette.ink(self.palette.yellow),
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
                        color: self.palette.subtext0,
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
                color: self.palette.surface1,
                width: 2.0,
            });
        }

        for id in &ids {
            if let Some(snap) = self.manager.tree.get_snapshot(*id) {
                let is_selected = self.selected_id == Some(*id);

                // Selection highlight.
                if is_selected {
                    self.palette.push_surface(
                        rt,
                        timeline_x + 20.0,
                        entry_y,
                        self.window_width - timeline_x - 40.0,
                        TIMELINE_ENTRY_HEIGHT - 4.0,
                        4.0,
                        Surface::Selected,
                    );
                }

                // Timeline dot.
                rt.push(RenderCommand::FillRect {
                    x: timeline_x - TIMELINE_DOT_RADIUS,
                    y: entry_y + TIMELINE_ENTRY_HEIGHT / 2.0 - TIMELINE_DOT_RADIUS,
                    width: TIMELINE_DOT_RADIUS * 2.0,
                    height: TIMELINE_DOT_RADIUS * 2.0,
                    color: snap.snapshot_type.indicator_color(&self.palette),
                    corner_radii: CornerRadii::all(TIMELINE_DOT_RADIUS),
                });

                // Snapshot name.
                let text_x = timeline_x + 24.0;
                rt.push(RenderCommand::Text {
                    x: text_x,
                    y: entry_y + 4.0,
                    text: snap.name.clone(),
                    color: if is_selected {
                        self.palette.text
                    } else {
                        self.palette.subtext1
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
                    color: self.palette.subtext0,
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
                    color: self.palette.subtext0,
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
            color: self.palette.text,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width),
            overflow: TextOverflow::Ellipsis,
        });

        if self.selected_id.is_some() && self.compare_id.is_some() {
            if let Some((_, Err(why))) = &self.compare_cache {
                rt.push(RenderCommand::Text {
                    x: panel_x,
                    y: y + PADDING + 24.0,
                    text: why.clone(),
                    color: self.palette.ink(self.palette.red),
                    font_size: FONT_SIZE,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            if let Some((_, Ok(diff))) = &self.compare_cache {
                // Summary.
                let summary = if diff.is_empty() {
                    "The two hold the same files".to_string()
                } else {
                    format!(
                        "{} additions, {} removals, {} modifications",
                        diff.addition_count(),
                        diff.removal_count(),
                        diff.modification_count(),
                    )
                };
                rt.push(RenderCommand::Text {
                    x: panel_x,
                    y: y + PADDING + 24.0,
                    text: summary,
                    color: self.palette.subtext0,
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
                    // All three inked: a diff's colours are the whole of
                    // what it communicates, and red is the worst accent on a
                    // deep card at 2.88:1 unfloored.
                    let color = if entry.is_addition() {
                        self.palette.ink(self.palette.green)
                    } else if entry.is_removal() {
                        self.palette.ink(self.palette.red)
                    } else {
                        self.palette.ink(self.palette.yellow)
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
                color: self.palette.subtext0,
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
            color: self.palette.text,
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
            self.palette.green
        } else {
            self.palette.red
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
            color: self.palette.base,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(60.0),
            overflow: TextOverflow::Ellipsis,
        });

        // What the controls are, beside the badge they act on.
        rt.push(RenderCommand::Text {
            x: panel_x + 96.0,
            y: y + PADDING + 35.0,
            text: "Space, or a click on it, turns it on or off; Left and Right change how often"
                .to_string(),
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - 100.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Frequency.
        let mut info_y = y + PADDING + 66.0;
        let label_x = panel_x + 8.0;
        let value_x = panel_x + 180.0;

        let components = schedule
            .components
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>()
            .join(", ");
        let retention = format!(
            "{} -- of scheduled restore points only; one you take yourself is never removed by it",
            schedule.retention.summary()
        );
        let rows = [
            ("Frequency:", schedule.frequency.label()),
            ("Components:", components.as_str()),
            ("Retention:", retention.as_str()),
            (
                "Runs:",
                "Only while System Restore is open -- nothing on this system runs it otherwise",
            ),
        ];

        for (rect, control) in self.schedule_controls() {
            let glyph = match control {
                ScheduleControl::Toggle => continue,
                ScheduleControl::Slower => "<",
                ScheduleControl::Faster => ">",
            };
            rt.push(RenderCommand::Text {
                x: rect.x + 6.0,
                y: rect.y + 2.0,
                text: glyph.to_string(),
                color: self.palette.ink(self.palette.lavender),
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(rect.w),
                overflow: TextOverflow::Clip,
            });
        }
        for (label, value) in &rows {
            rt.push(RenderCommand::Text {
                x: label_x,
                y: info_y,
                text: label.to_string(),
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.to_string(),
                color: self.palette.text,
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
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(160.0),
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: due_text,
                color: self.palette.ink(self.palette.lavender),
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
            color: self.palette.text,
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
            color: self.palette.surface0,
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
                color: self.palette.blue,
                corner_radii: CornerRadii::all(4.0),
            });
        }
        if auto_frac > 0.0 {
            rt.push(RenderCommand::FillRect {
                x: panel_x + 10.0 + bar_width * manual_frac,
                y: bar_y,
                width: bar_width * auto_frac,
                height: 24.0,
                color: self.palette.green,
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
            color: self.palette.blue,
            corner_radii: CornerRadii::all(2.0),
        });
        rt.push(RenderCommand::Text {
            x: panel_x + 28.0,
            y: legend_y,
            text: format!("Manual ({})", format_bytes(stats.manual_bytes)),
            color: self.palette.subtext0,
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
            color: self.palette.green,
            corner_radii: CornerRadii::all(2.0),
        });
        rt.push(RenderCommand::Text {
            x: panel_x + 248.0,
            y: legend_y,
            text: format!("Auto ({})", format_bytes(stats.auto_bytes)),
            color: self.palette.subtext0,
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
                color: self.palette.subtext0,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(200.0),
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: value_x,
                y: info_y,
                text: value.clone(),
                color: self.palette.text,
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
                color: self.palette.ink(self.palette.yellow),
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
                    color: self.palette.subtext0,
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
            color: self.palette.surface0,
            width: 1.0,
        });

        // Panel background.
        self.palette.push_surface(
            rt,
            0.0,
            panel_y,
            self.window_width,
            DETAILS_PANEL_HEIGHT,
            0.0,
            Surface::Card,
        );

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
            color: self.palette.text,
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
            color: snap.snapshot_type.indicator_color(&self.palette),
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: col2_x + 8.0,
            y: y + 3.0,
            text: snap.snapshot_type.label().to_string(),
            color: self.palette.base,
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
        let description_used = text::Paragraph::new(&snap.description, self.palette.subtext0)
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
            if snap.unread > 0 {
                ("Unread:", format!("{} not in it", snap.unread))
            } else {
                ("Components:", format!("{}", snap.component_count()))
            },
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
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(60.0),
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: label_x + 65.0,
                y,
                text: value.clone(),
                color: self.palette.text,
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
            color: self.palette.subtext0,
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
            color: self.palette.subtext1,
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
                self.palette
                    .push_surface(rt, tag_x, y, tag_width, 18.0, 9.0, Surface::Card);
                rt.push(RenderCommand::Text {
                    x: tag_x + 8.0,
                    y: y + 2.0,
                    text: tag.clone(),
                    color: self.palette.ink(self.palette.lavender),
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
                color: self.palette.subtext0,
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
                    color: self.palette.subtext0,
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
                    color: self.palette.subtext0,
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
                        color: self.palette.subtext0,
                        font_size: FONT_SIZE_SMALL,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(CHAIN_SEPARATOR_WIDTH),
                        overflow: TextOverflow::Ellipsis,
                    });
                    cx += CHAIN_SEPARATOR_WIDTH;
                }
                let name_color = if *ancestor_id == snap.id {
                    self.palette.blue
                } else {
                    self.palette.subtext0
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
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(250.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, rt: &mut RenderTree) {
        let bar_y = self.window_height - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            rt,
            0.0,
            bar_y,
            self.window_width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Left: what the background work last said, if anything; the view
        // and filter otherwise.
        let filter_text = if !self.status.is_empty() {
            self.status.clone()
        } else if let Some(ft) = self.type_filter {
            format!("View: {} | Filter: {}", self.view_mode.label(), ft.label())
        } else {
            format!("View: {}", self.view_mode.label())
        };
        rt.push(RenderCommand::Text {
            x: PADDING,
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: filter_text,
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Center: storage summary.
        let stats = self.manager.storage_stats();
        // What the points hold, and what the store takes on disk -- less,
        // since a file unchanged between points is kept once.
        let storage_text = match self.store_bytes {
            Some(disk) => format!(
                "{} restore points | {} held, {} on disk",
                stats.snapshot_count,
                stats.total_display(),
                format_bytes(disk),
            ),
            None => format!(
                "{} restore points | {} held",
                stats.snapshot_count,
                stats.total_display(),
            ),
        };
        rt.push(RenderCommand::Text {
            x: text::center_x(
                &storage_text,
                self.window_width / 2.0,
                FONT_SIZE_SMALL,
                FontWeightHint::Regular,
            ),
            y: bar_y + STATUS_BAR_HEIGHT / 2.0 - FONT_SIZE_SMALL / 2.0,
            text: storage_text,
            color: self.palette.subtext0,
            font_size: FONT_SIZE_SMALL,
            font_weight: FontWeightHint::Regular,
            max_width: Some(320.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Right: schedule status.
        let schedule_text = if self.manager.schedule.enabled {
            format!(
                "Schedule: {} (while open)",
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
                self.palette.ink(self.palette.green)
            } else {
                self.palette.overlay0
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
            color: self.palette.base,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Border.
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Create New Snapshot".to_string(),
            color: self.palette.text,
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
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(60.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.palette.push_surface(
            rt,
            dx + PADDING,
            field_y + 18.0,
            dialog_w - 2.0 * PADDING,
            28.0,
            4.0,
            Surface::Panel,
        );
        let name_display = if self.form_name.is_empty() {
            "Enter snapshot name..."
        } else {
            &self.form_name
        };
        let name_color = if self.form_name.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
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
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Bold,
            max_width: Some(100.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.palette.push_surface(
            rt,
            dx + PADDING,
            field_y + 18.0,
            dialog_w - 2.0 * PADDING,
            28.0,
            4.0,
            Surface::Panel,
        );

        // Components checkboxes.
        field_y += 56.0;
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: field_y,
            text: "Components:".to_string(),
            color: self.palette.subtext0,
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
                color: if checked {
                    self.palette.blue
                } else {
                    self.palette.surface0
                },
                corner_radii: CornerRadii::all(3.0),
            });
            if checked {
                rt.push(RenderCommand::Text {
                    x: cx + 3.0,
                    y: cy + 1.0,
                    text: "v".to_string(),
                    color: self.palette.base,
                    font_size: FONT_SIZE_SMALL,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(CHECKBOX_SIZE),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            // A component this system cannot keep is drawn faint: it is
            // listed so what a restore point does not hold is in plain view.
            let unavailable = !checked;
            rt.push(RenderCommand::Text {
                x: cx + CHECKBOX_SIZE + 4.0,
                y: cy + 1.0,
                text: comp.label().to_string(),
                color: if unavailable {
                    self.palette.overlay0
                } else {
                    self.palette.text
                },
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(col_width - CHECKBOX_SIZE - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // What it will hold. The estimate here was a figure per component
        // for an invented machine; the size is measured when it is taken.
        let est_y = dy + dialog_h - 70.0;
        let sources = self.form_sources();
        let holds = if sources.is_empty() {
            "None of these can be kept on this system".to_string()
        } else {
            format!(
                "Keeps {}; the others need the system's permission",
                sources
                    .iter()
                    .map(|p| shown_path(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: est_y,
            text: holds,
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Buttons.
        let btn_y = dy + dialog_h - 40.0;
        // Cancel.
        self.palette.push_surface(
            rt,
            dx + dialog_w - 220.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: self.palette.text,
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
            color: self.palette.green,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Create".to_string(),
            color: self.palette.base,
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
            color: self.palette.base,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        // Title.
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Confirm Restore".to_string(),
            color: self.palette.ink(self.palette.yellow),
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
                text: format!("Restore to \u{201c}{}\u{201d}?", snap.name),
                color: self.palette.text,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: if snap.stored.is_empty() {
                    "This restore point holds nothing that can be put back.".to_string()
                } else {
                    format!(
                        "Every program's settings and data go back to {}; files added since are removed.",
                        format_timestamp_short(snap.timestamp)
                    )
                },
                color: self.palette.ink(self.palette.red),
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 94.0,
                text:
                    "Close other programs first: one still running may save over what is put back."
                        .to_string(),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 114.0,
                text: if snap.unread > 0 {
                    format!(
                        "Holds {}; {} it could not read are left as they are",
                        snap.size_display(),
                        plural(
                            usize::try_from(snap.unread).unwrap_or(usize::MAX),
                            "item",
                            "items"
                        )
                    )
                } else {
                    format!("Holds {}", snap.size_display())
                },
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            // Tip: create a snapshot before restoring.
            self.palette.push_surface(
                rt,
                dx + PADDING,
                dy + 140.0,
                dialog_w - 2.0 * PADDING,
                28.0,
                4.0,
                Surface::Panel,
            );
            rt.push(RenderCommand::Text {
                x: dx + PADDING + 8.0,
                y: dy + 146.0,
                text: "The files as they are now are kept first, as a restore point of their own, so this can be undone."
                    .to_string(),
                color: self.palette.ink(self.palette.lavender),
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Buttons.
        let btn_y = dy + dialog_h - 40.0;
        self.palette.push_surface(
            rt,
            dx + dialog_w - 220.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: self.palette.text,
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
            color: self.palette.yellow,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Restore".to_string(),
            color: self.palette.base,
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
            color: self.palette.base,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Delete Snapshot?".to_string(),
            color: self.palette.ink(self.palette.red),
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
                color: self.palette.text,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });
            rt.push(RenderCommand::Text {
                x: dx + PADDING,
                y: dy + 70.0,
                text: format!("This will free {}.", snap.size_display()),
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(dialog_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });
        }

        let btn_y = dy + dialog_h - 40.0;
        self.palette.push_surface(
            rt,
            dx + dialog_w - 220.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: self.palette.text,
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
            color: self.palette.red,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Delete".to_string(),
            color: self.palette.text,
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
            color: self.palette.base,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Export Snapshots".to_string(),
            color: self.palette.text,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: format!("Export {} snapshot(s) to file.", self.manager.tree.count()),
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Path field.
        self.palette.push_surface(
            rt,
            dx + PADDING,
            dy + 80.0,
            dialog_w - 2.0 * PADDING,
            28.0,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: dy + 86.0,
            text: "/system/backups/snapshots.txt".to_string(),
            color: self.palette.text,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        let btn_y = dy + dialog_h - 40.0;
        self.palette.push_surface(
            rt,
            dx + dialog_w - 220.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: self.palette.text,
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
            color: self.palette.peach,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Export".to_string(),
            color: self.palette.base,
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
            color: self.palette.base,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });
        rt.push(RenderCommand::StrokeRect {
            x: dx,
            y: dy,
            width: dialog_w,
            height: dialog_h,
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(CORNER_RADIUS),
        });

        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + PADDING,
            text: "Import Snapshots".to_string(),
            color: self.palette.text,
            font_size: FONT_SIZE_HEADING,
            font_weight: FontWeightHint::Bold,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });
        rt.push(RenderCommand::Text {
            x: dx + PADDING,
            y: dy + 44.0,
            text: "Import snapshot metadata from file.".to_string(),
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Path field.
        self.palette.push_surface(
            rt,
            dx + PADDING,
            dy + 80.0,
            dialog_w - 2.0 * PADDING,
            28.0,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + PADDING + 8.0,
            y: dy + 86.0,
            text: "Select file...".to_string(),
            color: self.palette.subtext0,
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(dialog_w - 2.0 * PADDING - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        let btn_y = dy + dialog_h - 40.0;
        self.palette.push_surface(
            rt,
            dx + dialog_w - 220.0,
            btn_y,
            BUTTON_WIDTH,
            BUTTON_HEIGHT,
            4.0,
            Surface::Panel,
        );
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 200.0,
            y: btn_y + 8.0,
            text: "Cancel".to_string(),
            color: self.palette.text,
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
            color: self.palette.blue,
            corner_radii: CornerRadii::all(4.0),
        });
        rt.push(RenderCommand::Text {
            x: dx + dialog_w - 92.0,
            y: btn_y + 8.0,
            text: "Import".to_string(),
            color: self.palette.base,
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
                color: self.palette.base,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });
            rt.push(RenderCommand::StrokeRect {
                x: ox,
                y: oy,
                width: overlay_w,
                height: overlay_h,
                color: self.palette.surface1,
                line_width: 1.0,
                corner_radii: CornerRadii::all(CORNER_RADIUS),
            });

            // Step text.
            rt.push(RenderCommand::Text {
                x: ox + PADDING,
                y: oy + PADDING,
                text: progress.current_step.clone(),
                color: self.palette.text,
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Bold,
                max_width: Some(overlay_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            // Progress bar background.
            let bar_y = oy + 50.0;
            self.palette.push_surface(
                rt,
                ox + PADDING,
                bar_y,
                overlay_w - 2.0 * PADDING,
                PROGRESS_BAR_HEIGHT,
                4.0,
                Surface::ControlTrack,
            );

            // Progress bar fill.
            let fill_width = (overlay_w - 2.0 * PADDING) * progress.fraction();
            if fill_width > 0.0 {
                let bar_color = if progress.error.is_some() {
                    self.palette.red
                } else {
                    self.palette.blue
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
                color: self.palette.text,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Bold,
                max_width: Some(40.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Step counter.
            rt.push(RenderCommand::Text {
                x: ox + PADDING,
                y: bar_y + 28.0,
                text: if progress.complete {
                    "Enter or Esc closes this".to_string()
                } else if progress.total > 0 {
                    format!("{} of {} files", progress.done, progress.total)
                } else {
                    progress.title.clone()
                },
                color: self.palette.subtext0,
                font_size: FONT_SIZE_SMALL,
                font_weight: FontWeightHint::Regular,
                max_width: Some(overlay_w - 2.0 * PADDING),
                overflow: TextOverflow::Ellipsis,
            });

            // Error message if any; otherwise the first thing worth knowing
            // about how it went, with a count of the rest.
            let footer = progress.error.clone().or_else(|| {
                progress.notes.first().map(|first| {
                    if progress.notes.len() > 1 {
                        format!(
                            "{first} (and {} more)",
                            progress.notes.len().saturating_sub(1)
                        )
                    } else {
                        first.clone()
                    }
                })
            });
            if let Some(err) = footer {
                rt.push(RenderCommand::Text {
                    x: ox + PADDING,
                    y: oy + overlay_h - 24.0,
                    text: err,
                    color: self.palette.ink(self.palette.red),
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

/// `n` of a thing, in words: "1 item", "3 items".
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A path as the window shows it: by its bytes, with escapes where it is not
/// text -- never a lossy decode, which would show two names as one.
fn shown_path(path: &Path) -> String {
    pathtext::ShowPath::shown(path).to_string()
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

/// The wall clock, in seconds since the epoch.
///
/// `None` if the system clock is before 1970 or cannot be read, in which case
/// the caller keeps the clock it had. Refusing to answer is better than
/// answering zero: an age measured against 1970 reads "56 years
/// ago" for every snapshot, which looks like data rather than a missing clock.
fn system_now_secs() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

impl App for SystemRestoreUI {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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
        //
        // An overlay carrying a *refusal* is not running: there is nothing to
        // advance, so asking to be woken every 400 ms would be a wakeup per
        // frame in service of a bar that cannot move. Caught by
        // `a_restore_steps_through_its_progress_and_then_finishes`, whose tail
        // asserts the clock goes back to once a minute -- it was written for
        // the end of a simulated restore and holds just as well for one that
        // never starts.
        // Often while work runs, so its reports reach the overlay as they
        // come; once a minute otherwise, for the clock and the schedule.
        if self.work.is_some() {
            Some(PROGRESS_STEP)
        } else {
            Some(CLOCK_STEP)
        }
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            // Closing now would end the work where it stands -- a folder half
            // restored. The window stays until it is done, and says so.
            if self.work.is_some() {
                self.close_when_done = true;
                self.status = "Closing when the work in progress is done".to_string();
                return Response::KeepOpen;
            }
            return Response::Exit;
        }
        let result = self.handle_event(event);
        if self.close_when_done && self.work.is_none() {
            return Response::Exit;
        }
        match result {
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
        ui.set_now(now);
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

    fn ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// Every string the window draws, joined.
    fn card_text(ui: &SystemRestoreUI) -> String {
        ui.render_tree()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// Three states, because `Esc`, `Enter`, `Tab` and `Backspace` are claimed
    /// by the dialog handler when a dialog is up and by the list when it is
    /// not -- the same keys, two different jobs, and the dialog branch returns
    /// before the main match -- and `Space`, `Left` and `Right` are the
    /// Schedule view's.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [0, 1, 2].into_iter().any(|state| {
                    let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
                    match state {
                        1 => ui.open_create_dialog(),
                        2 => ui.view_mode = ViewMode::Schedule,
                        _ => {}
                    }
                    ui.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    ///
    /// The control is the half that matters: `Ctrl+N` behind the card must not
    /// open the create dialog, and asserting only that it does not would pass
    /// on an app that had lost `Ctrl+N` altogether.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        assert!(
            !card_text(&ui).contains("F1 or ? closes this"),
            "the list is up before anybody asked for it"
        );

        ui.handle_event(&press(Key::F1));
        let shown = card_text(&ui);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        let dialog = ui.dialog.clone();
        ui.handle_event(&ctrl(Key::N));
        assert_eq!(
            ui.dialog, dialog,
            "Ctrl+N opened a dialog through the shortcut card"
        );

        ui.handle_event(&press(Key::Escape));
        assert!(
            !card_text(&ui).contains("F1 or ? closes this"),
            "Escape did not close it"
        );

        ui.handle_event(&ctrl(Key::N));
        assert_ne!(
            ui.dialog, dialog,
            "control: Ctrl+N does nothing even with the card down"
        );
    }

    /// The snapshot list can be narrowed by type, and the status bar says so.
    ///
    /// `passes_filters` has consulted `type_filter` since it was written, and
    /// the field was `None` at construction with no writer anywhere in the
    /// crate -- so the status bar's "Filter: ..." half could never appear and
    /// the list could not be narrowed to, say, the snapshots taken before an
    /// update.
    #[test]
    fn ctrl_f_narrows_the_list_by_snapshot_type() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        let all = ui.visible_ids().len();
        assert!(all > 0, "control: the fixture has no snapshots");
        assert!(
            !status_text(&ui).contains("Filter:"),
            "control: the status bar should say nothing about a filter yet"
        );

        let mut narrowed_somewhere = false;
        for _ in SnapshotType::all() {
            assert_eq!(
                ui.handle_event(&press_ctrl(Key::F)),
                EventResult::Consumed,
                "Ctrl+F was ignored"
            );
            let Some(ty) = ui.type_filter else {
                continue;
            };
            assert!(
                status_text(&ui).contains(ty.label()),
                "the status bar does not name the filter that is on: {:?}",
                status_text(&ui)
            );
            let shown = ui.visible_ids();
            if shown.len() < all {
                narrowed_somewhere = true;
            }
            for id in &shown {
                let snap = ui
                    .manager
                    .tree
                    .get_snapshot(*id)
                    .expect("a visible id must name a snapshot");
                assert_eq!(
                    snap.snapshot_type,
                    ty,
                    "a {} filter left a {} snapshot in the list",
                    ty.label(),
                    snap.snapshot_type.label()
                );
            }
            // Whatever is selected has to be something the list is showing.
            if let Some(sel) = ui.selected_id {
                assert!(
                    shown.contains(&sel),
                    "the selection is a snapshot the filter has hidden"
                );
            }
        }
        assert!(
            narrowed_somewhere,
            "no filter value removed anything; the fixture cannot tell a \
working filter from a broken one"
        );

        // One more step comes back out to no filter at all.
        ui.handle_event(&press_ctrl(Key::F));
        assert!(
            ui.type_filter.is_none(),
            "the filter does not cycle back to showing everything"
        );
        assert_eq!(
            ui.visible_ids().len(),
            all,
            "clearing the filter did not bring every snapshot back"
        );
    }

    fn status_text(ui: &SystemRestoreUI) -> String {
        ui.render_tree()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .find(|t| t.starts_with("View: "))
            .unwrap_or_default()
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

    /// A tick that finds the clock where it left it has nothing to redraw.
    #[test]
    fn a_tick_at_the_same_second_asks_for_nothing() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        let now = ui.current_timestamp;
        assert_eq!(ui.tick_to(now), EventResult::Ignored);
    }

    // -- the toolbar --

    #[test]
    fn the_view_tabs_switch_views() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
            let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        assert_eq!(ui.handle_event(&press(Key::F5)), EventResult::Ignored);
        assert_eq!(ui.search_query, "");
    }

    // -- dialogs --

    #[test]
    fn enter_confirms_a_delete_and_the_snapshot_is_gone() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        let before = ui.selected_id;
        ui.handle_event(&press(Key::Delete));
        assert_ne!(ui.dialog, DialogKind::None);

        assert_eq!(ui.handle_event(&click(4.0, 4.0)), EventResult::Consumed);
        assert_eq!(ui.dialog, DialogKind::None);
        assert_eq!(ui.selected_id, before, "the click did not reach the list");
    }

    #[test]
    fn the_dialog_buttons_can_be_clicked() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        let id = ui.selected_id.expect("selected");
        ui.handle_event(&press_ctrl(Key::L));
        assert!(
            ui.manager.tree.get_snapshot(id).expect("there").locked,
            "Ctrl+L should have locked it"
        );

        ui.handle_event(&press(Key::Delete));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.manager.tree.get_snapshot(id).is_some(), "still there");
        // Refused out loud, saying how to unlock it.
        let why = ui.progress.as_ref().and_then(|p| p.error.clone()).unwrap();
        assert!(why.contains("Ctrl+L"), "{why}");
        ui.handle_event(&press(Key::Enter));

        // And it can be unlocked again: `unlock_snapshot` had no caller, so
        // anything locked was locked for the life of the process.
        ui.handle_event(&press_ctrl(Key::L));
        assert!(!ui.manager.tree.get_snapshot(id).expect("there").locked);
    }

    // -- operations --

    // -- geometry --

    /// One law, two callers. Every row a click can land on is a row the
    /// renderer drew.
    #[test]
    fn the_rows_are_laid_out_without_overlapping_and_inside_the_content_area() {
        let (_scratch, ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        // Five, and the restore's own undo.
        assert_eq!(SnapshotType::all().len(), 6);
        assert!(SnapshotType::all().contains(&SnapshotType::BeforeRestore));
    }

    #[test]
    fn test_snapshot_type_display() {
        assert_eq!(format!("{}", SnapshotType::Manual), "Manual");
    }

    #[test]
    fn test_snapshot_type_indicator_colors_unique() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let types = SnapshotType::all();
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(
                    types[i].indicator_color(&pal),
                    types[j].indicator_color(&pal),
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
    fn test_component_all() {
        assert_eq!(SnapshotComponent::all().len(), 10);
    }

    #[test]
    fn test_component_default_set() {
        // What this system can keep: the programs' settings and data. It was
        // six components, five of which nothing could capture.
        assert_eq!(
            SnapshotComponent::default_set(),
            vec![SnapshotComponent::UserSettings]
        );
    }

    #[test]
    fn test_component_display() {
        assert_eq!(
            format!("{}", SnapshotComponent::UserSettings),
            "Program Settings and Data"
        );
        // The old name still reads, so a list written before the rename loads.
        assert_eq!(
            SnapshotComponent::from_label("User Settings"),
            Some(SnapshotComponent::UserSettings)
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
    fn test_snapshot_size_display() {
        let mut snap = Snapshot::new(
            1,
            "Test",
            "",
            0,
            SnapshotType::Manual,
            vec![SnapshotComponent::UserSettings],
            None,
        );
        // Not an estimate: nothing is held until it is taken.
        assert_eq!(snap.size_bytes, 0);
        snap.size_bytes = 3 * 1024 * 1024;
        assert!(
            snap.size_display().contains("MiB"),
            "{}",
            snap.size_display()
        );
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
        // A size is measured when a point is taken; until then it is none.
        assert_eq!(mgr.storage_stats().total_bytes, 0);
        let id = mgr.tree.all_ids_by_timestamp()[0];
        mgr.tree.get_snapshot_mut(id).unwrap().size_bytes = 4096;
        let stats = mgr.storage_stats();
        assert_eq!(stats.snapshot_count, 1);
        assert_eq!(stats.total_bytes, 4096);
    }

    // --- OperationProgress tests ---

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
        let (_scratch, ui) = SystemRestoreUI::with_sample_restore_points();
        assert!(ui.manager.tree.count() >= 4);
        assert!(ui.selected_id.is_some());
    }

    #[test]
    fn test_ui_visible_ids_no_filter() {
        let (_scratch, ui) = SystemRestoreUI::with_sample_restore_points();
        let ids = ui.visible_ids();
        assert!(!ids.is_empty());
    }

    #[test]
    fn test_ui_visible_ids_with_type_filter() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.type_filter = Some(SnapshotType::Manual);
        let ids = ui.visible_ids();
        for id in &ids {
            let snap = ui.manager.tree.get_snapshot(*id).unwrap();
            assert_eq!(snap.snapshot_type, SnapshotType::Manual);
        }
    }

    #[test]
    fn test_ui_visible_ids_with_search() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
    fn test_ui_form_selected_components() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, ui) = SystemRestoreUI::with_sample_restore_points();
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
        // Should have a good number of render commands for the full UI.
        assert!(rt.len() > 30);
    }

    #[test]
    fn test_ui_render_with_dialog() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.dialog = DialogKind::CreateSnapshot;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_timeline_view() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.view_mode = ViewMode::Timeline;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_compare_view_no_selection() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.view_mode = ViewMode::Compare;
        ui.compare_id = None;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_compare_view_with_selection() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.view_mode = ViewMode::Schedule;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_storage_view() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.view_mode = ViewMode::Storage;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_delete_dialog() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmDelete(id);
        }
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_restore_dialog() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        if let Some(id) = ui.selected_id {
            ui.dialog = DialogKind::ConfirmRestore(id);
        }
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_export_dialog() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        ui.dialog = DialogKind::ExportDialog;
        let rt = ui.render_tree();
        assert!(!rt.is_empty());
    }

    #[test]
    fn test_ui_render_import_dialog() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut SystemRestoreUI) -> Vec<Color> {
            app.render(WINDOW_WIDTH, WINDOW_HEIGHT)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let (_scratch, mut app) = SystemRestoreUI::with_sample_restore_points();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // -- restore points kept for real (2026-09-27) --

    /// A window over scratch folders: programs' settings with two files in
    /// them, and an empty store.
    fn real_ui(tag: &str) -> (scratchdir::ScratchDir, SystemRestoreUI) {
        let scratch = scratchdir::ScratchDir::new(&format!("systemrestore_{tag}"));
        let settings = scratch.dir().join("settings");
        std::fs::create_dir_all(settings.join("notes")).unwrap();
        std::fs::write(settings.join("appearance.yaml"), "theme: dark\n").unwrap();
        std::fs::write(settings.join("notes/library.txt"), "a note\n").unwrap();
        let ui = SystemRestoreUI::with_locations(Locations {
            settings: Some(settings),
            store: Some(scratch.dir().join("store")),
        });
        (scratch, ui)
    }

    fn settings_of(scratch: &scratchdir::ScratchDir) -> std::path::PathBuf {
        scratch.dir().join("settings")
    }

    /// Tick until the running work has ended, then return what the overlay
    /// says.
    fn finish(ui: &mut SystemRestoreUI) {
        for _ in 0..20_000 {
            ui.handle_event(&Event::Tick { elapsed_ms: 1 });
            if ui.work.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the work never ended");
    }

    /// Take a restore point through the form, named `name`.
    fn take(ui: &mut SystemRestoreUI, name: &str) -> u64 {
        ui.handle_event(&press_ctrl(Key::N));
        for c in name.chars() {
            ui.handle_event(&types(c));
        }
        ui.handle_event(&press(Key::Enter));
        finish(ui);
        let progress = ui.progress.take().expect("the overlay says how it went");
        assert!(progress.error.is_none(), "{:?}", progress.error);
        ui.manager.current.expect("the new one is current")
    }

    /// A window opens on what is kept -- on a first run, nothing. It opened
    /// on five invented restore points with a schedule switched on.
    #[test]
    fn a_first_run_opens_on_nothing() {
        let (_scratch, mut ui) = real_ui("first");
        assert_eq!(ui.manager.tree.count(), 0);
        assert!(!ui.manager.schedule.enabled, "a schedule nobody set");
        assert!(ui.load_error.is_none());
        // And being off, it takes nothing, however long the window is open.
        ui.tick_to(2_000_000_000);
        assert!(
            ui.work.is_none(),
            "a schedule that is off took a restore point"
        );
    }

    /// The header says what a restore point holds and where it is.
    #[test]
    fn the_header_says_what_a_restore_point_holds() {
        let (scratch, ui) = real_ui("header");
        let texts: Vec<String> = ui
            .render_tree()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let dir = shown_path(&settings_of(&scratch));
        assert!(
            texts
                .iter()
                .any(|t| t.contains("every program's settings and data") && t.contains(&dir)),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("Demonstration")));
    }

    /// Only the programs' own folder can be kept; every other component says
    /// why, and none pretends.
    #[test]
    fn every_component_says_whether_it_can_be_kept() {
        let loc = Locations {
            settings: Some(std::path::PathBuf::from("/home/u/.config/slateos")),
            store: None,
        };
        for c in SnapshotComponent::all() {
            match c.source(&loc) {
                Ok(dir) => {
                    assert_eq!(*c, SnapshotComponent::UserSettings);
                    assert_eq!(dir, std::path::PathBuf::from("/home/u/.config/slateos"));
                }
                Err(why) => assert!(!why.is_empty(), "{c:?}"),
            }
        }
        let homeless = Locations {
            settings: None,
            store: None,
        };
        assert_eq!(
            SnapshotComponent::UserSettings.source(&homeless),
            Err(points::NO_HOME)
        );
    }

    /// Taking one keeps the folder in the store, adds it on top of the
    /// current one, and saves the list -- which a new window reads back.
    #[test]
    fn a_restore_point_is_taken_kept_and_listed() {
        let (scratch, mut ui) = real_ui("take");
        let first = take(&mut ui, "Before the upgrade");
        let point = ui.manager.tree.get_snapshot(first).unwrap().clone();
        assert_eq!(point.name, "Before the upgrade");
        assert_eq!(
            point.size_bytes, 19,
            "twelve bytes of theme and seven of note"
        );
        assert_eq!(point.stored.len(), 1);
        assert_eq!(point.parent_id, None);
        assert!(point.timestamp > 0);

        let second = take(&mut ui, "After");
        assert_eq!(
            ui.manager.tree.get_snapshot(second).unwrap().parent_id,
            Some(first),
            "a new one hangs from the one the files were last taken as"
        );

        let reopened = SystemRestoreUI::with_locations(ui.locations.clone());
        assert!(reopened.load_error.is_none(), "{:?}", reopened.load_error);
        assert_eq!(reopened.manager.tree.count(), 2);
        assert_eq!(reopened.manager.current, Some(second));
        assert_eq!(
            reopened.manager.tree.get_snapshot(first).unwrap().stored,
            point.stored
        );
        drop(scratch);
    }

    /// A restore keeps the files as they are first -- its own undo -- then
    /// puts them back: changed files as they were, an added one removed.
    #[test]
    fn a_restore_puts_the_files_back_and_keeps_the_ones_it_replaced() {
        let (scratch, mut ui) = real_ui("restore");
        let dir = settings_of(&scratch);
        let point = take(&mut ui, "Good");
        std::fs::write(dir.join("appearance.yaml"), "theme: broken\n").unwrap();
        std::fs::write(dir.join("added.txt"), "new\n").unwrap();

        ui.selected_id = Some(point);
        ui.handle_event(&press(Key::Enter));
        assert!(matches!(ui.dialog, DialogKind::ConfirmRestore(_)));
        ui.handle_event(&press(Key::Enter));
        finish(&mut ui);
        let progress = ui.progress.clone().expect("the overlay says how it went");
        assert!(progress.error.is_none(), "{:?}", progress.error);
        assert!(progress.complete);

        assert_eq!(
            std::fs::read_to_string(dir.join("appearance.yaml")).unwrap(),
            "theme: dark\n"
        );
        assert!(!dir.join("added.txt").exists());
        assert_eq!(
            ui.manager.current,
            Some(point),
            "the files are that point now"
        );

        // The undo: a restore point of what was there, which puts it back.
        let before = ui
            .manager
            .tree
            .all_ids_by_timestamp()
            .into_iter()
            .find(|id| {
                ui.manager.tree.get_snapshot(*id).unwrap().snapshot_type
                    == SnapshotType::BeforeRestore
            })
            .expect("the files as they were are kept first");
        ui.progress = None;
        ui.selected_id = Some(before);
        ui.handle_event(&press(Key::Enter));
        ui.handle_event(&press(Key::Enter));
        finish(&mut ui);
        assert_eq!(
            std::fs::read_to_string(dir.join("appearance.yaml")).unwrap(),
            "theme: broken\n"
        );
        assert!(dir.join("added.txt").exists());
    }

    /// While work runs the overlay lets nothing through and cannot be
    /// abandoned -- a folder half restored is worse than either -- and once
    /// it has ended, Enter puts it away.
    #[test]
    fn the_overlay_holds_until_the_work_has_ended() {
        let (_scratch, mut ui) = real_ui("overlay");
        ui.handle_event(&press_ctrl(Key::N));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.work.is_some());
        let view = ui.view_mode;
        assert_eq!(ui.handle_event(&press(Key::Tab)), EventResult::Ignored);
        assert_eq!(ui.view_mode, view);
        if ui.work.is_some() {
            assert_eq!(ui.handle_event(&press(Key::Escape)), EventResult::Ignored);
            assert!(ui.progress.is_some(), "running work was abandoned");
        }
        finish(&mut ui);
        assert!(ui.progress.as_ref().is_some_and(|p| p.complete));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.progress.is_none());
    }

    /// Asked to close while work runs, the window stays -- and closes itself
    /// when the work is done.
    #[test]
    fn closing_waits_for_the_work() {
        let (_scratch, mut ui) = real_ui("close");
        ui.handle_event(&press_ctrl(Key::N));
        ui.handle_event(&press(Key::Enter));
        if ui.work.is_none() {
            return; // Finished before the close could be asked: nothing to wait for.
        }
        assert_eq!(ui.on_event(&Event::CloseRequested), Response::KeepOpen);
        let mut response = Response::Idle;
        for _ in 0..20_000 {
            response = ui.on_event(&Event::Tick { elapsed_ms: 1 });
            if response == Response::Exit {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(response, Response::Exit);
    }

    /// Deleting one removes its files from the store and it from the list,
    /// and what was taken on top of it hangs from its parent instead.
    #[test]
    fn deleting_a_restore_point_keeps_the_branches() {
        let (_scratch, mut ui) = real_ui("delete");
        let a = take(&mut ui, "A");
        let b = take(&mut ui, "B");
        let c = take(&mut ui, "C");
        let stored = ui.manager.tree.get_snapshot(b).unwrap().stored.clone();
        ui.selected_id = Some(b);
        ui.handle_event(&press(Key::Delete));
        ui.handle_event(&press(Key::Enter));
        finish(&mut ui);
        assert!(ui.manager.tree.get_snapshot(b).is_none());
        assert_eq!(ui.manager.tree.get_snapshot(c).unwrap().parent_id, Some(a));
        let store = snapstore::Store::at(ui.locations.store.as_ref().unwrap());
        for id in stored.values() {
            assert!(store.meta(id).is_err(), "its files are still in the store");
        }
    }

    /// A locked one is refused, and says how to unlock it.
    #[test]
    fn a_locked_restore_point_is_not_deleted() {
        let (_scratch, mut ui) = real_ui("locked");
        let a = take(&mut ui, "A");
        ui.selected_id = Some(a);
        ui.handle_event(&press_ctrl(Key::L));
        ui.handle_event(&press(Key::Delete));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.work.is_none());
        let why = ui.progress.as_ref().and_then(|p| p.error.clone()).unwrap();
        assert!(why.contains("Ctrl+L"), "{why}");
        assert!(ui.manager.tree.get_snapshot(a).is_some());
    }

    /// A list that does not read is shown, and nothing is written over it:
    /// taking a restore point is refused rather than saving a list that would
    /// lose every one the file held.
    #[test]
    fn a_damaged_list_is_never_written_over() {
        let scratch = scratchdir::ScratchDir::new("systemrestore_damaged");
        let store = scratch.dir().join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("points.json"), "{ not a list").unwrap();
        let mut ui = SystemRestoreUI::with_locations(Locations {
            settings: Some(scratch.dir().join("settings")),
            store: Some(store.clone()),
        });
        assert!(ui.load_error.is_some());
        ui.handle_event(&press_ctrl(Key::N));
        ui.handle_event(&press(Key::Enter));
        assert!(ui.work.is_none());
        assert!(ui.progress.as_ref().is_some_and(|p| p.error.is_some()));
        assert_eq!(
            std::fs::read_to_string(store.join("points.json")).unwrap(),
            "{ not a list"
        );
    }

    /// The list round-trips: every field of every point, the tree, the
    /// current one and the schedule.
    #[test]
    fn the_list_of_restore_points_round_trips() {
        let mut m = chained_manager();
        m.current = m.tree.all_ids_by_timestamp().last().copied();
        m.schedule.enabled = true;
        m.schedule.frequency = ScheduleFrequency::Daily;
        m.schedule.last_snapshot_timestamp = 1_800_000_000;
        for id in m.tree.all_ids_by_timestamp() {
            let p = m.tree.get_snapshot_mut(id).unwrap();
            // A real point holds exactly the components it has kept.
            p.components = vec![SnapshotComponent::UserSettings];
            p.size_bytes = id * 1000;
            p.unread = id;
            p.stored
                .insert(SnapshotComponent::UserSettings, format!("{id}-full"));
            p.tags.push(format!("tag{id}"));
        }
        let text = points::to_text(&m);
        let back = points::parse(&text).unwrap();
        assert_eq!(points::to_text(&back), text);
        assert_eq!(back.current, m.current);
        assert_eq!(back.schedule.frequency, ScheduleFrequency::Daily);
        for id in m.tree.all_ids_by_timestamp() {
            assert_eq!(back.tree.get_snapshot(id), m.tree.get_snapshot(id), "{id}");
        }
        assert_eq!(back.tree.next_id(), m.tree.next_id());
    }

    /// A damaged or foreign list is refused whole.
    #[test]
    fn a_list_that_is_not_one_is_refused() {
        assert!(points::parse("{}").is_err());
        assert!(points::parse(r#"{"version": 99, "points": []}"#).is_err());
        // One point that does not read spoils the whole list: saving the
        // others back would lose it.
        let good = points::to_text(&chained_manager());
        assert!(points::parse(&good).is_ok());
        // The newest point's lock (it is locked), spoiled: a leaf, so skipping it would
        // leave a list that still hangs together -- the case that tempts a
        // reader to keep what it understood.
        let at = good.rfind("\"locked\": true").expect("a lock to spoil");
        let mut spoiled = good.clone();
        spoiled.replace_range(at..at + "\"locked\": true".len(), "\"locked\": \"yes\"");
        assert!(points::parse(&spoiled).is_err(), "a list read in part");
        let mut m = chained_manager();
        m.current = Some(9_999);
        assert!(
            points::parse(&points::to_text(&m)).is_err(),
            "a current point not in it"
        );
    }

    /// The schedule is off until it is turned on, and the Schedule view's
    /// controls change it and keep the change.
    #[test]
    fn the_schedule_is_changed_from_its_view() {
        let (_scratch, mut ui) = real_ui("schedule");
        ui.view_mode = ViewMode::Schedule;
        assert!(!ui.manager.schedule.enabled);
        ui.handle_event(&press(Key::Space));
        assert!(ui.manager.schedule.enabled);
        assert_eq!(ui.manager.schedule.frequency, ScheduleFrequency::Weekly);
        ui.handle_event(&press(Key::Right));
        assert_eq!(ui.manager.schedule.frequency, ScheduleFrequency::Daily);
        ui.handle_event(&press(Key::Left));
        ui.handle_event(&press(Key::Left));
        assert_eq!(ui.manager.schedule.frequency, ScheduleFrequency::Monthly);
        let (rect, _) = ui
            .schedule_controls()
            .into_iter()
            .find(|(_, c)| *c == ScheduleControl::Toggle)
            .unwrap();
        let (x, y) = centre(rect);
        ui.handle_event(&click(x, y));
        assert!(
            !ui.manager.schedule.enabled,
            "a click on the badge turns it off"
        );
        let reopened = SystemRestoreUI::with_locations(ui.locations.clone());
        assert_eq!(
            reopened.manager.schedule.frequency,
            ScheduleFrequency::Monthly
        );
    }

    /// Turned on, the schedule takes a restore point when one is due -- a
    /// real one, quietly, on the status bar -- and not again until the next.
    #[test]
    fn a_due_schedule_takes_a_real_restore_point() {
        let (_scratch, mut ui) = real_ui("due");
        ui.manager.schedule.enabled = true;
        ui.tick_to(1_900_000_000);
        assert!(ui.work.is_some(), "due, and nothing started");
        assert!(ui.progress.is_none(), "the schedule's work is quiet");
        finish(&mut ui);
        let id = ui.manager.current.expect("taken");
        let point = ui.manager.tree.get_snapshot(id).unwrap();
        assert_eq!(point.snapshot_type, SnapshotType::Scheduled);
        assert!(!point.stored.is_empty());
        assert!(ui.status.starts_with("Took"), "{}", ui.status);
        ui.tick_to(1_900_000_060);
        assert!(ui.work.is_none(), "taken again a minute later");
    }

    /// Retention removes only the schedule's own restore points: never one
    /// somebody took, never a locked one, never the current one.
    #[test]
    fn retention_removes_only_scheduled_restore_points() {
        let mut m = SnapshotManager::new();
        let day = 86_400u64;
        let manual = m
            .create_snapshot("Mine", "", day, SnapshotType::Manual, vec![], None)
            .unwrap();
        // An old leaf of somebody's own: the policy's age limit covers it,
        // and it is not the policy's to remove.
        m.create_snapshot(
            "Also mine",
            "",
            day,
            SnapshotType::Manual,
            vec![],
            Some(manual),
        )
        .unwrap();
        let old = m
            .create_snapshot(
                "Old",
                "",
                2 * day,
                SnapshotType::Scheduled,
                vec![],
                Some(manual),
            )
            .unwrap();
        let locked = m
            .create_snapshot(
                "Kept",
                "",
                3 * day,
                SnapshotType::Scheduled,
                vec![],
                Some(manual),
            )
            .unwrap();
        m.tree.lock_snapshot(locked).unwrap();
        let current = m
            .create_snapshot(
                "Now",
                "",
                4 * day,
                SnapshotType::Scheduled,
                vec![],
                Some(manual),
            )
            .unwrap();
        m.current = Some(current);
        m.schedule.retention = RetentionPolicy::new(0, day, 0);
        assert_eq!(m.retention_candidates(100 * day), vec![old]);
    }

    /// The comparison is of the files two restore points hold.
    #[test]
    fn the_comparison_is_of_the_files() {
        let (scratch, mut ui) = real_ui("compare");
        let dir = settings_of(&scratch);
        let a = take(&mut ui, "A");
        std::fs::write(dir.join("appearance.yaml"), "theme: light\n").unwrap();
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        std::fs::remove_file(dir.join("notes/library.txt")).unwrap();
        let b = take(&mut ui, "B");
        ui.selected_id = Some(a);
        ui.compare_id = Some(b);
        ui.handle_event(&press(Key::F2));
        let Some((_, Ok(diff))) = &ui.compare_cache else {
            panic!("no comparison: {:?}", ui.compare_cache);
        };
        let summaries: Vec<String> = diff.entries.iter().map(DiffEntry::summary).collect();
        assert!(
            summaries.contains(&"+ File: new.txt".to_string()),
            "{summaries:?}"
        );
        assert!(summaries.contains(&"~ File: appearance.yaml".to_string()));
        assert!(summaries.contains(&"- File: notes/library.txt".to_string()));
        assert_eq!(diff.entries.len(), 3);
    }

    /// The form keeps what this system can keep, and says so.
    #[test]
    fn the_form_says_what_it_keeps() {
        let (scratch, ui) = real_ui("form");
        assert_eq!(ui.form_sources(), vec![settings_of(&scratch)]);
        assert_eq!(
            ui.form_selected_components(),
            vec![SnapshotComponent::UserSettings]
        );
    }

    /// Progress is files done of files found, and whole once the work ends.
    #[test]
    fn progress_is_files_done_of_files_found() {
        let mut p = OperationProgress::new("Taking a restore point");
        assert_eq!(p.percentage(), 0);
        p.step("Keeping Program Settings and Data", 1, 4);
        assert_eq!(p.percentage(), 25);
        p.finish("Took it");
        assert!(p.complete);
        assert_eq!(p.percentage(), 100);
        let mut f = OperationProgress::new("x");
        f.fail("no");
        assert!(f.complete);
        assert_eq!(f.error.as_deref(), Some("no"));
    }

    /// The overlay draws what the work reports.
    #[test]
    fn the_overlay_draws_the_work() {
        let (_scratch, mut ui) = SystemRestoreUI::with_sample_restore_points();
        let mut p = OperationProgress::new("Restoring");
        p.step("Putting back Program Settings and Data", 3, 12);
        ui.progress = Some(p);
        let texts: Vec<String> = ui
            .render_tree()
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t == "3 of 12 files"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "25%"));
    }
}
